use std::collections::BTreeSet;

use proptest::prelude::*;

use super::*;
use crate::{CfgEdgeKind, MethodSignature, ScopeId, TypeInfo, Visibility};

fn add(cpg: &mut CodePropertyGraph, kind: CpgNodeKind) -> NodeId {
    cpg.add_node(CpgNode::new(NodeId::new(0), kind, SourceRange::default()))
}

fn add_with_id(cpg: &mut CodePropertyGraph, id: u32, kind: CpgNodeKind) -> NodeId {
    let id = NodeId::new(id);
    cpg.add_node_with_id(CpgNode::new(
        id,
        kind,
        SourceRange::from_bytes(id.0, id.0 + 1),
    ));
    id
}

fn function(cpg: &mut CodePropertyGraph, name: &str) -> NodeId {
    add(
        cpg,
        CpgNodeKind::Function {
            signature: Box::new(MethodSignature {
                name: name.into(),
                params: Default::default(),
                return_type: None,
                is_static: false,
                is_async: false,
                visibility: Visibility::Private,
            }),
        },
    )
}

fn connect_ast(cpg: &mut CodePropertyGraph, parent: NodeId, child: NodeId) {
    cpg.connect(parent, child, CpgEdgeKind::AstChild)
        .expect("fixture AST endpoints exist");
}

fn connect_cfg(cpg: &mut CodePropertyGraph, source: NodeId, target: NodeId) {
    cpg.connect(
        source,
        target,
        CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
    )
    .expect("fixture CFG endpoints exist");
}

fn child(cpg: &mut CodePropertyGraph, parent: NodeId, kind: CpgNodeKind) -> NodeId {
    let node = add(cpg, kind);
    connect_ast(cpg, parent, node);
    node
}

fn call(cpg: &mut CodePropertyGraph, parent: NodeId, path: &str) -> NodeId {
    let call = child(
        cpg,
        parent,
        CpgNodeKind::Call {
            target: None,
            is_method: path.contains('.') || path.contains("::"),
        },
    );
    child(
        cpg,
        call,
        CpgNodeKind::Identifier {
            name: path.into(),
            definition: None,
        },
    );
    call
}

fn argument(cpg: &mut CodePropertyGraph, call: NodeId, from: NodeId) -> NodeId {
    let argument = child(
        cpg,
        call,
        CpgNodeKind::Identifier {
            name: "value".into(),
            definition: Some(from),
        },
    );
    cpg.connect(from, argument, CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse))
        .expect("fixture DFG endpoints exist");
    argument
}

fn template(cwe: u32) -> CweTemplate {
    builtin_cwe_templates()
        .into_iter()
        .find(|template| template.cwe == cwe)
        .expect("shipped template exists")
}

fn analyze_one(cpg: &CodePropertyGraph, cwe: u32) -> CweAnalysis {
    analyze_cwe_templates(
        cpg,
        &CallGraphResolution::default(),
        &[template(cwe)],
        &CweAnalysisConfig::default(),
    )
    .expect("valid shipped template")
}

fn taint_fixture(source_path: &str, sanitizer: Option<&str>, sink_path: &str) -> CodePropertyGraph {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = function(&mut cpg, "entry");
    let source = call(&mut cpg, function, source_path);
    let value = if let Some(path) = sanitizer {
        let sanitizer = call(&mut cpg, function, path);
        argument(&mut cpg, sanitizer, source);
        sanitizer
    } else {
        source
    };
    let sink = call(&mut cpg, function, sink_path);
    argument(&mut cpg, sink, value);
    cpg
}

#[test]
fn shipped_registry_is_exactly_the_ten_required_cwes() {
    let templates = builtin_cwe_templates();
    assert_eq!(templates.len(), 10);
    assert_eq!(
        templates
            .iter()
            .map(|template| template.cwe)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([22, 78, 79, 89, 190, 416, 476, 502, 787, 798])
    );
    assert!(templates.iter().all(|template| {
        !template.source.name.is_empty()
            && !template.sanitizer.name.is_empty()
            && !template.sink.name.is_empty()
            && !template.patterns.is_empty()
    }));
}

