use std::thread;

use proptest::prelude::*;

use super::*;
use crate::{CpgNode, DfgEdgeKind, Language, MethodSignature, ScopeId, SourceRange, Visibility};

fn function() -> CpgNodeKind {
    CpgNodeKind::Function {
        signature: Box::new(MethodSignature {
            name: "fixture".into(),
            params: Default::default(),
            return_type: None,
            is_static: false,
            is_async: false,
            visibility: Visibility::Private,
        }),
    }
}

fn parameter(name: &str) -> CpgNodeKind {
    CpgNodeKind::Parameter {
        name: name.into(),
        param_type: None,
        is_variadic: false,
    }
}

fn identifier(name: &str) -> CpgNodeKind {
    CpgNodeKind::Identifier {
        name: name.into(),
        definition: None,
    }
}

fn integer(value: i64) -> CpgNodeKind {
    CpgNodeKind::Literal {
        kind: LiteralKind::Integer(value),
    }
}

fn node(id: u32, kind: CpgNodeKind) -> CpgNode {
    CpgNode::new(NodeId::new(id), kind, SourceRange::default())
}

fn add(cpg: &mut CodePropertyGraph, kind: CpgNodeKind) -> NodeId {
    cpg.add_node(CpgNode::new(NodeId::new(0), kind, SourceRange::default()))
}

fn ast(cpg: &mut CodePropertyGraph, parent: NodeId, child: NodeId) {
    cpg.connect(parent, child, CpgEdgeKind::AstChild);
}

fn cfg(cpg: &mut CodePropertyGraph, source: NodeId, target: NodeId, kind: CfgEdgeKind) {
    cpg.connect(source, target, CpgEdgeKind::ControlFlow(kind));
}

fn def_use(cpg: &mut CodePropertyGraph, definition: NodeId, usage: NodeId) {
    cpg.connect(
        definition,
        usage,
        CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse),
    );
}

#[derive(Clone, Copy)]
struct ComparisonSpec {
    operator: &'static str,
    constant: i64,
}

fn compound_fixture(
    logical: &str,
    left: ComparisonSpec,
    right: ComparisonSpec,
    reverse_nodes: bool,
) -> (CodePropertyGraph, NodeId, NodeId, NodeId) {
    let function_id = NodeId::new(0);
    let parameter_id = NodeId::new(1);
    let guard = NodeId::new(2);
    let left_cmp = NodeId::new(3);
    let left_use = NodeId::new(4);
    let left_literal = NodeId::new(5);
    let right_cmp = NodeId::new(6);
    let right_use = NodeId::new(7);
    let right_literal = NodeId::new(8);
    let true_target = NodeId::new(9);
    let false_target = NodeId::new(10);
    let mut nodes = vec![
        node(0, function()),
        node(1, parameter("x")),
        node(
            2,
            CpgNodeKind::BinaryOp {
                operator: logical.into(),
            },
        ),
        node(
            3,
            CpgNodeKind::BinaryOp {
                operator: left.operator.into(),
            },
        ),
        node(4, identifier("x")),
        node(5, integer(left.constant)),
        node(
            6,
            CpgNodeKind::BinaryOp {
                operator: right.operator.into(),
            },
        ),
        node(7, identifier("x")),
        node(8, integer(right.constant)),
        node(
            9,
            CpgNodeKind::Block {
                scope: ScopeId::GLOBAL,
            },
        ),
        node(
            10,
            CpgNodeKind::Block {
                scope: ScopeId::GLOBAL,
            },
        ),
    ];
    if reverse_nodes {
        nodes.reverse();
    }
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    for node in nodes {
        cpg.add_node_with_id(node);
    }
    for child in [parameter_id, guard, true_target, false_target] {
        ast(&mut cpg, function_id, child);
    }
    ast(&mut cpg, guard, left_cmp);
    ast(&mut cpg, guard, right_cmp);
    ast(&mut cpg, left_cmp, left_use);
    ast(&mut cpg, left_cmp, left_literal);
    ast(&mut cpg, right_cmp, right_use);
    ast(&mut cpg, right_cmp, right_literal);
    cfg(&mut cpg, function_id, parameter_id, CfgEdgeKind::Sequential);
    cfg(&mut cpg, parameter_id, guard, CfgEdgeKind::Sequential);
    cfg(&mut cpg, guard, true_target, CfgEdgeKind::ConditionalTrue);
    cfg(&mut cpg, guard, false_target, CfgEdgeKind::ConditionalFalse);
    def_use(&mut cpg, parameter_id, left_use);
    def_use(&mut cpg, parameter_id, right_use);
    (cpg, guard, true_target, false_target)
}

