use proptest::prelude::*;

use super::*;
use crate::{
    call_graph_sccs, CfgEdgeKind, CpgNode, Language, MethodSignature, SourceRange, TypeInfo,
    Visibility,
};

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

fn add(cpg: &mut CodePropertyGraph, kind: CpgNodeKind) -> NodeId {
    cpg.add_node(CpgNode::new(NodeId::new(0), kind, SourceRange::default()))
}

fn child(cpg: &mut CodePropertyGraph, parent: NodeId, kind: CpgNodeKind) -> NodeId {
    let node = add(cpg, kind);
    cpg.connect(parent, node, CpgEdgeKind::AstChild)
        .expect("heap fixture AST endpoints exist");
    node
}

fn function(cpg: &mut CodePropertyGraph, name: &str) -> NodeId {
    add(cpg, function_kind(name))
}

fn parameter(cpg: &mut CodePropertyGraph, function: NodeId, name: &str) -> NodeId {
    child(
        cpg,
        function,
        CpgNodeKind::Parameter {
            name: name.into(),
            param_type: None,
            is_variadic: false,
        },
    )
}

fn allocation(cpg: &mut CodePropertyGraph, function: NodeId) -> NodeId {
    let allocation = child(
        cpg,
        function,
        CpgNodeKind::Call {
            target: None,
            is_method: false,
        },
    );
    cpg.node_mut(allocation)
        .expect("allocation node exists")
        .allocation_type = Some(Box::new(TypeInfo::new("heap::Object")));
    child(
        cpg,
        allocation,
        CpgNodeKind::Identifier {
            name: "alloc".into(),
            definition: None,
        },
    );
    allocation
}

fn call(
    cpg: &mut CodePropertyGraph,
    function: NodeId,
    name: &str,
    target: Option<NodeId>,
    argument: Option<NodeId>,
) -> NodeId {
    let call = child(
        cpg,
        function,
        CpgNodeKind::Call {
            target,
            is_method: false,
        },
    );
    child(
        cpg,
        call,
        CpgNodeKind::Identifier {
            name: name.into(),
            definition: target,
        },
    );
    if let Some(argument) = argument {
        let use_site = child(
            cpg,
            call,
            CpgNodeKind::Identifier {
                name: "object".into(),
                definition: Some(argument),
            },
        );
        cpg.connect(
            argument,
            use_site,
            CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse),
        )
        .expect("heap fixture argument flow endpoints exist");
    }
    if let Some(target) = target {
        cpg.connect(call, target, CpgEdgeKind::CallSite)
            .expect("heap fixture call graph endpoints exist");
    }
    call
}

fn dereference(cpg: &mut CodePropertyGraph, function: NodeId, operand: NodeId) -> NodeId {
    let dereference = child(
        cpg,
        function,
        CpgNodeKind::UnaryOp {
            operator: "*".into(),
        },
    );
    cpg.connect(
        operand,
        dereference,
        CpgEdgeKind::DataFlow(DfgEdgeKind::Dereference),
    )
    .expect("heap fixture dereference endpoints exist");
    dereference
}

fn cfg(cpg: &mut CodePropertyGraph, nodes: &[NodeId]) {
    for pair in nodes.windows(2) {
        cpg.connect(
            pair[0],
            pair[1],
            CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
        )
        .expect("heap fixture CFG endpoints exist");
    }
    if let Some(&exit) = nodes.last() {
        cpg.add_cfg_exit(exit);
    }
}

fn analyze(cpg: &CodePropertyGraph) -> HeapAnalysis {
    analyze_heap(cpg, &call_graph_sccs(cpg), &IfdsCaps::default())
}

fn deterministic_fixture(reverse_edges: bool) -> CodePropertyGraph {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let nodes = [
        (1, function_kind("deterministic")),
        (
            2,
            CpgNodeKind::Call {
                target: None,
                is_method: false,
            },
        ),
        (
            3,
            CpgNodeKind::Identifier {
                name: "alloc".into(),
                definition: None,
            },
        ),
        (
            4,
            CpgNodeKind::Call {
                target: None,
                is_method: false,
            },
        ),
        (
            5,
            CpgNodeKind::Identifier {
                name: "free".into(),
                definition: None,
            },
        ),
        (
            6,
            CpgNodeKind::Identifier {
                name: "object".into(),
                definition: Some(NodeId::new(2)),
            },
        ),
        (
            7,
            CpgNodeKind::UnaryOp {
                operator: "*".into(),
            },
        ),
        (8, CpgNodeKind::Return),
    ];
    for (id, kind) in nodes {
        cpg.add_node_with_id(CpgNode::new(
            NodeId::new(id),
            kind,
            SourceRange::from_bytes(id * 2, id * 2 + 1),
        ));
    }
    cpg.node_mut(NodeId::new(2))
        .expect("determinism allocation exists")
        .allocation_type = Some(Box::new(TypeInfo::new("heap::Object")));
    let mut edges = vec![
        (1, 2, CpgEdgeKind::AstChild),
        (2, 3, CpgEdgeKind::AstChild),
        (1, 4, CpgEdgeKind::AstChild),
        (4, 5, CpgEdgeKind::AstChild),
        (4, 6, CpgEdgeKind::AstChild),
        (1, 7, CpgEdgeKind::AstChild),
        (1, 8, CpgEdgeKind::AstChild),
        (2, 6, CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse)),
        (2, 7, CpgEdgeKind::DataFlow(DfgEdgeKind::Dereference)),
        (1, 2, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential)),
        (2, 4, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential)),
        (4, 7, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential)),
        (7, 8, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential)),
    ];
    if reverse_edges {
        edges.reverse();
    }
    for (source, target, kind) in edges {
        cpg.connect(NodeId::new(source), NodeId::new(target), kind)
            .expect("determinism fixture endpoints exist");
    }
    cpg.add_cfg_exit(NodeId::new(8));
    cpg
}

