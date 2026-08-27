#![cfg(feature = "approximate-matching")]

use libcpg::{
    approximate_candidates, project_cpg_functions, ApproximateCancellation, ApproximateCompletion,
    ApproximateConfig, ApproximateConsumer, ApproximateId, ApproximateLimits, ApproximateRecord,
    ApproximateTape, ApproximateTokenIndex, CodePropertyGraph, CpgNode, CpgNodeKind, Language,
    MethodSignature, NodeId, SourceRange, Visibility,
};

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

fn project_chain(depth: usize) {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let root = cpg.add_node(CpgNode::new(
        NodeId::new(0),
        function("deep"),
        SourceRange::default(),
    ));
    let mut parent = root;
    for index in 0..depth {
        let child = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Identifier {
                name: format!("n{index}").into(),
                definition: None,
            },
            SourceRange::default(),
        ));
        cpg.node_mut(parent)
            .expect("chain parent")
            .children
            .push(child);
        parent = child;
    }
    let limits = ApproximateLimits {
        max_input_units: depth + 1,
        max_pair_cells: 1,
        max_work_units: (depth as u64 + 1) * 3,
        max_state_count: 1,
        max_memory_bytes: 128 * 1024 * 1024,
        ..ApproximateLimits::default()
    };
    let projection = project_cpg_functions(&cpg, 1, &limits).expect("bounded deep projection");
    assert_eq!(projection.completion, ApproximateCompletion::Complete);
    assert_eq!(projection.records.len(), 1);
}

#[test]
fn function_projection_is_stack_safe_at_20k_and_100k_depth() {
    std::thread::Builder::new()
        .name("approximate-projection-stack-gate".into())
        .stack_size(256 * 1024)
        .spawn(|| {
            project_chain(20_000);
            project_chain(100_000);
        })
        .expect("spawn small-stack gate")
        .join()
        .expect("iterative projection must not overflow the native stack");
}

#[test]
fn candidate_order_rank_clone_debug_and_drop_are_stack_safe_at_100k() {
    std::thread::Builder::new()
        .name("approximate-search-lifecycle-stack-gate".into())
        .stack_size(256 * 1024)
        .spawn(|| {
            const CANDIDATES: usize = 100_000;
            let query = ApproximateRecord::new(
                ApproximateId::External { corpus: 1, item: 0 },
                ApproximateTape::text("x"),
            );
            let candidates = (1..=CANDIDATES)
                .rev()
                .map(|item| {
                    ApproximateRecord::new(
                        ApproximateId::External {
                            corpus: 1,
                            item: u64::try_from(item).expect("candidate identifier fits u64"),
                        },
                        ApproximateTape::text("x"),
                    )
                })
                .collect::<Vec<_>>();
            let config = ApproximateConfig {
                max_distance: 0,
                top_k: 1,
                limits: ApproximateLimits {
                    max_candidates: CANDIDATES,
                    max_input_units: 1,
                    max_pair_cells: 4,
                    max_work_units: 4 * u64::try_from(CANDIDATES)
                        .expect("candidate count fits u64"),
                    max_state_count: 4,
                    max_memory_bytes: 128 * 1024 * 1024,
                    max_unique_tokens: 1,
                },
                ..ApproximateConfig::default()
            };

            let report = approximate_candidates(
                &query,
                &candidates,
                ApproximateConsumer::RevisionAlignment,
                &config,
                &ApproximateCancellation::default(),
            )
            .expect("bounded 100k candidate search");
            assert_eq!(report.completion, ApproximateCompletion::Complete);
            assert_eq!(report.counters.examined_candidates, CANDIDATES);
            assert_eq!(report.evidence.len(), 1);
            assert_eq!(
                report.evidence[0].candidate,
                ApproximateId::External { corpus: 1, item: 1 }
            );

            let cloned = report.clone();
            assert!(format!("{cloned:?}").contains("RevisionAlignment"));
            #[cfg(feature = "serde")]
            {
                let encoded = serde_json::to_vec(&cloned)
                    .expect("serialize evidence from the 100k-candidate lifecycle");
                let decoded: libcpg::ApproximateReport = serde_json::from_slice(&encoded)
                    .expect("deserialize evidence from the 100k-candidate lifecycle");
                assert_eq!(decoded, cloned);
            }
            drop(cloned);
            drop(report);
            drop(candidates);
        })
        .expect("spawn 100k lifecycle stack gate")
        .join()
        .expect("ordering and lifecycle must not overflow the native stack");
}

fn indexed_sequence_lifecycle(depth: usize) {
    let candidate = ApproximateRecord::new(
        ApproximateId::External { corpus: 2, item: 1 },
        ApproximateTape::tokens(std::iter::repeat_n("step", depth)),
    );
    let limits = ApproximateLimits {
        max_candidates: 1,
        max_input_units: depth,
        max_pair_cells: 11_000_000_000,
        max_work_units: 11_000_000_000,
        max_state_count: 1_000_000,
        max_memory_bytes: 256 * 1024 * 1024,
        max_unique_tokens: 1,
    };
    let index = ApproximateTokenIndex::build(std::slice::from_ref(&candidate), &limits)
        .expect("bounded deep sequence index");
    assert_eq!(index.completion(), &ApproximateCompletion::Complete);
    let report = index
        .search(
            &ApproximateRecord::new(
                ApproximateId::External { corpus: 2, item: 0 },
                ApproximateTape::tokens(std::iter::repeat_n("step", depth)),
            ),
            ApproximateConsumer::CloneCandidate,
            &ApproximateConfig {
                max_distance: 0,
                top_k: 1,
                limits,
                ..ApproximateConfig::default()
            },
            &ApproximateCancellation::default(),
        )
        .expect("bounded deep sequence query");
    assert_eq!(report.completion, ApproximateCompletion::Complete);
    assert_eq!(report.evidence[0].candidate, candidate.id);

    let cloned = index.clone();
    assert!(format!("{cloned:?}").contains("ApproximateTokenIndex"));
    drop(cloned);
    drop(index);
    drop(report);
    drop(candidate);
}

#[test]
fn token_vocabulary_dawg_query_clone_debug_and_drop_are_stack_safe() {
    std::thread::Builder::new()
        .name("approximate-token-index-stack-gate".into())
        .stack_size(256 * 1024)
        .spawn(|| {
            indexed_sequence_lifecycle(20_000);
            indexed_sequence_lifecycle(100_000);
        })
        .expect("spawn indexed small-stack gate")
        .join()
        .expect("indexed sequence lifecycle must not overflow the native stack");
}
