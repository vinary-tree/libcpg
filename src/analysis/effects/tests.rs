use proptest::prelude::*;

use super::*;
use crate::analysis::{call_graph_sccs, SccProjection};
use crate::{
    CpgEdgeKind, CpgNode, DfgEdgeKind, Language, LiteralKind, MethodSignature, ScopeId,
    SourceRange, Visibility,
};

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

fn node(id: u32, kind: CpgNodeKind) -> CpgNode {
    CpgNode::new(
        NodeId::new(id),
        kind,
        SourceRange {
            start: id,
            end: id.saturating_add(1),
            start_line: 1,
            start_col: id,
            end_line: 1,
            end_col: id.saturating_add(1),
        },
    )
}

fn add(cpg: &mut CodePropertyGraph, id: u32, kind: CpgNodeKind) -> NodeId {
    cpg.add_node_with_id(node(id, kind))
}

fn connect_ast(cpg: &mut CodePropertyGraph, parent: NodeId, child: NodeId) {
    cpg.connect(parent, child, CpgEdgeKind::AstChild)
        .expect("fixture AST endpoints exist");
}

fn child(cpg: &mut CodePropertyGraph, parent: NodeId, id: u32, kind: CpgNodeKind) -> NodeId {
    let child = add(cpg, id, kind);
    connect_ast(cpg, parent, child);
    child
}

fn parameter(name: &str) -> CpgNodeKind {
    CpgNodeKind::Parameter {
        name: name.into(),
        param_type: None,
        is_variadic: false,
    }
}

fn identifier(name: &str, definition: Option<NodeId>) -> CpgNodeKind {
    CpgNodeKind::Identifier {
        name: name.into(),
        definition,
    }
}

fn literal(value: i64) -> CpgNodeKind {
    CpgNodeKind::Literal {
        kind: LiteralKind::Integer(value),
    }
}

fn returned(cpg: &mut CodePropertyGraph, function: NodeId, id: u32, expression: NodeId) -> NodeId {
    let returned = child(cpg, function, id, CpgNodeKind::Return);
    connect_ast(cpg, returned, expression);
    returned
}

fn call(
    cpg: &mut CodePropertyGraph,
    function: NodeId,
    id: u32,
    target: Option<NodeId>,
    arguments: &[NodeId],
) -> NodeId {
    let call = child(
        cpg,
        function,
        id,
        CpgNodeKind::Call {
            target,
            is_method: false,
        },
    );
    let callee = add(cpg, id + 100_000, identifier("callee", target));
    connect_ast(cpg, call, callee);
    for &argument in arguments {
        connect_ast(cpg, call, argument);
    }
    call
}

#[test]
fn pure_leaf_and_scalar_return_are_exact() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = add(&mut cpg, 10, function_kind("answer"));
    let value = add(&mut cpg, 12, literal(42));
    returned(&mut cpg, function, 11, value);

    let analysis = analyze_effects(&cpg, &call_graph_sccs(&cpg), &IfdsCaps::default());
    let summary = &analysis.summaries[&function];
    assert!(analysis.is_complete());
    assert_eq!(summary.purity, Purity::Pure);
    assert!(summary.reads.is_empty());
    assert!(summary.writes.is_empty());
    assert_eq!(summary.const_return, Some(ConstValue::Int(42)));
    assert!(!summary.const_return_overdefined);
}

#[test]
fn globals_fields_and_parameter_writes_are_classified_from_cpg_evidence() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let global = add(
        &mut cpg,
        1,
        CpgNodeKind::Variable {
            name: "GLOBAL".into(),
            var_type: None,
            scope: ScopeId::new(0),
            is_mutable: true,
        },
    );
    let function = add(&mut cpg, 10, function_kind("effects"));
    let param = child(&mut cpg, function, 11, parameter("p"));
    let global_read = child(&mut cpg, function, 12, identifier("GLOBAL", Some(global)));
    let assignment = child(
        &mut cpg,
        function,
        13,
        CpgNodeKind::Assignment {
            operator: "=".into(),
        },
    );
    let global_write = child(&mut cpg, assignment, 14, identifier("GLOBAL", Some(global)));
    let rhs = child(&mut cpg, assignment, 15, literal(1));
    let param_assignment = child(
        &mut cpg,
        function,
        16,
        CpgNodeKind::Assignment {
            operator: "=".into(),
        },
    );
    child(&mut cpg, param_assignment, 17, identifier("p", Some(param)));
    child(&mut cpg, param_assignment, 18, literal(2));
    let member = child(
        &mut cpg,
        function,
        19,
        CpgNodeKind::MemberAccess {
            member: "field".into(),
        },
    );
    cpg.connect(
        global_read,
        member,
        CpgEdgeKind::DataFlow(DfgEdgeKind::FieldRead),
    )
    .expect("field read endpoints exist");
    cpg.connect(
        assignment,
        member,
        CpgEdgeKind::DataFlow(DfgEdgeKind::FieldWrite),
    )
    .expect("field write endpoints exist");
    let value = add(&mut cpg, 21, literal(0));
    returned(&mut cpg, function, 20, value);

    let read_only = add(&mut cpg, 30, function_kind("read_only"));
    child(&mut cpg, read_only, 31, identifier("GLOBAL", Some(global)));
    let read_only_value = add(&mut cpg, 33, literal(0));
    returned(&mut cpg, read_only, 32, read_only_value);

    let compound = add(&mut cpg, 40, function_kind("compound"));
    let compound_assignment = child(
        &mut cpg,
        compound,
        41,
        CpgNodeKind::Assignment {
            operator: "+=".into(),
        },
    );
    child(
        &mut cpg,
        compound_assignment,
        42,
        identifier("GLOBAL", Some(global)),
    );
    child(&mut cpg, compound_assignment, 43, literal(1));
    let compound_value = add(&mut cpg, 45, literal(0));
    returned(&mut cpg, compound, 44, compound_value);

    let summaries = effect_summaries(&cpg, &call_graph_sccs(&cpg), &IfdsCaps::default());
    let summary = &summaries[&function];
    assert_eq!(summary.purity, Purity::WritesGlobal);
    assert!(summary.reads.contains(&Place::Global("GLOBAL".into())));
    assert!(summary.writes.contains(&Place::Global("GLOBAL".into())));
    assert!(summary.reads.contains(&Place::Field("field".into())));
    assert!(summary.writes.contains(&Place::Field("field".into())));
    assert!(summary.writes.contains(&Place::Param(0)));
    assert_eq!(summaries[&read_only].purity, Purity::ReadsGlobal);
    assert!(summaries[&read_only].writes.is_empty());
    assert_eq!(summaries[&compound].purity, Purity::WritesGlobal);
    assert!(summaries[&compound]
        .reads
        .contains(&Place::Global("GLOBAL".into())));
    assert!(summaries[&compound]
        .writes
        .contains(&Place::Global("GLOBAL".into())));
    assert_eq!(cpg.ast_parent(global_write), Some(assignment));
    assert_eq!(cpg.ast_parent(rhs), Some(assignment));
}

