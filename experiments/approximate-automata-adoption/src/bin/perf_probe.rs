use std::collections::BTreeMap;
use std::hint::black_box;

use libcpg_approximate_automata_adoption::{
    as_integral_distances, bounded_scan, deterministic_terms, dictionary, duallity_walk, Completion,
};

fn checksum(distances: &BTreeMap<String, usize>) -> u64 {
    distances
        .iter()
        .fold(0xcbf29ce484222325, |hash, (term, distance)| {
            term.bytes()
                .chain(distance.to_le_bytes())
                .fold(hash, |hash, byte| {
                    (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
                })
        })
}

fn main() {
    let mut arguments = std::env::args().skip(1);
    let mode = arguments.next().expect("mode: direct | duallity");
    let iterations: usize = arguments
        .next()
        .expect("iterations")
        .parse()
        .expect("iterations is an integer");
    assert!(iterations > 0);

    let terms = deterministic_terms(4_096);
    let query = "scope::module_0013::symbol_02048x";
    let dictionary = dictionary(&terms);
    let mut aggregate = 0u64;

    for _ in 0..iterations {
        let current = match mode.as_str() {
            "direct" => checksum(&bounded_scan(black_box(&terms), query, 2)),
            "duallity" => {
                let report = duallity_walk(&dictionary, query, 2, 2_000_000, 8_000_000);
                assert_eq!(report.completion, Completion::Complete);
                checksum(&as_integral_distances(&report).expect("integral tropical weights"))
            }
            _ => panic!("unknown mode {mode:?}"),
        };
        aggregate = aggregate.wrapping_add(black_box(current));
    }

    println!("mode={mode}\titerations={iterations}\tchecksum={aggregate}");
}
