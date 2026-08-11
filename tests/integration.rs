//! End-to-end integration tests exercising libcpg through its PUBLIC API only,
//! as a downstream consumer would. Every section is feature-gated, so this file
//! compiles (to an empty binary if nothing is enabled) under any feature subset.
//!
//! Run the full set with: `cargo test --features "full rholang metta"`.

#![allow(clippy::items_after_test_module)]

mod scc_analysis {
    use libcpg::{
        call_graph_sccs, control_flow_sccs, CfgEdgeKind, CodePropertyGraph, CpgEdgeKind, CpgNode,
        CpgNodeKind, Language, MethodSignature, NodeId, SourceRange, Visibility,
    };

    fn node(kind: CpgNodeKind) -> CpgNode {
        CpgNode::new(NodeId::new(0), kind, SourceRange::default())
    }

    fn function(name: &str) -> CpgNodeKind {
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

    #[test]
    fn root_api_reports_cfg_cycles_and_recursive_call_clusters() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let first = cpg.add_node(node(function("first")));
        let second = cpg.add_node(node(function("second")));
        let first_call = cpg.add_node(node(CpgNodeKind::Call {
            target: Some(second),
            is_method: false,
        }));
        let second_call = cpg.add_node(node(CpgNodeKind::Call {
            target: Some(first),
            is_method: false,
        }));
        cpg.connect(first, first_call, CpgEdgeKind::AstChild);
        cpg.connect(second, second_call, CpgEdgeKind::AstChild);
        cpg.connect(
            first,
            first_call,
            CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
        );
        cpg.connect(
            first_call,
            first_call,
            CpgEdgeKind::ControlFlow(CfgEdgeKind::LoopBack),
        );

        let cfg = control_flow_sccs(&cpg, first).expect("valid function");
        assert!(cfg.component_of(first_call).expect("call component").cyclic);

        let calls = call_graph_sccs(&cpg);
        assert_eq!(
            calls.component_of(first).expect("recursive cluster").nodes,
            vec![first, second]
        );
    }
}

mod dominator_analysis {
    use libcpg::{dominator_tree, GraphProjection, NodeId, ProjectionKind};

    #[test]
    fn root_api_computes_dominators_without_language_features() {
        let root = NodeId::new(10);
        let left = NodeId::new(20);
        let right = NodeId::new(30);
        let join = NodeId::new(40);
        let projection = GraphProjection::from_edges(
            ProjectionKind::External,
            [root, left, right, join],
            [
                (root, left, 1.0),
                (root, right, 1.0),
                (left, join, 1.0),
                (right, join, 1.0),
            ],
        );

        let tree = dominator_tree(&projection, root).expect("valid projection root");
        assert_eq!(tree.immediate_dominator(join), Some(root));
        assert_eq!(tree.frontier[&left], vec![join]);
        assert_eq!(tree.frontier[&right], vec![join]);
        assert!(tree.dominates(root, join));
        assert_eq!(
            tree.dominators_of(join).collect::<Vec<_>>(),
            vec![join, root]
        );
    }
}

mod reachability_analysis {
    use libcpg::{
        unreachable_code, CfgEdgeKind, CodePropertyGraph, CpgEdgeKind, CpgNode, CpgNodeKind,
        Language, MethodSignature, NodeId, ScopeId, SourceRange, Visibility,
    };

    fn node(id: u32, kind: CpgNodeKind) -> CpgNode {
        CpgNode::new(NodeId::new(id), kind, SourceRange::default())
    }

    #[test]
    fn root_api_partitions_cfg_nodes_without_mutating_the_graph() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let function = cpg.add_node_with_id(node(
            10,
            CpgNodeKind::Function {
                signature: MethodSignature {
                    name: "f".into(),
                    params: Default::default(),
                    return_type: None,
                    is_static: false,
                    is_async: false,
                    visibility: Visibility::Private,
                },
            },
        ));
        let live = cpg.add_node_with_id(node(
            20,
            CpgNodeKind::Block {
                scope: ScopeId::GLOBAL,
            },
        ));
        let dead = cpg.add_node_with_id(node(30, CpgNodeKind::If));
        let dead_target = cpg.add_node_with_id(node(40, CpgNodeKind::Return));
        for child in [live, dead, dead_target] {
            cpg.connect(function, child, CpgEdgeKind::AstChild);
        }
        cpg.connect(
            function,
            live,
            CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
        );
        cpg.connect(
            dead,
            dead_target,
            CpgEdgeKind::ControlFlow(CfgEdgeKind::ConditionalTrue),
        );