#[test]
fn impure_callees_and_unresolved_calls_propagate_transitively() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let caller = add(&mut cpg, 10, function_kind("caller"));
    let impure = add(&mut cpg, 20, function_kind("impure"));
    child(&mut cpg, impure, 21, CpgNodeKind::Throw);
    call(&mut cpg, caller, 30, Some(impure), &[]);
    call(&mut cpg, caller, 40, None, &[]);

    let analysis = analyze_effects(&cpg, &call_graph_sccs(&cpg), &IfdsCaps::default());
    assert!(analysis.is_complete());
    assert_eq!(analysis.summaries[&impure].purity, Purity::Impure);
    assert_eq!(analysis.summaries[&caller].purity, Purity::Impure);
    assert_eq!(analysis.processed_functions[0], impure);
}

#[test]
fn callee_parameter_effects_are_mapped_to_caller_places() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let global = add(
        &mut cpg,
        1,
        CpgNodeKind::Variable {
            name: "STATE".into(),
            var_type: None,
            scope: ScopeId::new(0),
            is_mutable: true,
        },
    );
    let caller = add(&mut cpg, 10, function_kind("caller"));
    let callee = add(&mut cpg, 20, function_kind("mutate"));
    let param = child(&mut cpg, callee, 21, parameter("p"));
    let assignment = child(
        &mut cpg,
        callee,
        22,
        CpgNodeKind::Assignment {
            operator: "=".into(),
        },
    );
    child(&mut cpg, assignment, 23, identifier("p", Some(param)));
    child(&mut cpg, assignment, 24, literal(1));
    let argument = add(&mut cpg, 31, identifier("STATE", Some(global)));
    call(&mut cpg, caller, 30, Some(callee), &[argument]);

    let summaries = effect_summaries(&cpg, &call_graph_sccs(&cpg), &IfdsCaps::default());
    assert!(summaries[&callee].writes.contains(&Place::Param(0)));
    assert_eq!(summaries[&callee].purity, Purity::Pure);
    assert!(summaries[&caller]
        .writes
        .contains(&Place::Global("STATE".into())));
    assert_eq!(summaries[&caller].purity, Purity::WritesGlobal);
}

#[test]
fn constants_and_parameter_flow_compose_through_calls() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let constant = add(&mut cpg, 10, function_kind("constant"));
    let binary = add(
        &mut cpg,
        12,
        CpgNodeKind::BinaryOp {
            operator: "+".into(),
        },
    );
    let two = child(&mut cpg, binary, 13, literal(2));
    let three = child(&mut cpg, binary, 14, literal(3));
    returned(&mut cpg, constant, 11, binary);

    let identity = add(&mut cpg, 20, function_kind("identity"));
    let param = child(&mut cpg, identity, 21, parameter("value"));
    let use_site = add(&mut cpg, 23, identifier("value", Some(param)));
    returned(&mut cpg, identity, 22, use_site);

    let wrapper = add(&mut cpg, 30, function_kind("wrapper"));
    let wrapper_param = child(&mut cpg, wrapper, 31, parameter("input"));
    let argument = add(&mut cpg, 34, identifier("input", Some(wrapper_param)));
    let identity_call = call(&mut cpg, wrapper, 33, Some(identity), &[argument]);
    returned(&mut cpg, wrapper, 32, identity_call);

    let constant_wrapper = add(&mut cpg, 40, function_kind("constant_wrapper"));
    let constant_call = call(&mut cpg, constant_wrapper, 42, Some(constant), &[]);
    returned(&mut cpg, constant_wrapper, 41, constant_call);

    let summaries = effect_summaries(&cpg, &call_graph_sccs(&cpg), &IfdsCaps::default());
    assert_eq!(summaries[&constant].const_return, Some(ConstValue::Int(5)));
    assert_eq!(summaries[&identity].param_to_return, 1);
    assert!(summaries[&identity].const_return_overdefined);
    assert_eq!(summaries[&wrapper].param_to_return, 1);
    assert_eq!(
        summaries[&constant_wrapper].const_return,
        Some(ConstValue::Int(5))
    );
    assert_eq!(cpg.ast_parent(two), Some(binary));
    assert_eq!(cpg.ast_parent(three), Some(binary));
}

