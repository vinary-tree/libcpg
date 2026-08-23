mod support;

use std::fmt::Write as _;
use std::hint::black_box;
use std::mem::size_of;

use libcpg::builder::{FunctionAnalysisCache, FunctionAnalysisCacheLimits};
use libcpg::pattern::Vf2Matcher;
use libcpg::{
    call_graph_projection, call_graph_sccs, dominator_tree, liveness, post_dominator_tree,
    structural_clone_classes, wl_labels, BitSetDomain, CfgEdgeKind, CfgExtractor,
    CodePropertyGraph, CpgBuilder, CpgBuilderConfig, CpgEdge, CpgEdgeKind, CpgNode, CpgNodeKind,
    DfgExtractor, GraphProjection, Language, Lattice, MethodSignature, NodeId, PdgBuilder, ScopeId,
    SourceRange, SubgraphMatcher, TreeSitterCpgBuilder, Visibility,
};

use support::{
    assert_synthetic_graph, assert_traversal_graph, linear_graph, parallel_analysis_graph,
    synthetic_graph, traversal_graph, CorpusFile, SyntheticShape, CROSS_LANGUAGE, FIXTURE_SEED,
    RUST_LARGE, RUST_MEDIUM,
};

fn construction() -> usize {
    TreeSitterCpgBuilder::new()
        .build(RUST_MEDIUM.source, RUST_MEDIUM.language)
        .expect("profile construction corpus")
        .node_count()
}

fn traversal() -> usize {
    let (graph, nodes) = traversal_graph(8_192);
    assert_traversal_graph(&graph, &nodes);
    let mut checksum = 0;
    for endpoints in nodes.windows(2) {
        checksum += usize::from(graph.node(endpoints[0]).is_some());
        checksum += graph.ast_children(endpoints[0]).len();
        checksum += graph.cfg_successors(endpoints[0]).len();
        checksum += graph.dfg_successors(endpoints[0]).len();
        checksum += graph.edges_between(endpoints[0], endpoints[1]).len();
    }
    checksum
}

fn dfg() -> usize {
    let builder = TreeSitterCpgBuilder::with_config(CpgBuilderConfig::default().with_dfg(false));
    let mut graph = builder
        .build(RUST_MEDIUM.source, RUST_MEDIUM.language)
        .expect("profile DFG corpus");
    DfgExtractor::new().extract(&mut graph);
    graph.edges().filter(|edge| edge.kind.is_dfg()).count()
}

fn pdg() -> usize {
    let mut graph = TreeSitterCpgBuilder::new()
        .build(RUST_MEDIUM.source, RUST_MEDIUM.language)
        .expect("profile PDG corpus");
    let functions: Vec<_> = graph.functions().map(|node| node.id).collect();
    for function in functions {
        PdgBuilder::new().build(&mut graph, function);
    }
    graph.edges().filter(|edge| edge.kind.is_pdg()).count()
}

fn repeated_parallel_analysis(parallel: bool, repetitions: usize) -> usize {
    let template = parallel_analysis_graph(512, 16);
    let mut checksum = 0usize;
    if parallel {
        let workers = std::env::var("LIBCPG_PARALLEL_THREADS")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(32);
        assert!(
            workers > 0,
            "parallel profile worker count must be positive"
        );
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(workers)
            .stack_size(256 * 1024)
            .build()
            .expect("bounded profile Rayon pool");
        for _ in 0..repetitions {
            let mut graph = template.clone();
            pool.install(|| {
                CfgExtractor::new().extract_parallel(&mut graph);
                DfgExtractor::new().extract_parallel(&mut graph);
                PdgBuilder::new().build_all_parallel(&mut graph);
            });
            checksum = checksum.wrapping_add(graph.edge_count());
        }
    } else {
        for _ in 0..repetitions {
            let mut graph = template.clone();
            CfgExtractor::new().extract(&mut graph);
            DfgExtractor::new().extract(&mut graph);
            PdgBuilder::new().build_all(&mut graph);
            checksum = checksum.wrapping_add(graph.edge_count());
        }
    }
    checksum
}

