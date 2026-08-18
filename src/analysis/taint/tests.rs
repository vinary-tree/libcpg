use std::collections::{BTreeSet, VecDeque};

use proptest::prelude::*;

use super::*;
use crate::{
    CpgEdgeKind, CpgNode, Language, LiteralKind, MethodSignature, SourceRange, Visibility,
};

fn function_kind(name: &str) -> CpgNodeKind {
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

fn add(cpg: &mut CodePropertyGraph, kind: CpgNodeKind) -> NodeId {
    cpg.add_node(CpgNode::new(NodeId::new(0), kind, SourceRange::default()))
}

fn connect_ast(cpg: &mut CodePropertyGraph, parent: NodeId, child: NodeId) {
    cpg.connect(parent, child, CpgEdgeKind::AstChild)
        .expect("fixture AST endpoints exist");
    cpg.node_mut(child).expect("fixture child exists").parent = Some(parent);
    cpg.node_mut(parent)
        .expect("fixture parent exists")
        .children
        .push(child);
}

fn child(cpg: &mut CodePropertyGraph, parent: NodeId, kind: CpgNodeKind) -> NodeId {
    let node = add(cpg, kind);
    connect_ast(cpg, parent, node);
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

fn call(cpg: &mut CodePropertyGraph, parent: NodeId, path: &str, target: Option<NodeId>) -> NodeId {
    let call = child(
        cpg,
        parent,
        CpgNodeKind::Call {
            target,
            is_method: path.contains("::") || path.contains('.'),
        },
    );
    child(
        cpg,
        call,
        CpgNodeKind::Identifier {
            name: path.into(),
            definition: target,
        },
    );
    if let Some(target) = target {
        cpg.connect(call, target, CpgEdgeKind::CallSite)
            .expect("fixture call topology endpoints exist");
    }
    call
}

fn argument(cpg: &mut CodePropertyGraph, call: NodeId, name: &str, from: Option<NodeId>) -> NodeId {
    let argument = child(
        cpg,
        call,
        CpgNodeKind::Identifier {
            name: name.into(),
            definition: from,
        },
    );
    if let Some(from) = from {
        cpg.connect(from, argument, CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse))
            .expect("fixture DFG endpoints exist");
    }
    argument
}

fn return_value(
    cpg: &mut CodePropertyGraph,
    function: NodeId,
    value_kind: CpgNodeKind,
    from: Option<NodeId>,
) -> NodeId {
    let returned = child(cpg, function, CpgNodeKind::Return);
    let value = child(cpg, returned, value_kind);
    if let Some(from) = from {
        cpg.connect(from, value, CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse))
            .expect("fixture return DFG endpoints exist");
    }
    value
}

fn direct_fixture(source_path: &str, sink_path: &str) -> (CodePropertyGraph, NodeId, NodeId) {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = function(&mut cpg, "entry");
    let source = call(&mut cpg, function, source_path, None);
    let sink = call(&mut cpg, function, sink_path, None);
    argument(&mut cpg, sink, "tainted", Some(source));
    (cpg, source, sink)
}

fn builtin(cpg: &CodePropertyGraph) -> TaintAnalysis {
    analyze_taint_report(
        cpg,
        &CallGraphResolution::default(),
        &TaintCatalog::builtin_generic(),
        &IfdsCaps::default(),
    )
}

#[test]
fn scenario_01_direct_source_to_sink_is_reported_with_cwe() {
    let (cpg, source, sink) = direct_fixture("std::env::var", "std::process::Command::new");
    let analysis = builtin(&cpg);
    assert!(analysis.is_complete());
    assert_eq!(analysis.flows.len(), 1);
    let flow = &analysis.flows[0];
    assert_eq!(flow.source, source);
    assert_eq!(flow.sink, sink);
    assert_eq!(flow.source_class.as_ref(), "env");
    assert_eq!(flow.sink_class.as_ref(), "command");
    assert_eq!(flow.cwe, Some(78));
    assert!(witness_is_typed_path(&cpg, &flow.witness));
}