#[test]
fn disagreeing_return_constants_become_permanently_overdefined() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = add(&mut cpg, 10, function_kind("branch"));
    let one = add(&mut cpg, 12, literal(1));
    returned(&mut cpg, function, 11, one);
    let two = add(&mut cpg, 14, literal(2));
    returned(&mut cpg, function, 13, two);

    let summary = &effect_summaries(&cpg, &call_graph_sccs(&cpg), &IfdsCaps::default())[&function];
    assert_eq!(summary.const_return, None);
    assert!(summary.const_return_overdefined);

    let mut joined = EffectSummary::default();
    let first = EffectSummary {
        const_return: Some(ConstValue::Int(1)),
        ..EffectSummary::default()
    };
    let second = EffectSummary {
        const_return: Some(ConstValue::Int(2)),
        ..EffectSummary::default()
    };
    joined.join(&first);
    joined.join(&second);
    joined.join(&first);
    assert!(joined.const_return_overdefined);
    assert_eq!(joined.const_return, None);
}

#[test]
fn mutual_recursion_converges_and_preserves_direct_effects() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let left = add(&mut cpg, 10, function_kind("left"));
    let right = add(&mut cpg, 20, function_kind("right"));
    call(&mut cpg, left, 30, Some(right), &[]);
    call(&mut cpg, right, 40, Some(left), &[]);
    let left_value = add(&mut cpg, 51, literal(7));
    returned(&mut cpg, left, 50, left_value);
    let right_value = add(&mut cpg, 61, literal(7));
    returned(&mut cpg, right, 60, right_value);

    let analysis = analyze_effects(&cpg, &call_graph_sccs(&cpg), &IfdsCaps::default());
    assert!(analysis.is_complete());
    assert_eq!(
        analysis.summaries[&left].const_return,
        Some(ConstValue::Int(7))
    );
    assert_eq!(
        analysis.summaries[&right].const_return,
        Some(ConstValue::Int(7))
    );
    assert!(analysis
        .component_iterations
        .values()
        .all(|&iterations| iterations == 2));
}

#[test]
fn shared_and_domain_caps_remain_explicit() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = add(&mut cpg, 10, function_kind("deep"));
    let unary = add(
        &mut cpg,
        12,
        CpgNodeKind::UnaryOp {
            operator: "-".into(),
        },
    );
    child(&mut cpg, unary, 13, literal(1));
    returned(&mut cpg, function, 11, unary);
    let call_graph = call_graph_sccs(&cpg);

    let no_work = IfdsCaps {
        max_worklist_steps: 0,
        ..IfdsCaps::default()
    };
    let shared = analyze_effects(&cpg, &call_graph, &no_work);
    assert!(shared.capped);
    assert!(shared.summaries.is_empty());
    assert!(shared
        .cap_events
        .contains(&IfdsCapEvent::WorklistSteps { needed: 1, cap: 0 }));

    let no_depth = IfdsCaps {
        max_call_depth: 0,
        ..IfdsCaps::default()
    };
    let domain = analyze_effects(&cpg, &call_graph, &no_depth);
    assert!(domain.capped);
    assert_eq!(domain.domain_capped_functions, BTreeSet::from([function]));
    assert!(domain.summaries[&function].capped);
    assert!(domain.summaries[&function].const_return_overdefined);
}

#[test]
fn malformed_call_graph_is_rejected_without_mutating_the_cpg() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = add(&mut cpg, 10, function_kind("f"));
    let before = (cpg.node_count(), cpg.edge_count());
    let mut malformed = call_graph_sccs(&cpg);
    malformed.projection = SccProjection::ControlFlow { function };

    let analysis = analyze_effects(&cpg, &malformed, &IfdsCaps::default());
    assert!(analysis.issues.contains(&IfdsIssue::NotCallGraph));
    assert!(analysis.summaries.is_empty());
    assert_eq!((cpg.node_count(), cpg.edge_count()), before);
}

#[test]
fn parameter_masks_surface_their_fixed_width_boundary() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = add(&mut cpg, 10, function_kind("wide"));
    for index in 0..65u32 {
        child(
            &mut cpg,
            function,
            100 + index,
            parameter(&format!("p{index}")),
        );
    }
    let value = add(&mut cpg, 1000, literal(0));
    returned(&mut cpg, function, 999, value);
    let analysis = analyze_effects(&cpg, &call_graph_sccs(&cpg), &IfdsCaps::default());
    let summary = &analysis.summaries[&function];
    assert!(summary.param_mask_truncated);
    assert!(summary.capped);
    assert!(analysis.capped);
    assert!(!analysis.is_complete());
    assert_eq!(analysis.domain_capped_functions, BTreeSet::from([function]));
}

#[cfg(feature = "serde")]
#[test]
fn analysis_round_trips_with_ordered_effect_evidence() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = add(&mut cpg, 10, function_kind("answer"));
    let value = add(&mut cpg, 12, literal(42));
    returned(&mut cpg, function, 11, value);
    let analysis = analyze_effects(&cpg, &call_graph_sccs(&cpg), &IfdsCaps::default());
    let json = serde_json::to_string(&analysis).expect("effect analysis serializes");
    let decoded: EffectAnalysis =
        serde_json::from_str(&json).expect("effect analysis deserializes");
    assert_eq!(decoded, analysis);
}