#[test]
fn cwe_89_sql_injection_and_sanitized_oracle() {
    let vulnerable = taint_fixture("request.body", None, "sqlx::query");
    let graph_size = (vulnerable.node_count(), vulnerable.edge_count());
    let report = analyze_one(&vulnerable, 89);
    assert!(report.is_complete(), "{report:#?}");
    assert_eq!(report.candidates.len(), 1);
    assert_eq!(report.candidates[0].cwe, 89);
    assert!(matches!(
        report.candidates[0].evidence,
        CweEvidence::Taint { .. }
    ));
    assert!(vulnerable.node(report.candidates[0].site).is_some());
    assert_eq!(
        graph_size,
        (vulnerable.node_count(), vulnerable.edge_count())
    );

    let sanitized = taint_fixture("request.body", Some("parameterize"), "sqlx::query");
    let report = analyze_one(&sanitized, 89);
    assert!(report.is_complete(), "{report:#?}");
    assert!(report.candidates.is_empty());
}

#[test]
fn taint_backed_command_path_xss_and_deserialization_oracles() {
    for (cwe, sanitizer, sink) in [
        (78, "shell_escape", "process::Command::new"),
        (22, "canonicalize", "File::open"),
        (79, "escape", "response.write"),
        (502, "safe_load", "pickle.load"),
    ] {
        let vulnerable = analyze_one(&taint_fixture("request.body", None, sink), cwe);
        assert!(vulnerable.is_complete(), "CWE-{cwe}: {vulnerable:#?}");
        assert_eq!(vulnerable.candidates.len(), 1, "CWE-{cwe}");
        assert_eq!(vulnerable.candidates[0].cwe, cwe);

        let clean = analyze_one(&taint_fixture("request.body", Some(sanitizer), sink), cwe);
        assert!(clean.is_complete(), "CWE-{cwe}: {clean:#?}");
        assert!(clean.candidates.is_empty(), "CWE-{cwe}");
    }
}

fn overflow_fixture(left: i64, right: i64) -> CodePropertyGraph {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = function(&mut cpg, "overflow");
    let declaration = child(
        &mut cpg,
        function,
        CpgNodeKind::Variable {
            name: "value".into(),
            var_type: Some(Box::new(TypeInfo::new("i8"))),
            scope: ScopeId::GLOBAL,
            is_mutable: false,
        },
    );
    child(
        &mut cpg,
        declaration,
        CpgNodeKind::Identifier {
            name: "value".into(),
            definition: Some(declaration),
        },
    );
    let expression = child(
        &mut cpg,
        declaration,
        CpgNodeKind::BinaryOp {
            operator: "+".into(),
        },
    );
    child(
        &mut cpg,
        expression,
        CpgNodeKind::Literal {
            kind: LiteralKind::Integer(left),
        },
    );
    child(
        &mut cpg,
        expression,
        CpgNodeKind::Literal {
            kind: LiteralKind::Integer(right),
        },
    );
    connect_cfg(&mut cpg, function, declaration);
    cpg
}

#[test]
fn cwe_190_uses_interval_result_and_fixed_width_type() {
    let vulnerable = analyze_one(&overflow_fixture(127, 1), 190);
    assert!(vulnerable.is_complete(), "{vulnerable:#?}");
    assert_eq!(vulnerable.candidates.len(), 1);
    assert!(matches!(
        vulnerable.candidates[0].evidence,
        CweEvidence::IntegerOverflow {
            value: Interval {
                lo: Bound::Finite(128),
                hi: Bound::Finite(128)
            },
            ..
        }
    ));

    let clean = analyze_one(&overflow_fixture(126, 1), 190);
    assert!(clean.is_complete(), "{clean:#?}");
    assert!(clean.candidates.is_empty());
}

