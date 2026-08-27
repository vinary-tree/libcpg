use std::collections::{BTreeMap, HashMap};

use duallity::{LazyWfst, LevenshteinWfst, StateId, TropicalWeight, Wfst};
use libdictenstein::dynamic_dawg::char::DynamicDawgChar;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Completion {
    Complete,
    StateLimit,
    WorkLimit,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WalkReport {
    pub completion: Completion,
    pub accepted: BTreeMap<String, f64>,
    pub distinct_configurations: usize,
    pub work_items: usize,
}

pub fn standard_oracle(source: &str, target: &str) -> usize {
    let source: Vec<char> = source.chars().collect();
    let target: Vec<char> = target.chars().collect();
    let mut previous: Vec<usize> = (0..=target.len()).collect();
    let mut current = vec![0; target.len() + 1];

    for (row, source_char) in source.iter().enumerate() {
        current[0] = row + 1;
        for (column, target_char) in target.iter().enumerate() {
            current[column + 1] = (previous[column + 1] + 1)
                .min(current[column] + 1)
                .min(previous[column] + usize::from(source_char != target_char));
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[target.len()]
}

pub fn osa_oracle(source: &str, target: &str) -> usize {
    let source: Vec<char> = source.chars().collect();
    let target: Vec<char> = target.chars().collect();
    let mut two_back = vec![0; target.len() + 1];
    let mut previous: Vec<usize> = (0..=target.len()).collect();
    let mut current = vec![0; target.len() + 1];

    for i in 1..=source.len() {
        current[0] = i;
        for j in 1..=target.len() {
            let mut distance = (previous[j] + 1)
                .min(current[j - 1] + 1)
                .min(previous[j - 1] + usize::from(source[i - 1] != target[j - 1]));
            if i > 1 && j > 1 && source[i - 1] == target[j - 2] && source[i - 2] == target[j - 1] {
                distance = distance.min(two_back[j - 2] + 1);
            }
            current[j] = distance;
        }
        std::mem::swap(&mut two_back, &mut previous);
        std::mem::swap(&mut previous, &mut current);
    }
    previous[target.len()]
}

pub fn merge_split_oracle(source: &str, target: &str) -> usize {
    let source: Vec<char> = source.chars().collect();
    let target: Vec<char> = target.chars().collect();
    let width = target.len() + 1;
    let mut distance = vec![usize::MAX; (source.len() + 1) * width];
    distance[0] = 0;

    for i in 0..=source.len() {
        for j in 0..=target.len() {
            let current = distance[i * width + j];
            if current == usize::MAX {
                continue;
            }
            let mut relax = |next_i: usize, next_j: usize, cost: usize| {
                let cell = &mut distance[next_i * width + next_j];
                *cell = (*cell).min(current + cost);
            };
            if i < source.len() {
                relax(i + 1, j, 1);
            }
            if j < target.len() {
                relax(i, j + 1, 1);
            }
            if i < source.len() && j < target.len() {
                relax(i + 1, j + 1, usize::from(source[i] != target[j]));
            }
            if i < source.len() && j + 1 < target.len() {
                relax(i + 1, j + 2, 1);
            }
            if i + 1 < source.len() && j < target.len() {
                relax(i + 2, j + 1, 1);
            }
        }
    }
    distance[source.len() * width + target.len()]
}

pub fn bounded_scan(terms: &[String], query: &str, max_distance: usize) -> BTreeMap<String, usize> {
    terms
        .iter()
        .filter_map(|term| {
            let distance =
                liblevenshtein::distance::standard_distance_bounded(term, query, max_distance)?;
            Some((term.clone(), distance))
        })
        .collect()
}

pub fn duallity_walk(
    dictionary: &DynamicDawgChar<()>,
    query: &str,
    max_distance: usize,
    state_limit: usize,
    work_limit: usize,
) -> WalkReport {
    let mut wfst = LevenshteinWfst::new(dictionary, query, max_distance);
    let mut best_at: HashMap<(StateId, String), f64> = HashMap::new();
    let mut accepted = BTreeMap::new();
    let mut frontier = vec![(wfst.start(), String::new(), 0.0)];
    let mut work_items = 0usize;

    while let Some((state, output, weight)) = frontier.pop() {
        work_items += 1;
        if work_items > work_limit {
            return WalkReport {
                completion: Completion::WorkLimit,
                accepted,
                distinct_configurations: best_at.len(),
                work_items,
            };
        }

        let key = (state, output.clone());
        if best_at.get(&key).is_some_and(|seen| *seen <= weight) {
            continue;
        }
        if !best_at.contains_key(&key) && best_at.len() >= state_limit {
            return WalkReport {
                completion: Completion::StateLimit,
                accepted,
                distinct_configurations: best_at.len(),
                work_items,
            };
        }
        best_at.insert(key, weight);

        if wfst.is_final(state) {
            let total = weight + wfst.final_weight(state).value();
            accepted
                .entry(output.clone())
                .and_modify(|incumbent| *incumbent = f64::min(*incumbent, total))
                .or_insert(total);
        }

        let transitions: Vec<_> = wfst
            .transitions_lazy(state)
            .iter()
            .map(|transition| (transition.to, transition.output, transition.weight.value()))
            .collect();
        for (target, output_label, transition_weight) in transitions {
            let mut next_output = output.clone();
            if let Some(label) = output_label {
                next_output.push(label);
            }
            frontier.push((target, next_output, weight + transition_weight));
        }
    }

    WalkReport {
        completion: Completion::Complete,
        accepted,
        distinct_configurations: best_at.len(),
        work_items,
    }
}

pub fn all_words(alphabet: &[char], max_len: usize) -> Vec<String> {
    let mut words = vec![String::new()];
    for length in 1..=max_len {
        let count = alphabet.len().pow(length as u32);
        for ordinal in 0..count {
            let mut value = ordinal;
            let mut word = vec![alphabet[0]; length];
            for character in word.iter_mut().rev() {
                *character = alphabet[value % alphabet.len()];
                value /= alphabet.len();
            }
            words.push(word.into_iter().collect());
        }
    }
    words
}

pub fn deterministic_terms(count: usize) -> Vec<String> {
    (0..count)
        .map(|index| format!("scope::module_{:04}::symbol_{:05}", index % 97, index))
        .collect()
}

pub fn dictionary(terms: &[String]) -> DynamicDawgChar<()> {
    DynamicDawgChar::from_terms(terms.iter().map(String::as_str))
}

pub fn as_integral_distances(report: &WalkReport) -> Option<BTreeMap<String, usize>> {
    report
        .accepted
        .iter()
        .map(|(term, weight)| {
            if weight.is_finite() && *weight >= 0.0 && weight.fract() == 0.0 {
                Some((term.clone(), *weight as usize))
            } else {
                None
            }
        })
        .collect()
}

pub fn tropical_value(weight: TropicalWeight) -> f64 {
    weight.value()
}