        let before = (cpg.node_count(), cpg.edge_count());
        let report = unreachable_code(&cpg, function).expect("function reachability");
        assert_eq!(report.reachable, vec![function, live]);
        assert_eq!(report.unreachable, vec![dead, dead_target]);
        assert_eq!(report.dead_branches.len(), 1);
        assert_eq!((cpg.node_count(), cpg.edge_count()), before);
    }
}

mod basic_block_analysis {
    use libcpg::{
        block_cfg, CfgEdgeKind, CodePropertyGraph, CpgEdgeKind, CpgNode, CpgNodeKind, Language,
        MethodSignature, NodeId, SourceRange, Visibility,
    };

    fn node(id: u32, kind: CpgNodeKind) -> CpgNode {
        CpgNode::new(NodeId::new(id), kind, SourceRange::default())
    }

    #[test]
    fn root_api_preserves_ids_and_projects_typed_block_edges() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let function = cpg.add_node_with_id(node(
            10,
            CpgNodeKind::Function {
                signature: MethodSignature {
                    name: "blocks".into(),
                    params: Default::default(),
                    return_type: None,
                    is_static: false,
                    is_async: false,
                    visibility: Visibility::Private,
                },
            },
        ));
        let guard = cpg.add_node_with_id(node(20, CpgNodeKind::If));
        let yes = cpg.add_node_with_id(node(30, CpgNodeKind::Return));
        let no = cpg.add_node_with_id(node(40, CpgNodeKind::Return));
        for child in [guard, yes, no] {
            cpg.connect(function, child, CpgEdgeKind::AstChild);
        }
        for (source, target, kind) in [
            (function, guard, CfgEdgeKind::Sequential),
            (guard, yes, CfgEdgeKind::ConditionalTrue),
            (guard, no, CfgEdgeKind::ConditionalFalse),
        ] {
            cpg.connect(source, target, CpgEdgeKind::ControlFlow(kind));
        }

        let before = (cpg.node_count(), cpg.edge_count());
        let cfg = block_cfg(&cpg, function).expect("public block CFG");
        assert_eq!(cfg.blocks[0].nodes, vec![function, guard]);
        assert_eq!(cfg.blocks[1].leader, yes);
        assert_eq!(cfg.blocks[2].leader, no);
        assert_eq!(cfg.successors(cfg.entry).count(), 2);
        assert_eq!(cfg.exit_blocks.len(), 2);
        assert_eq!(cfg.reverse_post_order().len(), cfg.blocks.len());
        assert_eq!((cpg.node_count(), cpg.edge_count()), before);
    }
}

mod dataflow_framework {
    use libcpg::{
        block_cfg, dead_stores, liveness, solve, BitSetDomain, BlockCfg, BlockId, CfgEdgeKind,
        CodePropertyGraph, Confluence, CpgEdgeKind, CpgNode, CpgNodeKind, DataflowAnalysis,
        DefinitionKind, Direction, Language, MethodSignature, NodeId, ScopeId, SolverConfig,
        SourceRange, Visibility,
    };

    struct ReachableBlocks;

    impl DataflowAnalysis for ReachableBlocks {
        type Domain = BitSetDomain;

        const NAME: &'static str = "integration-reachable-blocks";
        const DIRECTION: Direction = Direction::Forward;
        const CONFLUENCE: Confluence = Confluence::May;

        fn bottom(&self, cfg: &BlockCfg) -> Self::Domain {
            BitSetDomain::new(cfg.blocks.len())
        }

        fn boundary(&self, cfg: &BlockCfg) -> Self::Domain {
            BitSetDomain::new(cfg.blocks.len())
        }

        fn transfer_block(
            &self,
            _cpg: &CodePropertyGraph,
            _cfg: &BlockCfg,
            block: BlockId,
            state: &mut Self::Domain,
        ) {
            state.insert(block.as_u32());
        }
    }

