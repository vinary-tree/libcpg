#![cfg(feature = "approximate-matching")]

use std::cmp::Ordering;

use libcpg::{
    approximate_candidates, ApproximateCancellation, ApproximateCompletion, ApproximateConfig,
    ApproximateConsumer, ApproximateError, ApproximateId, ApproximateLimit, ApproximateLimits,
    ApproximateMethod, ApproximateRecord, ApproximateTape, ApproximateTokenIndex,
};
use proptest::prelude::*;

fn tokens(item: u64, terms: impl IntoIterator<Item = impl Into<String>>) -> ApproximateRecord {
    ApproximateRecord::new(
        ApproximateId::External {
            corpus: 0x0054_4f4b_454e,
            item,
        },
        ApproximateTape::tokens(terms.into_iter().map(Into::into)),
    )
}

fn search(
    index: &ApproximateTokenIndex,
    query: &ApproximateRecord,
    config: &ApproximateConfig,
) -> libcpg::ApproximateReport {
    index
        .search(
            query,
            ApproximateConsumer::CloneCandidate,
            config,
            &ApproximateCancellation::default(),
        )
        .expect("bounded indexed token search")
}

fn standard_distance(left: &[u8], right: &[u8]) -> usize {
    let mut previous = (0..=right.len()).collect::<Vec<_>>();
    let mut current = vec![0; right.len() + 1];
    for (left_index, left_unit) in left.iter().enumerate() {
        current[0] = left_index + 1;
        for (right_index, right_unit) in right.iter().enumerate() {
            current[right_index + 1] = (previous[right_index + 1] + 1)
                .min(current[right_index] + 1)
                .min(previous[right_index] + usize::from(left_unit != right_unit));
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[right.len()]
}

fn compare_rank(
    left: &(ApproximateId, usize, usize),
    right: &(ApproximateId, usize, usize),
) -> Ordering {
    ((left.1 as u128) * (right.2 as u128))
        .cmp(&((right.1 as u128) * (left.2 as u128)))
        .then_with(|| left.1.cmp(&right.1))
        .then_with(|| left.0.cmp(&right.0))
}

#[test]
fn two_level_dictionary_is_reversible_and_retains_duplicate_sequences() {
    let candidates = [
        tokens(3, ["call", "read"]),
        tokens(1, ["call", "read"]),
        tokens(2, ["call", "write"]),
    ];
    let index = ApproximateTokenIndex::build(&candidates, &ApproximateLimits::default()).unwrap();
    let counters = index.counters();

    assert_eq!(index.completion(), &ApproximateCompletion::Complete);
    assert_eq!(counters.supplied_candidates, 3);
    assert_eq!(counters.indexed_candidates, 3);
    assert_eq!(counters.indexed_sequences, 2);
    assert_eq!(counters.vocabulary_terms, 3);
    assert_eq!(counters.retained_units, 6);
    assert!(counters.dictionary_edge_upper_bound >= counters.indexed_sequences);
    assert_eq!(index.token_id("call"), Some(1));
    assert_eq!(index.token_id("read"), Some(2));
    assert_eq!(index.token_id("write"), Some(3));
    assert_eq!(index.token(1).as_deref(), Some("call"));
    assert_eq!(index.token(2).as_deref(), Some("read"));
    assert_eq!(index.token(3).as_deref(), Some("write"));
    assert_eq!(index.token_id("missing"), None);
    assert_eq!(index.token(4), None);

    let report = search(
        &index,
        &tokens(0, ["call", "read"]),
        &ApproximateConfig {
            max_distance: 1,
            top_k: 3,
            ..ApproximateConfig::default()
        },
    );
    let actual = report
        .evidence
        .iter()
        .map(|evidence| (evidence.candidate, evidence.raw_cost.numerator))
        .collect::<Vec<_>>();
    assert_eq!(
        actual,
        vec![
            (candidates[1].id, 0),
            (candidates[0].id, 0),
            (candidates[2].id, 1),
        ]
    );
}

#[test]
fn unknown_query_terms_are_fresh_but_repeated_unknowns_remain_equal() {
    let candidates = [tokens(1, ["a", "b"]), tokens(2, ["a", "c"])];
    let index = ApproximateTokenIndex::build(&candidates, &ApproximateLimits::default()).unwrap();
    let config = ApproximateConfig {
        max_distance: 2,
        top_k: 2,
        ..ApproximateConfig::default()
    };

    let fresh = search(&index, &tokens(0, ["a", "x"]), &config);
    assert_eq!(fresh.completion, ApproximateCompletion::Complete);
    assert_eq!(fresh.evidence.len(), 2);
    assert!(fresh
        .evidence
        .iter()
        .all(|evidence| evidence.raw_cost.numerator == 1));

    let repeated = search(&index, &tokens(0, ["x", "x"]), &config);
    assert!(repeated
        .evidence
        .iter()
        .all(|evidence| evidence.raw_cost.numerator == 2));
}

#[test]
fn every_indexed_method_uses_token_units_and_preserves_exact_matches() {
    let candidates = [
        tokens(1, ["a", "b"]),
        tokens(2, ["b", "a"]),
        tokens(3, ["a", "b", "c"]),
    ];
    let index = ApproximateTokenIndex::build(&candidates, &ApproximateLimits::default()).unwrap();
    for method in [
        ApproximateMethod::StandardLevenshtein,
        ApproximateMethod::OptimalStringAlignment,
        ApproximateMethod::DamerauLevenshtein,
        ApproximateMethod::GeneralizedMergeSplit,
    ] {
        let report = search(
            &index,
            &tokens(0, ["a", "b"]),
            &ApproximateConfig {
                method,
                max_distance: 3,
                top_k: 3,
                ..ApproximateConfig::default()
            },
        );
        assert_eq!(report.completion, ApproximateCompletion::Complete);
        assert_eq!(report.evidence[0].candidate, candidates[0].id);
        assert_eq!(report.evidence[0].raw_cost.numerator, 0);
        assert!(report
            .provenance
            .method_identity
            .ends_with(".libdictenstein-u64-index.v1"));

        let transposed = report
            .evidence
            .iter()
            .find(|evidence| evidence.candidate == candidates[1].id)
            .unwrap();
        let expected = match method {
            ApproximateMethod::OptimalStringAlignment | ApproximateMethod::DamerauLevenshtein => 1,
            ApproximateMethod::StandardLevenshtein | ApproximateMethod::GeneralizedMergeSplit => 2,
        };
        assert_eq!(transposed.raw_cost.numerator, expected, "{method:?}");
    }
}

#[test]
fn build_and_search_limits_never_turn_incomplete_work_into_no_match() {
    let candidates = [tokens(1, ["a", "b"]), tokens(2, ["a", "c"])];
    let build_cases = [
        (
            ApproximateLimit::CandidateCount,
            ApproximateLimits {
                max_candidates: 1,
                ..ApproximateLimits::default()
            },
        ),
        (
            ApproximateLimit::InputUnits,
            ApproximateLimits {
                max_input_units: 1,
                ..ApproximateLimits::default()
            },
        ),
        (
            ApproximateLimit::TotalWork,
            ApproximateLimits {
                max_work_units: 1,
                ..ApproximateLimits::default()
            },
        ),
        (
            ApproximateLimit::Memory,
            ApproximateLimits {
                max_memory_bytes: 1,
                ..ApproximateLimits::default()
            },
        ),
        (
            ApproximateLimit::UniqueTokens,
            ApproximateLimits {
                max_unique_tokens: 1,
                ..ApproximateLimits::default()
            },
        ),
    ];
    for (expected, limits) in build_cases {
        let index = ApproximateTokenIndex::build(&candidates, &limits).unwrap();
        let ApproximateCompletion::Incomplete(reasons) = index.completion() else {
            panic!("{expected:?} must make index construction incomplete");
        };
        assert!(reasons.contains(&expected), "{expected:?}: {reasons:?}");
    }

    let index = ApproximateTokenIndex::build(&candidates, &ApproximateLimits::default()).unwrap();
    let query = tokens(0, ["a", "x"]);
    let search_cases = [
        (
            ApproximateLimit::CandidateCount,
            ApproximateLimits {
                max_candidates: 1,
                ..ApproximateLimits::default()
            },
        ),
        (
            ApproximateLimit::InputUnits,
            ApproximateLimits {
                max_input_units: 1,
                ..ApproximateLimits::default()
            },
        ),
        (
            ApproximateLimit::PairWork,
            ApproximateLimits {
                max_pair_cells: 1,
                ..ApproximateLimits::default()
            },
        ),
        (
            ApproximateLimit::TotalWork,
            ApproximateLimits {
                max_work_units: 1,
                ..ApproximateLimits::default()
            },
        ),
        (
            ApproximateLimit::StateCount,
            ApproximateLimits {
                max_state_count: 1,
                ..ApproximateLimits::default()
            },
        ),
        (
            ApproximateLimit::Memory,
            ApproximateLimits {
                max_memory_bytes: 1,
                ..ApproximateLimits::default()
            },
        ),
        (
            ApproximateLimit::UniqueTokens,
            ApproximateLimits {
                max_unique_tokens: 3,
                ..ApproximateLimits::default()
            },
        ),
    ];
    for (expected, limits) in search_cases {
        let report = search(
            &index,
            &query,
            &ApproximateConfig {
                max_distance: 2,
                limits,
                ..ApproximateConfig::default()
            },
        );
        let ApproximateCompletion::Incomplete(reasons) = report.completion else {
            panic!("{expected:?} must make indexed search incomplete");
        };
        assert!(report.evidence.is_empty());
        assert!(reasons.contains(&expected), "{expected:?}: {reasons:?}");
    }

    let cancellation = ApproximateCancellation::default();
    cancellation.cancel();
    let cancelled = index
        .search(
            &query,
            ApproximateConsumer::CloneCandidate,
            &ApproximateConfig {
                max_distance: 2,
                ..ApproximateConfig::default()
            },
            &cancellation,
        )
        .unwrap();
    assert_eq!(
        cancelled.completion,
        ApproximateCompletion::Incomplete(vec![ApproximateLimit::Cancelled])
    );
    assert!(cancelled.evidence.is_empty());
}

#[test]
fn index_rejects_wrong_tapes_and_duplicate_stable_identifiers() {
    let text = ApproximateRecord::new(
        ApproximateId::External {
            corpus: 0x0054_4f4b_454e,
            item: 1,
        },
        ApproximateTape::text("text"),
    );
    assert!(matches!(
        ApproximateTokenIndex::build(std::slice::from_ref(&text), &ApproximateLimits::default()),
        Err(ApproximateError::IndexedCandidateRequiresTokens { .. })
    ));

    let duplicate = tokens(1, ["token"]);
    assert!(matches!(
        ApproximateTokenIndex::build(
            &[duplicate.clone(), duplicate],
            &ApproximateLimits::default()
        ),
        Err(ApproximateError::DuplicateCandidate(_))
    ));

    let index =
        ApproximateTokenIndex::build(&[tokens(1, ["token"])], &ApproximateLimits::default())
            .unwrap();
    assert_eq!(
        index.search(
            &text,
            ApproximateConsumer::CloneCandidate,
            &ApproximateConfig::default(),
            &ApproximateCancellation::default(),
        ),
        Err(ApproximateError::IndexedQueryRequiresTokens)
    );
}

proptest! {
    #[test]
    fn indexed_standard_search_matches_two_row_oracle_and_bijective_renaming(
        query in prop::collection::vec(0u8..4, 0..7),
        corpus in prop::collection::vec(prop::collection::vec(0u8..4, 0..7), 0..24),
        shift in 0u8..4,
    ) {
        let candidates = corpus
            .iter()
            .enumerate()
            .map(|(index, sequence)| {
                tokens(
                    index as u64 + 1,
                    sequence.iter().map(|unit| format!("term-{unit}")),
                )
            })
            .collect::<Vec<_>>();
        let query_record = tokens(0, query.iter().map(|unit| format!("term-{unit}")));
        let config = ApproximateConfig {
            max_distance: 8,
            top_k: candidates.len().max(1),
            ..ApproximateConfig::default()
        };
        let report = approximate_candidates(
            &query_record,
            &candidates,
            ApproximateConsumer::CloneCandidate,
            &config,
            &ApproximateCancellation::default(),
        ).unwrap();

        let mut expected = corpus
            .iter()
            .enumerate()
            .map(|(index, candidate)| {
                (
                    candidates[index].id,
                    standard_distance(&query, candidate),
                    query.len().max(candidate.len()).max(1),
                )
            })
            .collect::<Vec<_>>();
        expected.sort_by(compare_rank);
        let actual = report.evidence.iter().map(|evidence| {
            (
                evidence.candidate,
                evidence.raw_cost.numerator as usize,
                evidence.normalized_cost.denominator as usize,
            )
        }).collect::<Vec<_>>();
        prop_assert_eq!(actual, expected);
        prop_assert_eq!(report.completion, ApproximateCompletion::Complete);

        let rename = |unit: &u8| format!("renamed-{}", (*unit + shift) % 4);
        let renamed_candidates = corpus
            .iter()
            .enumerate()
            .map(|(index, sequence)| tokens(index as u64 + 1, sequence.iter().map(rename)))
            .collect::<Vec<_>>();
        let renamed_query = tokens(0, query.iter().map(rename));
        let renamed = approximate_candidates(
            &renamed_query,
            &renamed_candidates,
            ApproximateConsumer::CloneCandidate,
            &config,
            &ApproximateCancellation::default(),
        ).unwrap();
        let original_scores = report.evidence.iter().map(|evidence| {
            (evidence.candidate, evidence.raw_cost, evidence.normalized_cost)
        }).collect::<Vec<_>>();
        let renamed_scores = renamed.evidence.iter().map(|evidence| {
            (evidence.candidate, evidence.raw_cost, evidence.normalized_cost)
        }).collect::<Vec<_>>();
        prop_assert_eq!(renamed_scores, original_scores);
    }
}