fn constant_fixture(value: bool) -> (CodePropertyGraph, NodeId, NodeId, NodeId) {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function_id = add(&mut cpg, function());
    let parameter_id = add(&mut cpg, parameter("x"));
    let guard = add(
        &mut cpg,
        CpgNodeKind::Literal {
            kind: LiteralKind::Bool(value),
        },
    );
    let true_target = add(
        &mut cpg,
        CpgNodeKind::Block {
            scope: ScopeId::GLOBAL,
        },
    );
    let false_target = add(
        &mut cpg,
        CpgNodeKind::Block {
            scope: ScopeId::GLOBAL,
        },
    );
    for child in [parameter_id, guard, true_target, false_target] {
        ast(&mut cpg, function_id, child);
    }
    cfg(&mut cpg, function_id, parameter_id, CfgEdgeKind::Sequential);
    cfg(&mut cpg, parameter_id, guard, CfgEdgeKind::Sequential);
    cfg(&mut cpg, guard, true_target, CfgEdgeKind::ConditionalTrue);
    cfg(&mut cpg, guard, false_target, CfgEdgeKind::ConditionalFalse);
    (cpg, guard, true_target, false_target)
}

#[test]
fn guard_local_interval_state_includes_prior_same_block_assignments() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function_id = add(&mut cpg, function());
    let declaration = add(
        &mut cpg,
        CpgNodeKind::Variable {
            name: "x".into(),
            var_type: None,
            scope: ScopeId::GLOBAL,
            is_mutable: false,
        },
    );
    let binder = add(&mut cpg, identifier("x"));
    let initializer = add(&mut cpg, integer(7));
    let guard = add(
        &mut cpg,
        CpgNodeKind::BinaryOp {
            operator: ">".into(),
        },
    );
    let usage = add(&mut cpg, identifier("x"));
    let limit = add(&mut cpg, integer(5));
    let true_target = add(
        &mut cpg,
        CpgNodeKind::Block {
            scope: ScopeId::GLOBAL,
        },
    );
    let false_target = add(
        &mut cpg,
        CpgNodeKind::Block {
            scope: ScopeId::GLOBAL,
        },
    );
    for child in [declaration, guard, true_target, false_target] {
        ast(&mut cpg, function_id, child);
    }
    ast(&mut cpg, declaration, binder);
    ast(&mut cpg, declaration, initializer);
    ast(&mut cpg, guard, usage);
    ast(&mut cpg, guard, limit);
    cfg(&mut cpg, function_id, declaration, CfgEdgeKind::Sequential);
    cfg(&mut cpg, declaration, guard, CfgEdgeKind::Sequential);
    cfg(&mut cpg, guard, true_target, CfgEdgeKind::ConditionalTrue);
    cfg(&mut cpg, guard, false_target, CfgEdgeKind::ConditionalFalse);
    def_use(&mut cpg, declaration, usage);

    let analysis = analyze_infeasible_paths(&cpg, &InfeasiblePathConfig::default()).unwrap();
    assert!(analysis.is_complete(), "{analysis:#?}");
    let redundant = analysis
        .findings
        .iter()
        .find(|finding| finding.kind == InfeasiblePathKind::RedundantCondition { value: true })
        .unwrap();
    assert_eq!(
        redundant.entry_intervals,
        vec![("x".into(), Interval::singleton(7))]
    );
    assert!(analysis.findings.iter().any(|finding| {
        finding.kind
            == InfeasiblePathKind::InfeasibleBranch {
                branch: BranchOutcome::False,
                target: false_target,
            }
    }));
}

