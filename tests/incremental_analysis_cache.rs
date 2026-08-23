#![cfg(feature = "lang-rust")]

use std::fmt::Write as _;

use libcpg::builder::{
    CpgBuilder, CpgBuilderConfig, FunctionAnalysisCache, FunctionAnalysisCacheLimits, PdgBuilder,
    TreeSitterCpgBuilder,
};
use libcpg::{CodePropertyGraph, Language};
use proptest::prelude::*;

fn fingerprint(cpg: &CodePropertyGraph) -> String {
    format!(
        "language={:?}\nroot={:?}\nentries={:?}\nexits={:?}\nnodes={:?}\nedges={:?}",
        cpg.language(),
        cpg.root(),
        cpg.cfg_entries(),
        cpg.cfg_exits(),
        cpg.nodes().collect::<Vec<_>>(),
        cpg.edges().collect::<Vec<_>>()
    )
}

fn assert_cached_equals_fresh(
    builder: &TreeSitterCpgBuilder,
    source: &str,
    language: Language,
    cache: &mut FunctionAnalysisCache,
) -> CodePropertyGraph {
    let fresh = builder
        .build(source, language)
        .expect("fresh analysis succeeds");
    let cached = builder
        .build_with_analysis_cache(source, language, cache)
        .expect("cached analysis succeeds");
    assert_eq!(fingerprint(&cached), fingerprint(&fresh));
    cached
}

#[test]
fn cold_hit_and_edit_matrix_are_exact() {
    let builder =
        TreeSitterCpgBuilder::with_config(CpgBuilderConfig::default().with_import_resolution(true));
    let base = r#"
struct Boxed { value: i64 }
fn callee(input: i64) -> i64 { input + 1 }
fn caller(seed: i64) -> i64 { callee(seed) }
fn duplicate_a(value: i64) -> i64 { value * 2 }
fn duplicate_b(value: i64) -> i64 { value * 2 }
"#;
    let edits = [
        base.to_owned(),
        base.replace("input + 1", "input + 2"),
        base.replace("fn callee", "fn renamed"),
        base.replace("input: i64", "input: u64"),
        base.replace("callee(seed)", "duplicate_a(seed)"),
        format!("const GLOBAL: i64 = 9;\n{base}"),
        format!("use std::fmt::Debug;\n{base}"),
        base.replace("struct Boxed", "struct RenamedBox"),
        base.replace("value: i64", "value: u64"),
    ];

    let mut cache = FunctionAnalysisCache::new();
    let first = assert_cached_equals_fresh(&builder, base, Language::Rust, &mut cache);
    let preserved = fingerprint(&first);
    let second = assert_cached_equals_fresh(&builder, base, Language::Rust, &mut cache);
    assert_eq!(
        fingerprint(&first),
        preserved,
        "later reuse cannot mutate an older graph"
    );
    assert_eq!(fingerprint(&second), preserved);
    assert!(cache.stats().cfg_hits >= 4);
    assert!(cache.stats().dfg_hits >= 3);

    for source in edits {
        let _ = assert_cached_equals_fresh(&builder, &source, Language::Rust, &mut cache);
    }

    let changed_options = TreeSitterCpgBuilder::with_config(
        CpgBuilderConfig::default()
            .with_comments(true)
            .with_import_resolution(false)
            .with_parallel_analysis(true),
    );
    let _ = assert_cached_equals_fresh(
        &changed_options,
        "// retained only by this configuration\nfn f(x: i64) -> i64 { x + 1 }",
        Language::Rust,
        &mut cache,
    );
}

#[test]
fn bounded_eviction_and_low_hit_churn_preserve_results() {
    let builder = TreeSitterCpgBuilder::new();
    let mut cache =
        FunctionAnalysisCache::with_limits(FunctionAnalysisCacheLimits::new(3, 128 * 1024));
    for revision in 0..12 {
        let source = format!(
            "fn stable(x: i64) -> i64 {{ x + 1 }}\nfn churn_{revision}(x: i64) -> i64 {{ x + {revision} }}"
        );
        let _ = assert_cached_equals_fresh(&builder, &source, Language::Rust, &mut cache);
        assert!(cache.stats().entries <= 3);
        assert!(cache.stats().resident_bytes <= 128 * 1024);
        let stats = cache.stats();
        assert_eq!(
            stats.resident_bytes,
            stats
                .resident_key_bytes
                .saturating_add(stats.resident_artifact_bytes)
                .saturating_add(stats.resident_metadata_bytes)
        );
        assert!(stats.resident_key_bytes > 0);
        assert!(stats.resident_artifact_bytes > 0);
    }
    assert!(cache.stats().evictions > 0);
}

