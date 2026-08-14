use proptest::prelude::*;

use super::*;
use crate::analysis::{call_graph_sccs, SccProjection};
use crate::{
    CpgEdgeKind, CpgNode, DfgEdgeKind, Language, LiteralKind, MethodSignature, ScopeId,
    SourceRange, Visibility,
};

fn function_kind(name: &str) -> CpgNodeKind {
    CpgNodeKind::Function {
        signature: MethodSignature {
            name: name.into(),
            params: Default::default(),
            return_type: None,
            is_static: false,
            is_async: false,
            visibility: Visibility::Private,
        },
    }
}

fn node(id: u32, kind: CpgNodeKind) -> CpgNode {
    CpgNode::new(
        NodeId::new(id),
        kind,
        SourceRange {
            start: id,
            end: id.saturating_add(1),
            start_line: 1,
            start_col: id,
            end_line: 1,
            end_col: id.saturating_add(1),
        },
    )
}

fn add(cpg: &mut CodePropertyGraph, id: u32, kind: CpgNodeKind) -> NodeId {
    cpg.add_node_with_id(node(id, kind))
}

fn connect_ast(cpg: &mut CodePropertyGraph, parent: NodeId, child: NodeId) {
    cpg.connect(parent, child, CpgEdgeKind::AstChild)
        .expect("fixture AST endpoints exist");
    cpg.node_mut(child).expect("fixture child exists").parent = Some(parent);
}

fn child(cpg: &mut CodePropertyGraph, parent: NodeId, id: u32, kind: CpgNodeKind) -> NodeId {
    let child = add(cpg, id, kind);
    connect_ast(cpg, parent, child);
    child
}

fn parameter(name: &str) -> CpgNodeKind {
    CpgNodeKind::Parameter {
        name: name.into(),
        param_type: None,
        is_variadic: false,
    }
}

fn identifier(name: &str, definition: Option<NodeId>) -> CpgNodeKind {
    CpgNodeKind::Identifier {
        name: name.into(),
        definition,
    }
}

fn literal(value: i64) -> CpgNodeKind {
    CpgNodeKind::Literal {
        kind: LiteralKind::Integer(value),
    }
}

fn returned(cpg: &mut CodePropertyGraph, function: NodeId, id: u32, expression: NodeId) -> NodeId {
    let returned = child(cpg, function, id, CpgNodeKind::Return);
    connect_ast(cpg, returned, expression);
    returned
}

fn call(
    cpg: &mut CodePropertyGraph,
    function: NodeId,
    id: u32,
    target: Option<NodeId>,
    arguments: &[NodeId],
) -> NodeId {
    let call = child(
        cpg,
        function,
        id,
        CpgNodeKind::Call {
            target,
            is_method: false,
        },
    );
    let callee = add(cpg, id + 100_000, identifier("callee", target));
    connect_ast(cpg, call, callee);
    for &argument in arguments {
        connect_ast(cpg, call, argument);
    }
    call
}

#[test]
fn pure_leaf_and_scalar_return_are_exact() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = add(&mut cpg, 10, function_kind("answer"));
    let value = add(&mut cpg, 12, literal(42));
    returned(&mut cpg, function, 11, value);

    let analysis = analyze_effects(&cpg, &call_graph_sccs(&cpg), &IfdsCaps::default());
    let summary = &analysis.summaries[&function];
    assert!(analysis.is_complete());
    assert_eq!(summary.purity, Purity::Pure);
    assert!(summary.reads.is_empty());
    assert!(summary.writes.is_empty());
    assert_eq!(summary.const_return, Some(ConstValue::Int(42)));
    assert!(!summary.const_return_overdefined);
}