#[allow(clippy::too_many_arguments)]
fn recursive_parameter_dependencies(
    cpg: &CodePropertyGraph,
    node: NodeId,
    local: &LocalEffects,
    ctx: &SummaryCtx<'_, EffectDomain>,
    depth: u32,
    seen: &mut BTreeSet<NodeId>,
    budget: &mut TraversalBudget,
) -> u64 {
    if !budget.enter(depth, ctx.caps) {
        return 0;
    }
    if !seen.insert(node) {
        budget.capped = true;
        return u64::MAX;
    }
    let mut mask = parameter_mask_for_node(cpg, node, local);
    if matches!(
        cpg.node(node).map(|node| &node.kind),
        Some(CpgNodeKind::Call { .. })
    ) {
        let arguments = call_arguments(cpg, node);
        match (ctx.resolve)(node) {
            CalleeRef::Resolved(callee) => {
                if let Some(callee_summary) = ctx.summaries.get(&callee) {
                    for (index, argument) in arguments.into_iter().enumerate() {
                        if callee_summary.param_mask_truncated {
                            budget.capped = true;
                        }
                        let represented =
                            index < 64 && callee_summary.param_to_return & (1u64 << index) != 0;
                        if represented || callee_summary.param_mask_truncated {
                            mask |= recursive_parameter_dependencies(
                                cpg,
                                argument,
                                local,
                                ctx,
                                depth.saturating_add(1),
                                seen,
                                budget,
                            );
                        }
                    }
                }
            }
            CalleeRef::External(_) | CalleeRef::Unresolved(_) => {
                for argument in arguments {
                    mask |= recursive_parameter_dependencies(
                        cpg,
                        argument,
                        local,
                        ctx,
                        depth.saturating_add(1),
                        seen,
                        budget,
                    );
                }
            }
        }
    } else {
        for &child in cpg.ast_children(node) {
            mask |= recursive_parameter_dependencies(
                cpg,
                child,
                local,
                ctx,
                depth.saturating_add(1),
                seen,
                budget,
            );
        }
        for definition in cpg.reaching_definitions(node) {
            mask |= recursive_parameter_dependencies(
                cpg,
                definition,
                local,
                ctx,
                depth.saturating_add(1),
                seen,
                budget,
            );
        }
    }
    seen.remove(&node);
    mask
}

#[allow(clippy::too_many_arguments)]
fn recursive_evaluate_constant(
    cpg: &CodePropertyGraph,
    node: NodeId,
    ctx: &SummaryCtx<'_, EffectDomain>,
    depth: u32,
    on_path: &mut BTreeSet<NodeId>,
    budget: &mut TraversalBudget,
) -> ConstLattice {
    if !budget.enter(depth, ctx.caps) {
        return ConstLattice::Overdefined;
    }
    if !on_path.insert(node) {
        return ConstLattice::Overdefined;
    }
    let value =
        match cpg.node(node).map(|node| &node.kind) {
            Some(CpgNodeKind::Literal { kind }) => literal_value(kind),
            Some(CpgNodeKind::Call { .. }) => match (ctx.resolve)(node) {
                CalleeRef::Resolved(callee) => ctx
                    .summaries
                    .get(&callee)
                    .map_or(ConstLattice::Overdefined, const_lattice),
                CalleeRef::External(_) | CalleeRef::Unresolved(_) => ConstLattice::Overdefined,
            },
            Some(CpgNodeKind::Identifier { .. }) => {
                let mut definitions = cpg.reaching_definitions(node);
                if let Some(CpgNodeKind::Identifier {
                    definition: Some(definition),
                    ..
                }) = cpg.node(node).map(|node| &node.kind)
                {
                    definitions.push(*definition);
                }
                definitions.sort_unstable();
                definitions.dedup();
                if definitions.is_empty() {
                    ConstLattice::Overdefined
                } else {
                    let mut result = ConstLattice::Unknown;
                    for definition in definitions {
                        let value = recursive_evaluate_constant(
                            cpg,
                            definition,
                            ctx,
                            depth.saturating_add(1),
                            on_path,
                            budget,
                        );
                        DataflowLattice::join(&mut result, &value);
                    }
                    result
                }
            }
            Some(CpgNodeKind::BinaryOp { operator }) => {
                let children = cpg.ast_children(node);
                match children {
                    [left, right, ..] => {
                        let left = recursive_evaluate_constant(
                            cpg,
                            *left,
                            ctx,
                            depth.saturating_add(1),
                            on_path,
                            budget,
                        );
                        let right = recursive_evaluate_constant(
                            cpg,
                            *right,
                            ctx,
                            depth.saturating_add(1),
                            on_path,
                            budget,
                        );
                        fold_binary(operator, &left, &right)
                    }
                    _ => ConstLattice::Overdefined,
                }
            }
            Some(CpgNodeKind::UnaryOp { operator }) => cpg
                .ast_children(node)
                .first()
                .copied()
                .map_or(ConstLattice::Overdefined, |operand| {
                    let operand = recursive_evaluate_constant(
                        cpg,
                        operand,
                        ctx,
                        depth.saturating_add(1),
                        on_path,
                        budget,
                    );
                    fold_unary(operator, &operand)
                }),
            Some(CpgNodeKind::Assignment { .. }) => {
                cpg.ast_children(node)
                    .get(1)
                    .copied()
                    .map_or(ConstLattice::Overdefined, |rhs| {
                        recursive_evaluate_constant(
                            cpg,
                            rhs,
                            ctx,
                            depth.saturating_add(1),
                            on_path,
                            budget,
                        )
                    })
            }
            Some(CpgNodeKind::Variable { name, .. }) => cpg
                .ast_children(node)
                .iter()
                .copied()
                .rev()
                .find(|&child| {
                    !matches!(
                        cpg.node(child).map(|node| &node.kind),
                        Some(CpgNodeKind::Identifier { name: child_name, .. }) if child_name == name
                    )
                })
                .map_or(ConstLattice::Overdefined, |initializer| {
                    recursive_evaluate_constant(
                        cpg,
                        initializer,
                        ctx,
                        depth.saturating_add(1),
                        on_path,
                        budget,
                    )
                }),
            Some(CpgNodeKind::Return) => cpg.ast_children(node).first().copied().map_or(
                ConstLattice::Overdefined,
                |returned| {
                    recursive_evaluate_constant(
                        cpg,
                        returned,
                        ctx,
                        depth.saturating_add(1),
                        on_path,
                        budget,
                    )
                },
            ),
            Some(CpgNodeKind::Parameter { .. }) | None | Some(_) => ConstLattice::Overdefined,
        };
    on_path.remove(&node);
    value
}

