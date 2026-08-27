use std::collections::HashSet;

use duallity::{GeneralizedWfst, LazyWfst, LevenshteinWfst, Wfst};
use libcpg_approximate_automata_adoption::{deterministic_terms, dictionary};
use libdictenstein::dynamic_dawg::char::DynamicDawgChar;
use liblevenshtein::distance::standard_distance;
use liblevenshtein::transducer::{OperationSetBuilder, OperationType};

const SMALL_STACK: usize = 256 * 1024;

fn on_small_stack(operation: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(SMALL_STACK)
        .spawn(operation)
        .expect("small-stack thread starts")
        .join()
        .expect("small-stack operation completes");
}

fn direct_probe(length: usize) {
    on_small_stack(move || {
        let source = "a".repeat(length);
        let mut target = source.clone();
        target.replace_range(length - 1.., "b");
        assert_eq!(standard_distance(&source, &target), 1);
    });
}

fn duallity_probe(length: usize) {
    on_small_stack(move || {
        let terms = deterministic_terms(1_024);
        let dictionary = dictionary(&terms);
        let query = "x".repeat(length);
        let mut wfst = LevenshteinWfst::new(&dictionary, &query, 2);
        let mut frontier = vec![wfst.start()];
        let mut seen = HashSet::new();
        while let Some(state) = frontier.pop() {
            if !seen.insert(state) || seen.len() >= 100_000 {
                continue;
            }
            frontier.extend(wfst.transitions_lazy(state).iter().map(|arc| arc.to));
        }
        println!("visited={}", seen.len());
    });
}

fn generalized_recursive_probe(depth: usize) {
    on_small_stack(move || {
        let term = "a".repeat(depth);
        let dictionary = DynamicDawgChar::<()>::from_terms([term.as_str()]);
        let operations = OperationSetBuilder::new()
            .with_operation(OperationType::new(depth, 1, 1.0, "deep-rewrite"))
            .build();
        let mut wfst = GeneralizedWfst::new(&dictionary, "x", 1, operations);
        let start = wfst.start();
        let transitions = wfst.transitions_lazy(start).len();
        println!("transitions={transitions}");
    });
}

fn main() {
    let mut arguments = std::env::args().skip(1);
    let mode = arguments
        .next()
        .expect("mode: direct | duallity | generalized");
    let length: usize = arguments
        .next()
        .expect("length")
        .parse()
        .expect("length is an integer");
    assert!(length > 0);
    match mode.as_str() {
        "direct" => direct_probe(length),
        "duallity" => duallity_probe(length),
        "generalized" => generalized_recursive_probe(length),
        _ => panic!("unknown mode {mode:?}"),
    }
}