#[test]
fn scenario_02_helper_laundering_is_caught_interprocedurally() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let caller = function(&mut cpg, "caller");
    let helper = function(&mut cpg, "helper");
    let param = parameter(&mut cpg, helper, "value");
    let callee_sink = call(&mut cpg, helper, "os.system", None);
    let sink_arg = argument(&mut cpg, callee_sink, "value", Some(param));

    let source = call(&mut cpg, caller, "std::env::var", None);
    let helper_call = call(&mut cpg, caller, "helper", Some(helper));
    let helper_arg = argument(&mut cpg, helper_call, "input", Some(source));
    cpg.connect(
        helper_arg,
        param,
        CpgEdgeKind::DataFlow(DfgEdgeKind::Parameter),
    )
    .expect("interprocedural parameter edge exists");

    let analysis = builtin(&cpg);
    assert!(analysis.is_complete());
    let flow = analysis
        .flows
        .iter()
        .find(|flow| flow.source == source && flow.sink == callee_sink)
        .expect("helper flow is reported");
    assert!(flow.witness.contains(&helper_arg));
    assert!(flow.witness.contains(&param));
    assert!(flow.witness.contains(&sink_arg));
    assert!(witness_is_typed_path(&cpg, &flow.witness));
}

#[test]
fn scenario_03_constant_argument_is_not_flagged() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = function(&mut cpg, "entry");
    let sink = call(&mut cpg, function, "os.system", None);
    child(
        &mut cpg,
        sink,
        CpgNodeKind::Literal {
            kind: LiteralKind::String("fixed".into()),
        },
    );
    assert!(builtin(&cpg).flows.is_empty());
}

#[test]
fn scenario_04_sanitizer_kills_taint() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = function(&mut cpg, "entry");
    let source = call(&mut cpg, function, "std::env::var", None);
    let sanitizer = call(&mut cpg, function, "shell_escape", None);
    argument(&mut cpg, sanitizer, "raw", Some(source));
    let sink = call(&mut cpg, function, "os.system", None);
    argument(&mut cpg, sink, "safe", Some(sanitizer));
    assert!(builtin(&cpg).flows.is_empty());
}

#[test]
fn scenario_05_external_passthrough_preserves_taint() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = function(&mut cpg, "entry");
    let source = call(&mut cpg, function, "std::env::var", None);
    let wrapper = call(&mut cpg, function, "third_party::identity", None);
    argument(&mut cpg, wrapper, "raw", Some(source));
    let sink = call(&mut cpg, function, "os.system", None);
    argument(&mut cpg, sink, "wrapped", Some(wrapper));
    let analysis = builtin(&cpg);
    assert_eq!(analysis.flows.len(), 1);
    assert!(analysis.flows[0].confidence <= 0.5);
}

#[test]
fn opaque_method_passthrough_preserves_receiver_taint() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = function(&mut cpg, "entry");
    let wrapper = child(
        &mut cpg,
        function,
        CpgNodeKind::Call {
            target: None,
            is_method: true,
        },
    );
    let member = child(
        &mut cpg,
        wrapper,
        CpgNodeKind::MemberAccess {
            member: "unwrap".into(),
        },
    );
    let source = call(&mut cpg, member, "std::env::var", None);
    child(
        &mut cpg,
        member,
        CpgNodeKind::Identifier {
            name: "unwrap".into(),
            definition: None,
        },
    );
    let sink = call(&mut cpg, function, "os.system", None);
    argument(&mut cpg, sink, "wrapped", Some(wrapper));

    let analysis = builtin(&cpg);
    let flow = analysis
        .flows
        .iter()
        .find(|flow| flow.source == source && flow.sink == sink)
        .expect("opaque method result retains receiver taint");
    assert!(flow.witness.contains(&wrapper));
    assert!(flow.confidence <= 0.5);
}