    fn node(id: u32, kind: CpgNodeKind) -> CpgNode {
        CpgNode::new(NodeId::new(id), kind, SourceRange::default())
    }

    #[test]
    fn downstream_analysis_reaches_a_deterministic_fixpoint_without_graph_mutation() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let function = cpg.add_node_with_id(node(
            10,
            CpgNodeKind::Function {
                signature: MethodSignature {
                    name: "flow".into(),
                    params: Default::default(),
                    return_type: None,
                    is_static: false,
                    is_async: false,
                    visibility: Visibility::Private,
                },
            },
        ));
        let branch = cpg.add_node_with_id(node(20, CpgNodeKind::If));
        let left = cpg.add_node_with_id(node(30, CpgNodeKind::Return));
        let right = cpg.add_node_with_id(node(40, CpgNodeKind::Return));
        for child in [branch, left, right] {
            cpg.connect(function, child, CpgEdgeKind::AstChild);
        }
        for (source, target, kind) in [
            (function, branch, CfgEdgeKind::Sequential),
            (branch, left, CfgEdgeKind::ConditionalTrue),
            (branch, right, CfgEdgeKind::ConditionalFalse),
        ] {
            cpg.connect(source, target, CpgEdgeKind::ControlFlow(kind));
        }

        let graph_size = (cpg.node_count(), cpg.edge_count());
        let cfg = block_cfg(&cpg, function).expect("block CFG");
        let solution = solve(&cpg, &cfg, &ReachableBlocks, &SolverConfig::default(), None)
            .expect("fixed point");

        for exit in &cfg.exit_blocks {
            assert!(solution.after[exit.as_u32() as usize].contains(cfg.entry.as_u32()));
            assert!(solution.after[exit.as_u32() as usize].contains(exit.as_u32()));
        }
        assert_eq!((cpg.node_count(), cpg.edge_count()), graph_size);
    }

    #[test]
    fn public_liveness_reports_only_the_unused_value_as_dead() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let function = cpg.add_node_with_id(node(
            10,
            CpgNodeKind::Function {
                signature: MethodSignature {
                    name: "stores".into(),
                    params: Default::default(),
                    return_type: None,
                    is_static: false,
                    is_async: false,
                    visibility: Visibility::Private,
                },
            },
        ));
        let parameter = cpg.add_node_with_id(node(
            20,
            CpgNodeKind::Parameter {
                name: "unused_parameter".into(),
                param_type: None,
                is_variadic: false,
            },
        ));
        let dead = cpg.add_node_with_id(node(
            30,
            CpgNodeKind::Variable {
                name: "dead".into(),
                var_type: None,
                is_mutable: true,
                scope: ScopeId::GLOBAL,
            },
        ));
        let live = cpg.add_node_with_id(node(
            40,
            CpgNodeKind::Variable {
                name: "live".into(),
                var_type: None,
                is_mutable: true,
                scope: ScopeId::GLOBAL,
            },
        ));
        let live_use = cpg.add_node_with_id(node(
            50,
            CpgNodeKind::Identifier {
                name: "live".into(),
                definition: None,
            },
        ));
        for child in [parameter, dead, live, live_use] {
            cpg.connect(function, child, CpgEdgeKind::AstChild);
            cpg.node_mut(function)
                .expect("function node")
                .children
                .push(child);
            cpg.node_mut(child).expect("child node").parent = Some(function);
        }
        for (source, target) in [
            (function, parameter),
            (parameter, dead),
            (dead, live),
            (live, live_use),
        ] {
            cpg.connect(
                source,
                target,
                CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
            );
        }

        let graph_size = (cpg.node_count(), cpg.edge_count());
        let live_result = liveness(&cpg, function).expect("public liveness");
        let dead_report = dead_stores(&cpg, function).expect("public dead stores");
        assert_eq!(dead_report.dead.len(), 1);
        assert_eq!(dead_report.dead[0].node, dead);
        assert_eq!(&*dead_report.dead[0].variable, "dead");
        assert_eq!(dead_report.dead[0].kind, DefinitionKind::Declaration);
        assert!(live_result.live_out[&live].contains(&"live".into()));
        assert_eq!((cpg.node_count(), cpg.edge_count()), graph_size);
    }
}

