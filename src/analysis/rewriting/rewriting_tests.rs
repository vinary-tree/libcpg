use proptest::prelude::*;

use super::*;

fn build_metta(source: &str) -> CodePropertyGraph {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_metta::LANGUAGE.into())
        .expect("set MeTTa grammar");
    let tree = parser.parse(source, None).expect("parse MeTTa fixture");
    crate::TreeSitterCpgBuilder::new()
        .build_from_tree(&tree, source, Language::MeTTa)
        .expect("build MeTTa CPG")
}

fn rule_name(cpg: &CodePropertyGraph, rule: NodeId) -> String {
    match &cpg.node(rule).expect("rule exists").kind {
        CpgNodeKind::Function { signature } => signature.name.to_string(),
        other => panic!("expected rule Function, got {other:?}"),
    }
}

fn named_pairs(cpg: &CodePropertyGraph, graph: &DpGraph) -> Vec<(String, String)> {
    let mut pairs: Vec<_> = graph
        .pairs
        .iter()
        .map(|pair| {
            (
                rule_name(cpg, pair.source_rule),
                rule_name(cpg, pair.target_rule),
            )
        })
        .collect();
    pairs = crate::analysis::scc::heap_sorted(pairs);
    pairs.dedup();
    pairs
}

#[test]
fn dependency_pairs_use_normalized_ast_and_existing_iterative_sccs() {
    let cpg = build_metta("(= (f (S $x)) (g $x))\n(= (g (S $x)) (f $x))\n");
    let graph = dependency_pairs(&cpg);

    assert!(graph.complete, "unexpected issues: {:?}", graph.issues);
    assert_eq!(
        named_pairs(&cpg, &graph),
        vec![("f".into(), "g".into()), ("g".into(), "f".into())]
    );
    assert_eq!(graph.projection.kind, ProjectionKind::RewriteDependency);
    assert_eq!(graph.sccs.cyclic_components().count(), 1);
    assert_eq!(graph.sccs.components[0].nodes.len(), 2);
}

#[test]
fn polynomial_reduction_pair_proves_decreasing_mutual_recursion() {
    let cpg = build_metta("(= (f (S $x)) (g $x))\n(= (g (S $x)) (f $x))\n");

    match terminates(&cpg) {
        TerminationVerdict::Terminates(proof) => {
            assert_eq!(proof.reduction_pairs.len(), 1);
            assert_eq!(proof.reduction_pairs[0].weakly_oriented_rules, 2);
            assert_eq!(proof.reduction_pairs[0].strictly_oriented_pairs, 2);
        }
        other => panic!("expected termination proof, got {other:?}"),
    }
}

#[test]
fn reduction_pair_requires_every_rewrite_rule_to_be_weakly_oriented() {
    let cpg = build_metta("(= (f (S $x)) (f $x))\n(= (expand $x) (pair $x $x))\n");

    match terminates(&cpg) {
        TerminationVerdict::Unknown(unknown) => {
            assert_eq!(unknown.unoriented_components.len(), 1);
            assert!(unknown.issues.is_empty());
        }
        other => panic!("expected an honest unknown verdict, got {other:?}"),
    }
}

#[test]
fn alpha_equivalent_self_loop_refutes_termination() {
    let cpg = build_metta("(= (loop $x) (loop $x))\n");

    match terminates(&cpg) {
        TerminationVerdict::DoesNotTerminate(witness) => {
            assert_eq!(witness.rule_cycle.len(), 2);
            assert_eq!(witness.rule_cycle[0], witness.rule_cycle[1]);
            assert_eq!(witness.call_sites.len(), 1);
        }
        other => panic!("expected nontermination witness, got {other:?}"),
    }
}

#[test]
fn unorientable_cycle_is_unknown_not_a_false_refutation() {
    let cpg = build_metta("(= (f $x) (g (S $x)))\n(= (g $x) (f $x))\n");

    match terminates(&cpg) {
        TerminationVerdict::Unknown(unknown) => {
            assert_eq!(unknown.unoriented_components.len(), 1);
            assert!(unknown.issues.is_empty());
        }
        other => panic!("expected an honest unknown verdict, got {other:?}"),
    }
}

#[test]
fn distinct_normal_forms_produce_critical_conflict() {
    let cpg = build_metta("(= (pick $x) A)\n(= (pick $x) B)\n");
    let analysis = analyze_critical_pairs(&cpg);

    assert!(
        analysis.complete,
        "unexpected issues: {:?}",
        analysis.issues
    );
    assert!(!analysis.pairs.is_empty());
    assert!(analysis
        .pairs
        .iter()
        .all(|pair| { pair.outer_reduct_fingerprint != pair.inner_reduct_fingerprint }));
}