#[test]
fn resolved_constant_return_does_not_leak_argument_taint() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let caller = function(&mut cpg, "caller");
    let helper = function(&mut cpg, "constant");
    parameter(&mut cpg, helper, "ignored");
    return_value(
        &mut cpg,
        helper,
        CpgNodeKind::Literal {
            kind: LiteralKind::String("fixed".into()),
        },
        None,
    );

    let source = call(&mut cpg, caller, "std::env::var", None);
    let helper_call = call(&mut cpg, caller, "constant", Some(helper));
    argument(&mut cpg, helper_call, "tainted", Some(source));
    let sink = call(&mut cpg, caller, "os.system", None);
    argument(&mut cpg, sink, "constant", Some(helper_call));

    assert!(builtin(&cpg).flows.is_empty());
}

#[test]
fn scenario_06_external_sink_model_reports_flow() {
    let mut catalog = TaintCatalog::builtin_generic();
    catalog.externals.insert(
        0,
        (
            Pattern::Exact("vendor::danger".into()),
            ExternalModel::Sink {
                class: "custom".into(),
                cwe: Some(999),
            },
        ),
    );
    let (cpg, _, sink) = direct_fixture("std::env::var", "vendor::danger");
    let flows = analyze_taint(
        &cpg,
        &CallGraphResolution::default(),
        &catalog,
        &IfdsCaps::default(),
    );
    assert_eq!(flows.len(), 1);
    assert_eq!(flows[0].sink, sink);
    assert_eq!(flows[0].sink_class.as_ref(), "custom");
    assert_eq!(flows[0].cwe, Some(999));
}

#[test]
fn scenario_07_external_sanitizer_model_kills_taint() {
    let mut catalog = TaintCatalog::builtin_generic();
    catalog.externals.push((
        Pattern::Exact("vendor::clean".into()),
        ExternalModel::Sanitizer,
    ));
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = function(&mut cpg, "entry");
    let source = call(&mut cpg, function, "std::env::var", None);
    let sanitizer = call(&mut cpg, function, "vendor::clean", None);
    argument(&mut cpg, sanitizer, "raw", Some(source));
    let sink = call(&mut cpg, function, "os.system", None);
    argument(&mut cpg, sink, "clean", Some(sanitizer));
    assert!(analyze_taint(
        &cpg,
        &CallGraphResolution::default(),
        &catalog,
        &IfdsCaps::default()
    )
    .is_empty());
}

#[test]
fn scenario_08_self_recursion_terminates_at_a_fixpoint() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = function(&mut cpg, "recursive");
    let param = parameter(&mut cpg, function, "value");
    let sink = call(&mut cpg, function, "os.system", None);
    argument(&mut cpg, sink, "value", Some(param));
    let recursive = call(&mut cpg, function, "recursive", Some(function));
    let recursive_arg = argument(&mut cpg, recursive, "value", Some(param));
    cpg.connect(
        recursive_arg,
        param,
        CpgEdgeKind::DataFlow(DfgEdgeKind::Parameter),
    )
    .expect("self parameter edge exists");
    let analysis = builtin(&cpg);
    assert!(analysis.is_complete());
    assert!(analysis.summaries[&function].param_sinks[0].contains("command"));
}

