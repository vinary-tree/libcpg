use proptest::prelude::*;

use super::*;
use crate::{
    CpgEdgeKind, CpgNode, CpgNodeKind, DfgEdgeKind, Language, MethodSignature, ScopeId,
    SourceRange, Visibility,
};

fn add(cpg: &mut CodePropertyGraph, id: u32, kind: CpgNodeKind) -> NodeId {
    let id = NodeId::new(id);
    cpg.add_node_with_id(CpgNode::new(
        id,
        kind,
        SourceRange::from_bytes(id.as_u32() * 3, id.as_u32() * 3 + 1),
    ));
    id
}

fn function_kind(name: &str) -> CpgNodeKind {
    CpgNodeKind::Function {
        signature: MethodSignature {
            name: name.into(),
            params: Default::default(),
            return_type: None,
            is_static: false,
            is_async: false,
            visibility: Visibility::Private,
        },
    }
}

fn variable_kind(name: &str) -> CpgNodeKind {
    CpgNodeKind::Variable {
        name: name.into(),
        var_type: None,
        scope: ScopeId::GLOBAL,
        is_mutable: true,
    }
}

fn identifier(name: &str, definition: Option<NodeId>) -> CpgNodeKind {
    CpgNodeKind::Identifier {
        name: name.into(),
        definition,
    }
}

fn ast(cpg: &mut CodePropertyGraph, parent: NodeId, child: NodeId) {
    cpg.connect(parent, child, CpgEdgeKind::AstChild)
        .expect("fixture AST endpoints exist");
    cpg.node_mut(parent)
        .expect("fixture parent exists")
        .children
        .push(child);
    cpg.node_mut(child).expect("fixture child exists").parent = Some(parent);
}

#[test]
fn use_before_definition_is_flagged_while_a_reaching_definition_is_clean() {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    let function = add(&mut cpg, 1, function_kind("entry"));
    let variable = add(&mut cpg, 10, variable_kind("x"));
    let binder = add(&mut cpg, 11, identifier("x", Some(variable)));
    let before = add(&mut cpg, 20, identifier("x", None));
    let after = add(&mut cpg, 21, identifier("x", Some(variable)));
    ast(&mut cpg, function, before);
    ast(&mut cpg, function, variable);
    ast(&mut cpg, variable, binder);
    ast(&mut cpg, function, after);
    cpg.connect(
        variable,
        after,
        CpgEdgeKind::DataFlow(DfgEdgeKind::ReachingDef),
    )
    .expect("fixture DFG endpoints exist");
    let before_counts = (cpg.node_count(), cpg.edge_count());

    let report = analyze_uninitialized_uses(&cpg);

    assert!(report.is_complete());
    assert_eq!(report.processed_functions, vec![function]);
    assert_eq!(report.uses.len(), 1, "{report:#?}");
    assert_eq!(report.uses[0].use_site, before);
    assert_eq!(report.uses[0].variable.as_ref(), "x");
    assert!(report.uses[0].out_of_scope_definitions.is_empty());
    assert_eq!(uninitialized_uses(&cpg), report.uses);
    assert_eq!((cpg.node_count(), cpg.edge_count()), before_counts);
}

#[test]
fn unbound_function_names_are_syntax_but_bound_callbacks_are_variable_uses() {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    let function = add(&mut cpg, 1, function_kind("entry"));
    let call = add(
        &mut cpg,
        10,
        CpgNodeKind::Call {
            target: None,
            is_method: false,
        },
    );
    let function_name = add(&mut cpg, 11, identifier("consume", None));
    ast(&mut cpg, function, call);
    ast(&mut cpg, call, function_name);

    let callback = add(&mut cpg, 20, variable_kind("callback"));
    let callback_call = add(
        &mut cpg,
        21,
        CpgNodeKind::Call {
            target: None,
            is_method: false,
        },
    );
    let callback_name = add(&mut cpg, 22, identifier("callback", Some(callback)));
    ast(&mut cpg, function, callback);
    ast(&mut cpg, function, callback_call);
    ast(&mut cpg, callback_call, callback_name);

    let report = analyze_uninitialized_uses(&cpg);

    assert_eq!(report.uses.len(), 1, "{report:#?}");
    assert_eq!(report.uses[0].use_site, callback_name);
    assert_eq!(report.uses[0].variable.as_ref(), "callback");
    assert!(!report
        .uses
        .iter()
        .any(|finding| finding.use_site == function_name));
}

#[test]
fn definitions_from_another_function_do_not_initialize_the_local_use() {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    let first = add(&mut cpg, 1, function_kind("first"));
    let use_site = add(&mut cpg, 2, identifier("x", None));
    ast(&mut cpg, first, use_site);
    let second = add(&mut cpg, 10, function_kind("second"));
    let external = add(&mut cpg, 11, variable_kind("x"));
    ast(&mut cpg, second, external);
    cpg.connect(
        external,
        use_site,
        CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse),
    )
    .expect("cross-function DFG endpoints exist");

    let report = analyze_uninitialized_uses(&cpg);

    assert_eq!(report.processed_functions, vec![first, second]);
    assert_eq!(report.uses.len(), 1, "{report:#?}");
    assert_eq!(report.uses[0].use_site, use_site);
    assert_eq!(report.uses[0].out_of_scope_definitions, vec![external]);
}

fn generated_fixture(initialized: &[bool], reverse_edges: bool) -> CodePropertyGraph {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    let function = add(&mut cpg, 1, function_kind("generated"));
    let variable = add(&mut cpg, 2, variable_kind("x"));
    let binder = add(&mut cpg, 3, identifier("x", Some(variable)));
    ast(&mut cpg, function, variable);
    ast(&mut cpg, variable, binder);

    let mut edges = Vec::new();
    for (index, &is_initialized) in initialized.iter().enumerate() {
        let use_site = add(&mut cpg, 10 + index as u32, identifier("x", Some(variable)));
        ast(&mut cpg, function, use_site);
        if is_initialized {
            edges.push((variable, use_site));
        }
    }
    if reverse_edges {
        edges.reverse();
    }
    for (definition, use_site) in edges {
        cpg.connect(
            definition,
            use_site,
            CpgEdgeKind::DataFlow(DfgEdgeKind::ReachingDef),
        )
        .expect("generated DFG endpoints exist");
    }
    cpg
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn generated_uses_match_the_independent_empty_reaching_definition_oracle(
        initialized in prop::collection::vec(any::<bool>(), 0..40),
    ) {
        let forward = generated_fixture(&initialized, false);
        let reversed = generated_fixture(&initialized, true);
        let expected: Vec<_> = initialized
            .iter()
            .enumerate()
            .filter_map(|(index, initialized)| (!initialized).then_some(NodeId::new(10 + index as u32)))
            .collect();
        let forward_report = analyze_uninitialized_uses(&forward);
        let reversed_report = analyze_uninitialized_uses(&reversed);

        prop_assert_eq!(
            forward_report.uses.iter().map(|finding| finding.use_site).collect::<Vec<_>>(),
            expected,
        );
        prop_assert_eq!(forward_report, reversed_report);
    }
}

#[cfg(feature = "serde")]
#[test]
fn report_round_trips_through_json() {
    let cpg = generated_fixture(&[false, true, false], false);
    let report = analyze_uninitialized_uses(&cpg);
    let encoded = serde_json::to_string(&report).expect("serialize uninitialized-use report");
    let decoded: UninitializedUseAnalysis =
        serde_json::from_str(&encoded).expect("deserialize uninitialized-use report");
    assert_eq!(decoded, report);
}