#[test]
fn correlated_x_greater_five_and_x_less_three_is_contradictory() {
    let (cpg, guard, true_target, _) = compound_fixture(
        "&&",
        ComparisonSpec {
            operator: ">",
            constant: 5,
        },
        ComparisonSpec {
            operator: "<",
            constant: 3,
        },
        false,
    );
    let before = (cpg.node_count(), cpg.edge_count());
    let analysis = analyze_infeasible_paths(&cpg, &InfeasiblePathConfig::default()).unwrap();
    assert!(analysis.is_complete(), "{analysis:#?}");
    assert_eq!(analysis.analyzed_functions, vec![NodeId::new(0)]);
    assert_eq!(analysis.analyzed_conditions.len(), 1);
    assert_eq!(analysis.findings.len(), 3, "{analysis:#?}");
    assert!(analysis.findings.iter().any(|finding| {
        finding.source == guard
            && finding.kind
                == InfeasiblePathKind::InfeasibleBranch {
                    branch: BranchOutcome::True,
                    target: true_target,
                }
            && finding.proof == FeasibilityProofSource::Intervals
    }));
    let contradiction = analysis
        .findings
        .iter()
        .find(|finding| finding.kind == InfeasiblePathKind::ContradictoryCondition)
        .unwrap();
    assert_eq!(contradiction.interval_contradictions.len(), 1);
    assert_eq!(contradiction.interval_contradictions[0].predicates.len(), 2);
    assert_eq!(
        contradiction.interval_contradictions[0]
            .predicates
            .iter()
            .map(|predicate| (&*predicate.operator, predicate.constant))
            .collect::<Vec<_>>(),
        vec![(">", 5), ("<", 3)]
    );
    assert!(analysis.findings.iter().any(|finding| {
        finding.kind == InfeasiblePathKind::RedundantCondition { value: false }
    }));
    assert_eq!(
        infeasible_paths(&cpg, &InfeasiblePathConfig::default()).unwrap(),
        analysis.findings
    );
    assert_eq!((cpg.node_count(), cpg.edge_count()), before);
}

#[test]
fn literal_true_is_a_redundant_guard_and_sccp_prunes_false_edge() {
    let (cpg, guard, _, false_target) = constant_fixture(true);
    let analysis = analyze_infeasible_paths(&cpg, &InfeasiblePathConfig::default()).unwrap();
    assert!(analysis.is_complete(), "{analysis:#?}");
    assert_eq!(analysis.findings.len(), 2, "{analysis:#?}");
    assert!(analysis.findings.iter().any(|finding| {
        finding.source == guard
            && finding.sccp_value == Some(true)
            && finding.kind
                == InfeasiblePathKind::InfeasibleBranch {
                    branch: BranchOutcome::False,
                    target: false_target,
                }
    }));
    assert!(analysis.findings.iter().any(|finding| {
        finding.kind == InfeasiblePathKind::RedundantCondition { value: true }
            && finding.proof == FeasibilityProofSource::SccpAndIntervals
    }));
    assert!(!analysis
        .findings
        .iter()
        .any(|finding| finding.kind == InfeasiblePathKind::ContradictoryCondition));
}

#[test]
fn complementary_disjunction_is_always_true_but_not_a_contradiction() {
    let (cpg, _, _, false_target) = compound_fixture(
        "||",
        ComparisonSpec {
            operator: "<",
            constant: 3,
        },
        ComparisonSpec {
            operator: ">=",
            constant: 3,
        },
        false,
    );
    let analysis = analyze_infeasible_paths(&cpg, &InfeasiblePathConfig::default()).unwrap();
    assert!(analysis.is_complete(), "{analysis:#?}");
    assert_eq!(analysis.findings.len(), 2, "{analysis:#?}");
    assert!(analysis.findings.iter().any(|finding| {
        finding.kind
            == InfeasiblePathKind::InfeasibleBranch {
                branch: BranchOutcome::False,
                target: false_target,
            }
    }));
    assert!(analysis
        .findings
        .iter()
        .any(|finding| { finding.kind == InfeasiblePathKind::RedundantCondition { value: true } }));
    assert!(!analysis
        .findings
        .iter()
        .any(|finding| finding.kind == InfeasiblePathKind::ContradictoryCondition));
}