#[test]
fn scenario_09_mutual_recursion_composes_param_to_sink() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let left = function(&mut cpg, "left");
    let right = function(&mut cpg, "right");
    let left_param = parameter(&mut cpg, left, "left_value");
    let right_param = parameter(&mut cpg, right, "right_value");

    let to_right = call(&mut cpg, left, "right", Some(right));
    let left_arg = argument(&mut cpg, to_right, "left_value", Some(left_param));
    cpg.connect(
        left_arg,
        right_param,
        CpgEdgeKind::DataFlow(DfgEdgeKind::Parameter),
    )
    .expect("left-to-right parameter edge exists");

    let to_left = call(&mut cpg, right, "left", Some(left));
    let right_arg = argument(&mut cpg, to_left, "right_value", Some(right_param));
    cpg.connect(
        right_arg,
        left_param,
        CpgEdgeKind::DataFlow(DfgEdgeKind::Parameter),
    )
    .expect("right-to-left parameter edge exists");
    let sink = call(&mut cpg, right, "os.system", None);
    argument(&mut cpg, sink, "right_value", Some(right_param));

    let analysis = builtin(&cpg);
    assert!(analysis.is_complete());
    assert!(analysis.summaries[&left].param_sinks[0].contains("command"));
    assert!(analysis.summaries[&right].param_sinks[0].contains("command"));
}

#[test]
fn scenario_10_parameter_to_return_summary_is_materialized() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = function(&mut cpg, "identity");
    let param = parameter(&mut cpg, function, "value");
    return_value(
        &mut cpg,
        function,
        CpgNodeKind::Identifier {
            name: "value".into(),
            definition: Some(param),
        },
        Some(param),
    );
    let analysis = builtin(&cpg);
    assert_eq!(analysis.summaries[&function].param_to_return, 1);
    assert!(analysis.summaries[&function]
        .param_return_sites
        .contains_key(&0));
}

#[test]
fn scenario_11_source_returning_helper_taints_caller_sink() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let caller = function(&mut cpg, "caller");
    let helper = function(&mut cpg, "source_helper");
    let returned = child(&mut cpg, helper, CpgNodeKind::Return);
    let source = call(&mut cpg, returned, "std::env::var", None);

    let helper_call = call(&mut cpg, caller, "source_helper", Some(helper));
    cpg.connect(
        source,
        helper_call,
        CpgEdgeKind::DataFlow(DfgEdgeKind::ReturnValue),
    )
    .expect("return-value edge exists");
    let sink = call(&mut cpg, caller, "os.system", None);
    argument(&mut cpg, sink, "value", Some(helper_call));

    let analysis = builtin(&cpg);
    assert!(analysis
        .flows
        .iter()
        .any(|flow| flow.source == source && flow.sink == sink));
}

#[test]
fn scenario_12_external_source_model_introduces_an_open_class() {
    let mut catalog = TaintCatalog::builtin_generic();
    catalog.externals.push((
        Pattern::Exact("vendor::input".into()),
        ExternalModel::Source {
            class: "message_bus".into(),
        },
    ));
    let (cpg, source, sink) = direct_fixture("vendor::input", "os.system");
    let flows = analyze_taint(
        &cpg,
        &CallGraphResolution::default(),
        &catalog,
        &IfdsCaps::default(),
    );
    assert_eq!(flows.len(), 1);
    assert_eq!(flows[0].source, source);
    assert_eq!(flows[0].sink, sink);
    assert_eq!(flows[0].source_class.as_ref(), "message_bus");
}

#[test]
fn swapping_catalogs_changes_findings_without_graph_mutation() {
    let (cpg, _, _) = direct_fixture("project::read", "project::write");
    let before = (cpg.node_count(), cpg.edge_count());
    assert!(analyze_taint(
        &cpg,
        &CallGraphResolution::default(),
        &TaintCatalog::empty(),
        &IfdsCaps::default()
    )
    .is_empty());
    let catalog = TaintCatalog {
        sources: vec![CatalogRule {
            pattern: Pattern::Exact("project::read".into()),
            class: "project_input".into(),
            cwe: None,
        }],
        sinks: vec![CatalogRule {
            pattern: Pattern::Exact("project::write".into()),
            class: "project_sink".into(),
            cwe: Some(1),
        }],
        ..TaintCatalog::empty()
    };
    assert_eq!(
        analyze_taint(
            &cpg,
            &CallGraphResolution::default(),
            &catalog,
            &IfdsCaps::default()
        )
        .len(),
        1
    );
    assert_eq!((cpg.node_count(), cpg.edge_count()), before);
}

