use libcpg::{
    ck_metrics, function_metrics, method_field_graph, CkOptions, CodePropertyGraph, CpgEdgeKind,
    CpgNode, CpgNodeKind, DfgEdgeKind, Language, MethodSignature, NodeId, SourceRange, Visibility,
};

fn add_child(cpg: &mut CodePropertyGraph, id: u32, parent: NodeId, kind: CpgNodeKind) -> NodeId {
    let id = NodeId::new(id);
    cpg.add_node_with_id(CpgNode::new(id, kind, SourceRange::default()).with_parent(parent));
    cpg.connect(parent, id, CpgEdgeKind::AstChild)
        .expect("public fixture AST endpoints exist");
    id
}

fn method(name: &str) -> CpgNodeKind {
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

#[test]
fn public_feature_free_metrics_api_consumes_resolved_field_evidence() {
    let mut cpg = CodePropertyGraph::new(Language::Java);
    let class = cpg.add_node_with_id(CpgNode::new(
        NodeId::new(10),
        CpgNodeKind::Class {
            name: "Counter".into(),
            is_abstract: false,
        },
        SourceRange::default(),
    ));
    let field = add_child(
        &mut cpg,
        20,
        class,
        CpgNodeKind::Field {
            name: "value".into(),
            field_type: None,
            visibility: Visibility::Private,
        },
    );
    let method = add_child(&mut cpg, 30, class, method("get"));
    let access = add_child(
        &mut cpg,
        40,
        method,
        CpgNodeKind::MemberAccess {
            member: "value".into(),
        },
    );
    cpg.connect(access, field, CpgEdgeKind::Reference);
    cpg.connect(
        method,
        access,
        CpgEdgeKind::DataFlow(DfgEdgeKind::FieldRead),
    );

    let before = (cpg.node_count(), cpg.edge_count());
    let graph = method_field_graph(&cpg, class);
    let classes = ck_metrics(&cpg, &CkOptions::default());
    let function = function_metrics(&cpg, method).expect("method metrics");
    assert_eq!(graph.methods, vec![method]);
    assert_eq!(graph.fields, vec![field]);
    assert_eq!(graph.accesses.len(), 1);
    assert_eq!(classes.len(), 1);
    assert_eq!((classes[0].wmc, classes[0].rfc), (1, 1));
    assert_eq!(function.cyclomatic_complexity, 1);
    assert!(function.halstead.total_operators > 0);
    assert_eq!((cpg.node_count(), cpg.edge_count()), before);
}

#[cfg(feature = "lang-rust")]
#[test]
fn parsed_rust_impl_exposes_real_ck_lcom_and_function_metrics() {
    use libcpg::{CpgBuilder, TreeSitterCpgBuilder};

    let source = r#"
        struct Counter { value: i32, limit: i32 }

        impl Counter {
            fn bump(&mut self) {
                self.value += 1;
                if self.value < self.limit { self.value += 1; }
            }

            fn reset(&mut self) { self.value = 0; }
        }
    "#;
    let cpg = TreeSitterCpgBuilder::new()
        .build(source, Language::Rust)
        .expect("Rust CPG");
    let counter = cpg
        .nodes()
        .find(
            |node| matches!(&node.kind, CpgNodeKind::Struct { name } if name.as_ref() == "Counter"),
        )
        .expect("Counter declaration")
        .id;
    let bump = cpg
        .functions()
        .find(|node| {
            matches!(&node.kind, CpgNodeKind::Function { signature } if signature.name.as_ref() == "bump")
        })
        .expect("bump method")
        .id;

    let graph = method_field_graph(&cpg, counter);
    assert_eq!(graph.methods.len(), 2);
    assert_eq!(graph.fields.len(), 2);
    assert!(graph.accesses.len() >= 4);
    assert!(graph.accesses.iter().all(|access| {
        cpg.edges_between(access.access, access.field)
            .iter()
            .any(|edge| matches!(edge.kind, CpgEdgeKind::Reference))
    }));

    let row = ck_metrics(&cpg, &CkOptions::default())
        .into_iter()
        .find(|row| row.class == counter)
        .expect("Counter CK row");
    assert!(row.wmc >= 3);
    assert_eq!((row.lcom.lcom1, row.lcom.lcom3), (0, 1));
    assert!((row.lcom.lcom5 - 0.5).abs() < 1e-12);

    let metrics = function_metrics(&cpg, bump).expect("bump metrics");
    assert!(metrics.cyclomatic_complexity >= 2);
    assert!(metrics.cognitive_complexity >= 1);
    assert!(metrics.halstead.volume > 0.0);
    assert!((0.0..=100.0).contains(&metrics.maintainability_index));
}