fn dominators_fixture() -> (GraphProjection, CodePropertyGraph, NodeId) {
    let (projection, cpg, function) = support::dominator_diamond_fixture(2_048);
    support::assert_dominator_diamond_fixture(&projection, &cpg, function, 2_048);
    (projection, cpg, function)
}

fn repeated_dominators(
    projection: &GraphProjection,
    cpg: &CodePropertyGraph,
    function: NodeId,
    repetitions: usize,
) -> usize {
    let mut checksum = 0usize;
    for _ in 0..repetitions {
        let dominators =
            dominator_tree(black_box(projection), NodeId::new(0)).expect("profile dominators");
        let post_dominators = post_dominator_tree(black_box(cpg), black_box(function))
            .expect("profile post-dominators");
        checksum = checksum
            .wrapping_add(dominators.idom.len())
            .wrapping_add(post_dominators.ipdom.len())
            .wrapping_add(
                post_dominators
                    .frontier
                    .values()
                    .map(Vec::len)
                    .sum::<usize>(),
            );
    }
    checksum
}

fn scc() -> usize {
    let graph = synthetic_graph(SyntheticShape::DenseCallGraph, 128, FIXTURE_SEED);
    assert_synthetic_graph(&graph, SyntheticShape::DenseCallGraph, 128);
    call_graph_sccs(&graph).components.len()
}

fn vf2_fixture() -> (CodePropertyGraph, CodePropertyGraph, Vf2Matcher) {
    let pattern = linear_graph(4);
    let target = linear_graph(4_096);
    let matcher = Vf2Matcher::new().with_max_matches(1);
    (pattern, target, matcher)
}

fn vf2() -> usize {
    let (pattern, target, matcher) = vf2_fixture();
    matcher.find_matches(&pattern, &target).len()
}

fn repeated_vf2(repetitions: usize) -> usize {
    let (pattern, target, matcher) = vf2_fixture();
    let mut checksum = 0usize;
    for _ in 0..repetitions {
        checksum = checksum.wrapping_add(
            black_box(&matcher)
                .find_matches(black_box(&pattern), black_box(&target))
                .len(),
        );
    }
    checksum
}

fn wl() -> usize {
    let graph = synthetic_graph(SyntheticShape::DenseCallGraph, 128, FIXTURE_SEED);
    let projection = call_graph_projection(&graph);
    wl_labels(&projection, 4).len() + structural_clone_classes(&projection, 4, 2).len()
}

fn analyze(files: &[CorpusFile]) -> usize {
    let builder =
        TreeSitterCpgBuilder::with_config(CpgBuilderConfig::default().with_import_resolution(true));
    files
        .iter()
        .map(|file| {
            let graph = builder
                .build(file.source, file.language)
                .expect("profile corpus parses");
            graph.node_count()
                + graph.edge_count()
                + call_graph_sccs(&graph).components.len()
                + wl_labels(&call_graph_projection(&graph), 4).len()
        })
        .sum()
}

fn corpus() -> usize {
    analyze(&RUST_LARGE) + analyze(&CROSS_LANGUAGE)
}

fn repeated_bitset_primitives(repetitions: usize) -> usize {
    const WIDTH: usize = 65_536;
    const PASSES_PER_REPETITION: usize = 2_000_000;
    let left = BitSetDomain::from_indices(
        WIDTH,
        (0..WIDTH)
            .filter(|index| index % 2 == 0)
            .map(|index| index as u32),
    );
    let right = BitSetDomain::from_indices(
        WIDTH,
        (0..WIDTH)
            .filter(|index| index % 2 == 1)
            .map(|index| index as u32),
    );
    let mut state = left.clone();
    let mut checksum = 0usize;
    for _ in 0..repetitions.saturating_mul(PASSES_PER_REPETITION) {
        checksum = checksum.wrapping_add(usize::from(state.join(black_box(&right))));
        checksum = checksum.wrapping_add(usize::from(state.intersect_with(black_box(&left))));
    }
    assert_eq!(state, left);
    checksum.wrapping_add(state.len())
}

fn profile_node(kind: CpgNodeKind) -> CpgNode {
    CpgNode::new(NodeId::new(0), kind, SourceRange::default())
}