#[test]
fn globals_fields_and_parameter_writes_are_classified_from_cpg_evidence() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let global = add(
        &mut cpg,
        1,
        CpgNodeKind::Variable {
            name: "GLOBAL".into(),
            var_type: None,
            scope: ScopeId::new(0),
            is_mutable: true,
        },
    );
    let function = add(&mut cpg, 10, function_kind("effects"));
    let param = child(&mut cpg, function, 11, parameter("p"));
    let global_read = child(&mut cpg, function, 12, identifier("GLOBAL", Some(global)));
    let assignment = child(
        &mut cpg,
        function,
        13,
        CpgNodeKind::Assignment {
            operator: "=".into(),
        },
    );
    let global_write = child(&mut cpg, assignment, 14, identifier("GLOBAL", Some(global)));
    let rhs = child(&mut cpg, assignment, 15, literal(1));
    let param_assignment = child(
        &mut cpg,
        function,
        16,
        CpgNodeKind::Assignment {
            operator: "=".into(),
        },
    );
    child(&mut cpg, param_assignment, 17, identifier("p", Some(param)));
    child(&mut cpg, param_assignment, 18, literal(2));
    let member = child(
        &mut cpg,
        function,
        19,
        CpgNodeKind::MemberAccess {
            member: "field".into(),
        },
    );
    cpg.connect(
        global_read,
        member,
        CpgEdgeKind::DataFlow(DfgEdgeKind::FieldRead),
    )
    .expect("field read endpoints exist");
    cpg.connect(
        assignment,
        member,
        CpgEdgeKind::DataFlow(DfgEdgeKind::FieldWrite),
    )
    .expect("field write endpoints exist");
    let value = add(&mut cpg, 21, literal(0));
    returned(&mut cpg, function, 20, value);

    let read_only = add(&mut cpg, 30, function_kind("read_only"));
    child(&mut cpg, read_only, 31, identifier("GLOBAL", Some(global)));
    let read_only_value = add(&mut cpg, 33, literal(0));
    returned(&mut cpg, read_only, 32, read_only_value);

    let compound = add(&mut cpg, 40, function_kind("compound"));
    let compound_assignment = child(
        &mut cpg,
        compound,
        41,
        CpgNodeKind::Assignment {
            operator: "+=".into(),
        },
    );
    child(
        &mut cpg,
        compound_assignment,
        42,
        identifier("GLOBAL", Some(global)),
    );
    child(&mut cpg, compound_assignment, 43, literal(1));
    let compound_value = add(&mut cpg, 45, literal(0));
    returned(&mut cpg, compound, 44, compound_value);

    let summaries = effect_summaries(&cpg, &call_graph_sccs(&cpg), &IfdsCaps::default());
    let summary = &summaries[&function];
    assert_eq!(summary.purity, Purity::WritesGlobal);
    assert!(summary.reads.contains(&Place::Global("GLOBAL".into())));
    assert!(summary.writes.contains(&Place::Global("GLOBAL".into())));
    assert!(summary.reads.contains(&Place::Field("field".into())));
    assert!(summary.writes.contains(&Place::Field("field".into())));
    assert!(summary.writes.contains(&Place::Param(0)));
    assert_eq!(summaries[&read_only].purity, Purity::ReadsGlobal);
    assert!(summaries[&read_only].writes.is_empty());
    assert_eq!(summaries[&compound].purity, Purity::WritesGlobal);
    assert!(summaries[&compound]
        .reads
        .contains(&Place::Global("GLOBAL".into())));
    assert!(summaries[&compound]
        .writes
        .contains(&Place::Global("GLOBAL".into())));
    assert_eq!(cpg.ast_parent(global_write), Some(assignment));
    assert_eq!(cpg.ast_parent(rhs), Some(assignment));
}

#[test]
fn impure_callees_and_unresolved_calls_propagate_transitively() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let caller = add(&mut cpg, 10, function_kind("caller"));
    let impure = add(&mut cpg, 20, function_kind("impure"));
    child(&mut cpg, impure, 21, CpgNodeKind::Throw);
    call(&mut cpg, caller, 30, Some(impure), &[]);
    call(&mut cpg, caller, 40, None, &[]);

    let analysis = analyze_effects(&cpg, &call_graph_sccs(&cpg), &IfdsCaps::default());
    assert!(analysis.is_complete());
    assert_eq!(analysis.summaries[&impure].purity, Purity::Impure);
    assert_eq!(analysis.summaries[&caller].purity, Purity::Impure);
    assert_eq!(analysis.processed_functions[0], impure);
}

#[test]
fn callee_parameter_effects_are_mapped_to_caller_places() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let global = add(
        &mut cpg,
        1,
        CpgNodeKind::Variable {
            name: "STATE".into(),
            var_type: None,
            scope: ScopeId::new(0),
            is_mutable: true,
        },
    );
    let caller = add(&mut cpg, 10, function_kind("caller"));
    let callee = add(&mut cpg, 20, function_kind("mutate"));
    let param = child(&mut cpg, callee, 21, parameter("p"));
    let assignment = child(
        &mut cpg,
        callee,
        22,
        CpgNodeKind::Assignment {
            operator: "=".into(),
        },
    );
    child(&mut cpg, assignment, 23, identifier("p", Some(param)));
    child(&mut cpg, assignment, 24, literal(1));
    let argument = add(&mut cpg, 31, identifier("STATE", Some(global)));
    call(&mut cpg, caller, 30, Some(callee), &[argument]);

    let summaries = effect_summaries(&cpg, &call_graph_sccs(&cpg), &IfdsCaps::default());
    assert!(summaries[&callee].writes.contains(&Place::Param(0)));
    assert_eq!(summaries[&callee].purity, Purity::Pure);
    assert!(summaries[&caller]
        .writes
        .contains(&Place::Global("STATE".into())));
    assert_eq!(summaries[&caller].purity, Purity::WritesGlobal);
}