fn shallow_machine_fixture(
    steps: &[u8],
    parameter_leaf: bool,
) -> (CodePropertyGraph, NodeId, NodeId) {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = add(&mut cpg, 10, function_kind("generated"));
    let param = child(&mut cpg, function, 11, parameter("p"));
    let mut next = 20u32;
    let mut root = if parameter_leaf {
        let leaf = add(&mut cpg, next, identifier("p", Some(param)));
        next += 1;
        leaf
    } else {
        let leaf = add(&mut cpg, next, literal(1));
        next += 1;
        leaf
    };
    for &step in steps {
        if step % 3 == 0 {
            let parent = add(
                &mut cpg,
                next,
                CpgNodeKind::UnaryOp {
                    operator: "-".into(),
                },
            );
            next += 1;
            connect_ast(&mut cpg, parent, root);
            root = parent;
        } else {
            let parent = add(
                &mut cpg,
                next,
                CpgNodeKind::BinaryOp {
                    operator: "+".into(),
                },
            );
            next += 1;
            let right = add(&mut cpg, next, literal(i64::from(step % 7)));
            next += 1;
            connect_ast(&mut cpg, parent, root);
            connect_ast(&mut cpg, parent, right);
            root = parent;
        }
    }
    returned(&mut cpg, function, next, root);
    (cpg, function, root)
}

fn reaching_join_fixture(values: &[i64], reverse_edges: bool) -> (CodePropertyGraph, NodeId) {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = add(&mut cpg, 10, function_kind("join"));
    let root = add(&mut cpg, 11, identifier("joined", None));
    returned(&mut cpg, function, 12, root);
    let definitions: Vec<_> = values
        .iter()
        .enumerate()
        .map(|(index, &value)| add(&mut cpg, 100 + index as u32, literal(value)))
        .collect();
    let order: Box<dyn Iterator<Item = &NodeId>> = if reverse_edges {
        Box::new(definitions.iter().rev())
    } else {
        Box::new(definitions.iter())
    };
    for &definition in order {
        cpg.connect(
            definition,
            root,
            CpgEdgeKind::DataFlow(DfgEdgeKind::ReachingDef),
        )
        .expect("reaching-definition endpoints exist");
    }
    (cpg, function)
}

#[test]
fn terminal_cache_and_uncached_machines_have_identical_semantics_and_accounting() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = add(&mut cpg, 10, function_kind("shared"));
    let root = add(
        &mut cpg,
        11,
        CpgNodeKind::BinaryOp {
            operator: "+".into(),
        },
    );
    let shared = add(&mut cpg, 12, literal(1));
    connect_ast(&mut cpg, root, shared);
    connect_ast(&mut cpg, root, shared);
    returned(&mut cpg, function, 13, root);

    let local = local_effects(&cpg, function);
    let summaries = BTreeMap::new();
    let resolve = |_| CalleeRef::Unresolved("fixture".into());
    let caps = IfdsCaps::default();
    let ctx = SummaryCtx {
        summaries: &summaries,
        resolve: &resolve,
        caps: &caps,
    };
    let mut cached_budget = TraversalBudget::default();
    let mut cached_stats = EvaluationMachineStats::default();
    let cached = evaluate_constant_machine(
        &cpg,
        root,
        &ctx,
        &mut cached_budget,
        true,
        &mut cached_stats,
    );
    let mut plain_budget = TraversalBudget::default();
    let mut plain_stats = EvaluationMachineStats::default();
    let plain =
        evaluate_constant_machine(&cpg, root, &ctx, &mut plain_budget, false, &mut plain_stats);
    assert_eq!(cached, ConstLattice::Constant(ConstValue::Int(2)));
    assert_eq!(cached, plain);
    assert_eq!(cached_budget.steps, plain_budget.steps);
    assert_eq!(cached_budget.capped, plain_budget.capped);
    assert_eq!(cached_stats.cache_hits, 1);
    assert_eq!(plain_stats.cache_hits, 0);

    let mut cached_budget = TraversalBudget::default();
    let mut cached_stats = EvaluationMachineStats::default();
    let cached = parameter_dependencies_machine(
        &cpg,
        root,
        &local,
        &ctx,
        &mut cached_budget,
        true,
        &mut cached_stats,
    );
    let mut plain_budget = TraversalBudget::default();
    let mut plain_stats = EvaluationMachineStats::default();
    let plain = parameter_dependencies_machine(
        &cpg,
        root,
        &local,
        &ctx,
        &mut plain_budget,
        false,
        &mut plain_stats,
    );
    assert_eq!(cached, 0);
    assert_eq!(cached, plain);
    assert_eq!(cached_budget.steps, plain_budget.steps);
    assert_eq!(cached_budget.capped, plain_budget.capped);
    assert_eq!(cached_stats.cache_hits, 1);
    assert_eq!(plain_stats.cache_hits, 0);
}

