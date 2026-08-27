use std::collections::BTreeMap;

use duallity::Semiring;
use libcpg_approximate_automata_adoption::{
    all_words, as_integral_distances, bounded_scan, dictionary, duallity_walk, merge_split_oracle,
    osa_oracle, standard_oracle, tropical_value, Completion,
};
use liblevenshtein::distance::{standard_distance, transposition_distance};
use liblevenshtein::transducer::generalized::GeneralizedAutomaton;
use liblevenshtein::transducer::OperationSet;
use lling_llang::composition::compose;
use lling_llang::semiring::TropicalWeight;
use lling_llang::wfst::VectorWfstBuilder;
use proptest::prelude::*;

fn relation(
    paths: impl IntoIterator<Item = lling_llang::composition::ComposedPath<char, TropicalWeight>>,
) -> BTreeMap<(String, String), f64> {
    let mut result = BTreeMap::new();
    for path in paths {
        let key = (
            path.inputs.into_iter().collect(),
            path.outputs.into_iter().collect(),
        );
        result
            .entry(key)
            .and_modify(|weight| *weight = f64::min(*weight, path.weight.value()))
            .or_insert(path.weight.value());
    }
    result
}

fn mapping(arcs: &[(char, char, u8)]) -> lling_llang::wfst::VectorWfst<char, TropicalWeight> {
    let mut builder = VectorWfstBuilder::new()
        .add_states(2)
        .start(0)
        .final_state(1, TropicalWeight::one());
    for (input, output, weight) in arcs {
        builder = builder.arc(
            0,
            Some(*input),
            Some(*output),
            1,
            TropicalWeight::new(f64::from(*weight)),
        );
    }
    builder.build()
}

fn materialize_one_step(
    relation: &BTreeMap<(String, String), f64>,
) -> lling_llang::wfst::VectorWfst<char, TropicalWeight> {
    let arcs: Vec<_> = relation
        .iter()
        .map(|((input, output), weight)| {
            let mut inputs = input.chars();
            let mut outputs = output.chars();
            let arc = (
                inputs.next().expect("one input label"),
                outputs.next().expect("one output label"),
                u8::try_from(*weight as usize).expect("small integral test weight"),
            );
            assert!(inputs.next().is_none() && outputs.next().is_none());
            arc
        })
        .collect();
    mapping(&arcs)
}

#[test]
fn example_distances_match_independent_oracles() {
    let cases = [
        ("", "", 0, 0),
        ("kitten", "sitting", 3, 3),
        ("ab", "ba", 2, 1),
        ("CA", "ABC", 3, 3),
        ("café", "cafe", 1, 1),
        ("東京", "京東", 2, 1),
        ("👩‍💻", "👨‍💻", 1, 1),
    ];
    for (source, target, standard, osa) in cases {
        assert_eq!(standard_oracle(source, target), standard);
        assert_eq!(standard_distance(source, target), standard);
        assert_eq!(osa_oracle(source, target), osa);
        assert_eq!(transposition_distance(source, target), osa);
    }
}

#[test]
fn exhaustive_small_strings_match_standard_osa_and_merge_split_oracles() {
    let words = all_words(&['a', 'b'], 4);
    let merge_split = GeneralizedAutomaton::with_operations(8, OperationSet::with_merge_split());
    for source in &words {
        for target in &words {
            assert_eq!(
                standard_distance(source, target),
                standard_oracle(source, target)
            );
            assert_eq!(
                transposition_distance(source, target),
                osa_oracle(source, target)
            );
            assert_eq!(
                merge_split.scaled_distance(source, target).unwrap(),
                Some(merge_split_oracle(source, target)),
                "merge/split mismatch for {source:?} -> {target:?}",
            );
        }
    }
}