#[test]
fn zero_capacity_cache_is_an_observable_exact_passthrough() {
    let source = "fn passthrough(value: i64) -> i64 { value + 1 }";
    let builder = TreeSitterCpgBuilder::new();
    let fresh = builder
        .build(source, Language::Rust)
        .expect("fresh passthrough build succeeds");
    let mut disabled = FunctionAnalysisCache::with_limits(FunctionAnalysisCacheLimits::new(0, 0));
    let cached = builder
        .build_with_analysis_cache(source, Language::Rust, &mut disabled)
        .expect("zero-capacity cached build succeeds");

    assert_eq!(fingerprint(&cached), fingerprint(&fresh));
    let stats = disabled.stats();
    assert_eq!(stats.capacity_bypasses, 1);
    assert_eq!(stats.entries, 0);
    assert_eq!(stats.cfg_hits + stats.cfg_misses, 0);
    assert_eq!(stats.dfg_hits + stats.dfg_misses, 0);
}

#[test]
fn nonlocal_dfg_dependencies_fail_closed_and_remain_exact() {
    let builder =
        TreeSitterCpgBuilder::with_config(CpgBuilderConfig::default().with_import_resolution(true));
    let sources = [
        "fn callee(x: i64)->i64{x}\nfn caller()->i64{callee(1)}",
        "fn callee(x: i64)->i64{x}\nfn caller()->i64{callee(2)}",
        "let global = 1; fn captures()->i64{global}",
        "struct S { field: i64 } impl S { fn read(&self)->i64{self.field} }",
        "fn outer()->i64{fn nested()->i64{1} nested()}",
    ];
    let mut cache = FunctionAnalysisCache::new();
    for source in sources {
        let _ = assert_cached_equals_fresh(&builder, source, Language::Rust, &mut cache);
        let _ = assert_cached_equals_fresh(&builder, source, Language::Rust, &mut cache);
    }
    assert!(cache.stats().dfg_dependency_bypasses > 0);
}

#[test]
fn pdg_cache_matches_fresh_serial_and_parallel_builds() {
    let source = r#"
fn classify(mut x: i64) -> i64 {
    if x > 0 { x = x + 1; } else { x = x - 1; }
    x
}
fn second(y: i64) -> i64 { y * 2 }
"#;
    let builder = TreeSitterCpgBuilder::new();
    let mut cache = FunctionAnalysisCache::new();

    let mut first = builder.build(source, Language::Rust).expect("build first");
    PdgBuilder::new().build_all_with_analysis_cache(&mut first, &mut cache);
    let first_fingerprint = fingerprint(&first);

    let mut second = builder.build(source, Language::Rust).expect("build second");
    PdgBuilder::new().build_all_parallel_with_analysis_cache(&mut second, &mut cache);
    assert_eq!(fingerprint(&second), first_fingerprint);
    assert_eq!(cache.stats().pdg_hits, 2);

    let mut fresh = builder.build(source, Language::Rust).expect("build oracle");
    PdgBuilder::new().build_all(&mut fresh);
    assert_eq!(fingerprint(&fresh), first_fingerprint);
}

#[cfg(feature = "lang-python")]
#[test]
fn one_cache_is_language_separated_across_rust_and_python() {
    let builder = TreeSitterCpgBuilder::new();
    let mut cache = FunctionAnalysisCache::new();
    let _ = assert_cached_equals_fresh(
        &builder,
        "fn identity(x: i64) -> i64 { x }",
        Language::Rust,
        &mut cache,
    );
    let _ = assert_cached_equals_fresh(
        &builder,
        "def identity(x):\n    return x\n",
        Language::Python,
        &mut cache,
    );
    let _ = assert_cached_equals_fresh(
        &builder,
        "def identity(x):\n    return x\n",
        Language::Python,
        &mut cache,
    );
    assert!(cache.stats().cfg_hits >= 1);
}

#[cfg(feature = "serde")]
#[test]
fn cached_graph_serialization_is_deterministic() {
    let source = "fn deterministic(x: i64) -> i64 { x + 1 }";
    let builder = TreeSitterCpgBuilder::new();
    let mut cache = FunctionAnalysisCache::new();
    let first = builder
        .build_with_analysis_cache(source, Language::Rust, &mut cache)
        .expect("first cached build");
    let second = builder
        .build_with_analysis_cache(source, Language::Rust, &mut cache)
        .expect("second cached build");
    assert_eq!(
        serde_json::to_vec(&first).expect("serialize first"),
        serde_json::to_vec(&second).expect("serialize second")
    );
}

