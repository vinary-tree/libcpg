#![cfg(feature = "approximate-matching")]

use std::cmp::Ordering;
use std::collections::BTreeMap;

use libcpg::{
    approximate_candidates, ApproximateCancellation, ApproximateCompletion, ApproximateConfig,
    ApproximateConsumer, ApproximateError, ApproximateId, ApproximateLimit, ApproximateLimits,
    ApproximateRecord, ApproximateTape, FiniteRelationError, FiniteRelationPath, FiniteRewriteRule,
    NormalizedTapeDomain, StructuralTapeDomain, SymbolTapeDomain, TypedFiniteRelation,
    MAX_EXACT_TROPICAL_INTEGER,
};
use proptest::prelude::*;

fn record(item: u64, text: &str) -> ApproximateRecord {
    ApproximateRecord::new(
        ApproximateId::External { corpus: 91, item },
        ApproximateTape::text(text),
    )
}

fn independent_standard_distance(left: &str, right: &str) -> usize {
    let left = left.chars().collect::<Vec<_>>();
    let right = right.chars().collect::<Vec<_>>();
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

fn binary_strings(max_len: usize) -> Vec<String> {
    let mut strings = vec![String::new()];
    for length in 1..=max_len {
        for bits in 0..(1usize << length) {
            let text = (0..length)
                .map(|offset| {
                    if bits & (1usize << offset) == 0 {
                        'a'
                    } else {
                        'b'
                    }
                })
                .collect();
            strings.push(text);
        }
    }
    strings
}

fn evidence_key(
    query_units: usize,
    candidate: &ApproximateRecord,
    distance: usize,
) -> (ApproximateId, u64, u64) {
    (
        candidate.id,
        u64::try_from(distance).unwrap(),
        u64::try_from(query_units.max(candidate.tape_units()).max(1)).unwrap(),
    )
}

trait TapeUnits {
    fn tape_units(&self) -> usize;
}

impl TapeUnits for ApproximateRecord {
    fn tape_units(&self) -> usize {
        match self.tape.as_ref() {
            ApproximateTape::Text(text) => text.chars().count(),
            ApproximateTape::Tokens(tokens) => tokens.len(),
        }
    }
}

fn compare_oracle_key(
    left: &(ApproximateId, u64, u64),
    right: &(ApproximateId, u64, u64),
) -> Ordering {
    (u128::from(left.1) * u128::from(right.2))
        .cmp(&(u128::from(right.1) * u128::from(left.2)))
        .then_with(|| left.1.cmp(&right.1))
        .then_with(|| left.0.cmp(&right.0))
}

#[test]
fn exhaustive_binary_strings_match_an_independent_oracle_and_direct_engine() {
    let strings = binary_strings(3);
    let candidates = strings
        .iter()
        .enumerate()
        .map(|(index, text)| record(u64::try_from(index).unwrap() + 1, text))
        .collect::<Vec<_>>();
    let config = ApproximateConfig {
        max_distance: 6,
        top_k: candidates.len(),
        ..ApproximateConfig::default()
    };

    for query_text in &strings {
        let query = record(0, query_text);
        let report = approximate_candidates(
            &query,
            &candidates,
            ApproximateConsumer::RevisionAlignment,
            &config,
            &ApproximateCancellation::default(),
        )
        .expect("exhaustive bounded search");
        let mut expected = candidates
            .iter()
            .map(|candidate| {
                let ApproximateTape::Text(candidate_text) = candidate.tape.as_ref() else {
                    unreachable!();
                };
                let oracle = independent_standard_distance(query_text, candidate_text);
                assert_eq!(
                    liblevenshtein::distance::standard_distance_bounded(
                        query_text,
                        candidate_text,
                        6,
                    ),
                    Some(oracle),
                );
                evidence_key(query_text.chars().count(), candidate, oracle)
            })
            .collect::<Vec<_>>();
        expected.sort_by(compare_oracle_key);
        let actual = report
            .evidence
            .iter()
            .map(|evidence| {
                (
                    evidence.candidate,
                    evidence.raw_cost.numerator,
                    evidence.normalized_cost.denominator,
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(report.completion, ApproximateCompletion::Complete);
        assert_eq!(actual, expected, "query {query_text:?}");
    }
}

#[test]
fn exhaustive_finite_composition_matches_join_oracle_and_duallity_bridge() {
    let sources = ["", "a", "b"];
    let middles = ["", "x", "y"];
    let targets = ["", "p", "q"];
    let left_rules = sources
        .iter()
        .enumerate()
        .flat_map(|(source_index, source)| {
            middles
                .iter()
                .enumerate()
                .map(move |(middle_index, middle)| {
                    FiniteRewriteRule::new(
                        *source,
                        *middle,
                        u64::try_from(source_index + 2 * middle_index).unwrap(),
                    )
                })
        })
        .collect::<Vec<_>>();
    let right_rules = middles
        .iter()
        .enumerate()
        .flat_map(|(middle_index, middle)| {
            targets
                .iter()
                .enumerate()
                .map(move |(target_index, target)| {
                    FiniteRewriteRule::new(
                        *middle,
                        *target,
                        u64::try_from(3 * middle_index + target_index).unwrap(),
                    )
                })
        })
        .collect::<Vec<_>>();
    let limits = ApproximateLimits::default();
    let left = TypedFiniteRelation::<SymbolTapeDomain, NormalizedTapeDomain>::from_rules(
        &left_rules,
        &limits,
    )
    .unwrap();
    let right = TypedFiniteRelation::<NormalizedTapeDomain, StructuralTapeDomain>::from_rules(
        &right_rules,
        &limits,
    )
    .unwrap();
    let observed = left
        .compose(&right, &limits)
        .unwrap()
        .observe(&ApproximateCancellation::default())
        .unwrap();

    let mut oracle = BTreeMap::<(String, String), u64>::new();
    for left_rule in &left_rules {
        for right_rule in &right_rules {
            if left_rule.output == right_rule.input {
                let cost = left_rule.cost + right_rule.cost;
                oracle
                    .entry((left_rule.input.to_string(), right_rule.output.to_string()))
                    .and_modify(|current| *current = (*current).min(cost))
                    .or_insert(cost);
            }
        }
    }
    let expected = oracle
        .into_iter()
        .map(|((input, output), cost)| FiniteRelationPath {
            input: input.into(),
            output: output.into(),
            cost,
        })
        .collect::<Vec<_>>();
    assert_eq!(observed.completion, ApproximateCompletion::Complete);
    assert_eq!(observed.paths, expected);

    let duallity_left = left_rules
        .iter()
        .map(|rule| duallity::RewriteRule::with_cost(&rule.input, &rule.output, rule.cost as f64))
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let duallity_right = right_rules
        .iter()
        .map(|rule| duallity::RewriteRule::with_cost(&rule.input, &rule.output, rule.cost as f64))
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let bridged_left =
        TypedFiniteRelation::<SymbolTapeDomain, NormalizedTapeDomain>::from_duallity_rules(
            &duallity_left,
            &limits,
        )
        .unwrap();
    let bridged_right =
        TypedFiniteRelation::<NormalizedTapeDomain, StructuralTapeDomain>::from_duallity_rules(
            &duallity_right,
            &limits,
        )
        .unwrap();
    let bridged = bridged_left
        .compose(&bridged_right, &limits)
        .unwrap()
        .observe(&ApproximateCancellation::default())
        .unwrap();
    assert_eq!(bridged.paths, expected);
}

#[test]
fn every_search_cap_has_a_typed_incomplete_outcome() {
    let query = record(0, "ab");
    let candidates = [record(1, "ac"), record(2, "ab")];
    let cases = [
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
    ];
    for (expected, limits) in cases {
        let report = approximate_candidates(
            &query,
            &candidates,
            ApproximateConsumer::RevisionAlignment,
            &ApproximateConfig {
                max_distance: 8,
                limits,
                ..ApproximateConfig::default()
            },
            &ApproximateCancellation::default(),
        )
        .unwrap();
        let ApproximateCompletion::Incomplete(reasons) = report.completion else {
            panic!("{expected:?} cap must make the report incomplete");
        };
        assert!(reasons.contains(&expected), "{expected:?}: {reasons:?}");
    }

    let token_report = approximate_candidates(
        &ApproximateRecord::new(
            ApproximateId::External {
                corpus: 92,
                item: 0,
            },
            ApproximateTape::tokens(["open", "read"]),
        ),
        &[ApproximateRecord::new(
            ApproximateId::External {
                corpus: 92,
                item: 1,
            },
            ApproximateTape::tokens(["open", "close"]),
        )],
        ApproximateConsumer::ApiSequenceNeighbor,
        &ApproximateConfig {
            max_distance: 8,
            limits: ApproximateLimits {
                max_unique_tokens: 1,
                ..ApproximateLimits::default()
            },
            ..ApproximateConfig::default()
        },
        &ApproximateCancellation::default(),
    )
    .unwrap();
    assert_eq!(
        token_report.completion,
        ApproximateCompletion::Incomplete(vec![ApproximateLimit::UniqueTokens])
    );

    let cancellation = ApproximateCancellation::default();
    cancellation.cancel();
    let cancelled = approximate_candidates(
        &query,
        &candidates,
        ApproximateConsumer::RevisionAlignment,
        &ApproximateConfig::default(),
        &cancellation,
    )
    .unwrap();
    assert_eq!(
        cancelled.completion,
        ApproximateCompletion::Incomplete(vec![ApproximateLimit::Cancelled])
    );
}

#[test]
fn invalid_limits_duplicate_ids_and_mixed_tapes_are_rejected() {
    let query = record(0, "a");
    let candidate = record(1, "b");
    let mut invalid = Vec::new();
    let mut push = |name: &'static str, config: ApproximateConfig| invalid.push((name, config));
    push(
        "top_k",
        ApproximateConfig {
            top_k: 0,
            ..ApproximateConfig::default()
        },
    );
    for name in [
        "max_candidates",
        "max_input_units",
        "max_pair_cells",
        "max_work_units",
        "max_state_count",
        "max_memory_bytes",
        "max_unique_tokens",
    ] {
        let mut config = ApproximateConfig::default();
        match name {
            "max_candidates" => config.limits.max_candidates = 0,
            "max_input_units" => config.limits.max_input_units = 0,
            "max_pair_cells" => config.limits.max_pair_cells = 0,
            "max_work_units" => config.limits.max_work_units = 0,
            "max_state_count" => config.limits.max_state_count = 0,
            "max_memory_bytes" => config.limits.max_memory_bytes = 0,
            "max_unique_tokens" => config.limits.max_unique_tokens = 0,
            _ => unreachable!(),
        }
        push(name, config);
    }
    for (name, config) in invalid {
        assert_eq!(
            approximate_candidates(
                &query,
                std::slice::from_ref(&candidate),
                ApproximateConsumer::RevisionAlignment,
                &config,
                &ApproximateCancellation::default(),
            ),
            Err(ApproximateError::ZeroLimit(name)),
        );
    }

    assert!(matches!(
        approximate_candidates(
            &query,
            &[candidate.clone(), candidate],
            ApproximateConsumer::RevisionAlignment,
            &ApproximateConfig::default(),
            &ApproximateCancellation::default(),
        ),
        Err(ApproximateError::DuplicateCandidate(_))
    ));
    let tokens = ApproximateRecord::new(
        ApproximateId::External {
            corpus: 91,
            item: 2,
        },
        ApproximateTape::tokens(["a"]),
    );
    assert!(matches!(
        approximate_candidates(
            &query,
            &[tokens],
            ApproximateConsumer::RevisionAlignment,
            &ApproximateConfig::default(),
            &ApproximateCancellation::default(),
        ),
        Err(ApproximateError::TapeKindMismatch { .. })
    ));
}

#[test]
fn invalid_weights_and_epsilon_zero_cost_paths_are_bounded() {
    let limits = ApproximateLimits::default();
    let too_large = FiniteRewriteRule::new("a", "b", MAX_EXACT_TROPICAL_INTEGER + 1);
    assert!(matches!(
        TypedFiniteRelation::<SymbolTapeDomain, NormalizedTapeDomain>::from_rules(
            &[too_large],
            &limits,
        ),
        Err(FiniteRelationError::InexactCost { .. })
    ));
    for cost in [f64::NAN, f64::INFINITY, -1.0, 0.5] {
        let rule = duallity::RewriteRule {
            input: "a".into(),
            output: "b".into(),
            cost,
            priority: 0,
        };
        assert!(matches!(
            TypedFiniteRelation::<SymbolTapeDomain, NormalizedTapeDomain>::from_duallity_rules(
                &[rule],
                &limits,
            ),
            Err(FiniteRelationError::InexactDuallityCost { .. })
        ));
    }

    let epsilon = TypedFiniteRelation::<SymbolTapeDomain, NormalizedTapeDomain>::from_rules(
        &[FiniteRewriteRule::new("", "", 0)],
        &limits,
    )
    .unwrap();
    let identity =
        TypedFiniteRelation::<NormalizedTapeDomain, NormalizedTapeDomain>::identity([""], &limits)
            .unwrap();
    let observed = epsilon
        .compose(&identity, &limits)
        .unwrap()
        .observe(&ApproximateCancellation::default())
        .unwrap();
    assert_eq!(observed.completion, ApproximateCompletion::Complete);
    assert_eq!(
        observed.paths,
        vec![FiniteRelationPath {
            input: "".into(),
            output: "".into(),
            cost: 0,
        }]
    );

    let cancellation = ApproximateCancellation::default();
    cancellation.cancel();
    let cancelled = epsilon
        .compose(&identity, &limits)
        .unwrap()
        .observe(&cancellation)
        .unwrap();
    assert_eq!(
        cancelled.completion,
        ApproximateCompletion::Incomplete(vec![ApproximateLimit::Cancelled])
    );
    assert!(cancelled.paths.is_empty());
}

#[test]
fn explicit_normalization_repetition_and_thread_count_preserve_semantics() {
    let query = record(0, "é");
    let decomposed = [record(1, "e\u{301}")];
    let raw = approximate_candidates(
        &query,
        &decomposed,
        ApproximateConsumer::RevisionAlignment,
        &ApproximateConfig::default(),
        &ApproximateCancellation::default(),
    )
    .unwrap();
    assert_eq!(raw.evidence[0].raw_cost.numerator, 2);
    let caller_normalized = approximate_candidates(
        &query,
        &[record(1, "é")],
        ApproximateConsumer::RevisionAlignment,
        &ApproximateConfig::default(),
        &ApproximateCancellation::default(),
    )
    .unwrap();
    assert_eq!(caller_normalized.evidence[0].raw_cost.numerator, 0);

    let candidates = [record(3, "parse"), record(2, "parser"), record(1, "parcel")];
    let run = || {
        approximate_candidates(
            &record(0, "parser"),
            &candidates,
            ApproximateConsumer::RevisionAlignment,
            &ApproximateConfig::default(),
            &ApproximateCancellation::default(),
        )
        .unwrap()
    };
    let first = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .unwrap()
        .install(run);
    let repeated = run();
    let four_threads = rayon::ThreadPoolBuilder::new()
        .num_threads(4)
        .build()
        .unwrap()
        .install(run);
    assert_eq!(first, repeated);
    assert_eq!(first, four_threads);
}

proptest! {
    #[test]
    fn common_affixes_preserve_distance_and_nonbinding_budgets_preserve_results(
        left in "[a-z]{0,12}",
        right in "[a-z]{0,12}",
        prefix in "[a-z]{0,4}",
        suffix in "[a-z]{0,4}",
    ) {
        let config = ApproximateConfig {
            max_distance: 32,
            ..ApproximateConfig::default()
        };
        let base = approximate_candidates(
            &record(0, &left),
            &[record(1, &right)],
            ApproximateConsumer::RevisionAlignment,
            &config,
            &ApproximateCancellation::default(),
        ).unwrap();
        let extended_left = format!("{prefix}{left}{suffix}");
        let extended_right = format!("{prefix}{right}{suffix}");
        let extended = approximate_candidates(
            &record(0, &extended_left),
            &[record(1, &extended_right)],
            ApproximateConsumer::RevisionAlignment,
            &config,
            &ApproximateCancellation::default(),
        ).unwrap();
        prop_assert_eq!(base.evidence[0].raw_cost, extended.evidence[0].raw_cost);

        let mut larger = config.clone();
        larger.limits.max_pair_cells *= 2;
        larger.limits.max_work_units *= 2;
        larger.limits.max_state_count *= 2;
        larger.limits.max_memory_bytes *= 2;
        let larger_report = approximate_candidates(
            &record(0, &left),
            &[record(1, &right)],
            ApproximateConsumer::RevisionAlignment,
            &larger,
            &ApproximateCancellation::default(),
        ).unwrap();
        prop_assert_eq!(larger_report.completion, ApproximateCompletion::Complete);
        prop_assert_eq!(
            larger_report.evidence[0].raw_cost,
            base.evidence[0].raw_cost,
        );
    }

    #[test]
    fn equal_score_corpora_are_deterministic_and_never_duplicate_results(
        count in 1usize..512,
        top_k in 1usize..128,
    ) {
        let candidates = (1..=count)
            .rev()
            .map(|item| record(u64::try_from(item).unwrap(), "same"))
            .collect::<Vec<_>>();
        let retained = top_k.min(count);
        let report = approximate_candidates(
            &record(0, "same"),
            &candidates,
            ApproximateConsumer::RevisionAlignment,
            &ApproximateConfig {
                max_distance: 0,
                top_k: retained,
                ..ApproximateConfig::default()
            },
            &ApproximateCancellation::default(),
        ).unwrap();
        let actual = report.evidence.iter().map(|evidence| evidence.candidate).collect::<Vec<_>>();
        let expected = (1..=retained)
            .map(|item| ApproximateId::External {
                corpus: 91,
                item: u64::try_from(item).unwrap(),
            })
            .collect::<Vec<_>>();
        prop_assert_eq!(actual, expected);
    }
}
