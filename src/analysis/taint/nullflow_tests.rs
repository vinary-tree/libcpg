use std::collections::VecDeque;

use proptest::prelude::*;

use super::*;
use crate::{
    CodePropertyGraph, CpgEdgeKind, CpgNode, Language, MethodSignature, ScopeId, SourceRange,
    TypeInfo, Visibility,
};

fn function_kind(name: &str, return_type: Option<&str>) -> CpgNodeKind {
    CpgNodeKind::Function {
        signature: Box::new(MethodSignature {
            name: name.into(),
            params: Default::default(),
            return_type: return_type.map(TypeInfo::new),
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
        .expect("null fixture AST endpoints exist");
}

fn child(cpg: &mut CodePropertyGraph, parent: NodeId, kind: CpgNodeKind) -> NodeId {
    let node = add(cpg, kind);
    connect_ast(cpg, parent, node);
    node
}

fn function(cpg: &mut CodePropertyGraph, name: &str, return_type: Option<&str>) -> NodeId {
    add(cpg, function_kind(name, return_type))
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

fn call(cpg: &mut CodePropertyGraph, parent: NodeId, name: &str, target: Option<NodeId>) -> NodeId {
    let call = child(
        cpg,
        parent,
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
    if let Some(target) = target {
        cpg.connect(call, target, CpgEdgeKind::CallSite)
            .expect("null fixture call topology endpoints exist");
    }
    call
}

fn argument(cpg: &mut CodePropertyGraph, call: NodeId, name: &str, source: NodeId) -> NodeId {
    let argument = child(
        cpg,
        call,
        CpgNodeKind::Identifier {
            name: name.into(),
            definition: Some(source),
        },
    );
    cpg.connect(source, argument, CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse))
        .expect("null fixture argument flow endpoints exist");
    argument
}

fn dereference(cpg: &mut CodePropertyGraph, function: NodeId, source: NodeId) -> NodeId {
    let sink = child(
        cpg,
        function,
        CpgNodeKind::UnaryOp {
            operator: "*".into(),
        },
    );
    cpg.connect(
        source,
        sink,
        CpgEdgeKind::DataFlow(DfgEdgeKind::Dereference),
    )
    .expect("null fixture dereference endpoints exist");
    sink
}

fn analyze(cpg: &CodePropertyGraph) -> NullAnalysis {
    analyze_null_flows_report(cpg, &CallGraphResolution::default(), &IfdsCaps::default())
}

#[test]
fn null_literal_reaching_dereference_is_reported() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = function(&mut cpg, "entry", None);
    let literal = child(
        &mut cpg,
        function,
        CpgNodeKind::Literal {
            kind: LiteralKind::Null,
        },
    );
    let sink = dereference(&mut cpg, function, literal);
    let analysis = analyze(&cpg);
    assert!(analysis.is_complete());
    assert_eq!(analysis.flows.len(), 1);
    assert_eq!(analysis.flows[0].origin, literal);
    assert_eq!(analysis.flows[0].origin_kind, NullOriginKind::Literal);
    assert_eq!(analysis.flows[0].sink, sink);
    assert!(typed_witness(&cpg, &analysis.flows[0].witness));
}

#[test]
fn uninitialized_variable_reaching_dereference_is_reported() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = function(&mut cpg, "entry", None);
    let variable = child(
        &mut cpg,
        function,
        CpgNodeKind::Variable {
            name: "pointer".into(),
            var_type: None,
            scope: ScopeId::new(1),
            is_mutable: true,
        },
    );
    let sink = dereference(&mut cpg, function, variable);
    let analysis = analyze(&cpg);
    assert!(analysis.flows.iter().any(|flow| {
        flow.origin == variable
            && flow.origin_kind == NullOriginKind::UninitializedVariable
            && flow.sink == sink
    }));
}

#[test]
fn initialized_non_null_variable_is_not_a_null_origin() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = function(&mut cpg, "entry", None);
    let variable = child(
        &mut cpg,
        function,
        CpgNodeKind::Variable {
            name: "pointer".into(),
            var_type: None,
            scope: ScopeId::new(1),
            is_mutable: true,
        },
    );
    child(
        &mut cpg,
        variable,
        CpgNodeKind::Literal {
            kind: LiteralKind::Integer(1),
        },
    );
    dereference(&mut cpg, function, variable);
    assert!(analyze(&cpg).flows.is_empty());
}

#[test]
fn nullable_return_type_introduces_a_ranked_origin() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let caller = function(&mut cpg, "caller", None);
    let callee = function(&mut cpg, "lookup", Some("Option<String>"));
    let call = call(&mut cpg, caller, "lookup", Some(callee));
    let sink = dereference(&mut cpg, caller, call);
    let analysis = analyze(&cpg);
    let flow = analysis
        .flows
        .iter()
        .find(|flow| flow.origin == call && flow.sink == sink)
        .expect("nullable return reaches dereference");
    assert_eq!(flow.origin_kind, NullOriginKind::NullableReturn);
    assert!(flow.confidence <= 0.85);
}