fn wide_liveness_fixture() -> (CodePropertyGraph, NodeId) {
    const VARIABLES: usize = 4_096;
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = cpg.add_node(profile_node(CpgNodeKind::Function {
        signature: Box::new(MethodSignature {
            name: "profile_wide_liveness".into(),
            params: Default::default(),
            return_type: None,
            is_static: false,
            is_async: false,
            visibility: Visibility::Private,
        }),
    }));
    for index in 0..VARIABLES {
        let variable = cpg.add_node(profile_node(CpgNodeKind::Variable {
            name: format!("wide_{index}").into(),
            var_type: None,
            is_mutable: true,
            scope: ScopeId::GLOBAL,
        }));
        cpg.connect(function, variable, CpgEdgeKind::AstChild);
    }
    let guard = cpg.add_node(profile_node(CpgNodeKind::Identifier {
        name: "guard".into(),
        definition: None,
    }));
    let left = cpg.add_node(profile_node(CpgNodeKind::Identifier {
        name: "wide_4094".into(),
        definition: None,
    }));
    let right = cpg.add_node(profile_node(CpgNodeKind::Identifier {
        name: "wide_4095".into(),
        definition: None,
    }));
    let exit = cpg.add_node(profile_node(CpgNodeKind::Return));
    for child in [guard, left, right, exit] {
        cpg.connect(function, child, CpgEdgeKind::AstChild);
    }
    for (source, target, kind) in [
        (function, guard, CfgEdgeKind::Sequential),
        (guard, left, CfgEdgeKind::ConditionalTrue),
        (guard, right, CfgEdgeKind::ConditionalFalse),
        (left, exit, CfgEdgeKind::Sequential),
        (right, exit, CfgEdgeKind::Sequential),
    ] {
        cpg.connect(source, target, CpgEdgeKind::ControlFlow(kind));
    }
    (cpg, function)
}

fn repeated_wide_liveness(repetitions: usize) -> usize {
    const PASSES_PER_REPETITION: usize = 256;
    let (cpg, function) = wide_liveness_fixture();
    let oracle = liveness(&cpg, function).expect("wide liveness profile oracle");
    assert!(oracle
        .live_in
        .values()
        .flatten()
        .any(|name| &**name == "wide_4094"));
    assert!(oracle
        .live_in
        .values()
        .flatten()
        .any(|name| &**name == "wide_4095"));

    let mut checksum = 0usize;
    for _ in 0..repetitions.saturating_mul(PASSES_PER_REPETITION) {
        let report =
            liveness(black_box(&cpg), black_box(function)).expect("wide liveness profile run");
        checksum = checksum
            .wrapping_add(report.live_in.len())
            .wrapping_add(report.live_out.len());
    }
    checksum
}

const REANALYZE_FUNCTIONS: usize = 64;
const REANALYZE_STATEMENTS: usize = 32;

fn reanalysis_source(edited: impl Fn(usize) -> bool) -> String {
    let mut source = String::new();
    for function in 0..REANALYZE_FUNCTIONS {
        writeln!(&mut source, "fn function_{function}(seed: i64) -> i64 {{")
            .expect("writing to a String cannot fail");
        writeln!(&mut source, "    let mut value = seed;")
            .expect("writing to a String cannot fail");
        for statement in 0..REANALYZE_STATEMENTS {
            writeln!(
                &mut source,
                "    value = value.wrapping_add({});",
                function * REANALYZE_STATEMENTS + statement
            )
            .expect("writing to a String cannot fail");
        }
        if edited(function) {
            writeln!(&mut source, "    value = value.wrapping_mul(3);")
                .expect("writing to a String cannot fail");
        }
        writeln!(&mut source, "    value\n}}").expect("writing to a String cannot fail");
    }
    source
}

fn shared_dependency_source(offset: i64) -> String {
    let mut source = format!("const OFFSET: i64 = {offset};\n");
    for function in 0..REANALYZE_FUNCTIONS {
        writeln!(
            &mut source,
            "fn shared_{function}(seed: i64) -> i64 {{ seed + OFFSET + {function} }}"
        )
        .expect("writing to a String cannot fail");
    }
    source
}