#[test]
fn critical_branches_preserve_shared_variable_identity() {
    let cpg = build_metta("(= (swap $x $y) (pair $x $y))\n(= (swap $x $y) (pair $y $x))\n");
    let analysis = analyze_critical_pairs(&cpg);

    assert!(
        analysis.complete,
        "unexpected issues: {:?}",
        analysis.issues
    );
    assert!(
        !analysis.pairs.is_empty(),
        "joint normalization must not collapse pair(x,y) and pair(y,x)"
    );
}

#[test]
fn repeated_lhs_variables_share_identity_and_qualify_confluence() {
    let cpg = build_metta("(= (same $x $x) (same $x $x))\n");

    assert!(matches!(
        terminates(&cpg),
        TerminationVerdict::DoesNotTerminate(_)
    ));
    let critical = analyze_critical_pairs(&cpg);
    assert!(!critical.complete);
    assert!(critical.issues.iter().any(|issue| matches!(
        issue,
        RewriteAnalysisIssue::NonLeftLinearRule { variables, .. }
            if variables.len() == 1
    )));
}

#[test]
fn bounded_rewrite_search_disposes_of_joinable_overlap() {
    let cpg =
        build_metta("(= (join $x) (norm $x))\n(= (join $x) (step $x))\n(= (step $x) (norm $x))\n");
    let analysis = analyze_critical_pairs(&cpg);

    assert!(
        analysis.complete,
        "unexpected issues: {:?}",
        analysis.issues
    );
    assert!(analysis.pairs.is_empty());
}

#[test]
fn joinability_rewrites_preserve_critical_branch_variable_identity() {
    let cpg = build_metta(
        "(= (swap $x $y) (left $x $y))\n\
         (= (swap $x $y) (right $x $y))\n\
         (= (left $x $y) (pair $x $y))\n\
         (= (right $x $y) (pair $y $x))\n",
    );
    let analysis = analyze_critical_pairs(&cpg);

    assert!(
        analysis.complete,
        "unexpected issues: {:?}",
        analysis.issues
    );
    assert!(
        !analysis.pairs.is_empty(),
        "successor canonicalization must not independently rename the two branches"
    );
}

#[test]
fn non_root_and_self_overlaps_are_renamed_apart() {
    let cpg = build_metta("(= (f (f $x)) A)\n");
    let analysis = analyze_critical_pairs(&cpg);

    assert!(
        analysis.complete,
        "unexpected issues: {:?}",
        analysis.issues
    );
    assert_eq!(analysis.pairs.len(), 1);
    let rule = cpg.functions().next().expect("one rule").id;
    assert_eq!(analysis.pairs[0].outer_rule, rule);
    assert_eq!(analysis.pairs[0].inner_rule, rule);
    assert_ne!(analysis.pairs[0].overlap_site, rule);
}

#[test]
fn joinability_cap_is_reported_without_a_false_conflict() {
    let cpg = build_metta("(= (pick $x) A)\n(= (pick $x) B)\n");
    let analysis = analyze_critical_pairs_with_caps(
        &cpg,
        RewriteAnalysisCaps {
            max_joinability_states: 0,
            ..RewriteAnalysisCaps::default()
        },
    );

    assert!(!analysis.complete);
    assert!(analysis.pairs.is_empty());
    assert!(analysis
        .issues
        .contains(&RewriteAnalysisIssue::JoinabilityLimitExceeded { limit: 0 }));
}

