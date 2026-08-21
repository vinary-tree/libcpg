use std::thread;

use proptest::prelude::*;

use super::*;
use crate::{CfgEdgeKind, CpgNode, Language, MethodSignature, ScopeId, SourceRange, Visibility};

fn function(name: &str) -> CpgNodeKind {
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

fn rename_fixture(anomalies: usize, reverse_nodes: bool) -> CodePropertyGraph {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let left = NodeId::new(0);
    let right = NodeId::new(100);
    let mut nodes = vec![node(0, function("left")), node(100, function("right"))];
    for index in 0..10u32 {
        nodes.push(node(index + 1, identifier("source")));
        nodes.push(node(
            index + 101,
            identifier(if (index as usize) < anomalies {
                "typo"
            } else {
                "target"
            }),
        ));
    }
    if reverse_nodes {
        nodes.reverse();
    }
    for node in nodes {
        cpg.add_node_with_id(node);
    }
    for index in 0..10u32 {
        ast(&mut cpg, left, NodeId::new(index + 1));
        ast(&mut cpg, right, NodeId::new(index + 101));
    }
    cpg
}

fn guard_fixture(left_operator: &str, right_operator: &str) -> CodePropertyGraph {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let left = add(&mut cpg, function("left"));
    let left_guard = add(
        &mut cpg,
        CpgNodeKind::BinaryOp {
            operator: left_operator.into(),
        },
    );
    let left_name = add(&mut cpg, identifier("x"));
    let left_limit = add(&mut cpg, integer(5));
    let left_true = add(
        &mut cpg,
        CpgNodeKind::Block {
            scope: ScopeId::GLOBAL,
        },
    );
    let left_false = add(
        &mut cpg,
        CpgNodeKind::Block {
            scope: ScopeId::GLOBAL,
        },
    );
    for child in [left_guard, left_true, left_false] {
        ast(&mut cpg, left, child);
    }
    ast(&mut cpg, left_guard, left_name);
    ast(&mut cpg, left_guard, left_limit);
    cfg(
        &mut cpg,
        left_guard,
        left_true,
        CfgEdgeKind::ConditionalTrue,
    );
    cfg(
        &mut cpg,
        left_guard,
        left_false,
        CfgEdgeKind::ConditionalFalse,
    );

    let right = add(&mut cpg, function("right"));
    let right_guard = add(
        &mut cpg,
        CpgNodeKind::BinaryOp {
            operator: right_operator.into(),
        },
    );
    let right_name = add(&mut cpg, identifier("x"));
    let right_limit = add(&mut cpg, integer(5));
    let right_true = add(
        &mut cpg,
        CpgNodeKind::Block {
            scope: ScopeId::GLOBAL,
        },
    );
    let right_false = add(
        &mut cpg,
        CpgNodeKind::Block {
            scope: ScopeId::GLOBAL,
        },
    );
    for child in [right_guard, right_true, right_false] {
        ast(&mut cpg, right, child);
    }
    ast(&mut cpg, right_guard, right_name);
    ast(&mut cpg, right_guard, right_limit);
    cfg(
        &mut cpg,
        right_guard,
        right_true,
        CfgEdgeKind::ConditionalTrue,
    );
    cfg(
        &mut cpg,
        right_guard,
        right_false,
        CfgEdgeKind::ConditionalFalse,
    );
    cpg
}

#[test]
fn one_of_ten_identifier_renames_is_reported_with_exact_support() {
    let cpg = rename_fixture(1, false);
    let analysis = analyze_inconsistent_clones(&cpg, &CloneBugConfig::default()).unwrap();
    assert!(analysis.is_complete());
    assert_eq!(analysis.analyzed_pairs.len(), 1);
    assert_eq!(analysis.analyzed_pairs[0].clone_type, CloneType::Type2);
    assert!(analysis.analyzed_pairs[0].is_low_edit);
    assert_eq!(analysis.analyzed_pairs[0].structural_edits, 0);
    assert_eq!(analysis.findings.len(), 1);
    assert_eq!(
        analysis.findings[0].kind,
        InconsistentCloneKind::IdentifierRename {
            direction: RenameDirection::LeftToRight,
            left_name: "source".into(),
            right_name: "typo".into(),
            expected: "target".into(),
            support: PopulationFraction::new(9, 10),
        }
    );
    assert_eq!(analysis.findings[0].left_node, NodeId::new(1));
    assert_eq!(analysis.findings[0].right_node, NodeId::new(101));
    assert_eq!(
        inconsistent_clone_bugs(&cpg, &CloneBugConfig::default()).unwrap(),
        analysis.findings
    );
}

#[test]
fn rename_support_boundary_is_exact_and_never_uses_floating_point() {
    let accepted =
        analyze_inconsistent_clones(&rename_fixture(2, false), &CloneBugConfig::default()).unwrap();
    assert_eq!(accepted.findings.len(), 2);
    assert!(accepted.findings.iter().all(|finding| {
        matches!(
            finding.kind,
            InconsistentCloneKind::IdentifierRename {
                support: PopulationFraction {
                    supporting: 8,
                    population: 10
                },
                ..
            }
        )
    }));

    let rejected =
        analyze_inconsistent_clones(&rename_fixture(3, false), &CloneBugConfig::default()).unwrap();
    assert!(rejected.findings.is_empty());
    assert!(rejected.analyzed_pairs[0].is_low_edit);
}

#[test]
fn aligned_guard_operator_flip_is_reported_as_typed_evidence() {
    let cpg = guard_fixture(">", "<");
    let analysis = analyze_inconsistent_clones(&cpg, &CloneBugConfig::default()).unwrap();
    assert!(analysis.is_complete());
    assert_eq!(analysis.findings.len(), 1);
    assert_eq!(analysis.findings[0].clone_type, CloneType::Type2);
    assert_eq!(
        analysis.findings[0].kind,
        InconsistentCloneKind::GuardDivergence {
            left: CloneGuardValue::BinaryOperator(">".into()),
            right: CloneGuardValue::BinaryOperator("<".into()),
        }
    );
}

#[test]
fn equal_guards_and_consistent_renames_are_not_reported() {
    assert!(
        analyze_inconsistent_clones(&guard_fixture("<", "<"), &CloneBugConfig::default())
            .unwrap()
            .findings
            .is_empty()
    );
    assert!(
        analyze_inconsistent_clones(&rename_fixture(0, false), &CloneBugConfig::default())
            .unwrap()
            .findings
            .is_empty()
    );
}

#[test]
fn type3_insertion_is_evaluated_once_with_exact_edit_fraction() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let left = add(&mut cpg, function("same"));
    let right = add(&mut cpg, function("same"));
    for name in ["a", "b", "c", "d"] {
        let left_child = add(&mut cpg, identifier(name));
        let right_child = add(&mut cpg, identifier(name));
        ast(&mut cpg, left, left_child);
        ast(&mut cpg, right, right_child);
    }
    let inserted = add(&mut cpg, CpgNodeKind::Return);
    ast(&mut cpg, right, inserted);

    let analysis = analyze_inconsistent_clones(&cpg, &CloneBugConfig::default()).unwrap();
    assert!(analysis.is_complete());
    assert_eq!(
        analysis.analyzed_pairs.len(),
        1,
        "pair relation is not duplicated"
    );
    assert_eq!(analysis.analyzed_pairs[0].clone_type, CloneType::Type3);
    assert_eq!(analysis.analyzed_pairs[0].structural_edits, 1);
    assert_eq!(analysis.analyzed_pairs[0].structural_size, 6);
    assert!(analysis.analyzed_pairs[0].is_low_edit);
    assert!(analysis.findings.is_empty());
}