fn generated_program(constants: &[i16], edit: Option<(usize, i16)>) -> String {
    let mut source = String::new();
    for (index, &constant) in constants.iter().enumerate() {
        let constant = edit
            .filter(|(edited, _)| *edited == index)
            .map_or(constant, |(_, replacement)| replacement);
        writeln!(
            &mut source,
            "fn function_{index}(input: i64) -> i64 {{ input.wrapping_add({constant}) }}"
        )
        .expect("writing to a String cannot fail");
    }
    source
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn generated_edit_sequences_equal_fresh_oracle(
        constants in proptest::collection::vec(any::<i16>(), 1..24),
        replacement in any::<i16>(),
        permutation_seed in any::<u64>(),
    ) {
        let builder = TreeSitterCpgBuilder::new();
        let mut cache = FunctionAnalysisCache::new();
        let original = generated_program(&constants, None);
        let edited_index = (permutation_seed as usize) % constants.len();
        let edited = generated_program(&constants, Some((edited_index, replacement)));

        let first = builder
            .build_with_analysis_cache(&original, Language::Rust, &mut cache)
            .expect("generated original parses");
        let cached = builder
            .build_with_analysis_cache(&edited, Language::Rust, &mut cache)
            .expect("generated edit parses");
        let fresh = builder
            .build(&edited, Language::Rust)
            .expect("generated fresh oracle parses");
        prop_assert_eq!(fingerprint(&cached), fingerprint(&fresh));

        let mut permuted = constants.clone();
        let rotation = (permutation_seed as usize) % permuted.len();
        permuted.rotate_left(rotation);
        let permuted_source = generated_program(&permuted, None);
        let permuted_cached = builder
            .build_with_analysis_cache(&permuted_source, Language::Rust, &mut cache)
            .expect("generated permutation parses");
        let permuted_fresh = builder
            .build(&permuted_source, Language::Rust)
            .expect("generated permutation oracle parses");
        prop_assert_eq!(fingerprint(&permuted_cached), fingerprint(&permuted_fresh));
        prop_assert_eq!(fingerprint(&first), fingerprint(
            &builder.build(&original, Language::Rust).expect("original remains deterministic")
        ));
    }
}

#[test]
fn deep_and_wide_cache_key_replay_evict_and_drop_are_small_stack_safe() {
    const DEPTH: usize = 100_000;
    const WIDTH: usize = 100_000;
    let handle = std::thread::Builder::new()
        .name("function-analysis-cache-small-stack".into())
        .stack_size(256 * 1024)
        .spawn(|| {
            let builder =
                TreeSitterCpgBuilder::with_config(CpgBuilderConfig::default().with_dfg(false));
            let mut cache = FunctionAnalysisCache::with_limits(FunctionAnalysisCacheLimits::new(
                1,
                512 * 1024 * 1024,
            ));

            let mut deep = String::with_capacity(32 + DEPTH * 2);
            deep.push_str("fn deep(){let _x=");
            deep.push_str(&"(".repeat(DEPTH));
            deep.push('0');
            deep.push_str(&")".repeat(DEPTH));
            deep.push_str(";}");
            let first = builder
                .build_with_analysis_cache(&deep, Language::Rust, &mut cache)
                .expect("deep cold build");
            let second = builder
                .build_with_analysis_cache(&deep, Language::Rust, &mut cache)
                .expect("deep cache hit");
            assert_eq!(fingerprint(&first), fingerprint(&second));
            assert_eq!(cache.stats().cfg_hits, 1);
            drop(first);
            drop(second);

            let mut wide = String::with_capacity(16 + WIDTH * 2);
            wide.push_str("fn wide(){");
            for _ in 0..WIDTH {
                wide.push_str("0;");
            }
            wide.push('}');
            let wide_graph = builder
                .build_with_analysis_cache(&wide, Language::Rust, &mut cache)
                .expect("wide cold build and deterministic eviction");
            assert!(wide_graph.node_count() > WIDTH);
            assert!(cache.stats().evictions >= 1);
            drop(wide_graph);
            cache.clear();
            drop(cache);
        })
        .expect("spawn small-stack cache test");
    handle.join().expect("small-stack cache test completes");
}