#[test]
fn constants_and_parameter_flow_compose_through_calls() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let constant = add(&mut cpg, 10, function_kind("constant"));
    let binary = add(
        &mut cpg,
        12,
        CpgNodeKind::BinaryOp {
            operator: "+".into(),
        },
    );
    let two = child(&mut cpg, binary, 13, literal(2));
    let three = child(&mut cpg, binary, 14, literal(3));
    returned(&mut cpg, constant, 11, binary);

    let identity = add(&mut cpg, 20, function_kind("identity"));
    let param = child(&mut cpg, identity, 21, parameter("value"));
    let use_site = add(&mut cpg, 23, identifier("value", Some(param)));
    returned(&mut cpg, identity, 22, use_site);

    let wrapper = add(&mut cpg, 30, function_kind("wrapper"));
    let wrapper_param = child(&mut cpg, wrapper, 31, parameter("input"));
    let argument = add(&mut cpg, 34, identifier("input", Some(wrapper_param)));
    let identity_call = call(&mut cpg, wrapper, 33, Some(identity), &[argument]);
    returned(&mut cpg, wrapper, 32, identity_call);

    let constant_wrapper = add(&mut cpg, 40, function_kind("constant_wrapper"));
    let constant_call = call(&mut cpg, constant_wrapper, 42, Some(constant), &[]);
    returned(&mut cpg, constant_wrapper, 41, constant_call);

    let summaries = effect_summaries(&cpg, &call_graph_sccs(&cpg), &IfdsCaps::default());
    assert_eq!(summaries[&constant].const_return, Some(ConstValue::Int(5)));
    assert_eq!(summaries[&identity].param_to_return, 1);
    assert!(summaries[&identity].const_return_overdefined);
    assert_eq!(summaries[&wrapper].param_to_return, 1);
    assert_eq!(
        summaries[&constant_wrapper].const_return,
        Some(ConstValue::Int(5))
    );
    assert_eq!(cpg.ast_parent(two), Some(binary));
    assert_eq!(cpg.ast_parent(three), Some(binary));
}

#[test]
fn disagreeing_return_constants_become_permanently_overdefined() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = add(&mut cpg, 10, function_kind("branch"));
    let one = add(&mut cpg, 12, literal(1));
    returned(&mut cpg, function, 11, one);
    let two = add(&mut cpg, 14, literal(2));
    returned(&mut cpg, function, 13, two);

    let summary = &effect_summaries(&cpg, &call_graph_sccs(&cpg), &IfdsCaps::default())[&function];
    assert_eq!(summary.const_return, None);
    assert!(summary.const_return_overdefined);

    let mut joined = EffectSummary::default();
    let first = EffectSummary {
        const_return: Some(ConstValue::Int(1)),
        ..EffectSummary::default()
    };
    let second = EffectSummary {
        const_return: Some(ConstValue::Int(2)),
        ..EffectSummary::default()
    };
    joined.join(&first);
    joined.join(&second);
    joined.join(&first);
    assert!(joined.const_return_overdefined);
    assert_eq!(joined.const_return, None);
}

#[test]
fn mutual_recursion_converges_and_preserves_direct_effects() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let left = add(&mut cpg, 10, function_kind("left"));
    let right = add(&mut cpg, 20, function_kind("right"));
    call(&mut cpg, left, 30, Some(right), &[]);
    call(&mut cpg, right, 40, Some(left), &[]);
    let left_value = add(&mut cpg, 51, literal(7));
    returned(&mut cpg, left, 50, left_value);
    let right_value = add(&mut cpg, 61, literal(7));
    returned(&mut cpg, right, 60, right_value);

    let analysis = analyze_effects(&cpg, &call_graph_sccs(&cpg), &IfdsCaps::default());
    assert!(analysis.is_complete());
    assert_eq!(
        analysis.summaries[&left].const_return,
        Some(ConstValue::Int(7))
    );
    assert_eq!(
        analysis.summaries[&right].const_return,
        Some(ConstValue::Int(7))
    );
    assert!(analysis
        .component_iterations
        .values()
        .all(|&iterations| iterations == 2));
}