#[test]
fn malformed_and_bounded_evidence_is_explicit() {
    let mut cyclic = rename_fixture(1, false);
    cyclic.connect(NodeId::new(1), NodeId::new(0), CpgEdgeKind::AstChild);
    cyclic.connect(NodeId::new(101), NodeId::new(100), CpgEdgeKind::AstChild);
    let cycle = analyze_inconsistent_clones(&cyclic, &CloneBugConfig::default()).unwrap();
    assert!(!cycle.is_complete());
    assert!(cycle.issues.contains(&CloneBugIssue::CyclicAst {
        function: NodeId::new(0),
        node: NodeId::new(0),
    }));

    let capped = analyze_inconsistent_clones(
        &rename_fixture(1, false),
        &CloneBugConfig {
            ged_options: GedOptions {
                max_nodes: 1,
                ..GedOptions::default()
            },
            ..CloneBugConfig::default()
        },
    )
    .unwrap();
    assert_eq!(
        capped.issues,
        BTreeSet::from([CloneBugIssue::GedCapped {
            left_function: NodeId::new(0),
            right_function: NodeId::new(100),
        }])
    );
    assert!(capped.analyzed_pairs.is_empty());

    let mut unequal = rename_fixture(1, false);
    let extra = add(&mut unequal, CpgNodeKind::Return);
    ast(&mut unequal, NodeId::new(100), extra);
    let alignment_cap = analyze_inconsistent_clones(
        &unequal,
        &CloneBugConfig {
            clone_options: CloneOptions {
                type3_threshold: 0.0,
                ..CloneOptions::default()
            },
            max_alignment_cells: 1,
            ..CloneBugConfig::default()
        },
    )
    .unwrap();
    assert!(matches!(
        alignment_cap.issues.iter().next(),
        Some(CloneBugIssue::AlignmentCellCap { cap: 1, .. })
    ));
}