#[test]
fn cwe_190_reports_contradictory_represented_integer_types() {
    let mut cpg = overflow_fixture(127, 1);
    let expression = cpg
        .nodes()
        .find(|node| matches!(node.kind, CpgNodeKind::BinaryOp { .. }))
        .map(|node| node.id)
        .expect("fixture contains an arithmetic expression");
    let second_declaration = add(
        &mut cpg,
        CpgNodeKind::Variable {
            name: "other".into(),
            var_type: Some(Box::new(TypeInfo::new("i16"))),
            scope: ScopeId::GLOBAL,
            is_mutable: false,
        },
    );
    connect_ast(&mut cpg, second_declaration, expression);

    let report = analyze_one(&cpg, 190);
    assert!(!report.is_complete());
    assert!(report.candidates.is_empty());
    assert!(report
        .issues
        .contains(&CweAnalysisIssue::AmbiguousIntegerType {
            site: expression,
            type_names: vec![Arc::from("i16"), Arc::from("i8")],
        }));
}

#[test]
fn cwe_476_projects_maybe_null_dereference() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = function(&mut cpg, "null_deref");
    let dereference = child(
        &mut cpg,
        function,
        CpgNodeKind::UnaryOp {
            operator: "*".into(),
        },
    );
    let null = child(
        &mut cpg,
        dereference,
        CpgNodeKind::Literal {
            kind: LiteralKind::Null,
        },
    );
    cpg.connect(
        null,
        dereference,
        CpgEdgeKind::DataFlow(DfgEdgeKind::Dereference),
    )
    .expect("dereference evidence endpoints exist");
    connect_cfg(&mut cpg, function, dereference);

    let report = analyze_one(&cpg, 476);
    assert!(report.is_complete(), "{report:#?}");
    assert_eq!(report.candidates.len(), 1);
    assert_eq!(report.candidates[0].source, Some(null));
    assert_eq!(report.candidates[0].site, dereference);
}

fn use_after_free_fixture(events: &[&str]) -> CodePropertyGraph {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = function(&mut cpg, "lifecycle");
    let mut calls = Vec::new();
    for event in std::iter::once("alloc").chain(events.iter().copied()) {
        let site = call(&mut cpg, function, event);
        if let Some(&object) = calls.first() {
            argument(&mut cpg, site, object);
        }
        calls.push(site);
    }
    let mut previous = function;
    for site in calls {
        connect_cfg(&mut cpg, previous, site);
        previous = site;
    }
    cpg
}

#[test]
fn cwe_416_projects_only_post_free_use_violation() {
    let vulnerable = analyze_one(&use_after_free_fixture(&["free", "use"]), 416);
    assert!(vulnerable.is_complete(), "{vulnerable:#?}");
    assert_eq!(vulnerable.candidates.len(), 1);
    assert!(matches!(
        &vulnerable.candidates[0].evidence,
        CweEvidence::Typestate { from, event, .. }
            if from.as_ref() == "freed" && event.as_ref() == "use"
    ));

    let clean = analyze_one(&use_after_free_fixture(&["use", "free"]), 416);
    assert!(clean.is_complete(), "{clean:#?}");
    assert!(clean.candidates.is_empty());
}

fn out_of_bounds_fixture(index: i64) -> CodePropertyGraph {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = function(&mut cpg, "index_write");
    let assignment = child(
        &mut cpg,
        function,
        CpgNodeKind::Assignment {
            operator: "=".into(),
        },
    );
    let access = child(&mut cpg, assignment, CpgNodeKind::IndexAccess);
    let array = child(
        &mut cpg,
        access,
        CpgNodeKind::Literal {
            kind: LiteralKind::Array,
        },
    );
    for value in [1, 2, 3] {
        child(
            &mut cpg,
            array,
            CpgNodeKind::Literal {
                kind: LiteralKind::Integer(value),
            },
        );
    }
    child(
        &mut cpg,
        access,
        CpgNodeKind::Literal {
            kind: LiteralKind::Integer(index),
        },
    );
    child(
        &mut cpg,
        assignment,
        CpgNodeKind::Literal {
            kind: LiteralKind::Integer(9),
        },
    );
    connect_cfg(&mut cpg, function, assignment);
    cpg
}

