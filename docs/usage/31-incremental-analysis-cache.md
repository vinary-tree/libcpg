# Usage 31 -- reuse unchanged function analyses

Use `FunctionAnalysisCache` when one process analyzes successive revisions and
you want exact reuse of unchanged per-function CFG/DFG plans. Parsing and AST
construction always run; the cache accelerates the analysis overlays.

## Basic Mode-A use

Enable the language grammar, create one cache for the lifetime you control,
and pass it to every revision:

```rust
use libcpg::builder::{
    CpgBuilderConfig, FunctionAnalysisCache, FunctionAnalysisCacheLimits,
    TreeSitterCpgBuilder,
};
use libcpg::Language;

fn main() -> libcpg::Result<()> {
    let builder = TreeSitterCpgBuilder::with_config(
        CpgBuilderConfig::default().with_import_resolution(true),
    );
    let mut cache = FunctionAnalysisCache::with_limits(
        FunctionAnalysisCacheLimits::new(2_048, 32 * 1024 * 1024),
    );

    let first = "fn value(x: i64) -> i64 { x + 1 }";
    let edited = "fn value(x: i64) -> i64 { x + 2 }";

    let _initial =
        builder.build_with_analysis_cache(first, Language::Rust, &mut cache)?;
    let _unchanged =
        builder.build_with_analysis_cache(first, Language::Rust, &mut cache)?;
    let _edited =
        builder.build_with_analysis_cache(edited, Language::Rust, &mut cache)?;

    let stats = cache.stats();
    println!(
        "CFG hits={}, misses={}, resident={} bytes",
        stats.cfg_hits, stats.cfg_misses, stats.resident_bytes
    );
    Ok(())
}
```

The second build can hit. The third changes exact function bytes and misses.

## Mode B with a caller-owned tree

`build_from_tree_with_analysis_cache` is the cached counterpart of
`build_from_tree`:

```rust
# use libcpg::builder::{FunctionAnalysisCache, TreeSitterCpgBuilder};
# use libcpg::Language;
# fn example(tree: &tree_sitter::Tree, source: &str) -> libcpg::Result<()> {
let builder = TreeSitterCpgBuilder::new();
let mut cache = FunctionAnalysisCache::new();
let cpg = builder.build_from_tree_with_analysis_cache(
    tree,
    source,
    Language::Rust,
    &mut cache,
)?;
assert_eq!(cpg.language(), Language::Rust);
# Ok(())
# }
```

Mode B includes exact local tree shape in its keys. The caller remains
responsible for ensuring the tree and source are a coherent pair, just as with
ordinary `build_from_tree`.

## Add cached PDGs

PDG plans use their own key domain and include the exact local CFG/DFG overlay:

```rust
# use libcpg::builder::{FunctionAnalysisCache, PdgBuilder};
# use libcpg::CodePropertyGraph;
# fn add_pdg(cpg: &mut CodePropertyGraph, cache: &mut FunctionAnalysisCache) {
PdgBuilder::new().build_all_with_analysis_cache(cpg, cache);
# }
```

Use `build_all_parallel_with_analysis_cache` when misses should be planned on
the current Rayon pool. Cache hits and graph mutation remain deterministically
ordered.

## Choose bounds

The two limits are independent:

- `max_entries` bounds plan count and implementation metadata;
- `max_bytes` bounds estimated keys, plans, and entry records.

Start from defaults, observe `resident_*_bytes` and evictions under a
representative edit stream, then lower or raise limits according to the
process memory budget. Setting either limit to zero disables reuse and selects
the ordinary path.

## Interpret statistics

| Counter | Interpretation |
|---|---|
| `cfg_hits` / `dfg_hits` / `pdg_hits` | full key equality and valid local plan |
| family miss | eligible exact key was absent |
| `dfg_dependency_bypasses` | nonlocal dependency required ordinary DFG analysis |
| `key_bypasses` | a complete canonical context could not be formed |
| `nonlocal_plan_bypasses` | fresh plan escaped its proposed local scope |
| `capacity_bypasses` | zero capacity or one entry larger than the byte limit |
| `evictions` | deterministic LRU removals |
| `resident_key_bytes` | exact canonical key material retained now |

`reset_stats` starts a new measurement interval without discarding plans.
`clear` drops plans iteratively but preserves accumulated counters.

## Recommended ownership

- one editor document or analysis session per cache;
- no cache shared across untrusted tenants;
- clear on an explicit lifecycle boundary;
- keep limits below the process's admitted memory budget; and
- use separate caches when independent clients need independent hit and LRU
  histories.

Do not wrap the cache in hidden global state. Its `&mut` API intentionally
makes reuse lifetime and ordering visible.

## When not to use it

Use ordinary `build` when there is only one revision, when every revision
changes most functions, or when retaining source-derived canonical keys is
unacceptable. A zero-capacity cache is useful for A/B instrumentation but is
otherwise equivalent to the ordinary path.

See the [component guide](../components/builder/incremental-analysis-cache.md)
and [security boundary](../security/21-incremental-cache-boundary.md).