#[test]
fn nonnullable_return_type_does_not_invent_null() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let caller = function(&mut cpg, "caller", None);
    let callee = function(&mut cpg, "lookup", Some("String"));
    let call = call(&mut cpg, caller, "lookup", Some(callee));
    dereference(&mut cpg, caller, call);
    assert!(analyze(&cpg).flows.is_empty());
}

#[test]
fn interprocedural_parameter_to_dereference_is_composed() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let caller = function(&mut cpg, "caller", None);
    let helper = function(&mut cpg, "helper", None);
    let param = parameter(&mut cpg, helper, "pointer");
    let sink = dereference(&mut cpg, helper, param);

    let literal = child(
        &mut cpg,
        caller,
        CpgNodeKind::Literal {
            kind: LiteralKind::Null,
        },
    );
    let helper_call = call(&mut cpg, caller, "helper", Some(helper));
    let arg = argument(&mut cpg, helper_call, "pointer", literal);
    cpg.connect(arg, param, CpgEdgeKind::DataFlow(DfgEdgeKind::Parameter))
        .expect("null parameter edge exists");

    let analysis = analyze(&cpg);
    let flow = analysis
        .flows
        .iter()
        .find(|flow| flow.origin == literal && flow.sink == sink)
        .expect("interprocedural null flow exists");
    assert_eq!(flow.param_index, Some(0));
    assert!(typed_witness(&cpg, &flow.witness));
}

#[test]
fn null_returning_helper_propagates_to_caller() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let caller = function(&mut cpg, "caller", None);
    let helper = function(&mut cpg, "helper", None);
    let returned = child(&mut cpg, helper, CpgNodeKind::Return);
    let literal = child(
        &mut cpg,
        returned,
        CpgNodeKind::Literal {
            kind: LiteralKind::Null,
        },
    );
    let helper_call = call(&mut cpg, caller, "helper", Some(helper));
    cpg.connect(
        literal,
        helper_call,
        CpgEdgeKind::DataFlow(DfgEdgeKind::ReturnValue),
    )
    .expect("null return-value edge exists");
    let sink = dereference(&mut cpg, caller, helper_call);
    let analysis = analyze(&cpg);
    assert!(analysis
        .flows
        .iter()
        .any(|flow| flow.origin == literal && flow.sink == sink));
}

#[test]
fn null_analysis_is_out_of_graph_and_reports_caps() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = function(&mut cpg, "entry", None);
    let literal = child(
        &mut cpg,
        function,
        CpgNodeKind::Literal {
            kind: LiteralKind::Null,
        },
    );
    dereference(&mut cpg, function, literal);
    let shape = (cpg.node_count(), cpg.edge_count());
    let caps = IfdsCaps {
        max_worklist_steps: 0,
        ..IfdsCaps::default()
    };
    let analysis = analyze_null_flows_report(&cpg, &CallGraphResolution::default(), &caps);
    assert!(analysis.capped);
    assert!(!analysis.is_complete());
    assert_eq!((cpg.node_count(), cpg.edge_count()), shape);
}

#[cfg(feature = "serde")]
#[test]
fn null_analysis_round_trips_with_ordered_evidence() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = function(&mut cpg, "entry", None);
    let literal = child(
        &mut cpg,
        function,
        CpgNodeKind::Literal {
            kind: LiteralKind::Null,
        },
    );
    dereference(&mut cpg, function, literal);
    let analysis = analyze(&cpg);
    let json = serde_json::to_string(&analysis).expect("null analysis serializes");
    let decoded: NullAnalysis = serde_json::from_str(&json).expect("null analysis deserializes");
    assert_eq!(decoded, analysis);
}

fn typed_witness(cpg: &CodePropertyGraph, witness: &[NodeId]) -> bool {
    !witness.is_empty()
        && witness.windows(2).all(|pair| {
            cpg.edges_between(pair[0], pair[1])
                .iter()
                .any(|edge| matches!(edge.kind, CpgEdgeKind::DataFlow(_)))
                || cpg.ast_parent(pair[0]) == Some(pair[1])
                || cpg.ast_parent(pair[1]) == Some(pair[0])
        })
}