#[test]
fn cwe_787_compares_index_interval_with_represented_array_length() {
    let vulnerable = analyze_one(&out_of_bounds_fixture(4), 787);
    assert!(vulnerable.is_complete(), "{vulnerable:#?}");
    assert_eq!(vulnerable.candidates.len(), 1);
    assert!(matches!(
        vulnerable.candidates[0].evidence,
        CweEvidence::OutOfBoundsWrite { length: 3, .. }
    ));

    let clean = analyze_one(&out_of_bounds_fixture(2), 787);
    assert!(clean.is_complete(), "{clean:#?}");
    assert!(clean.candidates.is_empty());
}

fn hardcoded_fixture(hardcoded: bool) -> CodePropertyGraph {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = function(&mut cpg, "credentials");
    let assignment = child(
        &mut cpg,
        function,
        CpgNodeKind::Assignment {
            operator: "=".into(),
        },
    );
    child(
        &mut cpg,
        assignment,
        CpgNodeKind::Identifier {
            name: "db_password".into(),
            definition: None,
        },
    );
    if hardcoded {
        child(
            &mut cpg,
            assignment,
            CpgNodeKind::Literal {
                kind: LiteralKind::String("redacted-fixture-secret".into()),
            },
        );
    } else {
        call(&mut cpg, assignment, "env::var");
    }
    cpg
}

#[test]
fn cwe_798_reports_literal_provenance_without_secret_value() {
    let vulnerable = analyze_one(&hardcoded_fixture(true), 798);
    assert!(vulnerable.is_complete(), "{vulnerable:#?}");
    assert_eq!(vulnerable.candidates.len(), 1);
    let rendered = format!("{:?}", vulnerable.candidates[0]);
    assert!(!rendered.contains("redacted-fixture-secret"));

    let clean = analyze_one(&hardcoded_fixture(false), 798);
    assert!(clean.is_complete(), "{clean:#?}");
    assert!(clean.candidates.is_empty());
}

#[test]
fn invalid_templates_and_structural_caps_fail_explicitly() {
    let duplicate = template(89);
    assert!(matches!(
        analyze_cwe_templates(
            &CodePropertyGraph::new(Language::Rust),
            &CallGraphResolution::default(),
            &[duplicate.clone(), duplicate],
            &CweAnalysisConfig::default(),
        ),
        Err(CweAnalysisError::DuplicateTemplateId { .. })
    ));

    let cpg = taint_fixture("request.body", None, "sqlx::query");
    let report = analyze_cwe_templates(
        &cpg,
        &CallGraphResolution::default(),
        &[template(89)],
        &CweAnalysisConfig {
            max_matches_per_pattern: 1,
            ..CweAnalysisConfig::default()
        },
    )
    .expect("valid capped request");
    assert!(!report.is_complete());
    assert!(report
        .issues
        .iter()
        .any(|issue| matches!(issue, CweAnalysisIssue::StructuralMatchCap { cap: 1, .. })));
}

fn permutation_fixture(reverse_relations: bool) -> CodePropertyGraph {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = add_with_id(
        &mut cpg,
        1,
        CpgNodeKind::Function {
            signature: Box::new(MethodSignature {
                name: "permutation".into(),
                params: Default::default(),
                return_type: None,
                is_static: false,
                is_async: false,
                visibility: Visibility::Private,
            }),
        },
    );
    let source = add_with_id(
        &mut cpg,
        10,
        CpgNodeKind::Call {
            target: None,
            is_method: true,
        },
    );
    let source_name = add_with_id(
        &mut cpg,
        11,
        CpgNodeKind::Identifier {
            name: "request.body".into(),
            definition: None,
        },
    );
    let sink = add_with_id(
        &mut cpg,
        20,
        CpgNodeKind::Call {
            target: None,
            is_method: true,
        },
    );
    let sink_name = add_with_id(
        &mut cpg,
        21,
        CpgNodeKind::Identifier {
            name: "sqlx::query".into(),
            definition: None,
        },
    );
    let argument = add_with_id(
        &mut cpg,
        22,
        CpgNodeKind::Identifier {
            name: "value".into(),
            definition: Some(source),
        },
    );
    for (parent, child) in [
        (function, source),
        (source, source_name),
        (function, sink),
        (sink, sink_name),
        (sink, argument),
    ] {
        connect_ast(&mut cpg, parent, child);
    }
    let mut relations = vec![(source, argument), (source, sink)];
    if reverse_relations {
        relations.reverse();
    }
    for (from, to) in relations {
        cpg.connect(from, to, CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse))
            .expect("permuted DFG endpoints exist");
    }
    cpg
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn reports_are_deterministic_under_relation_and_template_permutation(reverse in any::<bool>()) {
        let mut first_templates = vec![template(89), template(78)];
        let mut second_templates = first_templates.clone();
        if reverse {
            first_templates.reverse();
        } else {
            second_templates.reverse();
        }
        let first = analyze_cwe_templates(
            &permutation_fixture(false),
            &CallGraphResolution::default(),
            &first_templates,
            &CweAnalysisConfig::default(),
        )?;
        let second = analyze_cwe_templates(
            &permutation_fixture(true),
            &CallGraphResolution::default(),
            &second_templates,
            &CweAnalysisConfig::default(),
        )?;
        prop_assert_eq!(first, second);
    }
}

