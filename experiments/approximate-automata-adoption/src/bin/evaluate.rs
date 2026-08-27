use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::BTreeMap;
use std::hint::black_box;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use libcpg_approximate_automata_adoption::{
    as_integral_distances, bounded_scan, deterministic_terms, dictionary, duallity_walk, Completion,
};
use libdictenstein::Dictionary;

struct CountingAllocator;

static ALLOCATION_CALLS: AtomicU64 = AtomicU64::new(0);
static ALLOCATED_BYTES: AtomicU64 = AtomicU64::new(0);

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATION_CALLS.fetch_add(1, Ordering::Relaxed);
        ALLOCATED_BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        System.alloc(layout)
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        System.dealloc(pointer, layout);
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCATION_CALLS.fetch_add(1, Ordering::Relaxed);
        ALLOCATED_BYTES.fetch_add(new_size as u64, Ordering::Relaxed);
        System.realloc(pointer, layout, new_size)
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

struct Sample {
    median: Duration,
    calls: u64,
    bytes: u64,
    checksum: u64,
}

fn measure(mut operation: impl FnMut() -> u64) -> Sample {
    for _ in 0..3 {
        black_box(operation());
    }
    let mut durations = Vec::with_capacity(11);
    let mut calls = 0;
    let mut bytes = 0;
    let mut checksum = 0;
    for _ in 0..11 {
        ALLOCATION_CALLS.store(0, Ordering::Relaxed);
        ALLOCATED_BYTES.store(0, Ordering::Relaxed);
        let started = Instant::now();
        checksum ^= black_box(operation());
        durations.push(started.elapsed());
        calls = calls.max(ALLOCATION_CALLS.load(Ordering::Relaxed));
        bytes = bytes.max(ALLOCATED_BYTES.load(Ordering::Relaxed));
    }
    durations.sort_unstable();
    Sample {
        median: durations[durations.len() / 2],
        calls,
        bytes,
        checksum,
    }
}

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

fn emit(
    scenario: &str,
    corpus: usize,
    implementation: &str,
    sample: Sample,
    matches: usize,
    exact: bool,
) {
    println!(
        "{scenario}\t{corpus}\t{implementation}\t{}\t{}\t{}\t{}\t{matches}\t{exact}",
        sample.median.as_nanos(),
        sample.calls,
        sample.bytes,
        sample.checksum,
    );
}

fn api_sequences(count: usize) -> Vec<String> {
    const VERBS: [&str; 8] = [
        "open", "parse", "resolve", "validate", "lower", "analyze", "emit", "close",
    ];
    (0..count)
        .map(|index| {
            format!(
                "{}>{}>{}>{:05}",
                VERBS[index % VERBS.len()],
                VERBS[(index / VERBS.len() + 2) % VERBS.len()],
                VERBS[(index / (VERBS.len() * VERBS.len()) + 4) % VERBS.len()],
                index,
            )
        })
        .collect()
}

fn unicode_identifiers(count: usize) -> Vec<String> {
    const MODULES: [&str; 6] = ["解析", "δοκιμή", "данные", "módulo", "東京", "🦀"];
    (0..count)
        .map(|index| format!("{}::σύμβολο_{:05}", MODULES[index % MODULES.len()], index,))
        .collect()
}

fn clone_shingles(count: usize) -> Vec<String> {
    const OPERATORS: [&str; 5] = ["add", "sub", "mul", "div", "rem"];
    (0..count)
        .map(|index| {
            format!(
                "load local_{:03} | load constant_{:03} | {} | store result_{:05}",
                index % 113,
                index % 79,
                OPERATORS[index % OPERATORS.len()],
                index,
            )
        })
        .collect()
}

fn evaluate_scenario(scenario: &str, terms: Vec<String>, query: String) {
    let corpus_size = terms.len();
    let expected = bounded_scan(&terms, &query, 2);

    let exact_build = measure(|| {
        let index: BTreeMap<_, _> = terms.iter().map(|term| (term.clone(), ())).collect();
        index.len() as u64
    });
    emit(
        scenario,
        corpus_size,
        "libcpg-ordered-build",
        exact_build,
        0,
        true,
    );

    let exact_index: BTreeMap<_, _> = terms.iter().map(|term| (term.clone(), ())).collect();
    let exact_sample = measure(|| u64::from(exact_index.contains_key(black_box(&query))));
    emit(
        scenario,
        corpus_size,
        "libcpg-ordered-exact",
        exact_sample,
        0,
        true,
    );

    let direct_sample = measure(|| checksum(&bounded_scan(black_box(&terms), &query, 2)));
    emit(
        scenario,
        corpus_size,
        "liblevenshtein-bounded-scan",
        direct_sample,
        expected.len(),
        true,
    );

    let dawg_build = measure(|| dictionary(black_box(&terms)).len().unwrap_or(0) as u64);
    emit(
        scenario,
        corpus_size,
        "duallity-dawg-build",
        dawg_build,
        0,
        true,
    );

    let dictionary = dictionary(&terms);
    let mut last = None;
    let duallity_sample = measure(|| {
        let report = duallity_walk(&dictionary, &query, 2, 2_000_000, 8_000_000);
        assert_eq!(report.completion, Completion::Complete);
        let distances = as_integral_distances(&report).expect("integral tropical weights");
        let value = checksum(&distances);
        last = Some(distances);
        value
    });
    let observed = last.expect("measurement ran");
    emit(
        scenario,
        corpus_size,
        "duallity-wfst",
        duallity_sample,
        observed.len(),
        observed == expected,
    );
}

fn main() {
    println!("scenario\tcorpus\timplementation\tmedian_ns\tallocation_calls\tallocated_bytes\tchecksum\tmatches\texact");
    for corpus_size in [256, 4_096] {
        let query = format!(
            "scope::module_{:04}::symbol_{:05}x",
            (corpus_size / 2) % 97,
            corpus_size / 2,
        );
        evaluate_scenario("qualified-symbols", deterministic_terms(corpus_size), query);
    }
    evaluate_scenario(
        "unicode-identifiers",
        unicode_identifiers(1_024),
        "東京::σύμβολο_00512x".to_owned(),
    );
    evaluate_scenario(
        "api-sequences",
        api_sequences(1_024),
        "open>resolve>lower>00512x".to_owned(),
    );
    evaluate_scenario(
        "clone-shingles",
        clone_shingles(1_024),
        "load local_060 | load constant_038 | mul | store result_00512x".to_owned(),
    );
}