#[test]
fn rewrite_overlay_is_typed_idempotent_and_transactional() {
    let mut cpg = build_metta("(= (pick $x) A)\n(= (pick $x) B)\n");
    let dependencies = dependency_pairs(&cpg);
    let critical = analyze_critical_pairs(&cpg);

    let added = materialize_rewrite_dep_overlay(&mut cpg, &dependencies, &critical)
        .expect("valid endpoints");
    assert!(added > 0);
    assert_eq!(cpg.stats().rewrite_dep_edges, added);
    assert!(cpg
        .rewrite_dep_edges()
        .all(|edge| edge.kind.is_rewrite_dep()));
    assert_eq!(
        materialize_rewrite_dep_overlay(&mut cpg, &dependencies, &critical),
        Ok(0)
    );

    let before = cpg.edge_count();
    let invalid = DpGraph {
        rules: Vec::new(),
        pairs: vec![DependencyPair {
            source_rule: NodeId::new(u32::MAX - 1),
            target_rule: NodeId::new(u32::MAX),
            call_site: NodeId::new(0),
        }],
        projection: GraphProjection::from_edges(ProjectionKind::RewriteDependency, [], []),
        sccs: projection_sccs(&GraphProjection::from_edges(
            ProjectionKind::RewriteDependency,
            [],
            [],
        )),
        complete: true,
        issues: Vec::new(),
    };
    assert_eq!(
        materialize_rewrite_dep_overlay(&mut cpg, &invalid, &critical),
        Err(RewriteDepOverlayError::UnknownSource {
            rule: NodeId::new(u32::MAX - 1)
        })
    );
    assert_eq!(cpg.edge_count(), before);

    let non_rule = cpg
        .nodes()
        .find(|node| !matches!(node.kind, CpgNodeKind::Function { .. }))
        .expect("fixture contains syntax nodes")
        .id;
    let rule = cpg.functions().next().expect("fixture contains rules").id;
    let typed_invalid = DpGraph {
        pairs: vec![DependencyPair {
            source_rule: non_rule,
            target_rule: rule,
            call_site: non_rule,
        }],
        ..invalid
    };
    assert_eq!(
        materialize_rewrite_dep_overlay(&mut cpg, &typed_invalid, &critical),
        Err(RewriteDepOverlayError::SourceNotRule { rule: non_rule })
    );
    assert_eq!(cpg.edge_count(), before);
}

#[test]
fn zero_caps_are_explicit_instead_of_silently_truncating() {
    let cpg = build_metta("(= (loop $x) (loop $x))\n");
    let graph = dependency_pairs_with_caps(
        &cpg,
        RewriteAnalysisCaps {
            max_rules: 0,
            ..RewriteAnalysisCaps::default()
        },
    );

    assert!(!graph.complete);
    assert_eq!(graph.rules, Vec::<NodeId>::new());
    assert!(graph
        .issues
        .contains(&RewriteAnalysisIssue::RuleLimitExceeded { limit: 0 }));
}

#[test]
fn every_dependency_resource_cap_is_explicit() {
    let cpg = build_metta("(= (loop $x) (loop $x))\n");
    let cases = [
        (
            RewriteAnalysisCaps {
                max_term_nodes: 0,
                ..RewriteAnalysisCaps::default()
            },
            RewriteAnalysisIssue::TermNodeLimitExceeded { limit: 0 },
        ),
        (
            RewriteAnalysisCaps {
                max_dependency_pairs: 0,
                ..RewriteAnalysisCaps::default()
            },
            RewriteAnalysisIssue::DependencyPairLimitExceeded { limit: 0 },
        ),
        (
            RewriteAnalysisCaps {
                max_unification_steps: 0,
                ..RewriteAnalysisCaps::default()
            },
            RewriteAnalysisIssue::UnificationLimitExceeded { limit: 0 },
        ),
    ];

    for (caps, expected) in cases {
        let graph = dependency_pairs_with_caps(&cpg, caps);
        assert!(!graph.complete);
        assert!(graph.issues.contains(&expected), "missing {expected:?}");
    }
}

#[test]
fn critical_pair_caps_are_independent_from_dependency_pair_caps() {
    let cpg = build_metta("(= (pick $x) A)\n(= (pick $x) B)\n");
    let independent = analyze_critical_pairs_with_caps(
        &cpg,
        RewriteAnalysisCaps {
            max_dependency_pairs: 0,
            ..RewriteAnalysisCaps::default()
        },
    );
    assert!(independent.complete);
    assert!(!independent.pairs.is_empty());

    let bounded = analyze_critical_pairs_with_caps(
        &cpg,
        RewriteAnalysisCaps {
            max_critical_pairs: 0,
            ..RewriteAnalysisCaps::default()
        },
    );
    assert!(!bounded.complete);
    assert!(bounded.pairs.is_empty());
    assert!(bounded
        .issues
        .contains(&RewriteAnalysisIssue::CriticalPairLimitExceeded { limit: 0 }));
}

#[test]
fn non_metta_graph_is_explicitly_incomplete() {
    let cpg = CodePropertyGraph::new(Language::Rust);
    let graph = dependency_pairs(&cpg);
    let critical = analyze_critical_pairs(&cpg);

    assert!(!graph.complete);
    assert_eq!(
        graph.issues,
        vec![RewriteAnalysisIssue::UnsupportedLanguage {
            language: "Rust".to_owned()
        }]
    );
    assert!(!critical.complete);
    assert!(critical.pairs.is_empty());
    assert_eq!(critical.issues, graph.issues);
}

