mod support;

use std::hint::black_box;
use std::mem::size_of;

use libcpg::pattern::Vf2Matcher;
use libcpg::{
    call_graph_projection, call_graph_sccs, dominator_tree, post_dominator_tree,
    structural_clone_classes, wl_labels, CodePropertyGraph, CpgBuilder, CpgBuilderConfig, CpgEdge,
    CpgNode, DfgExtractor, GraphProjection, NodeId, PdgBuilder, SubgraphMatcher,
    TreeSitterCpgBuilder,
};

use support::{
    assert_synthetic_graph, assert_traversal_graph, linear_graph, synthetic_graph, traversal_graph,
    CorpusFile, SyntheticShape, CROSS_LANGUAGE, FIXTURE_SEED, RUST_LARGE, RUST_MEDIUM,
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
        "pattern_vf2" => repeated_vf2(repetitions),
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