#[test]
fn shared_and_domain_caps_remain_explicit() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = add(&mut cpg, 10, function_kind("deep"));
    let unary = add(
        &mut cpg,
        12,
        CpgNodeKind::UnaryOp {
            operator: "-".into(),
        },
    );
    child(&mut cpg, unary, 13, literal(1));
    returned(&mut cpg, function, 11, unary);
    let call_graph = call_graph_sccs(&cpg);

    let no_work = IfdsCaps {
        max_worklist_steps: 0,
        ..IfdsCaps::default()
    };
    let shared = analyze_effects(&cpg, &call_graph, &no_work);
    assert!(shared.capped);
    assert!(shared.summaries.is_empty());
    assert!(shared
        .cap_events
        .contains(&IfdsCapEvent::WorklistSteps { needed: 1, cap: 0 }));

    let no_depth = IfdsCaps {
        max_call_depth: 0,
        ..IfdsCaps::default()
    };
    let domain = analyze_effects(&cpg, &call_graph, &no_depth);
    assert!(domain.capped);
    assert_eq!(domain.domain_capped_functions, BTreeSet::from([function]));
    assert!(domain.summaries[&function].capped);
    assert!(domain.summaries[&function].const_return_overdefined);
}

#[test]
fn malformed_call_graph_is_rejected_without_mutating_the_cpg() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = add(&mut cpg, 10, function_kind("f"));
    let before = (cpg.node_count(), cpg.edge_count());
    let mut malformed = call_graph_sccs(&cpg);
    malformed.projection = SccProjection::ControlFlow { function };

    let analysis = analyze_effects(&cpg, &malformed, &IfdsCaps::default());
    assert!(analysis.issues.contains(&IfdsIssue::NotCallGraph));
    assert!(analysis.summaries.is_empty());
    assert_eq!((cpg.node_count(), cpg.edge_count()), before);
}

#[test]
fn parameter_masks_surface_their_fixed_width_boundary() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = add(&mut cpg, 10, function_kind("wide"));
    for index in 0..65u32 {
        child(
            &mut cpg,
            function,
            100 + index,
            parameter(&format!("p{index}")),
        );
    }
    let value = add(&mut cpg, 1000, literal(0));
    returned(&mut cpg, function, 999, value);
    let analysis = analyze_effects(&cpg, &call_graph_sccs(&cpg), &IfdsCaps::default());
    let summary = &analysis.summaries[&function];
    assert!(summary.param_mask_truncated);
    assert!(summary.capped);
    assert!(analysis.capped);
    assert!(!analysis.is_complete());
    assert_eq!(analysis.domain_capped_functions, BTreeSet::from([function]));
}

#[cfg(feature = "serde")]
#[test]
fn analysis_round_trips_with_ordered_effect_evidence() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = add(&mut cpg, 10, function_kind("answer"));
    let value = add(&mut cpg, 12, literal(42));
    returned(&mut cpg, function, 11, value);
    let analysis = analyze_effects(&cpg, &call_graph_sccs(&cpg), &IfdsCaps::default());
    let json = serde_json::to_string(&analysis).expect("effect analysis serializes");
    let decoded: EffectAnalysis =
        serde_json::from_str(&json).expect("effect analysis deserializes");
    assert_eq!(decoded, analysis);
}

fn summary_from_bits(reads: u8, writes: u8, mask: u8, constant: u8) -> EffectSummary {
    let places = [
        Place::Global("g".into()),
        Place::Param(0),
        Place::Field("f".into()),
    ];
    let mut summary = EffectSummary {
        purity: match (reads >> 3) & 0b11 {
            0 => Purity::Pure,
            1 => Purity::ReadsGlobal,
            2 => Purity::WritesGlobal,
            _ => Purity::Impure,
        },
        param_to_return: u64::from(mask),
        param_mask_truncated: reads & 0x20 != 0,
        capped: writes & 0x20 != 0,
        ..EffectSummary::default()
    };
    for (index, place) in places.into_iter().enumerate() {
        if reads & (1 << index) != 0 {
            summary.reads.insert(place.clone());
        }
        if writes & (1 << index) != 0 {
            summary.writes.insert(place);
        }
    }
    match constant % 4 {
        0 => {}
        1 => summary.const_return = Some(ConstValue::Int(1)),
        2 => summary.const_return = Some(ConstValue::Int(2)),
        _ => summary.const_return_overdefined = true,
    }
    summary
}