#[test]
fn duallity_standard_language_matches_bounded_scan() {
    let terms = [
        "hello",
        "help",
        "hell",
        "world",
        "héllo",
        "東京",
        "👩‍💻",
        "scope::symbol",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect::<Vec<_>>();
    let dictionary = dictionary(&terms);
    for query in ["helo", "hello", "hallo", "東京", "👨‍💻", "scope::symbal"] {
        let expected = bounded_scan(&terms, query, 2);
        let report = duallity_walk(&dictionary, query, 2, 2_000_000, 4_000_000);
        assert_eq!(report.completion, Completion::Complete);
        assert_eq!(as_integral_distances(&report), Some(expected));
    }
}

#[test]
fn explicit_limits_are_observable_instead_of_silent_partial_results() {
    let terms = ["hello", "help", "hell", "world"]
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let report = duallity_walk(&dictionary(&terms), "helo", 2, 1, 100);
    assert_eq!(report.completion, Completion::StateLimit);
}

#[test]
fn standard_distance_is_a_metric_on_the_exhaustive_domain() {
    let words = all_words(&['a', 'b'], 3);
    for a in &words {
        for b in &words {
            let ab = standard_distance(a, b);
            assert_eq!(ab, standard_distance(b, a));
            assert_eq!(ab == 0, a == b);
            for c in &words {
                assert!(ab <= standard_distance(a, c) + standard_distance(c, b));
            }
        }
    }
}

#[test]
fn osa_is_symmetric_and_separating_but_not_a_metric() {
    let words = all_words(&['a', 'b'], 3);
    for a in &words {
        for b in &words {
            let distance = transposition_distance(a, b);
            assert_eq!(distance, transposition_distance(b, a));
            assert_eq!(distance == 0, a == b);
        }
    }
    let direct = transposition_distance("CA", "ABC");
    let via = transposition_distance("CA", "AC") + transposition_distance("AC", "ABC");
    assert_eq!((direct, via), (3, 2));
}

#[test]
fn tropical_semiring_obeys_required_laws_on_finite_nonnegative_weights() {
    for a in 0..=8 {
        for b in 0..=8 {
            for c in 0..=8 {
                let a = TropicalWeight::new(f64::from(a));
                let b = TropicalWeight::new(f64::from(b));
                let c = TropicalWeight::new(f64::from(c));
                assert_eq!(a.plus(&b), b.plus(&a));
                assert_eq!(a.plus(&b).plus(&c), a.plus(&b.plus(&c)));
                assert_eq!(a.times(&b).times(&c), a.times(&b.times(&c)));
                assert_eq!(a.times(&b.plus(&c)), a.times(&b).plus(&a.times(&c)));
                assert_eq!(a.plus(&TropicalWeight::zero()), a);
                assert_eq!(a.times(&TropicalWeight::one()), a);
            }
        }
    }
    assert_eq!(tropical_value(TropicalWeight::zero()), f64::INFINITY);
}

#[test]
fn finite_wfst_composition_has_extensional_identities_and_associativity() {
    let first = [('a', 'x', 1), ('b', 'y', 2)];
    let second = [('x', 'm', 3), ('y', 'n', 1)];
    let third = [('m', 'p', 2), ('n', 'q', 4)];
    let left_identity = [('a', 'a', 0), ('b', 'b', 0)];
    let middle_identity = [('x', 'x', 0), ('y', 'y', 0)];

    let original = relation(compose(mapping(&first), mapping(&middle_identity)).accepting_paths());
    assert_eq!(
        original,
        relation(compose(mapping(&first), mapping(&middle_identity)).accepting_paths())
    );
    assert_eq!(
        relation(compose(mapping(&left_identity), mapping(&first)).accepting_paths()),
        original,
    );

    let first_second = relation(compose(mapping(&first), mapping(&second)).accepting_paths());
    let left =
        relation(compose(materialize_one_step(&first_second), mapping(&third)).accepting_paths());
    let second_third = relation(compose(mapping(&second), mapping(&third)).accepting_paths());
    let right =
        relation(compose(mapping(&first), materialize_one_step(&second_third)).accepting_paths());
    assert_eq!(left, right);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1_000))]

    #[test]
    fn property_standard_and_osa_match_independent_iterative_oracles(
        source in prop::collection::vec(any::<char>(), 0..24),
        target in prop::collection::vec(any::<char>(), 0..24),
    ) {
        let source: String = source.into_iter().collect();
        let target: String = target.into_iter().collect();
        prop_assert_eq!(standard_distance(&source, &target), standard_oracle(&source, &target));
        prop_assert_eq!(transposition_distance(&source, &target), osa_oracle(&source, &target));
    }
}