#[cfg(feature = "lang-rust")]
mod rust_pipeline {
    use std::collections::BTreeSet;

    use libcpg::{
        backward_slice, block_cfg, call_graph_projection, cfg_projection, dominator_tree,
        expression_facts, forward_slice, loop_forest, post_dominator_tree, unreachable_code,
        variable_facts, CpgBuilder, CpgBuilderConfig, CpgEdgeKind, CpgNodeKind, DfgEdgeKind,
        Language, LiteralKind, PdgBuilder, TreeSitterCpgBuilder,
    };

    const SRC: &str = "fn f(x: i32) -> i32 { let y = x + 1; if y > 0 { y } else { 0 } }";

    fn build() -> libcpg::CodePropertyGraph {
        TreeSitterCpgBuilder::new()
            .build(SRC, Language::Rust)
            .expect("build a Rust CPG")
    }

    #[test]
    fn builds_a_nonempty_graph_with_a_function() {
        let cpg = build();
        assert!(cpg.node_count() > 1);
        assert!(cpg.edge_count() >= 1);
        assert_eq!(cpg.language(), Language::Rust);
        assert_eq!(cpg.functions().count(), 1);
        let stats = cpg.stats();
        assert_eq!(stats.node_count, cpg.node_count());
        assert!(stats.cyclomatic_complexity >= 1);
    }

    #[test]
    fn parsed_function_builds_canonical_variable_and_expression_facts() {
        let cpg = build();
        let function = cpg.functions().next().expect("function").id;
        let variables = variable_facts(&cpg, function);
        let expressions = expression_facts(&cpg, function);

        assert!(variables.universe.id_of(&"x".into()).is_some());
        assert!(variables.universe.id_of(&"y".into()).is_some());
        assert!(!variables.definitions.is_empty());
        assert!(!variables.uses.is_empty());
        assert!(!expressions.universe.is_empty());
        assert!(expressions
            .occurrences
            .iter()
            .all(|occurrences| !occurrences.is_empty()));
    }

    #[test]
    fn pdg_then_slice_is_bounded_and_idempotent() {
        let mut cpg = build();
        let func = cpg.functions().map(|n| n.id).next().expect("a function");
        PdgBuilder::new().build(&mut cpg, func);
        let back = backward_slice(&cpg, func, 64);
        let fwd = forward_slice(&cpg, func, 64);
        assert!(back.len() <= 64 && fwd.len() <= 64);
        assert!(back.contains(&func));
        // Re-running PdgBuilder is idempotent (no duplicate edges).
        let before = cpg.edge_count();
        PdgBuilder::new().build(&mut cpg, func);
        assert_eq!(before, cpg.edge_count());
    }

    #[test]
    fn parsed_cfg_dominators_and_pdg_control_edges_share_one_analysis() {
        let mut cpg = build();
        let function = cpg
            .functions()
            .map(|node| node.id)
            .next()
            .expect("a function");
        let cfg = cfg_projection(&cpg, function).expect("function CFG");
        let dominators = dominator_tree(&cfg, function).expect("function dominators");
        assert!(dominators.dominates(function, function));
        assert!(!dominators.idom.is_empty());
        assert!(dominators
            .idom
            .keys()
            .copied()
            .all(|node| dominators.dominates(function, node)));

        let post_dominators =
            post_dominator_tree(&cpg, function).expect("function post-dominators");
        assert!(!post_dominators.is_empty());
        let expected: BTreeSet<_> = post_dominators
            .control_dependence_edges()
            .into_iter()
            .collect();

        PdgBuilder::new().build(&mut cpg, function);
        let actual: BTreeSet<_> = cpg
            .edges()
            .filter(|edge| matches!(edge.kind, CpgEdgeKind::ControlDependence))
            .map(|edge| (edge.source, edge.target))
            .collect();
        assert_eq!(actual, expected);
    }