#[test]
fn cycles_missing_nodes_and_exact_caps_are_conservative_and_monotone() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = add(&mut cpg, 10, function_kind("cycle"));
    let cyclic = add(
        &mut cpg,
        11,
        CpgNodeKind::UnaryOp {
            operator: "-".into(),
        },
    );
    connect_ast(&mut cpg, cyclic, cyclic);
    returned(&mut cpg, function, 12, cyclic);
    let local = local_effects(&cpg, function);
    let summaries = BTreeMap::new();
    let resolve = |_| CalleeRef::Unresolved("fixture".into());
    let generous = IfdsCaps::default();
    let ctx = SummaryCtx {
        summaries: &summaries,
        resolve: &resolve,
        caps: &generous,
    };
    let mut dependency_budget = TraversalBudget::default();
    assert_eq!(
        parameter_dependencies(&cpg, cyclic, &local, &ctx, &mut dependency_budget),
        u64::MAX
    );
    assert!(dependency_budget.capped);
    let mut constant_budget = TraversalBudget::default();
    assert_eq!(
        evaluate_constant(&cpg, cyclic, &ctx, &mut constant_budget),
        ConstLattice::Overdefined
    );
    assert!(!constant_budget.capped);

    let missing = NodeId::new(u32::MAX);
    let mut missing_budget = TraversalBudget::default();
    assert_eq!(
        parameter_dependencies(&cpg, missing, &local, &ctx, &mut missing_budget),
        0
    );
    assert!(!missing_budget.capped);
    assert_eq!(
        evaluate_constant(&cpg, missing, &ctx, &mut missing_budget),
        ConstLattice::Overdefined
    );

    let mut shallow = CodePropertyGraph::new(Language::Rust);
    let shallow_function = add(&mut shallow, 20, function_kind("caps"));
    let unary = add(
        &mut shallow,
        21,
        CpgNodeKind::UnaryOp {
            operator: "-".into(),
        },
    );
    let one = add(&mut shallow, 22, literal(1));
    connect_ast(&mut shallow, unary, one);
    returned(&mut shallow, shallow_function, 23, unary);
    let shallow_local = local_effects(&shallow, shallow_function);

    for (caps, expected_capped) in [
        (
            IfdsCaps {
                max_call_depth: 1,
                max_worklist_steps: 2,
                ..IfdsCaps::default()
            },
            false,
        ),
        (
            IfdsCaps {
                max_call_depth: 0,
                max_worklist_steps: 2,
                ..IfdsCaps::default()
            },
            true,
        ),
        (
            IfdsCaps {
                max_call_depth: 1,
                max_worklist_steps: 1,
                ..IfdsCaps::default()
            },
            true,
        ),
    ] {
        let ctx = SummaryCtx {
            summaries: &summaries,
            resolve: &resolve,
            caps: &caps,
        };
        let mut dependency_budget = TraversalBudget::default();
        let dependency = parameter_dependencies(
            &shallow,
            unary,
            &shallow_local,
            &ctx,
            &mut dependency_budget,
        );
        let mut constant_budget = TraversalBudget::default();
        let constant = evaluate_constant(&shallow, unary, &ctx, &mut constant_budget);
        assert_eq!(dependency_budget.capped, expected_capped);
        assert_eq!(constant_budget.capped, expected_capped);
        if expected_capped {
            assert_eq!(constant, ConstLattice::Overdefined);
        } else {
            assert_eq!(dependency, 0);
            assert_eq!(constant, ConstLattice::Constant(ConstValue::Int(-1)));
        }
    }
}

#[test]
fn transitive_reaching_definitions_preserve_parameters_and_constants() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);

    let parameter_function = add(&mut cpg, 10, function_kind("parameter_alias"));
    let parameter = child(&mut cpg, parameter_function, 11, parameter("p"));
    let intermediate = add(&mut cpg, 12, identifier("p", Some(parameter)));
    let parameter_result = add(&mut cpg, 13, identifier("alias", None));
    cpg.connect(
        intermediate,
        parameter_result,
        CpgEdgeKind::DataFlow(DfgEdgeKind::ReachingDef),
    )
    .expect("transitive parameter definition endpoints exist");
    returned(&mut cpg, parameter_function, 14, parameter_result);

    let constant_function = add(&mut cpg, 20, function_kind("constant_alias"));
    let variable = child(
        &mut cpg,
        constant_function,
        21,
        CpgNodeKind::Variable {
            name: "value".into(),
            var_type: None,
            scope: ScopeId::new(1),
            is_mutable: false,
        },
    );
    child(&mut cpg, variable, 22, identifier("value", Some(variable)));
    child(&mut cpg, variable, 23, literal(9));
    let constant_result = add(&mut cpg, 24, identifier("value", None));
    cpg.connect(
        variable,
        constant_result,
        CpgEdgeKind::DataFlow(DfgEdgeKind::ReachingDef),
    )
    .expect("transitive constant definition endpoints exist");
    returned(&mut cpg, constant_function, 25, constant_result);

    let analysis = analyze_effects(&cpg, &call_graph_sccs(&cpg), &IfdsCaps::default());
    assert!(analysis.is_complete());
    assert_eq!(analysis.summaries[&parameter_function].param_to_return, 1);
    assert_eq!(
        analysis.summaries[&constant_function].const_return,
        Some(ConstValue::Int(9))
    );
}

#[test]
fn wide_reaching_definition_join_is_stack_safe_and_deterministic() {
    const WIDTH: usize = 10_000;
    let values = vec![7; WIDTH];
    let (forward, function) = reaching_join_fixture(&values, false);
    let (reverse, reverse_function) = reaching_join_fixture(&values, true);
    let caps = IfdsCaps {
        max_call_depth: 1,
        max_worklist_steps: WIDTH * 3,
        ..IfdsCaps::default()
    };
    let forward = analyze_effects(&forward, &call_graph_sccs(&forward), &caps);
    let reverse = analyze_effects(&reverse, &call_graph_sccs(&reverse), &caps);
    assert!(forward.is_complete());
    assert!(reverse.is_complete());
    assert_eq!(forward.summaries, reverse.summaries);
    assert_eq!(
        forward.summaries[&function].const_return,
        Some(ConstValue::Int(7))
    );
    assert_eq!(function, reverse_function);
}