proptest! {
    #[test]
    fn dependency_pairs_are_deterministic_under_rule_permutation(
        priorities in prop::collection::vec(any::<u8>(), 1..24)
    ) {
        let count = priorities.len();
        let mut order: Vec<_> = priorities.into_iter().enumerate().map(|(index, priority)| {
            (priority, index)
        }).collect();
        order = crate::analysis::scc::heap_sorted(order);

        let canonical = (0..=count).map(|index| {
            if index == count {
                format!("(= r{index} Done)\n")
            } else {
                format!("(= r{index} r{})\n", index + 1)
            }
        }).collect::<String>();
        let permuted = order.into_iter().map(|(_, index)| {
            format!("(= r{index} r{})\n", index + 1)
        }).chain(std::iter::once(format!("(= r{count} Done)\n"))).collect::<String>();

        let canonical_cpg = build_metta(&canonical);
        let permuted_cpg = build_metta(&permuted);
        let canonical_graph = dependency_pairs(&canonical_cpg);
        let permuted_graph = dependency_pairs(&permuted_cpg);

        prop_assert!(canonical_graph.complete);
        prop_assert!(permuted_graph.complete);
        prop_assert_eq!(
            named_pairs(&canonical_cpg, &canonical_graph),
            named_pairs(&permuted_cpg, &permuted_graph)
        );
    }
}

#[test]
fn occurs_check_rejects_a_variable_containing_itself_without_recursion() {
    let left_variable = NodeId::new(7);
    let right_variable = NodeId::new(8);
    let left = FlatTerm {
        nodes: vec![
            FlatNode {
                symbol: FlatSymbol::Identifier(Arc::from("f")),
                source: NodeId::new(1),
                arity: 2,
                subtree_end: 3,
            },
            FlatNode {
                symbol: FlatSymbol::Variable(u64::from(left_variable.as_u32())),
                source: left_variable,
                arity: 0,
                subtree_end: 2,
            },
            FlatNode {
                symbol: FlatSymbol::Variable(u64::from(left_variable.as_u32())),
                source: left_variable,
                arity: 0,
                subtree_end: 3,
            },
        ],
    };
    let right = FlatTerm {
        nodes: vec![
            FlatNode {
                symbol: FlatSymbol::Identifier(Arc::from("f")),
                source: NodeId::new(2),
                arity: 2,
                subtree_end: 4,
            },
            FlatNode {
                symbol: FlatSymbol::Variable(u64::from(right_variable.as_u32())),
                source: right_variable,
                arity: 0,
                subtree_end: 2,
            },
            FlatNode {
                symbol: FlatSymbol::Identifier(Arc::from("g")),
                source: NodeId::new(3),
                arity: 1,
                subtree_end: 4,
            },
            FlatNode {
                symbol: FlatSymbol::Variable(u64::from(right_variable.as_u32())),
                source: right_variable,
                arity: 0,
                subtree_end: 4,
            },
        ],
    };
    let mut steps = 0;

    assert!(matches!(
        unify(&left, 0, &right, 0, &mut steps, 100),
        UnifyResult::NotUnified
    ));
}

#[test]
fn unification_and_structural_comparison_are_stack_safe_at_extreme_depth() {
    const DEPTH: usize = 100_000;

    std::thread::Builder::new()
        .name("rewrite-unification-small-stack".into())
        .stack_size(64 * 1024)
        .spawn(|| {
            let nested = |leaf: FlatSymbol| {
                let mut nodes = Vec::with_capacity(DEPTH + 1);
                for index in 0..DEPTH {
                    nodes.push(FlatNode {
                        symbol: FlatSymbol::Identifier(Arc::from("f")),
                        source: NodeId::new(u32::try_from(index).expect("depth fits NodeId")),
                        arity: 1,
                        subtree_end: u32::try_from(DEPTH + 1).expect("depth fits flat arena"),
                    });
                }
                nodes.push(FlatNode {
                    symbol: leaf,
                    source: NodeId::new(u32::try_from(DEPTH).expect("depth fits NodeId")),
                    arity: 0,
                    subtree_end: u32::try_from(DEPTH + 1).expect("depth fits flat arena"),
                });
                FlatTerm { nodes }
            };
            let left = nested(FlatSymbol::Identifier(Arc::from("leaf")));
            let right = nested(FlatSymbol::Variable(1));
            let mut steps = 0;

            assert!(matches!(
                unify(&left, 0, &right, 0, &mut steps, 1_000_000),
                UnifyResult::Unified(_)
            ));
            assert!(!alpha_equivalent(&left, 0, &right, 0));
            assert_ne!(fingerprint(&left), fingerprint(&right));
        })
        .expect("spawn small-stack unification worker")
        .join()
        .expect("small-stack unification worker completed");
}