    #[test]
    fn parsed_nested_loops_produce_a_forest_and_loop_exit_edges() {
        const LOOPS_SRC: &str = r#"
            fn loops(mut n: i32) {
                while n > 0 {
                    while n > 1 { n -= 2; }
                    n -= 1;
                }
                let done = n;
            }
        "#;
        let cpg = TreeSitterCpgBuilder::new()
            .build(LOOPS_SRC, Language::Rust)
            .expect("build nested loops");
        let function = cpg.functions().next().expect("loops function").id;
        let forest = loop_forest(&cpg, function).expect("natural-loop forest");

        assert_eq!(forest.loops.len(), 2);
        assert_eq!(forest.roots.len(), 1);
        assert_eq!(forest.max_depth(), 2);
        assert!(forest
            .loops
            .iter()
            .all(|natural_loop| !natural_loop.irreducible));
        assert!(cpg.edges().any(|edge| {
            matches!(
                edge.kind,
                CpgEdgeKind::ControlFlow(libcpg::CfgEdgeKind::LoopExit)
            )
        }));
    }

    #[test]
    fn parsed_branch_cfg_has_complete_basic_block_membership() {
        let cpg = build();
        let function = cpg.functions().next().expect("function").id;
        let cfg = block_cfg(&cpg, function).expect("parsed basic blocks");
        let projection = cfg_projection(&cpg, function).expect("parsed CFG projection");

        assert_eq!(cfg.block_of_node.len(), projection.node_count());
        assert!(projection
            .nodes
            .iter()
            .all(|node| cfg.block_of_node.contains_key(node)));
        assert!(cfg
            .blocks
            .iter()
            .all(|block| block.nodes.first() == Some(&block.leader)));
        assert_eq!(cfg.reverse_post_order().len(), cfg.blocks.len());
    }

    #[test]
    fn parsed_statements_after_return_are_reported_unreachable() {
        const DEAD_SRC: &str = r#"
            fn dead_tail() {
                return;
                let first = 1;
                let second = first + 1;
            }
        "#;
        let cpg = TreeSitterCpgBuilder::new()
            .build(DEAD_SRC, Language::Rust)
            .expect("build function with a dead tail");
        let function = cpg.functions().next().expect("dead_tail function").id;
        let report = unreachable_code(&cpg, function).expect("reachability report");
        let dead_declarations: Vec<_> = cpg
            .variables()
            .filter(|node| matches!(node.name(), Some("first" | "second")))
            .map(|node| node.id)
            .collect();
        assert_eq!(dead_declarations.len(), 2);
        assert!(
            dead_declarations
                .iter()
                .all(|node| report.unreachable.binary_search(node).is_ok()),
            "dead declarations {dead_declarations:?} were not all in {report:?}"
        );
    }