fn summary_from_bits(reads: u8, writes: u8, mask: u8, constant: u8) -> EffectSummary {
    let places = [
        Place::Global("g".into()),
        Place::Param(0),
        Place::Field("f".into()),
    ];
    let mut summary = EffectSummary {
        purity: match (reads >> 3) & 0b11 {
            0 => Purity::Pure,
            1 => Purity::ReadsGlobal,
            2 => Purity::WritesGlobal,
            _ => Purity::Impure,
        },
        param_to_return: u64::from(mask),
        param_mask_truncated: reads & 0x20 != 0,
        capped: writes & 0x20 != 0,
        ..EffectSummary::default()
    };
    for (index, place) in places.into_iter().enumerate() {
        if reads & (1 << index) != 0 {
            summary.reads.insert(place.clone());
        }
        if writes & (1 << index) != 0 {
            summary.writes.insert(place);
        }
    }
    match constant % 4 {
        0 => {}
        1 => summary.const_return = Some(ConstValue::Int(1)),
        2 => summary.const_return = Some(ConstValue::Int(2)),
        _ => summary.const_return_overdefined = true,
    }
    summary
}

fn purity_fixture(
    function_count: usize,
    edges: &[(usize, usize)],
    impure: &[bool],
) -> CodePropertyGraph {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let functions: Vec<_> = (0..function_count)
        .map(|index| {
            add(
                &mut cpg,
                10 + index as u32,
                function_kind(&format!("f{index}")),
            )
        })
        .collect();
    for (index, &is_impure) in impure.iter().take(function_count).enumerate() {
        if is_impure {
            child(
                &mut cpg,
                functions[index],
                500 + index as u32,
                CpgNodeKind::Throw,
            );
        }
    }
    for &(caller, callee) in edges {
        let id = 1_000 + (caller * function_count + callee) as u32 * 2;
        call(
            &mut cpg,
            functions[caller],
            id,
            Some(functions[callee]),
            &[],
        );
    }
    cpg
}

