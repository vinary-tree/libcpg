use std::collections::BTreeSet;
use std::sync::Arc;

use proptest::prelude::*;

use super::*;
use crate::analysis::MAX_WL_ITERATIONS;
use crate::{
    CodePropertyGraph, CpgEdgeKind, CpgNode, CpgNodeKind, Language, MethodSignature, NodeId,
    ScopeId, SourceRange, Visibility,
};

fn node(kind: CpgNodeKind) -> CpgNode {
    CpgNode::new(NodeId::new(0), kind, SourceRange::default())
}

fn function_kind(name: &str) -> CpgNodeKind {
    CpgNodeKind::Function {
        signature: Box::new(MethodSignature {
            name: Arc::from(name),
            params: Default::default(),
            return_type: None,
            is_static: false,
            is_async: false,
            visibility: Visibility::Private,
        }),
    }
}

fn add_pdg_function(cpg: &mut CodePropertyGraph, name: &str, edges: &[(usize, usize)]) -> NodeId {
    let root = cpg.add_node(node(function_kind(name)));
    let mut nodes = vec![root];
    for index in 0..4 {
        let kind = match index {
            0 => CpgNodeKind::If,
            1 => CpgNodeKind::Call {
                target: None,
                is_method: false,
            },
            2 => CpgNodeKind::Assignment {
                operator: Arc::from("="),
            },
            _ => CpgNodeKind::Return,
        };
        let id = cpg.add_node(node(kind).with_parent(root));
        cpg.connect(root, id, CpgEdgeKind::AstChild)
            .expect("test function AST endpoints exist");
        nodes.push(id);
    }
    for &(source, target) in edges {
        cpg.connect(nodes[source], nodes[target], CpgEdgeKind::ControlDependence)
            .expect("test function PDG endpoints exist");
    }
    root
}

fn corpus() -> (CodePropertyGraph, NodeId, NodeId, NodeId) {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let shape = [(0, 1), (1, 2), (1, 3), (3, 4)];
    let seed = add_pdg_function(&mut cpg, "vulnerable", &shape);
    let unrelated = add_pdg_function(&mut cpg, "unrelated", &[(0, 1), (0, 2), (0, 3), (0, 4)]);
    let planted = add_pdg_function(&mut cpg, "planted_near_duplicate", &shape);
    (cpg, seed, unrelated, planted)
}

#[test]
fn planted_near_duplicate_ranks_first_with_wl_and_vf2_evidence() {
    let (cpg, seed, unrelated, planted) = corpus();
    let shape = (cpg.node_count(), cpg.edge_count());
    let options = ExtrapolationOptions {
        minimum_similarity: 0.0,
        ..ExtrapolationOptions::default()
    };

    let analysis = analyze_vulnerability_extrapolation(&cpg, seed, &options);

    assert!(analysis.is_complete(), "{analysis:#?}");
    assert_eq!(analysis.candidates[0].function, planted);
    assert_eq!(analysis.candidates[0].score, 1.0);
    assert_eq!(analysis.candidates[0].wl_similarity, 1.0);
    assert!(analysis.candidates[0].vf2_confirmed);
    assert!(analysis
        .candidates
        .iter()
        .any(|candidate| candidate.function == unrelated && candidate.score < 1.0));
    assert_eq!(
        extrapolate(&cpg, seed, &options),
        analysis
            .candidates
            .iter()
            .map(|candidate| (candidate.function, candidate.score))
            .collect::<Vec<_>>()
    );
    assert_eq!((cpg.node_count(), cpg.edge_count()), shape);
}

#[test]
fn invalid_seed_and_options_are_explicit_and_total() {
    let (mut cpg, seed, _, _) = corpus();
    let not_function = cpg.add_node(node(CpgNodeKind::Return));
    let unknown = NodeId::new(999_999);

    let missing =
        analyze_vulnerability_extrapolation(&cpg, unknown, &ExtrapolationOptions::default());
    assert_eq!(
        missing.issues,
        BTreeSet::from([ExtrapolationIssue::UnknownSeed { seed: unknown }])
    );

    let wrong_kind =
        analyze_vulnerability_extrapolation(&cpg, not_function, &ExtrapolationOptions::default());
    assert_eq!(
        wrong_kind.issues,
        BTreeSet::from([ExtrapolationIssue::SeedNotFunction { seed: not_function }])
    );

    let invalid = analyze_vulnerability_extrapolation(
        &cpg,
        seed,
        &ExtrapolationOptions {
            minimum_similarity: f64::NAN,
            wl_iterations: MAX_WL_ITERATIONS + 1,
            ..ExtrapolationOptions::default()
        },
    );
    assert!(invalid.candidates.is_empty());
    assert_eq!(invalid.issues.len(), 2, "{invalid:#?}");
    assert!(!invalid.is_complete());
}