    #[test]
    fn opt_in_resolution_materializes_calls_before_interprocedural_dfg() {
        const RESOLVE_SRC: &str = r#"
            fn callee(x: i32) -> i32 { return x; }
            fn caller() -> i32 { callee(41) }
        "#;
        let unresolved = TreeSitterCpgBuilder::new()
            .build(RESOLVE_SRC, Language::Rust)
            .expect("default unresolved CPG");
        let unresolved_call = unresolved.calls().next().expect("parsed call").id;
        assert!(matches!(
            unresolved.node(unresolved_call).map(|node| &node.kind),
            Some(CpgNodeKind::Call { target: None, .. })
        ));

        let cpg = TreeSitterCpgBuilder::with_config(
            CpgBuilderConfig::default().with_import_resolution(true),
        )
        .build(RESOLVE_SRC, Language::Rust)
        .expect("resolved CPG");
        let callee = cpg
            .functions()
            .find(|node| node.name() == Some("callee"))
            .map(|node| node.id)
            .expect("callee function");
        let caller = cpg
            .functions()
            .find(|node| node.name() == Some("caller"))
            .map(|node| node.id)
            .expect("caller function");
        let call = cpg
            .calls()
            .find(|node| cpg.ast_ancestors(node.id).contains(&caller))
            .map(|node| node.id)
            .expect("caller call site");
        assert!(matches!(
            cpg.node(call).map(|node| &node.kind),
            Some(CpgNodeKind::Call {
                target: Some(target),
                ..
            }) if *target == callee
        ));
        assert_eq!(cpg.callees(call), vec![callee]);
        assert_eq!(cpg.callers(callee), vec![call]);
        let call_graph = call_graph_projection(&cpg);
        assert_eq!(
            call_graph
                .edges()
                .map(|(source, target, _)| {
                    (call_graph.node_id(source), call_graph.node_id(target))
                })
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([(caller, callee)])
        );
        assert!(cpg.edges_between(call, callee).into_iter().all(|edge| {
            !matches!(
                edge.kind,
                CpgEdgeKind::Reference
                    | CpgEdgeKind::StaticCall
                    | CpgEdgeKind::DynamicCall
                    | CpgEdgeKind::CallSite
            ) || edge.label.as_deref() == Some("exact_in_file")
        }));

        let parameter = cpg
            .nodes()
            .find(|node| {
                matches!(&node.kind, CpgNodeKind::Parameter { name, .. } if name.as_ref() == "x")
                    && cpg.ast_ancestors(node.id).contains(&callee)
            })
            .map(|node| node.id)
            .expect("callee parameter");
        let parameter_sources: Vec<_> = cpg
            .edges()
            .filter(|edge| {
                edge.target == parameter
                    && matches!(edge.kind, CpgEdgeKind::DataFlow(DfgEdgeKind::Parameter))
            })
            .map(|edge| edge.source)
            .collect();
        assert_eq!(parameter_sources.len(), 1);
        assert!(matches!(
            cpg.node(parameter_sources[0]).map(|node| &node.kind),
            Some(CpgNodeKind::Literal {
                kind: LiteralKind::Integer(41)
            })
        ));
        assert!(cpg.edges().any(|edge| {
            edge.target == call
                && matches!(edge.kind, CpgEdgeKind::DataFlow(DfgEdgeKind::ReturnValue))
        }));
    }
}

mod expression_dataflow_analysis {
    use libcpg::{
        available_expressions, very_busy_expressions, AvailableExpressions, CfgEdgeKind,
        CodePropertyGraph, CpgEdgeKind, CpgNode, CpgNodeKind, ExprKey, ExprOperand, Language,
        LiteralKind, MethodSignature, NodeId, SourceRange, VeryBusyExpressions, Visibility,
    };

    fn node(id: u32, kind: CpgNodeKind) -> CpgNode {
        CpgNode::new(NodeId::new(id), kind, SourceRange::default())
    }

    fn child(cpg: &mut CodePropertyGraph, parent: NodeId, child: NodeId) {
        cpg.connect(parent, child, CpgEdgeKind::AstChild);
        cpg.node_mut(parent).expect("parent").children.push(child);
        cpg.node_mut(child).expect("child").parent = Some(parent);
    }

    #[test]
    fn root_api_exposes_feature_free_cse_and_pre_evidence() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let function = cpg.add_node_with_id(node(
            0,
            CpgNodeKind::Function {
                signature: MethodSignature {
                    name: "expressions".into(),
                    params: Default::default(),
                    return_type: None,
                    is_static: false,
                    is_async: false,
                    visibility: Visibility::Private,
                },
            },
        ));
        let first = cpg.add_node_with_id(node(
            1,
            CpgNodeKind::BinaryOp {
                operator: "+".into(),
            },
        ));
        let second = cpg.add_node_with_id(node(
            2,
            CpgNodeKind::BinaryOp {
                operator: "+".into(),
            },
        ));
        let exit = cpg.add_node_with_id(node(3, CpgNodeKind::Return));
        for expression in [first, second] {
            child(&mut cpg, function, expression);
            let variable = cpg.add_node(node(
                0,
                CpgNodeKind::Identifier {
                    name: "x".into(),
                    definition: None,
                },
            ));
            let literal = cpg.add_node(node(
                0,
                CpgNodeKind::Literal {
                    kind: LiteralKind::Integer(1),
                },
            ));
            child(&mut cpg, expression, variable);
            child(&mut cpg, expression, literal);
        }
        child(&mut cpg, function, exit);
        for (source, target) in [(function, first), (first, second), (second, exit)] {
            cpg.connect(
                source,
                target,
                CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
            );
        }