fn brute_purity(function_count: usize, edges: &[(usize, usize)], impure: &[bool]) -> Vec<Purity> {
    let mut successors = vec![BTreeSet::new(); function_count];
    for &(source, target) in edges {
        successors[source].insert(target);
    }
    (0..function_count)
        .map(|start| {
            let mut stack = vec![start];
            let mut seen = BTreeSet::new();
            while let Some(node) = stack.pop() {
                if seen.insert(node) {
                    stack.extend(successors[node].iter().copied());
                }
            }
            if seen.into_iter().any(|node| impure[node]) {
                Purity::Impure
            } else {
                Purity::Pure
            }
        })
        .collect()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn effect_summary_is_a_join_semilattice(
        a in any::<(u8,u8,u8,u8)>(),
        b in any::<(u8,u8,u8,u8)>(),
        c in any::<(u8,u8,u8,u8)>(),
    ) {
        let left = summary_from_bits(a.0, a.1, a.2, a.3);
        let middle = summary_from_bits(b.0, b.1, b.2, b.3);
        let right = summary_from_bits(c.0, c.1, c.2, c.3);

        let mut lm = left.clone();
        lm.join(&middle);
        let mut ml = middle.clone();
        ml.join(&left);
        prop_assert_eq!(&lm, &ml);

        let mut associated_left = lm;
        associated_left.join(&right);
        let mut mr = middle;
        mr.join(&right);
        let mut associated_right = left.clone();
        associated_right.join(&mr);
        prop_assert_eq!(&associated_left, &associated_right);

        let mut idempotent = left.clone();
        prop_assert!(!idempotent.join(&left));
        prop_assert_eq!(&idempotent, &left);
        prop_assert!(associated_left.subsumes(&left));
    }

    #[test]
    fn flat_machines_match_the_bounded_recursive_oracles(
        steps in prop::collection::vec(any::<u8>(), 0..32),
        parameter_leaf in any::<bool>(),
    ) {
        let (cpg, function, root) = shallow_machine_fixture(&steps, parameter_leaf);
        let local = local_effects(&cpg, function);
        let summaries = BTreeMap::new();
        let resolve = |_| CalleeRef::Unresolved("generated".into());
        let caps = IfdsCaps::default();
        let ctx = SummaryCtx {
            summaries: &summaries,
            resolve: &resolve,
            caps: &caps,
        };

        let mut machine_budget = TraversalBudget::default();
        let machine_dependencies = parameter_dependencies(
            &cpg,
            root,
            &local,
            &ctx,
            &mut machine_budget,
        );
        let mut oracle_budget = TraversalBudget::default();
        let oracle_dependencies = recursive_parameter_dependencies(
            &cpg,
            root,
            &local,
            &ctx,
            0,
            &mut BTreeSet::new(),
            &mut oracle_budget,
        );
        prop_assert_eq!(machine_dependencies, oracle_dependencies);
        prop_assert_eq!(machine_budget.steps, oracle_budget.steps);
        prop_assert_eq!(machine_budget.capped, oracle_budget.capped);

        let mut machine_budget = TraversalBudget::default();
        let machine_constant = evaluate_constant(&cpg, root, &ctx, &mut machine_budget);
        let mut oracle_budget = TraversalBudget::default();
        let oracle_constant = recursive_evaluate_constant(
            &cpg,
            root,
            &ctx,
            0,
            &mut BTreeSet::new(),
            &mut oracle_budget,
        );
        prop_assert_eq!(machine_constant, oracle_constant);
        prop_assert_eq!(machine_budget.steps, oracle_budget.steps);
        prop_assert_eq!(machine_budget.capped, oracle_budget.capped);
    }

    #[test]
    fn raising_machine_caps_never_invalidates_a_complete_result(
        steps in prop::collection::vec(any::<u8>(), 0..24),
        parameter_leaf in any::<bool>(),
        low_depth in 0u32..28,
        extra_depth in 0u32..28,
        low_work in 0usize..64,
        extra_work in 0usize..64,
    ) {
        let (cpg, function, root) = shallow_machine_fixture(&steps, parameter_leaf);
        let local = local_effects(&cpg, function);
        let summaries = BTreeMap::new();
        let resolve = |_| CalleeRef::Unresolved("generated".into());
        let low_caps = IfdsCaps {
            max_call_depth: low_depth,
            max_worklist_steps: low_work,
            ..IfdsCaps::default()
        };
        let high_caps = IfdsCaps {
            max_call_depth: low_depth.saturating_add(extra_depth),
            max_worklist_steps: low_work.saturating_add(extra_work),
            ..IfdsCaps::default()
        };

        let low_ctx = SummaryCtx {
            summaries: &summaries,
            resolve: &resolve,
            caps: &low_caps,
        };
        let high_ctx = SummaryCtx {
            summaries: &summaries,
            resolve: &resolve,
            caps: &high_caps,
        };
        let mut low_dependency_budget = TraversalBudget::default();
        let low_dependencies = parameter_dependencies(
            &cpg,
            root,
            &local,
            &low_ctx,
            &mut low_dependency_budget,
        );
        let mut high_dependency_budget = TraversalBudget::default();
        let high_dependencies = parameter_dependencies(
            &cpg,
            root,
            &local,
            &high_ctx,
            &mut high_dependency_budget,
        );
        if !low_dependency_budget.capped {
            prop_assert!(!high_dependency_budget.capped);
            prop_assert_eq!(low_dependencies, high_dependencies);
        }
        if high_dependency_budget.capped {
            prop_assert!(low_dependency_budget.capped);
        }

        let mut low_constant_budget = TraversalBudget::default();
        let low_constant = evaluate_constant(&cpg, root, &low_ctx, &mut low_constant_budget);
        let mut high_constant_budget = TraversalBudget::default();
        let high_constant = evaluate_constant(&cpg, root, &high_ctx, &mut high_constant_budget);
        if !low_constant_budget.capped {
            prop_assert!(!high_constant_budget.capped);
            prop_assert_eq!(low_constant, high_constant);
        }
        if high_constant_budget.capped {
            prop_assert!(low_constant_budget.capped);
        }
    }

    #[test]
    fn reaching_definition_insertion_order_does_not_change_joins(
        values in prop::collection::vec(-8i64..=8, 1..24),
    ) {
        let (forward, _) = reaching_join_fixture(&values, false);
        let (reverse, _) = reaching_join_fixture(&values, true);
        let forward = analyze_effects(
            &forward,
            &call_graph_sccs(&forward),
            &IfdsCaps::default(),
        );
        let reverse = analyze_effects(
            &reverse,
            &call_graph_sccs(&reverse),
            &IfdsCaps::default(),
        );
        prop_assert!(forward.is_complete());
        prop_assert!(reverse.is_complete());
        prop_assert_eq!(forward, reverse);
    }

    #[test]
    fn summaries_match_transitive_purity_and_ignore_call_insertion_order(
        function_count in 1usize..=6,
        matrix in prop::collection::vec(any::<bool>(), 36),
        impure in prop::collection::vec(any::<bool>(), 6),
    ) {
        let edges: Vec<_> = (0..function_count)
            .flat_map(|source| (0..function_count).map(move |target| (source, target)))
            .filter(|&(source, target)| matrix[source * 6 + target])
            .collect();
        let cpg = purity_fixture(function_count, &edges, &impure);
        let mut reversed_edges = edges.clone();
        reversed_edges.reverse();
        let reversed = purity_fixture(function_count, &reversed_edges, &impure);
        let output = analyze_effects(&cpg, &call_graph_sccs(&cpg), &IfdsCaps::default());
        let permuted = analyze_effects(
            &reversed,
            &call_graph_sccs(&reversed),
            &IfdsCaps::default(),
        );
        prop_assert!(output.is_complete());
        prop_assert_eq!(&output, &permuted);

        for (index, expected) in brute_purity(function_count, &edges, &impure)
            .into_iter()
            .enumerate()
        {
            prop_assert_eq!(output.summaries[&NodeId::new(10 + index as u32)].purity, expected);
        }
    }

    #[test]
    fn raising_iteration_cap_never_invalidates_a_complete_effect_result(
        size in 1usize..=6,
        low_cap in 0u32..8,
        extra in 0u32..8,
    ) {
        let edges: Vec<_> = (0..size).map(|source| (source, (source + 1) % size)).collect();
        let impure = vec![false; size];
        let cpg = purity_fixture(size, &edges, &impure);
        let low_options = IfdsCaps {
            max_scc_iters: low_cap,
            ..IfdsCaps::default()
        };
        let mut high_options = low_options.clone();
        high_options.max_scc_iters = low_cap.saturating_add(extra);
        let low = analyze_effects(&cpg, &call_graph_sccs(&cpg), &low_options);
        let high = analyze_effects(&cpg, &call_graph_sccs(&cpg), &high_options);
        if low.is_complete() {
            prop_assert!(high.is_complete());
            prop_assert_eq!(low.summaries, high.summaries);
        }
        if high.capped {
            prop_assert!(low.capped);
        }
    }
}