#[test]
fn every_truncating_boundary_is_visible_in_completeness_evidence() {
    let (cpg, seed, _, _) = corpus();
    let functions = analyze_vulnerability_extrapolation(
        &cpg,
        seed,
        &ExtrapolationOptions {
            max_functions: 1,
            minimum_similarity: 0.0,
            ..ExtrapolationOptions::default()
        },
    );
    assert!(functions
        .issues
        .contains(&ExtrapolationIssue::FunctionsCapped {
            observed: 2,
            cap: 1,
        }));

    let pdg = analyze_vulnerability_extrapolation(
        &cpg,
        seed,
        &ExtrapolationOptions {
            max_pdg_nodes: 4,
            ..ExtrapolationOptions::default()
        },
    );
    assert!(pdg.issues.contains(&ExtrapolationIssue::PdgNodesCapped {
        function: seed,
        observed: 5,
        cap: 4,
    }));

    let vf2 = analyze_vulnerability_extrapolation(
        &cpg,
        seed,
        &ExtrapolationOptions {
            minimum_similarity: 0.0,
            vf2_threshold: 0.0,
            max_vf2_invocations: 0,
            ..ExtrapolationOptions::default()
        },
    );
    assert!(vf2
        .issues
        .contains(&ExtrapolationIssue::Vf2InvocationsCapped { needed: 2, cap: 0 }));

    let results = analyze_vulnerability_extrapolation(
        &cpg,
        seed,
        &ExtrapolationOptions {
            minimum_similarity: 0.0,
            max_results: 0,
            ..ExtrapolationOptions::default()
        },
    );
    assert!(results.candidates.is_empty());
    assert!(results.issues.contains(&ExtrapolationIssue::ResultsCapped {
        observed: 2,
        cap: 0
    }));
}

#[test]
fn deep_ast_scope_is_stack_safe_on_a_small_native_stack() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let seed = cpg.add_node(node(function_kind("deep_seed")));
    let mut parent = seed;
    for _ in 0..100_000 {
        let child = cpg.add_node(
            node(CpgNodeKind::Block {
                scope: ScopeId::GLOBAL,
            })
            .with_parent(parent),
        );
        cpg.connect(parent, child, CpgEdgeKind::AstChild)
            .expect("deep AST endpoints exist");
        parent = child;
    }
    add_pdg_function(&mut cpg, "candidate", &[]);

    std::thread::Builder::new()
        .stack_size(64 * 1024)
        .spawn(move || {
            let analysis = analyze_vulnerability_extrapolation(
                &cpg,
                seed,
                &ExtrapolationOptions {
                    minimum_similarity: 0.0,
                    max_pdg_nodes: 5,
                    ..ExtrapolationOptions::default()
                },
            );
            assert!(analysis.is_complete(), "{analysis:#?}");
            assert_eq!(analysis.analyzed_functions.len(), 1);
        })
        .expect("spawn extrapolation small-stack test")
        .join()
        .expect("iterative extrapolation completes on a small native stack");
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn ranking_is_deterministic_and_scores_are_bounded(
        candidate_shapes in prop::collection::vec(prop::collection::vec(any::<bool>(), 4), 0..12),
        threshold in 0u16..=1_000,
        rounds in 0usize..=MAX_WL_ITERATIONS,
    ) {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let seed = add_pdg_function(&mut cpg, "seed", &[(0, 1), (1, 2), (1, 3), (3, 4)]);
        for (index, shape) in candidate_shapes.iter().enumerate() {
            let edges = shape
                .iter()
                .enumerate()
                .filter(|(_, present)| **present)
                .map(|(target, _)| (0, target + 1))
                .collect::<Vec<_>>();
            add_pdg_function(&mut cpg, &format!("candidate_{index}"), &edges);
        }
        let options = ExtrapolationOptions {
            minimum_similarity: f64::from(threshold) / 1_000.0,
            wl_iterations: rounds,
            ..ExtrapolationOptions::default()
        };

        let first = analyze_vulnerability_extrapolation(&cpg, seed, &options);
        let second = analyze_vulnerability_extrapolation(&cpg, seed, &options);
        prop_assert_eq!(&first, &second);
        let scores_are_bounded = first.candidates.iter().all(|candidate| {
            candidate.score.is_finite()
                && (0.0..=1.0).contains(&candidate.score)
                && candidate.wl_similarity.is_finite()
                && (0.0..=1.0).contains(&candidate.wl_similarity)
        });
        prop_assert!(scores_are_bounded);
        let ranking_is_ordered = first.candidates.windows(2).all(|pair| {
            pair[0].score > pair[1].score
                || (pair[0].score == pair[1].score
                    && (pair[0].vf2_confirmed || !pair[1].vf2_confirmed))
        });
        prop_assert!(ranking_is_ordered);
    }
}

#[cfg(feature = "serde")]
#[test]
fn options_and_report_round_trip_with_serde() {
    let (cpg, seed, _, _) = corpus();
    let options = ExtrapolationOptions {
        minimum_similarity: 0.0,
        ..ExtrapolationOptions::default()
    };
    let report = analyze_vulnerability_extrapolation(&cpg, seed, &options);

    let options_json = serde_json::to_string(&options).expect("serialize extrapolation options");
    let report_json = serde_json::to_string(&report).expect("serialize extrapolation report");
    assert_eq!(
        serde_json::from_str::<ExtrapolationOptions>(&options_json)
            .expect("deserialize extrapolation options"),
        options
    );
    assert_eq!(
        serde_json::from_str::<ExtrapolationAnalysis>(&report_json)
            .expect("deserialize extrapolation report"),
        report
    );
}