fn purity_fixture(
    function_count: usize,
    edges: &[(usize, usize)],
    impure: &[bool],
) -> CodePropertyGraph {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let functions: Vec<_> = (0..function_count)
        .map(|index| {
            add(
                &mut cpg,
                10 + index as u32,
                function_kind(&format!("f{index}")),
            )
        })
        .collect();
    for (index, &is_impure) in impure.iter().take(function_count).enumerate() {
        if is_impure {
            child(
                &mut cpg,
                functions[index],
                500 + index as u32,
                CpgNodeKind::Throw,
            );
        }
    }
    for &(caller, callee) in edges {
        let id = 1_000 + (caller * function_count + callee) as u32 * 2;
        call(
            &mut cpg,
            functions[caller],
            id,
            Some(functions[callee]),
            &[],
        );
    }
    cpg
}

fn brute_purity(function_count: usize, edges: &[(usize, usize)], impure: &[bool]) -> Vec<Purity> {
    let mut successors = vec![BTreeSet::new(); function_count];
    for &(source, target) in edges {
        successors[source].insert(target);
    }
    (0..function_count)
        .map(|start| {
            let mut stack = vec![start];
            let mut seen = BTreeSet::new();
            while let Some(node) = stack.pop() {
                if seen.insert(node) {
                    stack.extend(successors[node].iter().copied());
                }
            }
            if seen.into_iter().any(|node| impure[node]) {
                Purity::Impure
            } else {
                Purity::Pure
            }
        })
        .collect()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn effect_summary_is_a_join_semilattice(
        a in any::<(u8,u8,u8,u8)>(),
        b in any::<(u8,u8,u8,u8)>(),
        c in any::<(u8,u8,u8,u8)>(),
    ) {
        let left = summary_from_bits(a.0, a.1, a.2, a.3);
        let middle = summary_from_bits(b.0, b.1, b.2, b.3);
        let right = summary_from_bits(c.0, c.1, c.2, c.3);

        let mut lm = left.clone();
        lm.join(&middle);
        let mut ml = middle.clone();
        ml.join(&left);
        prop_assert_eq!(&lm, &ml);

        let mut associated_left = lm;
        associated_left.join(&right);
        let mut mr = middle;
        mr.join(&right);
        let mut associated_right = left.clone();
        associated_right.join(&mr);
        prop_assert_eq!(&associated_left, &associated_right);

        let mut idempotent = left.clone();
        prop_assert!(!idempotent.join(&left));
        prop_assert_eq!(&idempotent, &left);
        prop_assert!(associated_left.subsumes(&left));
    }

    #[test]
    fn summaries_match_transitive_purity_and_ignore_call_insertion_order(
        function_count in 1usize..=6,
        matrix in prop::collection::vec(any::<bool>(), 36),
        impure in prop::collection::vec(any::<bool>(), 6),
    ) {
        let edges: Vec<_> = (0..function_count)
            .flat_map(|source| (0..function_count).map(move |target| (source, target)))
            .filter(|&(source, target)| matrix[source * 6 + target])
            .collect();
        let cpg = purity_fixture(function_count, &edges, &impure);
        let mut reversed_edges = edges.clone();
        reversed_edges.reverse();
        let reversed = purity_fixture(function_count, &reversed_edges, &impure);
        let output = analyze_effects(&cpg, &call_graph_sccs(&cpg), &IfdsCaps::default());
        let permuted = analyze_effects(
            &reversed,
            &call_graph_sccs(&reversed),
            &IfdsCaps::default(),
        );
        prop_assert!(output.is_complete());
        prop_assert_eq!(&output, &permuted);

        for (index, expected) in brute_purity(function_count, &edges, &impure)
            .into_iter()
            .enumerate()
        {
            prop_assert_eq!(output.summaries[&NodeId::new(10 + index as u32)].purity, expected);
        }
    }

    #[test]
    fn raising_iteration_cap_never_invalidates_a_complete_effect_result(
        size in 1usize..=6,
        low_cap in 0u32..8,
        extra in 0u32..8,
    ) {
        let edges: Vec<_> = (0..size).map(|source| (source, (source + 1) % size)).collect();
        let impure = vec![false; size];
        let cpg = purity_fixture(size, &edges, &impure);
        let low_options = IfdsCaps {
            max_scc_iters: low_cap,
            ..IfdsCaps::default()
        };
        let mut high_options = low_options.clone();
        high_options.max_scc_iters = low_cap.saturating_add(extra);
        let low = analyze_effects(&cpg, &call_graph_sccs(&cpg), &low_options);
        let high = analyze_effects(&cpg, &call_graph_sccs(&cpg), &high_options);
        if low.is_complete() {
            prop_assert!(high.is_complete());
            prop_assert_eq!(low.summaries, high.summaries);
        }
        if high.capped {
            prop_assert!(low.capped);
        }
    }
}
