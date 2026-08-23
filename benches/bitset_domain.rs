use std::time::Duration;

use criterion::{
    black_box, criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion, Throughput,
};
use libcpg::{BitSetDomain, Lattice};

const WIDTHS: [usize; 17] = [
    0, 1, 63, 64, 65, 255, 256, 257, 511, 512, 513, 767, 768, 769, 1_024, 8_192, 65_536,
];

fn patterned(width: usize, accepts: impl Fn(usize) -> bool) -> BitSetDomain {
    BitSetDomain::from_indices(
        width,
        (0..width)
            .filter(|&index| accepts(index))
            .map(|index| index as u32),
    )
}

fn benchmark_bitset_domain(c: &mut Criterion) {
    let mut union = c.benchmark_group("bitset_domain/union");
    union.sample_size(30);
    union.warm_up_time(Duration::from_secs(1));
    union.measurement_time(Duration::from_secs(2));
    for width in WIDTHS {
        let left = patterned(width, |index| index % 2 == 0);
        let right = patterned(width, |index| index % 2 == 1);
        let mut oracle = left.clone();
        oracle.join(&right);
        assert_eq!(oracle, BitSetDomain::full(width));
        union.throughput(Throughput::Elements(width.div_ceil(64) as u64));
        union.bench_with_input(BenchmarkId::from_parameter(width), &width, |b, _| {
            b.iter_batched(
                || left.clone(),
                |mut destination| black_box(destination.join(black_box(&right))),
                BatchSize::SmallInput,
            );
        });
    }
    union.finish();

    let mut intersection = c.benchmark_group("bitset_domain/intersection");
    intersection.sample_size(30);
    intersection.warm_up_time(Duration::from_secs(1));
    intersection.measurement_time(Duration::from_secs(2));
    for width in WIDTHS {
        let left = BitSetDomain::full(width);
        let right = patterned(width, |index| index % 3 == 0);
        let mut oracle = left.clone();
        oracle.intersect_with(&right);
        assert_eq!(oracle, right);
        intersection.throughput(Throughput::Elements(width.div_ceil(64) as u64));
        intersection.bench_with_input(BenchmarkId::from_parameter(width), &width, |b, _| {
            b.iter_batched(
                || left.clone(),
                |mut destination| black_box(destination.intersect_with(black_box(&right))),
                BatchSize::SmallInput,
            );
        });
    }
    intersection.finish();

    let mut subset = c.benchmark_group("bitset_domain/subset_true");
    subset.sample_size(30);
    subset.warm_up_time(Duration::from_secs(1));
    subset.measurement_time(Duration::from_secs(2));
    for width in WIDTHS {
        let left = patterned(width, |index| index % 3 == 0);
        let right = BitSetDomain::full(width);
        assert!(left.leq(&right));
        subset.throughput(Throughput::Elements(width.div_ceil(64) as u64));
        subset.bench_with_input(BenchmarkId::from_parameter(width), &width, |b, _| {
            b.iter(|| black_box(black_box(&left).leq(black_box(&right))));
        });
    }
    subset.finish();
}

criterion_group!(benches, benchmark_bitset_domain);
criterion_main!(benches);