#[test]
fn unknown_predicates_and_all_resource_boundaries_are_explicit() {
    let (mut unsupported, guard, _, _) = constant_fixture(true);
    unsupported.node_mut(guard).unwrap().kind = CpgNodeKind::Call {
        target: None,
        is_method: false,
    };
    let report = analyze_infeasible_paths(&unsupported, &InfeasiblePathConfig::default()).unwrap();
    assert!(!report.is_complete());
    assert!(report
        .issues
        .contains(&InfeasiblePathIssue::UnsupportedConditionNode {
            function: NodeId::new(0),
            condition: guard,
            node: guard,
        }));

    let (compound, guard, _, _) = compound_fixture(
        "||",
        ComparisonSpec {
            operator: "<",
            constant: 0,
        },
        ComparisonSpec {
            operator: ">",
            constant: 1,
        },
        false,
    );
    for (config, expected) in [
        (
            InfeasiblePathConfig {
                max_condition_nodes: 1,
                ..InfeasiblePathConfig::default()
            },
            "nodes",
        ),
        (
            InfeasiblePathConfig {
                max_condition_states: 1,
                ..InfeasiblePathConfig::default()
            },
            "states",
        ),
        (
            InfeasiblePathConfig {
                max_condition_steps: 1,
                ..InfeasiblePathConfig::default()
            },
            "steps",
        ),
    ] {
        let report = analyze_infeasible_paths(&compound, &config).unwrap();
        assert!(!report.is_complete());
        assert!(report.issues.iter().any(|issue| match (expected, issue) {
            ("nodes", InfeasiblePathIssue::ConditionNodeCap { condition, .. })
            | ("states", InfeasiblePathIssue::ConditionStateCap { condition, .. })
            | ("steps", InfeasiblePathIssue::ConditionStepCap { condition, .. }) => {
                *condition == guard
            }
            _ => false,
        }));
    }
}

#[test]
fn cyclic_condition_is_bounded_and_reported() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function_id = add(&mut cpg, function());
    let parameter_id = add(&mut cpg, parameter("x"));
    let guard = add(
        &mut cpg,
        CpgNodeKind::UnaryOp {
            operator: "!".into(),
        },
    );
    let true_target = add(
        &mut cpg,
        CpgNodeKind::Block {
            scope: ScopeId::GLOBAL,
        },
    );
    let false_target = add(
        &mut cpg,
        CpgNodeKind::Block {
            scope: ScopeId::GLOBAL,
        },
    );
    for child in [parameter_id, guard, true_target, false_target] {
        ast(&mut cpg, function_id, child);
    }
    ast(&mut cpg, guard, guard);
    cfg(&mut cpg, function_id, parameter_id, CfgEdgeKind::Sequential);
    cfg(&mut cpg, parameter_id, guard, CfgEdgeKind::Sequential);
    cfg(&mut cpg, guard, true_target, CfgEdgeKind::ConditionalTrue);
    cfg(&mut cpg, guard, false_target, CfgEdgeKind::ConditionalFalse);
    let report = analyze_infeasible_paths(&cpg, &InfeasiblePathConfig::default()).unwrap();
    assert!(report
        .issues
        .contains(&InfeasiblePathIssue::CyclicCondition {
            function: function_id,
            condition: guard,
            node: guard,
        }));
}

#[test]
fn invalid_configuration_is_rejected_before_graph_analysis() {
    for config in [
        InfeasiblePathConfig {
            max_condition_nodes: 0,
            ..InfeasiblePathConfig::default()
        },
        InfeasiblePathConfig {
            max_condition_states: MAX_CONDITION_STATES + 1,
            ..InfeasiblePathConfig::default()
        },
        InfeasiblePathConfig {
            max_condition_steps: MAX_CONDITION_STEPS + 1,
            ..InfeasiblePathConfig::default()
        },
    ] {
        assert!(matches!(
            analyze_infeasible_paths(&CodePropertyGraph::new(Language::Rust), &config),
            Err(AnalysisError::InvalidInput {
                analysis: "infeasible-path detection",
                ..
            })
        ));
    }
}

fn comparison_truth(operator: &str, value: i64, constant: i64) -> bool {
    match operator {
        "<" => value < constant,
        "<=" => value <= constant,
        ">" => value > constant,
        ">=" => value >= constant,
        "==" => value == constant,
        "!=" => value != constant,
        _ => unreachable!(),
    }
}

