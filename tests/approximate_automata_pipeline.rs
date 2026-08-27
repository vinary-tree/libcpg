#![cfg(feature = "approximate-matching")]

use std::collections::BTreeMap;

use libcpg::{
    ApiTapeDomain, ApproximateCancellation, ApproximateCompletion, ApproximateConfig,
    ApproximateConsumer, ApproximateId, ApproximateLimits, ApproximateRecord, ApproximateTape,
    ApproximateTokenIndex, ExactConfirmation, FiniteRewriteRule, NormalizedTapeDomain,
    SymbolTapeDomain, TypedFiniteRelation,
};

fn normalize_sequence(sequence: &[&str], paths: &BTreeMap<String, String>) -> Vec<String> {
    sequence
        .iter()
        .map(|event| {
            paths
                .get(*event)
                .cloned()
                .expect("the complete relation covers every fixture event")
        })
        .collect()
}

#[test]
fn duallity_lling_vocabulary_and_sequence_search_compose_end_to_end() {
    let limits = ApproximateLimits::default();
    let lexical_rules = [
        ("std.open", "file.open"),
        ("fs.open", "file.open"),
        ("std.read", "file.read"),
        ("fs.read", "file.read"),
        ("std.write", "file.write"),
        ("std.close", "file.close"),
        ("fs.close", "file.close"),
    ]
    .into_iter()
    .map(|(input, output)| duallity::RewriteRule::with_cost(input, output, 0.0).unwrap())
    .collect::<Vec<_>>();
    let structural_rules = [
        FiniteRewriteRule::new("file.open", "OPEN", 0),
        FiniteRewriteRule::new("file.read", "READ", 0),
        FiniteRewriteRule::new("file.write", "WRITE", 0),
        FiniteRewriteRule::new("file.close", "CLOSE", 0),
    ];

    let lexical =
        TypedFiniteRelation::<SymbolTapeDomain, NormalizedTapeDomain>::from_duallity_rules(
            &lexical_rules,
            &limits,
        )
        .unwrap();
    let structural = TypedFiniteRelation::<NormalizedTapeDomain, ApiTapeDomain>::from_rules(
        &structural_rules,
        &limits,
    )
    .unwrap();
    let observation = lexical
        .compose(&structural, &limits)
        .unwrap()
        .observe(&ApproximateCancellation::default())
        .unwrap()
        .into_complete()
        .expect("the finite acyclic relation is completely observed");
    let rewrites = observation
        .paths()
        .iter()
        .map(|path| (path.input.to_string(), path.output.to_string()))
        .collect::<BTreeMap<_, _>>();

    let raw_candidates = [
        (1, vec!["std.open", "std.read", "std.close"]),
        (2, vec!["std.open", "std.write", "std.close"]),
        (3, vec!["fs.open", "fs.read", "fs.close"]),
    ];
    let candidates = raw_candidates
        .iter()
        .map(|(item, events)| {
            ApproximateRecord::new(
                ApproximateId::External {
                    corpus: 0x0041_5049_5f53_4551,
                    item: *item,
                },
                ApproximateTape::tokens(normalize_sequence(events, &rewrites)),
            )
        })
        .collect::<Vec<_>>();
    let query = ApproximateRecord::new(
        ApproximateId::External {
            corpus: 0x0041_5049_5f53_4551,
            item: 0,
        },
        ApproximateTape::tokens(normalize_sequence(&["std.open", "std.read"], &rewrites)),
    );

    let index = ApproximateTokenIndex::build(&candidates, &limits).unwrap();
    assert_eq!(index.counters().indexed_sequences, 2);
    assert_eq!(index.counters().vocabulary_terms, 4);
    assert_eq!(index.token_id("OPEN"), Some(2));
    assert_eq!(
        index.token(index.token_id("READ").unwrap()).as_deref(),
        Some("READ")
    );

    let report = index
        .search(
            &query,
            ApproximateConsumer::ApiSequenceNeighbor,
            &ApproximateConfig {
                max_distance: 1,
                top_k: 3,
                ..ApproximateConfig::default()
            },
            &ApproximateCancellation::default(),
        )
        .unwrap();
    assert_eq!(report.completion, ApproximateCompletion::Complete);
    assert_eq!(report.evidence.len(), 2);
    assert_eq!(report.evidence[0].candidate, candidates[0].id);
    assert_eq!(report.evidence[1].candidate, candidates[2].id);
    assert!(report.evidence.iter().all(|evidence| {
        evidence.raw_cost.numerator == 1
            && evidence.exact_confirmation == ExactConfirmation::ExactApiUsageTemporalProperty
    }));

    // Approximation only narrows the corpus. The caller still confirms the
    // temporal property over the original typed events.
    for evidence in &report.evidence {
        let (_, events) = raw_candidates
            .iter()
            .find(|(item, _)| {
                evidence.candidate
                    == (ApproximateId::External {
                        corpus: 0x0041_5049_5f53_4551,
                        item: *item,
                    })
            })
            .unwrap();
        assert!(events.first().unwrap().ends_with("open"));
        assert!(events.last().unwrap().ends_with("close"));
    }
}