#[test]
fn null_dereference_is_flagged_from_typed_dfg_evidence() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = function(&mut cpg, "null_deref");
    let null = child(
        &mut cpg,
        function,
        CpgNodeKind::Literal {
            kind: LiteralKind::Null,
        },
    );
    let dereference = dereference(&mut cpg, function, null);
    cfg(&mut cpg, &[function, null, dereference]);

    let analysis = analyze(&cpg);
    assert!(analysis.is_complete());
    assert_eq!(
        analysis.violations(&cpg),
        vec![HeapViolation {
            kind: HeapViolationKind::NullDereference,
            function,
            object: Some(null),
            operation: dereference,
            witness: vec![null, dereference],
            capped: false,
        }]
    );
}

#[test]
fn allocation_released_before_exit_is_clean_and_has_an_invalid_post() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = function(&mut cpg, "safe");
    let allocation = allocation(&mut cpg, function);
    let release = call(&mut cpg, function, "free", None, Some(allocation));
    let exit = child(&mut cpg, function, CpgNodeKind::Return);
    cfg(&mut cpg, &[function, allocation, release, exit]);

    let analysis = analyze(&cpg);
    assert!(analysis.is_complete());
    assert!(analysis.violations(&cpg).is_empty());
    let summary = &analysis.summaries[&function];
    assert!(summary.postconditions.iter().any(|assertion| {
        assertion.predicate == HeapPredicate::Invalid
            && assertion.location == HeapLocation::Allocation(allocation)
    }));
}

#[test]
fn live_exit_is_a_leak_and_release_then_dereference_is_uaf() {
    let mut leak_cpg = CodePropertyGraph::new(Language::Rust);
    let leak_function = function(&mut leak_cpg, "leak");
    let leaked = allocation(&mut leak_cpg, leak_function);
    let leak_exit = child(&mut leak_cpg, leak_function, CpgNodeKind::Return);
    cfg(&mut leak_cpg, &[leak_function, leaked, leak_exit]);
    let leak = analyze(&leak_cpg).violations(&leak_cpg);
    assert!(leak
        .iter()
        .any(|finding| finding.kind == HeapViolationKind::Leak));

    let mut uaf_cpg = CodePropertyGraph::new(Language::Rust);
    let uaf_function = function(&mut uaf_cpg, "uaf");
    let allocation = allocation(&mut uaf_cpg, uaf_function);
    let release = call(&mut uaf_cpg, uaf_function, "free", None, Some(allocation));
    let dereference = dereference(&mut uaf_cpg, uaf_function, allocation);
    cfg(
        &mut uaf_cpg,
        &[uaf_function, allocation, release, dereference],
    );
    let uaf = analyze(&uaf_cpg).violations(&uaf_cpg);
    assert!(uaf.iter().any(|finding| {
        finding.kind == HeapViolationKind::UseAfterFree
            && finding.witness == vec![allocation, release, dereference]
    }));
}

#[test]
fn caller_substitutes_null_into_callee_antiframe() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let callee = function(&mut cpg, "callee");
    let parameter = parameter(&mut cpg, callee, "pointer");
    let callee_deref = dereference(&mut cpg, callee, parameter);
    cfg(&mut cpg, &[callee, callee_deref]);

    let caller = function(&mut cpg, "caller");
    let null = child(
        &mut cpg,
        caller,
        CpgNodeKind::Literal {
            kind: LiteralKind::Null,
        },
    );
    let call = call(&mut cpg, caller, "callee", Some(callee), Some(null));
    cfg(&mut cpg, &[caller, null, call]);

    let analysis = analyze(&cpg);
    assert!(analysis.is_complete());
    assert!(analysis.processed_functions.starts_with(&[callee]));
    assert!(analysis.violations(&cpg).iter().any(|finding| {
        finding.kind == HeapViolationKind::NullDereference
            && finding.function == caller
            && finding.operation == call
    }));
}

