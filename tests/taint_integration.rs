use libcpg::{
    analyze_null_flows, analyze_null_flows_report, analyze_taint, analyze_taint_report,
    CallGraphResolution, CatalogRule, CodePropertyGraph, CpgEdgeKind, CpgNode, CpgNodeKind,
    DfgEdgeKind, IfdsCaps, Language, LiteralKind, MethodSignature, NodeId, NullOriginKind, Pattern,
    SourceRange, TaintCatalog, Visibility,
};
#[cfg(feature = "lang-rust")]
use libcpg::{resolve_calls, CpgBuilder, CpgBuilderConfig, ResolveConfig, TreeSitterCpgBuilder};

fn function_kind(name: &str) -> CpgNodeKind {
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

fn connect_ast(cpg: &mut CodePropertyGraph, parent: NodeId, child: NodeId) {
    cpg.connect(parent, child, CpgEdgeKind::AstChild)
        .expect("public taint fixture AST endpoints exist");
}

fn child(cpg: &mut CodePropertyGraph, parent: NodeId, kind: CpgNodeKind) -> NodeId {
    let child = add(cpg, kind);
    connect_ast(cpg, parent, child);
    child
}

fn typed_path(cpg: &mut CodePropertyGraph, call: NodeId, segments: &[&str]) {
    let mut current = child(
        cpg,
        call,
        CpgNodeKind::MemberAccess {
            member: segments[segments.len() - 1].into(),
        },
    );
    for segment in segments[1..segments.len() - 1].iter().rev() {
        current = child(
            cpg,
            current,
            CpgNodeKind::MemberAccess {
                member: (*segment).into(),
            },
        );
    }
    child(
        cpg,
        current,
        CpgNodeKind::Identifier {
            name: segments[0].into(),
            definition: None,
        },
    );
}

#[test]
fn public_taint_api_classifies_typed_member_paths_and_preserves_shape() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = add(&mut cpg, function_kind("entry"));
    let source = child(
        &mut cpg,
        function,
        CpgNodeKind::Call {
            target: None,
            is_method: true,
        },
    );
    typed_path(&mut cpg, source, &["std", "env", "var"]);
    let sink = child(
        &mut cpg,
        function,
        CpgNodeKind::Call {
            target: None,
            is_method: true,
        },
    );
    typed_path(&mut cpg, sink, &["std", "process", "Command", "new"]);
    let argument = child(
        &mut cpg,
        sink,
        CpgNodeKind::Identifier {
            name: "input".into(),
            definition: Some(source),
        },
    );
    cpg.connect(source, argument, CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse))
        .expect("public taint fixture DFG endpoints exist");

    let shape = (cpg.node_count(), cpg.edge_count());
    let catalog = TaintCatalog::builtin_generic();
    let report = analyze_taint_report(
        &cpg,
        &CallGraphResolution::default(),
        &catalog,
        &IfdsCaps::default(),
    );
    assert!(report.is_complete());
    assert_eq!(report.flows.len(), 1);
    assert_eq!(report.flows[0].source, source);
    assert_eq!(report.flows[0].sink, sink);
    assert_eq!(report.flows[0].cwe, Some(78));
    assert_eq!(
        analyze_taint(
            &cpg,
            &CallGraphResolution::default(),
            &catalog,
            &IfdsCaps::default()
        ),
        report.flows
    );
    assert_eq!((cpg.node_count(), cpg.edge_count()), shape);
}

#[test]
#[cfg(feature = "lang-rust")]
fn tree_sitter_rust_cpg_supports_direct_builtin_taint_flow() {
    let source = r#"
fn entry() {
    let input = std::env::var("CMD").unwrap();
    std::process::Command::new(input);
}
"#;
    let mut cpg = TreeSitterCpgBuilder::with_config(CpgBuilderConfig {
        resolve_imports: true,
        ..CpgBuilderConfig::default()
    })
    .build(source, Language::Rust)
    .expect("Rust CPG");
    let resolution = resolve_calls(&mut cpg, &ResolveConfig::default());
    let report = analyze_taint_report(
        &cpg,
        &resolution,
        &TaintCatalog::builtin_generic(),
        &IfdsCaps::default(),
    );

    assert!(report.is_complete());
    assert!(report.flows.iter().any(|flow| {
        flow.source_class.as_ref() == "env" && flow.sink_class.as_ref() == "command"
    }));
}

#[test]
fn public_catalog_is_replaceable_without_language_specific_code() {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    let function = add(&mut cpg, function_kind("entry"));
    let source = child(
        &mut cpg,
        function,
        CpgNodeKind::Call {
            target: None,
            is_method: false,
        },
    );
    child(
        &mut cpg,
        source,
        CpgNodeKind::Identifier {
            name: "domain::receive".into(),
            definition: None,
        },
    );
    let sink = child(
        &mut cpg,
        function,
        CpgNodeKind::Call {
            target: None,
            is_method: false,
        },
    );
    child(
        &mut cpg,
        sink,
        CpgNodeKind::Identifier {
            name: "domain::consume".into(),
            definition: None,
        },
    );
    let argument = child(
        &mut cpg,
        sink,
        CpgNodeKind::Identifier {
            name: "message".into(),
            definition: Some(source),
        },
    );
    cpg.connect(source, argument, CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse))
        .expect("custom catalog fixture DFG endpoints exist");

    let custom = TaintCatalog {
        sources: vec![CatalogRule {
            pattern: Pattern::Exact("domain::receive".into()),
            class: "domain_input".into(),
            cwe: None,
        }],
        sinks: vec![CatalogRule {
            pattern: Pattern::Exact("domain::consume".into()),
            class: "domain_sink".into(),
            cwe: Some(400),
        }],
        ..TaintCatalog::empty()
    };
    assert!(analyze_taint(
        &cpg,
        &CallGraphResolution::default(),
        &TaintCatalog::empty(),
        &IfdsCaps::default()
    )
    .is_empty());
    let flows = analyze_taint(
        &cpg,
        &CallGraphResolution::default(),
        &custom,
        &IfdsCaps::default(),
    );
    assert_eq!(flows.len(), 1);
    assert_eq!(flows[0].source_class.as_ref(), "domain_input");
    assert_eq!(flows[0].sink_class.as_ref(), "domain_sink");
}

#[test]
fn public_nullflow_api_reports_literal_dereference_and_cap_evidence() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = add(&mut cpg, function_kind("entry"));
    let literal = child(
        &mut cpg,
        function,
        CpgNodeKind::Literal {
            kind: LiteralKind::Null,
        },
    );
    let dereference = child(
        &mut cpg,
        function,
        CpgNodeKind::UnaryOp {
            operator: "*".into(),
        },
    );
    cpg.connect(
        literal,
        dereference,
        CpgEdgeKind::DataFlow(DfgEdgeKind::Dereference),
    )
    .expect("public nullflow fixture DFG endpoints exist");

    let resolution = CallGraphResolution::default();
    let report = analyze_null_flows_report(&cpg, &resolution, &IfdsCaps::default());
    assert!(report.is_complete());
    assert_eq!(report.flows.len(), 1);
    assert_eq!(report.flows[0].origin_kind, NullOriginKind::Literal);
    assert_eq!(
        analyze_null_flows(&cpg, &resolution, &IfdsCaps::default()),
        report.flows
    );

    let capped = analyze_null_flows_report(
        &cpg,
        &resolution,
        &IfdsCaps {
            max_worklist_steps: 0,
            ..IfdsCaps::default()
        },
    );
    assert!(capped.capped);
    assert!(!capped.is_complete());
}
