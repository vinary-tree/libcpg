use libcpg::{
    build_summary_edges, call_graph_sccs, effect_summaries, interprocedural_backward_slice,
    interprocedural_forward_slice, CodePropertyGraph, CpgEdgeKind, CpgNode, CpgNodeKind,
    DfgEdgeKind, IfdsCaps, Language, MethodSignature, NodeId, PdgBuilder, SliceCaps, SourceRange,
    Visibility, SUMMARY_EDGE_LABEL,
};

fn function(name: &str) -> CpgNodeKind {
    CpgNodeKind::Function {
        signature: Box::new(MethodSignature {
            name: name.into(),
            params: Default::default(),
            return_type: None,
            is_static: false,
            is_async: false,
            visibility: Visibility::Public,
        }),
    }
}

fn add(cpg: &mut CodePropertyGraph, kind: CpgNodeKind) -> NodeId {
    cpg.add_node(CpgNode::new(NodeId::new(0), kind, SourceRange::default()))
}

fn child(cpg: &mut CodePropertyGraph, parent: NodeId, kind: CpgNodeKind) -> NodeId {
    let id = add(cpg, kind);
    cpg.connect(parent, id, CpgEdgeKind::AstChild)
        .expect("public SDG fixture AST endpoints exist");
    id
}

#[test]
fn public_effect_to_summary_to_slice_pipeline_preserves_call_context() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let caller = add(&mut cpg, function("caller"));
    let callee = add(&mut cpg, function("identity"));
    let parameter = child(
        &mut cpg,
        callee,
        CpgNodeKind::Parameter {
            name: "value".into(),
            param_type: None,
            is_variadic: false,
        },
    );
    let returned = child(&mut cpg, callee, CpgNodeKind::Return);
    let return_value = child(
        &mut cpg,
        returned,
        CpgNodeKind::Identifier {
            name: "value".into(),
            definition: Some(parameter),
        },
    );
    cpg.connect(
        parameter,
        return_value,
        CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse),
    );

    let call = child(
        &mut cpg,
        caller,
        CpgNodeKind::Call {
            target: Some(callee),
            is_method: false,
        },
    );
    child(
        &mut cpg,
        call,
        CpgNodeKind::Identifier {
            name: "identity".into(),
            definition: Some(callee),
        },
    );
    let argument = child(
        &mut cpg,
        call,
        CpgNodeKind::Identifier {
            name: "input".into(),
            definition: None,
        },
    );
    cpg.connect(call, callee, CpgEdgeKind::CallSite);
    cpg.connect(
        argument,
        parameter,
        CpgEdgeKind::DataFlow(DfgEdgeKind::Parameter),
    );
    cpg.connect(
        return_value,
        call,
        CpgEdgeKind::DataFlow(DfgEdgeKind::ReturnValue),
    );
    PdgBuilder::new().build(&mut cpg, callee);

    let summaries = effect_summaries(&cpg, &call_graph_sccs(&cpg), &IfdsCaps::default());
    assert_eq!(summaries[&callee].param_to_return, 1);
    let nodes_before = cpg.node_count();
    assert_eq!(build_summary_edges(&mut cpg, &summaries), 1);
    assert_eq!(build_summary_edges(&mut cpg, &summaries), 0);
    assert_eq!(cpg.node_count(), nodes_before);
    assert!(cpg.edges_between(argument, call).iter().any(|edge| {
        edge.kind == CpgEdgeKind::DataDependence
            && edge.label.as_deref() == Some(SUMMARY_EDGE_LABEL)
    }));

    let caps = SliceCaps::new(64, 4);
    let backward = interprocedural_backward_slice(&cpg, call, &caps);
    assert!(backward.contains(&argument));
    assert!(backward.contains(&parameter));
    assert!(backward.contains(&return_value));

    let forward = interprocedural_forward_slice(&cpg, argument, &caps);
    assert!(forward.contains(&call));
    assert!(forward.contains(&parameter));
    assert!(forward.contains(&return_value));
}