#[test]
fn heap_overlay_is_typed_append_only_and_idempotent() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = function(&mut cpg, "overlay");
    let allocation = allocation(&mut cpg, function);
    let dereference = dereference(&mut cpg, function, allocation);
    cfg(&mut cpg, &[function, allocation, dereference]);
    let summaries = biabduction_summaries(&cpg, &call_graph_sccs(&cpg), &IfdsCaps::default());
    let shape = (cpg.node_count(), cpg.edge_count());

    let added = materialize_heap_overlay(&mut cpg, &summaries).expect("valid heap overlay");
    assert!(added >= 2);
    assert_eq!(cpg.node_count(), shape.0);
    assert_eq!(
        materialize_heap_overlay(&mut cpg, &summaries).expect("idempotent heap overlay"),
        0
    );
    assert_eq!(cpg.stats().heap_edges, added);
    assert!(cpg.heap_edges().all(|edge| edge.kind.is_heap()));

    let invalid = BTreeMap::from([(
        function,
        HeapSummary {
            function: Some(function),
            relations: BTreeSet::from([HeapRelation {
                source: NodeId::new(u32::MAX),
                target: allocation,
                kind: HeapEdgeKind::PointsTo,
            }]),
            ..HeapSummary::default()
        },
    )]);
    let edge_count = cpg.edge_count();
    assert!(matches!(
        materialize_heap_overlay(&mut cpg, &invalid),
        Err(HeapOverlayError::UnknownSource { .. })
    ));
    assert_eq!(cpg.edge_count(), edge_count, "failure is transactional");
}

#[test]
fn zero_work_cap_and_non_call_graph_decomposition_fail_closed() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = function(&mut cpg, "capped");
    let allocation = allocation(&mut cpg, function);
    let exit = child(&mut cpg, function, CpgNodeKind::Return);
    cfg(&mut cpg, &[function, allocation, exit]);

    let caps = IfdsCaps {
        max_worklist_steps: 0,
        ..IfdsCaps::default()
    };
    let capped = analyze_heap(&cpg, &call_graph_sccs(&cpg), &caps);
    assert!(capped.capped);
    assert!(!capped.is_complete());
    assert!(capped.summaries.is_empty());

    let control_flow = crate::control_flow_sccs(&cpg, function).expect("valid fixture CFG");
    let malformed = analyze_heap(&cpg, &control_flow, &IfdsCaps::default());
    assert!(!malformed.is_complete());
    assert!(malformed.issues.contains(&IfdsIssue::NotCallGraph));
}

#[test]
fn recursive_must_invalidate_effect_is_explicitly_partial() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = function(&mut cpg, "recursive_release");
    let parameter = parameter(&mut cpg, function, "pointer");
    let release = call(&mut cpg, function, "free", None, Some(parameter));
    let recurse = call(
        &mut cpg,
        function,
        "recursive_release",
        Some(function),
        Some(parameter),
    );
    let exit = child(&mut cpg, function, CpgNodeKind::Return);
    cfg(&mut cpg, &[function, release, recurse, exit]);

    let analysis = analyze(&cpg);
    assert!(!analysis.is_complete());
    assert!(analysis.capped);
    assert!(analysis.domain_capped_functions.contains(&function));
    assert!(analysis
        .component_iterations
        .values()
        .all(|&rounds| rounds >= 2));
}

#[cfg(feature = "serde")]
#[test]
fn heap_analysis_and_overlay_round_trip_without_reordering() {
    let cpg = deterministic_fixture(false);
    let analysis = analyze(&cpg);
    let bytes = serde_json::to_vec(&analysis).expect("serialize heap analysis");
    let decoded: HeapAnalysis = serde_json::from_slice(&bytes).expect("deserialize heap analysis");
    assert_eq!(decoded, analysis);
    assert_eq!(
        serde_json::to_vec(&decoded).expect("reserialize heap analysis"),
        bytes
    );
}

proptest! {
    #[test]
    fn summaries_ignore_cpg_edge_insertion_order(reverse_edges in any::<bool>()) {
        let canonical = deterministic_fixture(false);
        let permuted = deterministic_fixture(reverse_edges);
        let canonical_analysis = analyze(&canonical);
        let permuted_analysis = analyze(&permuted);
        prop_assert_eq!(canonical_analysis, permuted_analysis);
    }

    #[test]
    fn release_before_exit_is_clean_for_arbitrary_straight_line_width(padding in 0usize..96) {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let function = function(&mut cpg, "generated_safe");
        let allocation = allocation(&mut cpg, function);
        let mut flow = vec![function, allocation];
        for _ in 0..padding {
            flow.push(child(
                &mut cpg,
                function,
                CpgNodeKind::Unknown { kind: "padding".into() },
            ));
        }
        let release = call(&mut cpg, function, "free", None, Some(allocation));
        let exit = child(&mut cpg, function, CpgNodeKind::Return);
        flow.extend([release, exit]);
        cfg(&mut cpg, &flow);

        let first = analyze(&cpg);
        let second = analyze(&cpg);
        prop_assert_eq!(&first, &second);
        prop_assert!(first.is_complete());
        prop_assert!(first.violations(&cpg).is_empty());
    }
}