#[test]
fn invalid_configuration_is_rejected_before_analysis() {
    for config in [
        CloneBugConfig {
            maximum_structural_edit_fraction: PopulationThreshold::new(1, 0),
            ..CloneBugConfig::default()
        },
        CloneBugConfig {
            minimum_rename_consistency: PopulationThreshold::new(2, 1),
            ..CloneBugConfig::default()
        },
        CloneBugConfig {
            minimum_rename_occurrences: 1,
            ..CloneBugConfig::default()
        },
        CloneBugConfig {
            max_ast_nodes: 0,
            ..CloneBugConfig::default()
        },
        CloneBugConfig {
            max_alignment_cells: MAX_CLONE_ALIGNMENT_CELLS + 1,
            ..CloneBugConfig::default()
        },
        CloneBugConfig {
            ged_options: GedOptions {
                edge_ins_del: f64::NAN,
                ..GedOptions::default()
            },
            ..CloneBugConfig::default()
        },
    ] {
        assert!(matches!(
            analyze_inconsistent_clones(&CodePropertyGraph::new(Language::Rust), &config),
            Err(AnalysisError::InvalidInput {
                analysis: "inconsistent-clone detection",
                ..
            })
        ));
    }
}

fn reference_distance(left: &[AstToken], right: &[AstToken]) -> usize {
    let mut previous: Vec<_> = (0..=right.len()).collect();
    let mut current = vec![0usize; right.len() + 1];
    for (row, left) in left.iter().enumerate() {
        current[0] = row + 1;
        for (column, right) in right.iter().enumerate() {
            current[column + 1] = (previous[column] + usize::from(!same_tag(left, right)))
                .min(previous[column + 1] + 1)
                .min(current[column] + 1);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[right.len()]
}

fn tape(values: &[(bool, u8)], base: u32) -> AstTape {
    AstTape {
        tokens: values
            .iter()
            .enumerate()
            .map(|(index, &(tag, arity))| AstToken {
                node: NodeId::new(base + index as u32),
                tag: if tag {
                    NodeKindTag::If
                } else {
                    NodeKindTag::While
                },
                child_count: usize::from(arity),
            })
            .collect(),
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn bounded_alignment_matches_independent_two_row_oracle(
        left in prop::collection::vec((any::<bool>(), 0u8..4), 0..24),
        right in prop::collection::vec((any::<bool>(), 0u8..4), 0..24),
    ) {
        let left = tape(&left, 1);
        let right = tape(&right, 10_000);
        let actual = align_tapes(&left, &right, MAX_CLONE_ALIGNMENT_CELLS).unwrap();
        prop_assert_eq!(actual.edits, reference_distance(&left.tokens, &right.tokens));
        prop_assert_eq!(actual.size, left.tokens.len().max(right.tokens.len()));
        prop_assert!(actual.pairs.len() <= left.tokens.len().min(right.tokens.len()));
    }

    #[test]
    fn findings_are_insertion_order_independent_and_repeatable(anomalies in 0usize..4) {
        let forward = analyze_inconsistent_clones(
            &rename_fixture(anomalies, false),
            &CloneBugConfig::default(),
        ).unwrap();
        let reversed = analyze_inconsistent_clones(
            &rename_fixture(anomalies, true),
            &CloneBugConfig::default(),
        ).unwrap();
        let repeated = analyze_inconsistent_clones(
            &rename_fixture(anomalies, false),
            &CloneBugConfig::default(),
        ).unwrap();
        prop_assert_eq!(&forward, &reversed);
        prop_assert_eq!(forward, repeated);
    }
}

#[test]
fn one_hundred_thousand_deep_ast_uses_no_native_recursion() {
    thread::Builder::new()
        .stack_size(64 * 1024)
        .spawn(|| {
            let mut cpg = CodePropertyGraph::new(Language::Rust);
            let root = add(&mut cpg, function("deep"));
            let mut parent = root;
            for _ in 0..100_000 {
                let child = add(
                    &mut cpg,
                    CpgNodeKind::UnaryOp {
                        operator: "!".into(),
                    },
                );
                ast(&mut cpg, parent, child);
                parent = child;
            }
            let tape = ast_tape(&cpg, root, MAX_CLONE_BUG_AST_NODES).unwrap();
            assert_eq!(tape.tokens.len(), 100_001);
        })
        .unwrap()
        .join()
        .unwrap();
}

#[cfg(feature = "serde")]
#[test]
fn public_configuration_and_report_round_trip_through_json() {
    let config = CloneBugConfig::default();
    let encoded = serde_json::to_string(&config).unwrap();
    assert_eq!(
        serde_json::from_str::<CloneBugConfig>(&encoded).unwrap(),
        config
    );

    let analysis =
        analyze_inconsistent_clones(&rename_fixture(1, false), &CloneBugConfig::default()).unwrap();
    let encoded = serde_json::to_string(&analysis).unwrap();
    assert_eq!(
        serde_json::from_str::<CloneBugAnalysis>(&encoded).unwrap(),
        analysis
    );
}