#[test]
fn caps_are_explicit_on_empty_and_nonempty_partial_results() {
    let (cpg, _, _) = direct_fixture("std::env::var", "os.system");
    let empty_cap = IfdsCaps {
        max_worklist_steps: 0,
        ..IfdsCaps::default()
    };
    let analysis = analyze_taint_report(
        &cpg,
        &CallGraphResolution::default(),
        &TaintCatalog::builtin_generic(),
        &empty_cap,
    );
    assert!(analysis.capped);
    assert!(!analysis.is_complete());

    let catalog = TaintCatalog {
        sources: vec![
            CatalogRule {
                pattern: Pattern::Exact("multi::source_a".into()),
                class: "a".into(),
                cwe: None,
            },
            CatalogRule {
                pattern: Pattern::Exact("multi::source_b".into()),
                class: "b".into(),
                cwe: None,
            },
        ],
        sinks: vec![CatalogRule {
            pattern: Pattern::Exact("multi::sink".into()),
            class: "sink".into(),
            cwe: Some(7),
        }],
        ..TaintCatalog::empty()
    };
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = function(&mut cpg, "entry");
    let source_a = call(&mut cpg, function, "multi::source_a", None);
    let source_b = call(&mut cpg, function, "multi::source_b", None);
    let sink = call(&mut cpg, function, "multi::sink", None);
    let sink_arg = argument(&mut cpg, sink, "combined", Some(source_a));
    cpg.connect(
        source_b,
        sink_arg,
        CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse),
    )
    .expect("second source reaches the shared sink argument");
    let fact_cap = IfdsCaps {
        max_facts_per_node: 1,
        ..IfdsCaps::default()
    };
    let analysis = analyze_taint_report(&cpg, &CallGraphResolution::default(), &catalog, &fact_cap);
    assert!(analysis.capped);
    assert!(!analysis.flows.is_empty());
    assert!(analysis.flows.iter().all(|flow| flow.capped));
}

#[test]
fn callee_path_depth_exhaustion_qualifies_an_empty_result() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = function(&mut cpg, "entry");
    let source = child(
        &mut cpg,
        function,
        CpgNodeKind::Call {
            target: None,
            is_method: true,
        },
    );
    let var = child(
        &mut cpg,
        source,
        CpgNodeKind::MemberAccess {
            member: "var".into(),
        },
    );
    let env = child(
        &mut cpg,
        var,
        CpgNodeKind::MemberAccess {
            member: "env".into(),
        },
    );
    child(
        &mut cpg,
        env,
        CpgNodeKind::Identifier {
            name: "std".into(),
            definition: None,
        },
    );
    let sink = call(&mut cpg, function, "os.system", None);
    argument(&mut cpg, sink, "command", Some(source));

    let caps = IfdsCaps {
        max_call_depth: 0,
        ..IfdsCaps::default()
    };
    let analysis = analyze_taint_report(
        &cpg,
        &CallGraphResolution::default(),
        &TaintCatalog::builtin_generic(),
        &caps,
    );

    assert!(analysis.flows.is_empty());
    assert!(analysis.capped);
    assert!(!analysis.is_complete());
    assert!(analysis.domain_capped_functions.contains(&function));
}

#[test]
fn parameter_mask_overflow_is_never_silently_truncated() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = function(&mut cpg, "wide");
    for index in 0..65 {
        parameter(&mut cpg, function, &format!("p{index}"));
    }
    let analysis = builtin(&cpg);
    assert!(analysis.capped);
    assert!(analysis.summaries[&function].param_mask_truncated);
}