fn graph_checksum(graph: &CodePropertyGraph) -> usize {
    graph.node_count().wrapping_add(graph.edge_count())
}

fn seed_reanalysis_cache(builder: &TreeSitterCpgBuilder, source: &str) -> FunctionAnalysisCache {
    let mut cache = FunctionAnalysisCache::new();
    let graph = builder
        .build_with_analysis_cache(source, Language::Rust, &mut cache)
        .expect("profile cache seed parses");
    assert_eq!(graph.functions().count(), REANALYZE_FUNCTIONS);
    cache.reset_stats();
    cache
}

fn repeated_reanalysis(workload: &str, repetitions: usize) -> usize {
    let original = reanalysis_source(|_| false);
    let edited = reanalysis_source(|function| function == REANALYZE_FUNCTIONS / 2);
    let churned = reanalysis_source(|function| function % 2 == 0);
    let shared_before = shared_dependency_source(1);
    let shared_after = shared_dependency_source(2);
    let builder =
        TreeSitterCpgBuilder::with_config(CpgBuilderConfig::default().with_import_resolution(true));
    let mut checksum = 0usize;
    let mut footprint = None;

    match workload {
        "reanalyze_fresh" => {
            for _ in 0..repetitions {
                let graph = builder
                    .build(black_box(&edited), Language::Rust)
                    .expect("fresh profile source parses");
                checksum = checksum.wrapping_add(graph_checksum(black_box(&graph)));
            }
        }
        "reanalyze_cold" => {
            for _ in 0..repetitions {
                let mut cache = FunctionAnalysisCache::new();
                let graph = builder
                    .build_with_analysis_cache(black_box(&edited), Language::Rust, &mut cache)
                    .expect("cold profile source parses");
                checksum = checksum
                    .wrapping_add(graph_checksum(black_box(&graph)))
                    .wrapping_add(cache.stats().entries);
                footprint = Some(cache.stats());
            }
        }
        "reanalyze_all_hit" => {
            let mut cache = seed_reanalysis_cache(&builder, &original);
            footprint = Some(cache.stats());
            for _ in 0..repetitions {
                let graph = builder
                    .build_with_analysis_cache(black_box(&original), Language::Rust, &mut cache)
                    .expect("all-hit profile source parses");
                checksum = checksum.wrapping_add(graph_checksum(black_box(&graph)));
            }
            assert_eq!(
                cache.stats().cfg_hits,
                (REANALYZE_FUNCTIONS * repetitions) as u64
            );
        }
        "reanalyze_edit_one" | "reanalyze_shared" | "reanalyze_half_churn" => {
            let (before, after) = match workload {
                "reanalyze_edit_one" => (&original, &edited),
                "reanalyze_shared" => (&shared_before, &shared_after),
                "reanalyze_half_churn" => (&original, &churned),
                _ => unreachable!("matched reanalysis edit workload"),
            };
            for _ in 0..repetitions {
                let mut cache = seed_reanalysis_cache(&builder, before);
                let graph = builder
                    .build_with_analysis_cache(black_box(after), Language::Rust, &mut cache)
                    .expect("edited profile source parses");
                checksum = checksum
                    .wrapping_add(graph_checksum(black_box(&graph)))
                    .wrapping_add(cache.stats().cfg_hits as usize)
                    .wrapping_add(cache.stats().dfg_hits as usize);
                footprint = Some(cache.stats());
            }
        }
        "reanalyze_eviction" => {
            for _ in 0..repetitions {
                let mut cache = FunctionAnalysisCache::with_limits(
                    FunctionAnalysisCacheLimits::new(16, 2 * 1024 * 1024),
                );
                let _ = builder
                    .build_with_analysis_cache(black_box(&original), Language::Rust, &mut cache)
                    .expect("eviction profile seed parses");
                cache.reset_stats();
                let graph = builder
                    .build_with_analysis_cache(black_box(&edited), Language::Rust, &mut cache)
                    .expect("eviction profile edit parses");
                assert!(cache.stats().evictions > 0);
                checksum = checksum.wrapping_add(graph_checksum(black_box(&graph)));
                footprint = Some(cache.stats());
            }
        }
        "reanalyze_disabled" => {
            let mut cache =
                FunctionAnalysisCache::with_limits(FunctionAnalysisCacheLimits::new(0, 0));
            for _ in 0..repetitions {
                let graph = builder
                    .build_with_analysis_cache(black_box(&edited), Language::Rust, &mut cache)
                    .expect("disabled-cache profile source parses");
                checksum = checksum.wrapping_add(graph_checksum(black_box(&graph)));
            }
            assert_eq!(cache.stats().entries, 0);
            footprint = Some(cache.stats());
        }
        _ => unreachable!("validated reanalysis profile workload"),
    }
    if let Some(stats) = footprint {
        println!(
            "cache entries={} resident_bytes={} key_bytes={} artifact_bytes={} metadata_bytes={}",
            stats.entries,
            stats.resident_bytes,
            stats.resident_key_bytes,
            stats.resident_artifact_bytes,
            stats.resident_metadata_bytes,
        );
    }
    checksum
}