fn reachability_fixture(n: usize, edges: &[(usize, usize)]) -> CodePropertyGraph {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = function(&mut cpg, "reachability", None);
    let source = child(
        &mut cpg,
        function,
        CpgNodeKind::Literal {
            kind: LiteralKind::Null,
        },
    );
    let mut nodes = vec![source];
    for index in 1..n.saturating_sub(1) {
        nodes.push(child(
            &mut cpg,
            function,
            CpgNodeKind::Identifier {
                name: format!("v{index}").into(),
                definition: None,
            },
        ));
    }
    let sink = child(
        &mut cpg,
        function,
        CpgNodeKind::UnaryOp {
            operator: "*".into(),
        },
    );
    nodes.push(sink);
    for &(from, to) in edges {
        let kind = if to == n - 1 {
            DfgEdgeKind::Dereference
        } else {
            DfgEdgeKind::DefUse
        };
        cpg.connect(nodes[from], nodes[to], CpgEdgeKind::DataFlow(kind))
            .expect("null oracle edge endpoints exist");
    }
    cpg
}

fn brute_reachable(n: usize, edges: &[(usize, usize)]) -> bool {
    let mut adjacency = vec![Vec::new(); n];
    for &(from, to) in edges {
        adjacency[from].push(to);
    }
    let mut seen = vec![false; n];
    let mut queue = VecDeque::from([0usize]);
    seen[0] = true;
    while let Some(node) = queue.pop_front() {
        for &next in &adjacency[node] {
            if !seen[next] {
                seen[next] = true;
                queue.push_back(next);
            }
        }
    }
    seen[n - 1]
}

fn summary_from_bits(bits: u8) -> NullSummary {
    let mut summary = NullSummary {
        param_count: usize::from(bits & 0b11),
        param_to_return: u64::from((bits >> 2) & 0b11),
        returns_maybe_null: bits & (1 << 4) != 0,
        param_mask_truncated: bits & (1 << 5) != 0,
        capped: bits & (1 << 6) != 0,
        ..NullSummary::default()
    };
    if bits & (1 << 7) != 0 {
        summary.return_sites.insert(
            MaybeNull::Origin {
                kind: NullOriginKind::Literal,
                node: NodeId::new(1),
            },
            TaintWitness::at(NodeId::new(1), 1_000),
        );
    }
    summary
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn null_summary_lattice_obeys_join_laws(a in any::<u8>(), b in any::<u8>(), c in any::<u8>()) {
        let a = summary_from_bits(a);
        let b = summary_from_bits(b);
        let c = summary_from_bits(c);
        let mut idem = a.clone();
        idem.join(&a);
        prop_assert_eq!(&idem, &a);
        let mut ab = a.clone();
        ab.join(&b);
        let mut ba = b.clone();
        ba.join(&a);
        prop_assert_eq!(&ab, &ba);
        let mut left = ab;
        left.join(&c);
        let mut bc = b;
        bc.join(&c);
        let mut right = a.clone();
        right.join(&bc);
        prop_assert_eq!(&left, &right);
        prop_assert!(left.subsumes(&a));
    }

    #[test]
    fn nullflow_matches_brute_reachability(
        n in 2usize..=7,
        bits in prop::collection::vec(any::<bool>(), 21),
    ) {
        let mut edges = Vec::new();
        let mut bit = 0usize;
        for from in 0..n {
            for to in (from + 1)..n {
                if bits[bit] {
                    edges.push((from, to));
                }
                bit += 1;
            }
        }
        let cpg = reachability_fixture(n, &edges);
        let analysis = analyze(&cpg);
        prop_assert_eq!(!analysis.flows.is_empty(), brute_reachable(n, &edges));
        prop_assert!(analysis.flows.iter().all(|flow| typed_witness(&cpg, &flow.witness)));
    }

    #[test]
    fn nullflow_is_stable_under_dfg_edge_insertion_permutation(
        n in 2usize..=7,
        bits in prop::collection::vec(any::<bool>(), 21),
    ) {
        let mut edges = Vec::new();
        let mut bit = 0usize;
        for from in 0..n {
            for to in (from + 1)..n {
                if bits[bit] {
                    edges.push((from, to));
                }
                bit += 1;
            }
        }
        let forward = reachability_fixture(n, &edges);
        edges.reverse();
        let reverse = reachability_fixture(n, &edges);
        prop_assert_eq!(analyze(&forward), analyze(&reverse));
    }
}