#[cfg(feature = "serde")]
#[test]
fn taint_analysis_round_trips_with_ordered_evidence() {
    let (cpg, _, _) = direct_fixture("std::env::var", "os.system");
    let analysis = builtin(&cpg);
    let json = serde_json::to_string(&analysis).expect("taint analysis serializes");
    let decoded: TaintAnalysis = serde_json::from_str(&json).expect("taint analysis deserializes");
    assert_eq!(decoded, analysis);
}

fn witness_is_typed_path(cpg: &CodePropertyGraph, witness: &[NodeId]) -> bool {
    !witness.is_empty()
        && witness.windows(2).all(|pair| {
            pair[0] == pair[1]
                || cpg.edges_between(pair[0], pair[1]).iter().any(|edge| {
                    matches!(
                        edge.kind,
                        CpgEdgeKind::DataFlow(_)
                            | CpgEdgeKind::CallSite
                            | CpgEdgeKind::StaticCall
                            | CpgEdgeKind::DynamicCall
                    )
                })
                || cpg.ast_parent(pair[0]) == Some(pair[1])
                || cpg.ast_parent(pair[1]) == Some(pair[0])
        })
}

fn reachability_fixture(n: usize, edges: &[(usize, usize)]) -> CodePropertyGraph {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = function(&mut cpg, "reachability");
    let source = call(&mut cpg, function, "std::env::var", None);
    let sink = call(&mut cpg, function, "os.system", None);
    let sink_arg = argument(&mut cpg, sink, "sink_arg", None);
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
    nodes.push(sink_arg);
    for &(from, to) in edges {
        cpg.connect(
            nodes[from],
            nodes[to],
            CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse),
        )
        .expect("oracle edge endpoints exist");
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

fn summary_from_bits(bits: u16) -> TaintSummary {
    let mut summary = TaintSummary {
        param_count: usize::from(bits & 0b11),
        param_sinks: vec![BTreeSet::new(); 2],
        param_to_return: u64::from((bits >> 2) & 0b11),
        param_mask_truncated: bits & (1 << 8) != 0,
        capped: bits & (1 << 9) != 0,
        ..TaintSummary::default()
    };
    if bits & (1 << 4) != 0 {
        summary.param_sinks[0].insert("command".into());
    }
    if bits & (1 << 5) != 0 {
        summary.param_sinks[1].insert("sql".into());
    }
    if bits & (1 << 6) != 0 {
        summary.return_sources.insert("env".into());
    }
    if bits & (1 << 7) != 0 {
        summary.return_sources.insert("request".into());
    }
    summary
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn summary_lattice_is_idempotent_commutative_and_associative(a in any::<u16>(), b in any::<u16>(), c in any::<u16>()) {
        let a = summary_from_bits(a);
        let b = summary_from_bits(b);
        let c = summary_from_bits(c);

        let mut idempotent = a.clone();
        idempotent.join(&a);
        prop_assert_eq!(&idempotent, &a);

        let mut left = a.clone();
        left.join(&b);
        let mut right = b.clone();
        right.join(&a);
        prop_assert_eq!(&left, &right);

        let mut left_assoc = a.clone();
        left_assoc.join(&b);
        left_assoc.join(&c);
        let mut bc = b.clone();
        bc.join(&c);
        let mut right_assoc = a.clone();
        right_assoc.join(&bc);
        prop_assert_eq!(&left_assoc, &right_assoc);
        prop_assert!(left_assoc.subsumes(&a));
        prop_assert!(left_assoc.subsumes(&b));
        prop_assert!(left_assoc.subsumes(&c));
    }

    #[test]
    fn analysis_matches_brute_force_reachability(
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
        let analysis = builtin(&cpg);
        prop_assert_eq!(!analysis.flows.is_empty(), brute_reachable(n, &edges));
        prop_assert!(analysis.flows.iter().all(|flow| witness_is_typed_path(&cpg, &flow.witness)));
    }

    #[test]
    fn dfg_edge_insertion_permutation_does_not_change_results(
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
        prop_assert_eq!(builtin(&forward), builtin(&reverse));
    }
}