fn generated_formula(
    logical_or: bool,
    left_operator: &str,
    left_constant: i64,
    right_operator: &str,
    right_constant: i64,
) -> (CodePropertyGraph, NodeId) {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let root = add(
        &mut cpg,
        CpgNodeKind::BinaryOp {
            operator: if logical_or { "||" } else { "&&" }.into(),
        },
    );
    for (operator, constant) in [
        (left_operator, left_constant),
        (right_operator, right_constant),
    ] {
        let comparison = add(
            &mut cpg,
            CpgNodeKind::BinaryOp {
                operator: operator.into(),
            },
        );
        let usage = add(&mut cpg, identifier("x"));
        let literal = add(&mut cpg, integer(constant));
        ast(&mut cpg, root, comparison);
        ast(&mut cpg, comparison, usage);
        ast(&mut cpg, comparison, literal);
    }
    (cpg, root)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn interval_machine_matches_exhaustive_integer_oracle(
        logical_or in any::<bool>(),
        left_operator in prop::sample::select(vec!["<", "<=", ">", ">=", "==", "!="]),
        right_operator in prop::sample::select(vec!["<", "<=", ">", ">=", "==", "!="]),
        left_constant in -8i64..=8,
        right_constant in -8i64..=8,
        desired in any::<bool>(),
    ) {
        let (cpg, root) = generated_formula(
            logical_or,
            left_operator,
            left_constant,
            right_operator,
            right_constant,
        );
        let arena = build_condition_arena(&cpg, root, MAX_CONDITION_NODES).unwrap();
        let outcome = evaluate_condition(
            &arena,
            root,
            desired,
            &[("x".into(), Interval::top())],
            &BTreeMap::new(),
            &InfeasiblePathConfig::default(),
        ).unwrap();
        let expected = (-16i64..=16).any(|value| {
            let left = comparison_truth(left_operator, value, left_constant);
            let right = comparison_truth(right_operator, value, right_constant);
            (if logical_or { left || right } else { left && right }) == desired
        });
        prop_assert!(outcome.unknown_nodes.is_empty());
        prop_assert_eq!(outcome.possible, expected);
        prop_assert_eq!(outcome.is_impossible(), !expected);
    }

    #[test]
    fn report_is_insertion_order_independent_and_repeatable(reverse in any::<bool>()) {
        let spec = (
            ComparisonSpec { operator: ">", constant: 5 },
            ComparisonSpec { operator: "<", constant: 3 },
        );
        let (first, ..) = compound_fixture("&&", spec.0, spec.1, reverse);
        let (second, ..) = compound_fixture("&&", spec.0, spec.1, !reverse);
        let first_report = analyze_infeasible_paths(&first, &InfeasiblePathConfig::default()).unwrap();
        let second_report = analyze_infeasible_paths(&second, &InfeasiblePathConfig::default()).unwrap();
        let repeated = analyze_infeasible_paths(&first, &InfeasiblePathConfig::default()).unwrap();
        prop_assert_eq!(&first_report, &second_report);
        prop_assert_eq!(first_report, repeated);
    }
}

#[test]
fn one_hundred_thousand_nested_negations_use_a_small_native_stack() {
    thread::Builder::new()
        .stack_size(64 * 1024)
        .spawn(|| {
            let mut cpg = CodePropertyGraph::new(Language::Rust);
            let mut nodes = Vec::with_capacity(100_001);
            for _ in 0..100_000 {
                nodes.push(add(
                    &mut cpg,
                    CpgNodeKind::UnaryOp {
                        operator: "!".into(),
                    },
                ));
            }
            let leaf = add(
                &mut cpg,
                CpgNodeKind::Literal {
                    kind: LiteralKind::Bool(true),
                },
            );
            nodes.push(leaf);
            for pair in nodes.windows(2) {
                ast(&mut cpg, pair[0], pair[1]);
            }
            let root = nodes[0];
            let arena = build_condition_arena(&cpg, root, MAX_CONDITION_NODES).unwrap();
            let outcome = evaluate_condition(
                &arena,
                root,
                true,
                &[("x".into(), Interval::top())],
                &BTreeMap::new(),
                &InfeasiblePathConfig::default(),
            )
            .unwrap();
            assert!(outcome.possible);
            assert!(outcome.unknown_nodes.is_empty());
        })
        .unwrap()
        .join()
        .unwrap();
}

#[cfg(feature = "serde")]
#[test]
fn public_config_and_report_round_trip_through_json() {
    let config = InfeasiblePathConfig::default();
    let encoded = serde_json::to_string(&config).unwrap();
    assert_eq!(
        serde_json::from_str::<InfeasiblePathConfig>(&encoded).unwrap(),
        config
    );
    let (cpg, ..) = compound_fixture(
        "&&",
        ComparisonSpec {
            operator: ">",
            constant: 5,
        },
        ComparisonSpec {
            operator: "<",
            constant: 3,
        },
        false,
    );
    let report = analyze_infeasible_paths(&cpg, &config).unwrap();
    let encoded = serde_json::to_string(&report).unwrap();
    assert_eq!(
        serde_json::from_str::<InfeasiblePathAnalysis>(&encoded).unwrap(),
        report
    );
}
