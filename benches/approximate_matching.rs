use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use libcpg::{
    approximate_candidates, ApproximateCancellation, ApproximateCompletion, ApproximateConfig,
    ApproximateConsumer, ApproximateId, ApproximateMethod, ApproximateRecord, ApproximateTape,
};

struct CountingAllocator;

static COUNT_ALLOCATIONS: AtomicBool = AtomicBool::new(false);
static ALLOCATION_CALLS: AtomicU64 = AtomicU64::new(0);
static ALLOCATED_BYTES: AtomicU64 = AtomicU64::new(0);

// The benchmark allocator delegates every operation to the platform allocator.
// Its atomics are enabled only around the measured call, so Criterion's own
// allocations do not contaminate the algorithm profile.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: this implementation preserves `GlobalAlloc::alloc`'s
        // contract by forwarding the unchanged layout to `System`.
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() && COUNT_ALLOCATIONS.load(Ordering::Relaxed) {
            ALLOCATION_CALLS.fetch_add(1, Ordering::Relaxed);
            ALLOCATED_BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        }
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: this implementation preserves `GlobalAlloc::alloc_zeroed`'s
        // contract by forwarding the unchanged layout to `System`.
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() && COUNT_ALLOCATIONS.load(Ordering::Relaxed) {
            ALLOCATION_CALLS.fetch_add(1, Ordering::Relaxed);
            ALLOCATED_BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: `pointer` and `layout` are exactly those supplied by the
        // caller under `GlobalAlloc::dealloc`'s contract.
        unsafe { System.dealloc(pointer, layout) };
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: the complete reallocation request is forwarded unchanged to
        // the platform allocator.
        let replacement = unsafe { System.realloc(pointer, layout, new_size) };
        if !replacement.is_null() && COUNT_ALLOCATIONS.load(Ordering::Relaxed) {
            ALLOCATION_CALLS.fetch_add(1, Ordering::Relaxed);
            ALLOCATED_BYTES.fetch_add(new_size as u64, Ordering::Relaxed);
        }
        replacement
    }
}

#[global_allocator]
static GLOBAL_ALLOCATOR: CountingAllocator = CountingAllocator;

#[derive(Clone, Copy, Debug)]
struct AllocationProfile {
    calls: u64,
    bytes: u64,
}

fn profile_allocations<T>(operation: impl FnOnce() -> T) -> (T, AllocationProfile) {
    COUNT_ALLOCATIONS.store(false, Ordering::SeqCst);
    ALLOCATION_CALLS.store(0, Ordering::Relaxed);
    ALLOCATED_BYTES.store(0, Ordering::Relaxed);
    COUNT_ALLOCATIONS.store(true, Ordering::SeqCst);
    let result = operation();
    COUNT_ALLOCATIONS.store(false, Ordering::SeqCst);
    (
        result,
        AllocationProfile {
            calls: ALLOCATION_CALLS.load(Ordering::Relaxed),
            bytes: ALLOCATED_BYTES.load(Ordering::Relaxed),
        },
    )
}

#[derive(Debug)]
struct SearchFixture {
    query: ApproximateRecord,
    candidates: Vec<ApproximateRecord>,
    exact_index: BTreeMap<String, ApproximateId>,
    expected: ApproximateId,
    config: ApproximateConfig,
}

fn symbol(index: usize) -> String {
    format!("workspace::module_{index:05}::validate_payload_{index:05}")
}

fn fixture(candidate_count: usize, noisy: bool) -> SearchFixture {
    let target = candidate_count / 2;
    let candidates = (0..candidate_count)
        .map(|index| {
            ApproximateRecord::new(
                ApproximateId::External {
                    corpus: candidate_count as u64,
                    item: index as u64,
                },
                ApproximateTape::text(symbol(index)),
            )
        })
        .collect::<Vec<_>>();
    let exact_index = candidates
        .iter()
        .map(|record| {
            let ApproximateTape::Text(text) = record.tape.as_ref() else {
                unreachable!("symbol fixture contains only text tapes");
            };
            (text.to_string(), record.id)
        })
        .collect();
    let expected = candidates[target].id;
    let mut query_text = symbol(target);
    if noisy {
        query_text = query_text.replacen("payload", "paylod", 1);
    }
    SearchFixture {
        query: ApproximateRecord::new(
            ApproximateId::External {
                corpus: candidate_count as u64,
                item: candidate_count as u64,
            },
            ApproximateTape::text(query_text),
        ),
        candidates,
        exact_index,
        expected,
        config: ApproximateConfig {
            method: ApproximateMethod::DamerauLevenshtein,
            max_distance: 1,
            top_k: 8,
            ..ApproximateConfig::default()
        },
    }
}

fn exact_lookup(fixture: &SearchFixture) -> Option<ApproximateId> {
    let ApproximateTape::Text(query) = fixture.query.tape.as_ref() else {
        unreachable!("symbol fixture contains only text tapes");
    };
    fixture.exact_index.get(query.as_ref()).copied()
}

fn approximate_lookup(fixture: &SearchFixture) -> libcpg::ApproximateReport {
    approximate_candidates(
        &fixture.query,
        &fixture.candidates,
        ApproximateConsumer::SymbolNavigation,
        &fixture.config,
        &ApproximateCancellation::default(),
    )
    .expect("bounded approximate symbol search")
}

#[derive(Clone, Copy, Debug)]
struct QualityMetrics {
    exact_recall_at_one: f64,
    approximate_recall_at_one: f64,
    approximate_mean_reciprocal_rank: f64,
}

