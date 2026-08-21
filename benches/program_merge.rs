use std::time::Duration;

use criterion::{
    black_box, criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion, Throughput,
};
use libcpg::{
    build_import_graph, merge_programs, CodePropertyGraph, CpgEdgeKind, CpgNode, CpgNodeKind,
    ImportResolutionKind, Language, MethodSignature, NodeId, SourceRange, Visibility,
};

fn node(kind: CpgNodeKind) -> CpgNode {
    CpgNode::new(NodeId::new(0), kind, SourceRange::default())
}

fn function(name: &str) -> CpgNodeKind {
    CpgNodeKind::Function {
        signature: MethodSignature {
            name: name.into(),
            params: Default::default(),
            return_type: None,
            is_static: false,
            is_async: false,
            visibility: Visibility::Public,
        },
    }
}

fn child(cpg: &mut CodePropertyGraph, parent: NodeId, kind: CpgNodeKind) -> NodeId {
    let child = cpg.add_node(node(kind));
    cpg.connect(parent, child, CpgEdgeKind::AstChild);
    child
}

fn fixture(file_count: usize, statements_per_file: usize) -> Vec<CodePropertyGraph> {
    (0..file_count)
        .map(|file| {
            let mut cpg = CodePropertyGraph::new(Language::Rust)
                .with_source_path(format!("src/module_{file}.rs"));
            let root = cpg.add_node(node(CpgNodeKind::Root));
            if file > 0 {
                child(
                    &mut cpg,
                    root,
                    CpgNodeKind::Import {
                        path: format!("use crate::module_{}::export_{};", file - 1, file - 1)
                            .into(),
                    },
                );
            }
            let declaration = child(&mut cpg, root, function(&format!("export_{file}")));
            for statement in 0..statements_per_file {
                child(
                    &mut cpg,
                    declaration,
                    CpgNodeKind::Identifier {
                        name: format!("value_{statement}").into(),
                        definition: Some(declaration),
                    },
                );
            }
            cpg
        })
        .collect()
}

fn assert_merge_oracle(
    program: &libcpg::ProgramCpg,
    expected_nodes: usize,
    expected_edges: usize,
    expected_files: usize,
) {
    assert_eq!(program.files.len(), expected_files);
    assert_eq!(program.cpg.node_count(), expected_nodes);
    assert_eq!(program.cpg.edge_count(), expected_edges);
    let mut ids: Vec<_> = program.cpg.node_ids().collect();
    ids.sort_unstable();
    assert_eq!(
        ids,
        (0..expected_nodes as u32)
            .map(NodeId::new)
            .collect::<Vec<_>>()
    );
    for node in program.cpg.nodes() {
        assert!(node
            .parent
            .is_none_or(|target| program.cpg.contains_node(target)));
        assert!(node
            .children
            .iter()
            .all(|&target| program.cpg.contains_node(target)));
        match &node.kind {
            CpgNodeKind::Call { target, .. } => {
                assert!(target.is_none_or(|target| program.cpg.contains_node(target)))
            }
            CpgNodeKind::Identifier { definition, .. } => {
                assert!(definition.is_none_or(|target| program.cpg.contains_node(target)))
            }
            CpgNodeKind::Lambda { captures } => assert!(captures
                .iter()
                .all(|&target| program.cpg.contains_node(target))),
            _ => {}
        }
    }
}

fn assert_import_oracle(program: &mut libcpg::ProgramCpg, expected_files: usize) {
    let imports = build_import_graph(program);
    assert_eq!(imports.resolutions.len(), expected_files.saturating_sub(1));
    assert_eq!(imports.dependencies.len(), expected_files.saturating_sub(1));
    assert!(imports
        .resolutions
        .values()
        .all(|resolution| resolution.kind == ImportResolutionKind::Exact));
    assert!(imports
        .resolutions
        .values()
        .all(|resolution| resolution.target_files.len() == 1));
}

fn benchmark_program_merge(c: &mut Criterion) {
    let mut merge_group = c.benchmark_group("merge_programs");
    merge_group.sample_size(20);
    merge_group.warm_up_time(Duration::from_secs(1));
    merge_group.measurement_time(Duration::from_secs(3));

    for file_count in [16usize, 64] {
        let files = fixture(file_count, 16);
        let expected_nodes: usize = files.iter().map(CodePropertyGraph::node_count).sum();
        let expected_edges: usize = files.iter().map(CodePropertyGraph::edge_count).sum();
        let program = merge_programs(files.clone());
        assert_merge_oracle(&program, expected_nodes, expected_edges, file_count);
        merge_group.throughput(Throughput::Elements(expected_nodes as u64));
        merge_group.bench_with_input(
            BenchmarkId::from_parameter(file_count),
            &files,
            |benchmark, files| {
                benchmark.iter_batched(
                    || files.clone(),
                    |files| black_box(merge_programs(black_box(files))),
                    BatchSize::SmallInput,
                );
            },
        );
    }
    merge_group.finish();

    let mut import_group = c.benchmark_group("build_import_graph");
    import_group.sample_size(20);
    import_group.warm_up_time(Duration::from_secs(1));
    import_group.measurement_time(Duration::from_secs(3));
    for file_count in [16usize, 64] {
        let files = fixture(file_count, 16);
        let mut program = merge_programs(files);
        assert_import_oracle(&mut program, file_count);
        let unlinked = merge_programs(fixture(file_count, 16));
        import_group.throughput(Throughput::Elements(file_count as u64));
        import_group.bench_with_input(
            BenchmarkId::from_parameter(file_count),
            &unlinked,
            |benchmark, program| {
                benchmark.iter_batched(
                    || program.clone(),
                    |mut program| black_box(build_import_graph(black_box(&mut program))),
                    BatchSize::SmallInput,
                );
            },
        );
    }
    import_group.finish();
}

criterion_group!(benches, benchmark_program_merge);
criterion_main!(benches);