#[test]
fn depth_100_000_interval_template_is_safe_on_64_kib_stack() {
    const DEPTH: u32 = 100_000;
    std::thread::Builder::new()
        .name("cwe-depth-100k".into())
        .stack_size(64 * 1024)
        .spawn(|| {
            let mut cpg = CodePropertyGraph::new(Language::Rust);
            let function = function(&mut cpg, "deep_cwe");
            let declaration = child(
                &mut cpg,
                function,
                CpgNodeKind::Variable {
                    name: "value".into(),
                    var_type: Some(Box::new(TypeInfo::new("i64"))),
                    scope: ScopeId::GLOBAL,
                    is_mutable: false,
                },
            );
            child(
                &mut cpg,
                declaration,
                CpgNodeKind::Identifier {
                    name: "value".into(),
                    definition: Some(declaration),
                },
            );
            let expression = child(
                &mut cpg,
                declaration,
                CpgNodeKind::BinaryOp {
                    operator: "+".into(),
                },
            );
            let mut root = add(
                &mut cpg,
                CpgNodeKind::Literal {
                    kind: LiteralKind::Integer(1),
                },
            );
            for _ in 0..DEPTH {
                let unary = add(
                    &mut cpg,
                    CpgNodeKind::UnaryOp {
                        operator: "+".into(),
                    },
                );
                connect_ast(&mut cpg, unary, root);
                root = unary;
            }
            connect_ast(&mut cpg, expression, root);
            child(
                &mut cpg,
                expression,
                CpgNodeKind::Literal {
                    kind: LiteralKind::Integer(1),
                },
            );
            connect_cfg(&mut cpg, function, declaration);

            let report = analyze_one(&cpg, 190);
            assert!(report.is_complete(), "{report:#?}");
            assert!(report.candidates.is_empty());
        })
        .expect("spawn 64 KiB CWE thread")
        .join()
        .expect("CWE analysis must not overflow the native stack");
}

#[cfg(feature = "serde")]
#[test]
fn templates_config_and_qualified_report_round_trip() {
    let templates = builtin_cwe_templates();
    let templates_json = serde_json::to_vec(&templates).expect("serialize templates");
    let decoded: Vec<CweTemplate> =
        serde_json::from_slice(&templates_json).expect("deserialize templates");
    assert_eq!(decoded, templates);

    let config = CweAnalysisConfig::default();
    let config_json = serde_json::to_vec(&config).expect("serialize config");
    assert_eq!(
        serde_json::from_slice::<CweAnalysisConfig>(&config_json).expect("deserialize config"),
        config
    );

    let report = analyze_one(&taint_fixture("request.body", None, "sqlx::query"), 89);
    let report_json = serde_json::to_vec(&report).expect("serialize report");
    assert_eq!(
        serde_json::from_slice::<CweAnalysis>(&report_json).expect("deserialize report"),
        report
    );
}