fn quality_metrics() -> QualityMetrics {
    let corpus = [
        ("parse_configuration", "parse_configuration"),
        ("validate_payload", "validate_payload"),
        ("calculate_checksum", "calculate_checksum"),
        ("resolve_qualified_name", "resolve_qualified_name"),
        ("serialize_evidence", "serialize_evidence"),
        ("compose_relation", "compose_relation"),
        ("authentication_guard", "authentcation_guard"),
        ("normalization_policy", "normalisation_policy"),
        ("transaction_boundary", "transaction_boundry"),
        ("Damerau_alignment", "Damerua_alignment"),
        ("café_parser", "cafe_parser"),
        ("generated_adapter_42", "generated_adaptor_42"),
    ];
    let candidates = corpus
        .iter()
        .enumerate()
        .map(|(index, (candidate, _))| {
            ApproximateRecord::new(
                ApproximateId::External {
                    corpus: 0x5155_414c,
                    item: index as u64,
                },
                ApproximateTape::text(*candidate),
            )
        })
        .collect::<Vec<_>>();
    let exact_index = candidates
        .iter()
        .map(|record| {
            let ApproximateTape::Text(text) = record.tape.as_ref() else {
                unreachable!("quality corpus contains only text tapes");
            };
            (text.to_string(), record.id)
        })
        .collect::<BTreeMap<_, _>>();
    let config = ApproximateConfig {
        method: ApproximateMethod::DamerauLevenshtein,
        max_distance: 2,
        top_k: 4,
        ..ApproximateConfig::default()
    };
    let mut exact_hits = 0usize;
    let mut approximate_hits = 0usize;
    let mut reciprocal_rank_sum = 0.0;
    for (index, (_, query_text)) in corpus.iter().enumerate() {
        let expected = candidates[index].id;
        exact_hits += usize::from(exact_index.get(*query_text).copied() == Some(expected));
        let query = ApproximateRecord::new(
            ApproximateId::External {
                corpus: 0x5155_414c,
                item: (corpus.len() + index) as u64,
            },
            ApproximateTape::text(*query_text),
        );
        let report = approximate_candidates(
            &query,
            &candidates,
            ApproximateConsumer::SymbolNavigation,
            &config,
            &ApproximateCancellation::default(),
        )
        .expect("quality-corpus candidate search");
        assert_eq!(report.completion, ApproximateCompletion::Complete);
        let rank = report
            .evidence
            .iter()
            .position(|evidence| evidence.candidate == expected)
            .map(|position| position + 1)
            .expect("labeled candidate occurs within the quality corpus top-k");
        approximate_hits += usize::from(rank == 1);
        reciprocal_rank_sum += 1.0 / rank as f64;
    }
    let denominator = corpus.len() as f64;
    QualityMetrics {
        exact_recall_at_one: exact_hits as f64 / denominator,
        approximate_recall_at_one: approximate_hits as f64 / denominator,
        approximate_mean_reciprocal_rank: reciprocal_rank_sum / denominator,
    }
}

fn assert_fixture(fixture: &SearchFixture, noisy: bool) {
    let exact = exact_lookup(fixture);
    if noisy {
        assert_eq!(exact, None);
    } else {
        assert_eq!(exact, Some(fixture.expected));
    }
    let report = approximate_lookup(fixture);
    assert_eq!(report.completion, ApproximateCompletion::Complete);
    assert_eq!(report.evidence[0].candidate, fixture.expected);
}

fn benchmark_approximate_matching(c: &mut Criterion) {
    let quality = quality_metrics();
    assert_eq!(quality.exact_recall_at_one, 0.5);
    assert_eq!(quality.approximate_recall_at_one, 1.0);
    assert_eq!(quality.approximate_mean_reciprocal_rank, 1.0);
    eprintln!(
        "quality,exact_recall_at_1={:.3},approximate_recall_at_1={:.3},approximate_mrr={:.3}",
        quality.exact_recall_at_one,
        quality.approximate_recall_at_one,
        quality.approximate_mean_reciprocal_rank,
    );

    let mut group = c.benchmark_group("approximate_matching/symbol_navigation");
    group.sample_size(10);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(2));
    for candidate_count in [256usize, 4_096] {
        for noisy in [false, true] {
            let fixture = fixture(candidate_count, noisy);
            assert_fixture(&fixture, noisy);
            let (profiled_report, approximate_allocations) =
                profile_allocations(|| approximate_lookup(&fixture));
            let (_, exact_allocations) = profile_allocations(|| exact_lookup(&fixture));
            eprintln!(
                "resources,candidates={candidate_count},noisy={noisy},\
                 exact_allocations={},exact_allocated_bytes={},\
                 approximate_allocations={},approximate_allocated_bytes={},\
                 algorithm_peak_live_bytes={}",
                exact_allocations.calls,
                exact_allocations.bytes,
                approximate_allocations.calls,
                approximate_allocations.bytes,
                profiled_report.counters.peak_memory_bytes,
            );

            let case = if noisy { "noisy" } else { "exact" };
            group.throughput(Throughput::Elements(candidate_count as u64));
            group.bench_with_input(
                BenchmarkId::new(format!("exact_btree/{case}"), candidate_count),
                &fixture,
                |benchmark, fixture| {
                    benchmark.iter(|| black_box(exact_lookup(black_box(fixture))));
                },
            );
            group.bench_with_input(
                BenchmarkId::new(format!("bounded_scan/{case}"), candidate_count),
                &fixture,
                |benchmark, fixture| {
                    benchmark.iter(|| black_box(approximate_lookup(black_box(fixture))));
                },
            );
        }
    }
    group.finish();
}

criterion_group!(benches, benchmark_approximate_matching);
criterion_main!(benches);