fn run(name: &str) -> usize {
    match name {
        "cpg_construction" => construction(),
        "traversal_primitives" => traversal(),
        "dfg_extraction" => dfg(),
        "pdg_construction" => pdg(),
        "dominator_analysis" => {
            let (projection, cpg, function) = dominators_fixture();
            repeated_dominators(&projection, &cpg, function, 1)
        }
        "scc_analysis" => scc(),
        "pattern_vf2" => vf2(),
        "parallel_analysis_serial" => repeated_parallel_analysis(false, 1),
        "parallel_analysis_parallel" => repeated_parallel_analysis(true, 1),
        "similarity_wl" => wl(),
        "analyze_corpus" => corpus(),
        "all" => construction() + traversal() + dfg() + pdg() + scc() + vf2() + wl() + corpus(),
        other => panic!("unknown workload {other:?}"),
    }
}

fn main() {
    let workload = std::env::args().nth(1).unwrap_or_else(|| "all".into());
    let repetitions = std::env::var("LIBCPG_PROFILE_REPETITIONS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(1);
    assert!(repetitions > 0, "profile repetitions must be positive");

    println!(
        "size_of CodePropertyGraph={} CpgNode={} CpgEdge={} NodeId={} GraphProjection={}",
        size_of::<CodePropertyGraph>(),
        size_of::<CpgNode>(),
        size_of::<CpgEdge>(),
        size_of::<NodeId>(),
        size_of::<GraphProjection>(),
    );
    // Keep the VF2 graphs alive across repetitions so this workload measures
    // search-state construction and traversal rather than graph construction.
    // The aggregate `all` workload intentionally retains its end-to-end shape.
    let checksum = match workload.as_str() {
        "reanalyze_fresh"
        | "reanalyze_cold"
        | "reanalyze_all_hit"
        | "reanalyze_edit_one"
        | "reanalyze_shared"
        | "reanalyze_half_churn"
        | "reanalyze_eviction"
        | "reanalyze_disabled" => repeated_reanalysis(&workload, repetitions),
        "pattern_vf2" => repeated_vf2(repetitions),
        "bitset_primitives" => repeated_bitset_primitives(repetitions),
        "dataflow_liveness_wide" => repeated_wide_liveness(repetitions),
        "parallel_analysis_serial" => repeated_parallel_analysis(false, repetitions),
        "parallel_analysis_parallel" => repeated_parallel_analysis(true, repetitions),
        "dominator_analysis" => {
            // Retain the immutable CSR/CPG fixture across repetitions so the
            // workload isolates analysis rather than fixture construction.
            let (projection, cpg, function) = dominators_fixture();
            repeated_dominators(&projection, &cpg, function, repetitions)
        }
        _ => {
            let mut checksum = 0usize;
            for _ in 0..repetitions {
                checksum = checksum.wrapping_add(black_box(run(black_box(&workload))));
            }
            checksum
        }
    };
    println!("workload={workload} repetitions={repetitions} checksum={checksum}");
}