        let available: AvailableExpressions =
            available_expressions(&cpg, function).expect("availability");
        let busy: VeryBusyExpressions =
            very_busy_expressions(&cpg, function).expect("anticipability");
        let key = ExprKey {
            operator: "+".into(),
            operands: vec![ExprOperand::Variable("x".into()), ExprOperand::Integer(1)],
        };
        assert!(available.available_in[&second].contains(&key));
        assert_eq!(available.redundant, vec![(first, second)]);
        assert!(busy.busy_out[&first].contains(&key));
        assert_eq!(busy.hoistable, vec![(key, first)]);
    }
}

#[cfg(feature = "lang-rust")]
mod graph_projections {
    use std::collections::BTreeSet;

    use libcpg::{
        call_graph_projection, cfg_projection, dfg_projection, module_graph_projection,
        pdg_projection, CodePropertyGraph, CpgBuilder, CpgEdgeKind, CpgNode, CpgNodeKind,
        GraphProjection, Language, NodeId, PdgBuilder, SourceRange, TreeSitterCpgBuilder,
    };

    const SRC: &str = r#"
        fn callee(x: i32) -> i32 { x + 1 }
        fn caller(flag: bool) -> i32 {
            let value = callee(1);
            if flag { value } else { 0 }
        }
    "#;

    fn projected_edges(projection: &GraphProjection) -> BTreeSet<(NodeId, NodeId)> {
        projection
            .edges()
            .map(|(source, target, _)| (projection.node_id(source), projection.node_id(target)))
            .collect()
    }

    fn direct_edges(
        cpg: &CodePropertyGraph,
        projection: &GraphProjection,
        include: impl Fn(&CpgEdgeKind) -> bool,
    ) -> BTreeSet<(NodeId, NodeId)> {
        cpg.edges()
            .filter(|edge| {
                include(&edge.kind)
                    && projection.dense_index(edge.source).is_some()
                    && projection.dense_index(edge.target).is_some()
            })
            .map(|edge| (edge.source, edge.target))
            .collect()
    }

    #[test]
    fn parsed_corpus_projections_equal_direct_overlay_traversal() {
        let mut cpg = TreeSitterCpgBuilder::new()
            .build(SRC, Language::Rust)
            .expect("build parsed Rust corpus");
        let caller = cpg
            .functions()
            .find(|node| node.name() == Some("caller"))
            .map(|node| node.id)
            .expect("caller function");
        let callee = cpg
            .functions()
            .find(|node| node.name() == Some("callee"))
            .map(|node| node.id)
            .expect("callee function");
        let call = cpg
            .calls()
            .find(|node| cpg.ast_ancestors(node.id).contains(&caller))
            .map(|node| node.id)
            .expect("call expression in caller");
        cpg.connect(call, callee, CpgEdgeKind::StaticCall);

        let root = cpg
            .nodes()
            .find(|node| matches!(node.kind, CpgNodeKind::Root))
            .map(|node| node.id)
            .expect("parsed root");
        let module = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Module {
                name: "dependency".into(),
            },
            SourceRange::default(),
        ));
        cpg.connect(root, module, CpgEdgeKind::Imports);
        PdgBuilder::new().build(&mut cpg, caller);

        let before = (cpg.node_count(), cpg.edge_count());
        let cfg = cfg_projection(&cpg, caller).expect("caller CFG");
        let dfg = dfg_projection(&cpg, None);
        let pdg = pdg_projection(&cpg, caller).expect("caller PDG");
        let modules = module_graph_projection(&cpg);
        let calls = call_graph_projection(&cpg);

        assert_eq!(
            projected_edges(&cfg),
            direct_edges(&cpg, &cfg, |kind| {
                kind.is_cfg()
                    && !matches!(kind, CpgEdgeKind::ControlFlow(libcpg::CfgEdgeKind::Call))
            })
        );
        assert_eq!(
            projected_edges(&dfg),
            direct_edges(&cpg, &dfg, CpgEdgeKind::is_dfg)
        );
        assert_eq!(
            projected_edges(&pdg),
            direct_edges(&cpg, &pdg, CpgEdgeKind::is_pdg)
        );
        assert_eq!(
            projected_edges(&modules),
            direct_edges(&cpg, &modules, |kind| matches!(kind, CpgEdgeKind::Imports))
        );
        assert_eq!(projected_edges(&calls), BTreeSet::from([(caller, callee)]));
        assert_eq!((cpg.node_count(), cpg.edge_count()), before);
    }
}

