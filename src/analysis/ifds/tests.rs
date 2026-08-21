use std::collections::{BTreeSet, VecDeque};

use proptest::prelude::*;

use super::*;
use crate::analysis::{call_graph_sccs, function_scope_nodes, SccProjection};
use crate::{
    CodePropertyGraph, CpgEdgeKind, CpgNode, CpgNodeKind, Language, MethodSignature, NodeId,
    SourceRange, Visibility,
};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
struct ReachSummary(BTreeSet<NodeId>);

impl Lattice for ReachSummary {
    fn join(&mut self, other: &Self) -> bool {
        let before = self.0.len();
        self.0.extend(other.0.iter().copied());
        self.0.len() != before
    }

    fn subsumes(&self, other: &Self) -> bool {
        self.0.is_superset(&other.0)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
struct ReachDomain;

impl SummaryDomain for ReachDomain {
    type Fact = NodeId;
    type Summary = ReachSummary;

    fn seed_facts(&self, _cpg: &CodePropertyGraph, function: NodeId) -> Vec<(NodeId, Self::Fact)> {
        vec![(function, function), (function, function)]
    }

    fn summarize(
        &self,
        cpg: &CodePropertyGraph,
        function: NodeId,
        ctx: &SummaryCtx<'_, Self>,
    ) -> Self::Summary {
        let mut summary = ReachSummary(BTreeSet::from([function]));
        let mut scope: Vec<_> = function_scope_nodes(cpg, function).into_iter().collect();
        scope.sort_unstable();
        for node in scope {
            if !cpg
                .node(node)
                .is_some_and(|node| matches!(node.kind, CpgNodeKind::Call { .. }))
            {
                continue;
            }
            if let CalleeRef::Resolved(callee) = (ctx.resolve)(node) {
                summary.0.insert(callee);
                if let Some(callee_summary) = ctx.summaries.get(&callee) {
                    summary.0.extend(callee_summary.0.iter().copied());
                }
            }
        }
        summary
    }
}

fn function_kind(name: &str) -> CpgNodeKind {
    CpgNodeKind::Function {
        signature: Box::new(MethodSignature {
            name: name.into(),
            params: Default::default(),
            return_type: None,
            is_static: false,
            is_async: false,
            visibility: Visibility::Private,
        }),
    }
}

fn node(id: u32, kind: CpgNodeKind) -> CpgNode {
    CpgNode::new(NodeId::new(id), kind, SourceRange::default())
}

fn connect_ast(cpg: &mut CodePropertyGraph, parent: NodeId, child: NodeId) {
    cpg.connect(parent, child, CpgEdgeKind::AstChild)
        .expect("fixture AST endpoints exist");
}

fn graph_fixture(function_count: usize, edges: &[(usize, usize)]) -> CodePropertyGraph {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let functions: Vec<_> = (0..function_count)
        .map(|index| {
            cpg.add_node_with_id(node(10 + index as u32, function_kind(&format!("f{index}"))))
        })
        .collect();

    let edge_set: BTreeSet<_> = edges.iter().copied().collect();
    for &(caller, callee) in &edge_set {
        let call_id = 1_000 + (caller * function_count + callee) as u32;
        cpg.add_node_with_id(node(
            call_id,
            CpgNodeKind::Call {
                target: Some(functions[callee]),
                is_method: false,
            },
        ));
    }
    for &(caller, callee) in edges {
        let call = NodeId::new(1_000 + (caller * function_count + callee) as u32);
        connect_ast(&mut cpg, functions[caller], call);
    }
    cpg
}

fn brute_reachability(function_count: usize, edges: &[(usize, usize)]) -> Vec<BTreeSet<NodeId>> {
    let mut successors = vec![BTreeSet::new(); function_count];
    for &(source, target) in edges {
        successors[source].insert(target);
    }

    (0..function_count)
        .map(|start| {
            let mut reached = BTreeSet::from([start]);
            let mut queue = VecDeque::from([start]);
            while let Some(node) = queue.pop_front() {
                for &next in &successors[node] {
                    if reached.insert(next) {
                        queue.push_back(next);
                    }
                }
            }
            reached
                .into_iter()
                .map(|index| NodeId::new(10 + index as u32))
                .collect()
        })
        .collect()
}

fn solve_reachability(cpg: &CodePropertyGraph, caps: IfdsCaps) -> IfdsOutput<ReachDomain> {
    solve_summaries(cpg, &ReachDomain, &call_graph_sccs(cpg), &caps)
}

#[test]
fn acyclic_summaries_run_once_in_callees_first_order() {
    let cpg = graph_fixture(3, &[(0, 1), (1, 2)]);
    let output = solve_reachability(&cpg, IfdsCaps::default());

    assert!(output.is_complete());
    assert_eq!(
        output.processed_functions,
        vec![NodeId::new(12), NodeId::new(11), NodeId::new(10)]
    );
    assert!(output
        .component_iterations
        .values()
        .all(|&rounds| rounds == 1));
    assert_eq!(
        output.summaries[&NodeId::new(10)].0,
        BTreeSet::from([NodeId::new(10), NodeId::new(11), NodeId::new(12)])
    );
    assert_eq!(
        output.seed_facts[&NodeId::new(10)][&NodeId::new(10)],
        BTreeSet::from([NodeId::new(10)])
    );
}

#[test]
fn mutual_and_self_recursion_converge_from_bottom() {
    let cpg = graph_fixture(3, &[(0, 1), (1, 0), (2, 2)]);
    let output = solve_reachability(&cpg, IfdsCaps::default());

    assert!(output.is_complete());
    assert_eq!(
        output.summaries[&NodeId::new(10)].0,
        BTreeSet::from([NodeId::new(10), NodeId::new(11)])
    );
    assert_eq!(
        output.summaries[&NodeId::new(11)],
        output.summaries[&NodeId::new(10)]
    );
    assert_eq!(
        output.summaries[&NodeId::new(12)].0,
        BTreeSet::from([NodeId::new(12)])
    );
    assert!(output
        .component_iterations
        .values()
        .all(|&rounds| rounds == 2));
}

#[test]
fn iteration_caps_are_explicit_and_monotone() {
    let cpg = graph_fixture(2, &[(0, 1), (1, 0)]);
    let one_round = IfdsCaps {
        max_scc_iters: 1,
        ..IfdsCaps::default()
    };
    let low = solve_reachability(&cpg, one_round);
    assert!(low.capped);
    assert!(low.cap_events.contains(&IfdsCapEvent::SccIterations {
        component: 0,
        iterations: 1,
        cap: 1,
    }));

    let two_rounds = IfdsCaps {
        max_scc_iters: 2,
        ..IfdsCaps::default()
    };
    let high = solve_reachability(&cpg, two_rounds);
    assert!(high.is_complete());

    let zero_rounds = IfdsCaps {
        max_scc_iters: 0,
        ..IfdsCaps::default()
    };
    let zero = solve_reachability(&cpg, zero_rounds);
    assert!(zero.capped);
    assert!(zero.processed_functions.is_empty());
}

#[test]
fn empty_program_is_complete_under_zero_caps() {
    let cpg = CodePropertyGraph::new(Language::Rust);
    let caps = IfdsCaps {
        max_scc_iters: 0,
        max_facts_per_node: 0,
        max_worklist_steps: 0,
        max_call_depth: 0,
        max_functions: 0,
    };
    assert!(solve_reachability(&cpg, caps).is_complete());
}

#[test]
fn function_and_worklist_caps_never_claim_completeness() {
    let cpg = graph_fixture(1, &[]);
    let function_caps = IfdsCaps {
        max_functions: 0,
        ..IfdsCaps::default()
    };
    let functions = solve_reachability(&cpg, function_caps);
    assert_eq!(
        functions.cap_events,
        BTreeSet::from([IfdsCapEvent::Functions {
            observed: 1,
            cap: 0
        }])
    );
    assert!(functions.summaries.is_empty());

    let work_caps = IfdsCaps {
        max_worklist_steps: 1,
        ..IfdsCaps::default()
    };
    let work = solve_reachability(&cpg, work_caps);
    assert!(work.capped);
    assert_eq!(work.worklist_steps, 1);
    assert!(work.processed_functions.is_empty());
    assert_eq!(
        work.cap_events,
        BTreeSet::from([IfdsCapEvent::WorklistSteps { needed: 2, cap: 1 }])
    );
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SeedDomain {
    seeds: Vec<(NodeId, u8)>,
}

impl SummaryDomain for SeedDomain {
    type Fact = u8;
    type Summary = ReachSummary;

    fn seed_facts(&self, _cpg: &CodePropertyGraph, _function: NodeId) -> Vec<(NodeId, Self::Fact)> {
        self.seeds.clone()
    }

    fn summarize(
        &self,
        _cpg: &CodePropertyGraph,
        function: NodeId,
        _ctx: &SummaryCtx<'_, Self>,
    ) -> Self::Summary {
        ReachSummary(BTreeSet::from([function]))
    }
}

#[test]
fn fact_caps_deduplicate_then_truncate_in_fact_order() {
    let cpg = graph_fixture(1, &[]);
    let function = NodeId::new(10);
    let domain = SeedDomain {
        seeds: vec![(function, 3), (function, 1), (function, 2), (function, 1)],
    };
    let caps = IfdsCaps {
        max_facts_per_node: 2,
        ..IfdsCaps::default()
    };
    let output = solve_summaries(&cpg, &domain, &call_graph_sccs(&cpg), &caps);

    assert!(output.capped);
    assert_eq!(
        output.seed_facts[&function][&function],
        BTreeSet::from([1, 2])
    );
    assert!(output.cap_events.contains(&IfdsCapEvent::FactsPerNode {
        function,
        node: function,
        observed: 3,
        cap: 2,
    }));
    assert!(output.summaries.is_empty());
}

#[test]
fn malformed_seed_nodes_are_evidence_not_panics() {
    let mut cpg = graph_fixture(2, &[]);
    let outside = cpg.add_node_with_id(node(500, CpgNodeKind::Return));
    let function = NodeId::new(10);
    let domain = SeedDomain {
        seeds: vec![(NodeId::new(999), 1), (outside, 2)],
    };
    let output = solve_summaries(&cpg, &domain, &call_graph_sccs(&cpg), &IfdsCaps::default());

    assert!(output.issues.contains(&IfdsIssue::UnknownSeedNode {
        function,
        node: NodeId::new(999),
    }));
    assert!(output.issues.contains(&IfdsIssue::SeedOutsideFunction {
        function,
        node: outside,
    }));
    assert!(output.summaries.is_empty());
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct CalleeSummary(BTreeSet<CalleeRef>);

impl Lattice for CalleeSummary {
    fn join(&mut self, other: &Self) -> bool {
        let before = self.0.len();
        self.0.extend(other.0.iter().cloned());
        self.0.len() != before
    }

    fn subsumes(&self, other: &Self) -> bool {
        self.0.is_superset(&other.0)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ResolutionDomain;

impl SummaryDomain for ResolutionDomain {
    type Fact = NodeId;
    type Summary = CalleeSummary;

    fn seed_facts(&self, _cpg: &CodePropertyGraph, _function: NodeId) -> Vec<(NodeId, Self::Fact)> {
        Vec::new()
    }

    fn summarize(
        &self,
        cpg: &CodePropertyGraph,
        function: NodeId,
        ctx: &SummaryCtx<'_, Self>,
    ) -> Self::Summary {
        let mut calls: Vec<_> = function_scope_nodes(cpg, function)
            .into_iter()
            .filter(|&node| {
                cpg.node(node)
                    .is_some_and(|node| matches!(node.kind, CpgNodeKind::Call { .. }))
            })
            .collect();
        calls.sort_unstable();
        CalleeSummary(calls.into_iter().map(ctx.resolve).collect())
    }
}

#[test]
fn default_and_custom_resolvers_cover_resolved_unresolved_external_and_out_of_graph() {
    let mut cpg = graph_fixture(2, &[(0, 1)]);
    let caller = NodeId::new(10);
    let unresolved = cpg.add_node_with_id(
        node(
            2_000,
            CpgNodeKind::Call {
                target: None,
                is_method: false,
            },
        )
        .with_text("fallback()"),
    );
    let name = cpg.add_node_with_id(node(
        2_001,
        CpgNodeKind::Identifier {
            name: "printf".into(),
            definition: None,
        },
    ));
    connect_ast(&mut cpg, caller, unresolved);
    connect_ast(&mut cpg, unresolved, name);

    let call_graph = call_graph_sccs(&cpg);
    let default = solve_summaries(&cpg, &ResolutionDomain, &call_graph, &IfdsCaps::default());
    assert!(default.is_complete());
    assert_eq!(
        default.summaries[&caller].0,
        BTreeSet::from([
            CalleeRef::Resolved(NodeId::new(11)),
            CalleeRef::Unresolved("printf".into()),
        ])
    );

    let custom = |site: NodeId| match site.as_u32() {
        1_001 => CalleeRef::External("ffi".into()),
        2_000 => CalleeRef::Resolved(NodeId::new(99_999)),
        _ => CalleeRef::Unresolved("other".into()),
    };
    let supplied = solve_summaries_with_resolver(
        &cpg,
        &ResolutionDomain,
        &call_graph,
        &IfdsCaps::default(),
        &custom,
    );
    assert!(supplied.is_complete());
    assert_eq!(
        supplied.summaries[&caller].0,
        BTreeSet::from([
            CalleeRef::External("ffi".into()),
            CalleeRef::Resolved(NodeId::new(99_999)),
        ])
    );
}

#[test]
fn stale_and_malformed_decompositions_are_rejected() {
    let cpg = graph_fixture(3, &[(0, 1), (1, 2)]);
    let mut wrong_projection = call_graph_sccs(&cpg);
    wrong_projection.projection = SccProjection::ControlFlow {
        function: NodeId::new(10),
    };
    let output = solve_summaries(&cpg, &ReachDomain, &wrong_projection, &IfdsCaps::default());
    assert!(output.issues.contains(&IfdsIssue::NotCallGraph));
    assert!(output.issues.contains(&IfdsIssue::StaleCallGraph));

    let mut malformed = call_graph_sccs(&cpg);
    malformed.components[0].id = 9;
    malformed
        .component_by_node
        .remove(&malformed.components[0].nodes[0]);
    malformed.condensation_edges.reverse();
    let output = solve_summaries(&cpg, &ReachDomain, &malformed, &IfdsCaps::default());
    assert!(output.issues.iter().any(|issue| matches!(
        issue,
        IfdsIssue::ComponentId {
            expected: 0,
            actual: 9
        }
    )));
    assert!(output
        .issues
        .iter()
        .any(|issue| matches!(issue, IfdsIssue::Membership { .. })));
    assert!(output.issues.contains(&IfdsIssue::NonCanonicalCondensation));
    assert!(output.summaries.is_empty());
}

#[test]
fn every_structural_decomposition_defect_has_ordered_evidence() {
    let mut cpg = graph_fixture(2, &[(0, 1)]);
    let non_function = cpg.add_node_with_id(node(500, CpgNodeKind::Return));
    let mut malformed = call_graph_sccs(&cpg);
    malformed.components[0].nodes = vec![NodeId::new(10), NodeId::new(10), non_function];
    malformed.components[1].nodes.clear();
    malformed.component_by_node.insert(non_function, 0);
    malformed.component_by_node.insert(NodeId::new(999), 99);
    malformed.condensation_edges = vec![
        crate::analysis::SccCondensationEdge {
            source: 99,
            target: 0,
        },
        crate::analysis::SccCondensationEdge {
            source: 0,
            target: 0,
        },
    ];

    let output = solve_summaries(&cpg, &ReachDomain, &malformed, &IfdsCaps::default());
    assert!(output
        .issues
        .contains(&IfdsIssue::EmptyComponent { component: 1 }));
    assert!(output
        .issues
        .contains(&IfdsIssue::NonCanonicalMembers { component: 0 }));
    assert!(output.issues.contains(&IfdsIssue::DuplicateMember {
        node: NodeId::new(10)
    }));
    assert!(output
        .issues
        .contains(&IfdsIssue::NotAFunction { node: non_function }));
    assert!(output.issues.contains(&IfdsIssue::MissingFunction {
        node: NodeId::new(11)
    }));
    assert!(output.issues.iter().any(|issue| matches!(
        issue,
        IfdsIssue::CondensationEdge {
            source: 99,
            target: 0
        }
    )));
    assert!(output.issues.contains(&IfdsIssue::NonCanonicalCondensation));
    assert!(output.issues.contains(&IfdsIssue::Membership {
        node: NodeId::new(999),
        component: None,
    }));
}

#[test]
fn cyclic_condensation_is_rejected_before_domain_evaluation() {
    let cpg = graph_fixture(2, &[(0, 1)]);
    let mut malformed = call_graph_sccs(&cpg);
    malformed
        .condensation_edges
        .push(crate::analysis::SccCondensationEdge {
            source: 1,
            target: 0,
        });
    malformed
        .condensation_edges
        .sort_unstable_by_key(|edge| (edge.source, edge.target));

    let output = solve_summaries(&cpg, &ReachDomain, &malformed, &IfdsCaps::default());
    assert!(output.issues.contains(&IfdsIssue::CyclicCondensation));
    assert!(output.processed_functions.is_empty());
}

#[test]
fn conflicting_resolution_evidence_remains_unresolved() {
    let mut cpg = graph_fixture(3, &[(0, 1)]);
    let call = NodeId::new(1_001);
    cpg.node_mut(call).expect("fixture call exists").text = Some("ambiguous()".into());
    cpg.connect(call, NodeId::new(12), CpgEdgeKind::CallSite)
        .expect("conflicting target exists");
    let output = solve_summaries(
        &cpg,
        &ResolutionDomain,
        &call_graph_sccs(&cpg),
        &IfdsCaps::default(),
    );
    assert!(output.is_complete());
    assert_eq!(
        output.summaries[&NodeId::new(10)].0,
        BTreeSet::from([CalleeRef::Unresolved("ambiguous()".into())])
    );
}

#[test]
fn default_resolver_honors_callee_label_depth() {
    let mut cpg = graph_fixture(1, &[]);
    let function = NodeId::new(10);
    let call = cpg.add_node_with_id(
        node(
            2_100,
            CpgNodeKind::Call {
                target: None,
                is_method: false,
            },
        )
        .with_text("fallback()"),
    );
    let name = cpg.add_node_with_id(node(
        2_101,
        CpgNodeKind::Identifier {
            name: "nested_name".into(),
            definition: None,
        },
    ));
    connect_ast(&mut cpg, function, call);
    connect_ast(&mut cpg, call, name);
    let call_graph = call_graph_sccs(&cpg);

    let zero_depth = IfdsCaps {
        max_call_depth: 0,
        ..IfdsCaps::default()
    };
    let shallow = solve_summaries(&cpg, &ResolutionDomain, &call_graph, &zero_depth);
    assert_eq!(
        shallow.summaries[&function].0,
        BTreeSet::from([CalleeRef::Unresolved("fallback()".into())])
    );

    let one_level = IfdsCaps {
        max_call_depth: 1,
        ..IfdsCaps::default()
    };
    let nested = solve_summaries(&cpg, &ResolutionDomain, &call_graph, &one_level);
    assert_eq!(
        nested.summaries[&function].0,
        BTreeSet::from([CalleeRef::Unresolved("nested_name".into())])
    );
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct BrokenSummary(bool);

impl Lattice for BrokenSummary {
    fn join(&mut self, _other: &Self) -> bool {
        false
    }

    fn subsumes(&self, other: &Self) -> bool {
        self.0 || !other.0
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct BrokenDomain;

impl SummaryDomain for BrokenDomain {
    type Fact = NodeId;
    type Summary = BrokenSummary;

    fn seed_facts(&self, _cpg: &CodePropertyGraph, _function: NodeId) -> Vec<(NodeId, Self::Fact)> {
        Vec::new()
    }

    fn summarize(
        &self,
        _cpg: &CodePropertyGraph,
        _function: NodeId,
        _ctx: &SummaryCtx<'_, Self>,
    ) -> Self::Summary {
        BrokenSummary(true)
    }
}

#[test]
fn inconsistent_lattice_contract_is_reported() {
    let cpg = graph_fixture(1, &[]);
    let output = solve_summaries(
        &cpg,
        &BrokenDomain,
        &call_graph_sccs(&cpg),
        &IfdsCaps::default(),
    );
    assert_eq!(
        output.issues,
        BTreeSet::from([IfdsIssue::LatticeLaw {
            function: NodeId::new(10)
        }])
    );
    assert!(!output.is_complete());
}

#[cfg(feature = "serde")]
#[test]
fn output_round_trips_with_ordered_evidence() {
    let cpg = graph_fixture(3, &[(0, 1), (1, 2), (2, 0)]);
    let output = solve_reachability(&cpg, IfdsCaps::default());
    let json = serde_json::to_string(&output).expect("IFDS output serializes");
    let decoded: IfdsOutput<ReachDomain> =
        serde_json::from_str(&json).expect("IFDS output deserializes");
    assert_eq!(decoded, output);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn summaries_match_breadth_first_reachability_and_ignore_edge_order(
        function_count in 1usize..=6,
        matrix in prop::collection::vec(any::<bool>(), 36),
    ) {
        let edges: Vec<_> = (0..function_count)
            .flat_map(|source| (0..function_count).map(move |target| (source, target)))
            .filter(|&(source, target)| matrix[source * 6 + target])
            .collect();
        let cpg = graph_fixture(function_count, &edges);
        let mut reversed_edges = edges.clone();
        reversed_edges.reverse();
        let reversed = graph_fixture(function_count, &reversed_edges);

        let output = solve_reachability(&cpg, IfdsCaps::default());
        let permuted = solve_reachability(&reversed, IfdsCaps::default());
        prop_assert!(output.is_complete());
        prop_assert_eq!(&output, &permuted);

        let oracle = brute_reachability(function_count, &edges);
        for (index, expected) in oracle.into_iter().enumerate() {
            prop_assert_eq!(&output.summaries[&NodeId::new(10 + index as u32)].0, &expected);
        }
    }

    #[test]
    fn raising_iteration_cap_never_turns_complete_into_capped(
        size in 1usize..=6,
        low_cap in 0u32..8,
        extra in 0u32..8,
    ) {
        let edges: Vec<_> = (0..size).map(|source| (source, (source + 1) % size)).collect();
        let cpg = graph_fixture(size, &edges);
        let low_options = IfdsCaps {
            max_scc_iters: low_cap,
            ..IfdsCaps::default()
        };
        let mut high_options = low_options.clone();
        high_options.max_scc_iters = low_cap.saturating_add(extra);
        let low = solve_reachability(&cpg, low_options);
        let high = solve_reachability(&cpg, high_options);

        if low.is_complete() {
            prop_assert!(high.is_complete());
            prop_assert_eq!(low.summaries, high.summaries);
        }
        if high.capped {
            prop_assert!(low.capped);
        }
    }

    #[test]
    fn set_lattice_obeys_join_semilattice_laws(
        left_bits in prop::collection::vec(any::<bool>(), 16),
        middle_bits in prop::collection::vec(any::<bool>(), 16),
        right_bits in prop::collection::vec(any::<bool>(), 16),
    ) {
        let make = |bits: &[bool]| ReachSummary(
            bits.iter()
                .enumerate()
                .filter_map(|(index, &present)| present.then_some(NodeId::new(index as u32)))
                .collect()
        );
        let left = make(&left_bits);
        let middle = make(&middle_bits);
        let right = make(&right_bits);

        let mut left_middle = left.clone();
        left_middle.join(&middle);
        let mut middle_left = middle.clone();
        middle_left.join(&left);
        prop_assert_eq!(&left_middle, &middle_left);

        let mut associated_left = left_middle.clone();
        associated_left.join(&right);
        let mut middle_right = middle.clone();
        middle_right.join(&right);
        let mut associated_right = left.clone();
        associated_right.join(&middle_right);
        prop_assert_eq!(associated_left, associated_right);

        let mut idempotent = left.clone();
        prop_assert!(!idempotent.join(&left));
        prop_assert_eq!(idempotent, left);
    }

    #[test]
    fn raising_worklist_cap_never_turns_complete_into_capped(
        size in 1usize..=12,
        low_cap in 0usize..30,
        extra in 0usize..30,
    ) {
        let edges: Vec<_> = (0..size.saturating_sub(1)).map(|source| (source, source + 1)).collect();
        let cpg = graph_fixture(size, &edges);
        let low_options = IfdsCaps {
            max_worklist_steps: low_cap,
            ..IfdsCaps::default()
        };
        let mut high_options = low_options.clone();
        high_options.max_worklist_steps = low_cap.saturating_add(extra);
        let low = solve_reachability(&cpg, low_options);
        let high = solve_reachability(&cpg, high_options);

        if low.is_complete() {
            prop_assert!(high.is_complete());
            prop_assert_eq!(low.summaries, high.summaries);
        }
        if high.capped {
            prop_assert!(low.capped);
        }
    }
}
