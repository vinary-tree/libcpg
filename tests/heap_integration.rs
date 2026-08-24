//! Public compositional-heap API and deep-stack integration gates.

use libcpg::{
    analyze_heap, analyze_heap_with_model, biabduction_summaries, call_graph_sccs,
    materialize_heap_overlay, memory_safety, CfgEdgeKind, CodePropertyGraph, CpgEdgeKind, CpgNode,
    CpgNodeKind, DfgEdgeKind, HeapModel, HeapPredicate, HeapViolationKind, IfdsCaps, Language,
    MethodSignature, NodeId, SourceRange, TypeInfo, Visibility,
};

fn node(kind: CpgNodeKind) -> CpgNode {
    CpgNode::new(NodeId::new(0), kind, SourceRange::default())
}

fn function_kind(name: &str) -> CpgNodeKind {
    CpgNodeKind::Function {
        signature: Box::new(MethodSignature {
            name: name.into(),
            params: Default::default(),
            return_type: None,
            is_static: false,
            is_async: false,
            visibility: Visibility::Private,
        }),
    }
}

fn child(cpg: &mut CodePropertyGraph, parent: NodeId, kind: CpgNodeKind) -> NodeId {
    let child = cpg.add_node(node(kind));
    cpg.connect(parent, child, CpgEdgeKind::AstChild)
        .expect("heap integration AST endpoints exist");
    child
}

fn release_call(
    cpg: &mut CodePropertyGraph,
    function: NodeId,
    name: &str,
    allocation: NodeId,
) -> NodeId {
    let call = child(
        cpg,
        function,
        CpgNodeKind::Call {
            target: None,
            is_method: false,
        },
    );
    child(
        cpg,
        call,
        CpgNodeKind::Identifier {
            name: name.into(),
            definition: None,
        },
    );
    let argument = child(
        cpg,
        call,
        CpgNodeKind::Identifier {
            name: "resource".into(),
            definition: Some(allocation),
        },
    );
    cpg.connect(
        allocation,
        argument,
        CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse),
    )
    .expect("heap integration DFG endpoints exist");
    call
}

#[test]
fn public_api_uses_custom_release_semantics_and_materializes_exact_relations() {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    let function = cpg.add_node(node(function_kind("entry")));
    let allocation = child(
        &mut cpg,
        function,
        CpgNodeKind::Call {
            target: None,
            is_method: false,
        },
    );
    cpg.node_mut(allocation)
        .expect("public allocation exists")
        .allocation_type = Some(Box::new(TypeInfo::new("Resource")));
    child(
        &mut cpg,
        allocation,
        CpgNodeKind::Identifier {
            name: "create".into(),
            definition: None,
        },
    );
    let release = release_call(&mut cpg, function, "destroy", allocation);
    let exit = child(&mut cpg, function, CpgNodeKind::Return);
    for pair in [function, allocation, release, exit].windows(2) {
        cpg.connect(
            pair[0],
            pair[1],
            CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
        )
        .expect("public heap CFG endpoints exist");
    }
    cpg.add_cfg_exit(exit);

    let call_graph = call_graph_sccs(&cpg);
    let default_analysis = analyze_heap(&cpg, &call_graph, &IfdsCaps::default());
    assert!(default_analysis
        .violations(&cpg)
        .iter()
        .any(|finding| finding.kind == HeapViolationKind::Leak));

    let model = HeapModel::default().with_release_call("destroy");
    let custom = analyze_heap_with_model(&cpg, &call_graph, &IfdsCaps::default(), &model);
    assert!(custom.is_complete());
    assert!(memory_safety(&cpg, &custom.summaries).is_empty());
    assert!(custom.summaries[&function]
        .postconditions
        .iter()
        .any(|assertion| assertion.predicate == HeapPredicate::Invalid));

    let default_summaries = biabduction_summaries(&cpg, &call_graph, &IfdsCaps::default());
    assert_eq!(default_summaries, default_analysis.summaries);
    let node_count = cpg.node_count();
    let added = materialize_heap_overlay(&mut cpg, &custom.summaries)
        .expect("public heap relations reference represented nodes");
    assert!(added >= 3);
    assert_eq!(cpg.node_count(), node_count);
    assert_eq!(cpg.heap_successors(allocation).count(), 2);
    assert!(cpg.heap_predecessors(release).count() >= 2);
}

#[test]
fn depth_100k_summary_overlay_and_drop_fit_a_256k_native_stack() {
    std::thread::Builder::new()
        .name("heap-depth-100k".into())
        .stack_size(256 * 1024)
        .spawn(|| {
            const DEPTH: usize = 100_000;
            let mut cpg = CodePropertyGraph::new(Language::Unknown);
            let function = cpg.add_node(node(function_kind("deep")));
            let allocation = child(
                &mut cpg,
                function,
                CpgNodeKind::Call {
                    target: None,
                    is_method: false,
                },
            );
            cpg.node_mut(allocation)
                .expect("deep allocation exists")
                .allocation_type = Some(Box::new(TypeInfo::new("DeepResource")));
            child(
                &mut cpg,
                allocation,
                CpgNodeKind::Identifier {
                    name: "create".into(),
                    definition: None,
                },
            );

            let mut ast_parent = function;
            let mut cfg_previous = allocation;
            for _ in 0..DEPTH {
                let wrapper = child(
                    &mut cpg,
                    ast_parent,
                    CpgNodeKind::Unknown {
                        kind: "deep_wrapper".into(),
                    },
                );
                cpg.connect(
                    cfg_previous,
                    wrapper,
                    CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
                )
                .expect("deep CFG endpoints exist");
                ast_parent = wrapper;
                cfg_previous = wrapper;
            }
            cpg.connect(
                function,
                allocation,
                CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
            )
            .expect("deep allocation CFG edge exists");
            let release = release_call(&mut cpg, ast_parent, "free", allocation);
            cpg.connect(
                cfg_previous,
                release,
                CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
            )
            .expect("deep release CFG edge exists");
            cpg.add_cfg_exit(release);

            let analysis = analyze_heap(&cpg, &call_graph_sccs(&cpg), &IfdsCaps::default());
            assert!(analysis.is_complete());
            assert!(analysis.violations(&cpg).is_empty());
            let added = materialize_heap_overlay(&mut cpg, &analysis.summaries)
                .expect("deep heap relations reference represented nodes");
            assert!(added >= 3);
            assert_eq!(
                materialize_heap_overlay(&mut cpg, &analysis.summaries)
                    .expect("deep overlay remains idempotent"),
                0
            );
            drop(analysis);
            drop(cpg);
        })
        .expect("spawn small-stack heap integration")
        .join()
        .expect("100k heap analysis remains stack-safe");
}