#[cfg(feature = "lang-rust")]
mod matching {
    use libcpg::pattern::{GraphSimilarity, SimilarityMetric, SubgraphMatcher, Vf2Matcher};
    use libcpg::{CpgBuilder, Language, TreeSitterCpgBuilder};

    #[test]
    fn vf2_self_match_and_similarity_reflexive() {
        let cpg = TreeSitterCpgBuilder::new()
            .build("fn g(a: i32) -> i32 { a * 2 }", Language::Rust)
            .expect("build");
        let matches = Vf2Matcher::new().find_matches(&cpg, &cpg);
        assert!(!matches.is_empty(), "a graph matches itself");
        let sim = GraphSimilarity::new().with_metric(SimilarityMetric::Jaccard);
        assert!((sim.similarity(&cpg, &cpg) - 1.0).abs() < 1e-9);
    }
}

#[cfg(all(feature = "lang-rust", feature = "design-patterns"))]
mod gof {
    use libcpg::patterns::design::{GofPatternDetector, PatternDetector};
    use libcpg::{CpgBuilder, Language, TreeSitterCpgBuilder};

    #[test]
    fn detector_runs_and_orders_by_confidence() {
        let cpg = TreeSitterCpgBuilder::new()
            .build("struct S; impl S { fn new() -> S { S } }", Language::Rust)
            .expect("build");
        let matches = GofPatternDetector::new().detect(&cpg);
        for w in matches.windows(2) {
            assert!(w[0].confidence >= w[1].confidence, "sorted descending");
        }
    }
}

#[cfg(all(feature = "lang-rust", feature = "algorithm-detection"))]
mod algorithms {
    use libcpg::algorithms::detection::DefaultAlgorithmDetector;
    use libcpg::algorithms::AlgorithmDetector;
    use libcpg::{CpgBuilder, Language, TreeSitterCpgBuilder};

    #[test]
    fn detect_runs_per_function() {
        let cpg = TreeSitterCpgBuilder::new()
            .build(
                "fn sort(a: &mut [i32]) { for i in 0..a.len() { for j in 0..a.len() { if a[i] < a[j] { a.swap(i, j); } } } }",
                Language::Rust,
            )
            .expect("build");
        let func = cpg.functions().map(|n| n.id).next().expect("a function");
        let detector = DefaultAlgorithmDetector::new();
        let found = detector.detect(&cpg, func); // exercises the pipeline end to end
        for w in found.windows(2) {
            assert!(w[0].confidence >= w[1].confidence, "sorted descending");
        }
    }
}

#[cfg(all(feature = "lang-rust", feature = "gnn"))]
mod gnn {
    use libcpg::gnn::{CpgGnn, GraphNeuralNetwork};
    use libcpg::{CpgBuilder, Language, TreeSitterCpgBuilder};

    #[test]
    fn propagate_embeds_every_node() {
        let cpg = TreeSitterCpgBuilder::new()
            .build("fn f(x: i32) -> i32 { x + 1 }", Language::Rust)
            .expect("build");
        let ids: Vec<_> = cpg.node_ids().collect();
        let mut gnn = CpgGnn::new(cpg).with_embedding_dim(16).with_num_layers(2);
        gnn.propagate(2);
        assert!(gnn.is_initialized());
        for id in ids {
            assert!(gnn.node_embedding(id).is_some());
        }
    }
}

#[cfg(all(feature = "lang-rust", feature = "serde"))]
mod serialization {
    use libcpg::{CpgBuilder, Language, TreeSitterCpgBuilder};

    #[test]
    fn whole_graph_json_round_trip_preserves_counts() {
        let cpg = TreeSitterCpgBuilder::new()
            .build("fn f(x: i32) -> i32 { x + 1 }", Language::Rust)
            .expect("build");
        let json = serde_json::to_string(&cpg).expect("serialize");
        let back: libcpg::CodePropertyGraph = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.node_count(), cpg.node_count());
        assert_eq!(back.edge_count(), cpg.edge_count());
    }
}
