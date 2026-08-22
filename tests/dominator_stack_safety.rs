use libcpg::{
    dominator_tree, post_dominator_tree, CfgEdgeKind, CodePropertyGraph, CpgEdgeKind, CpgNode,
    CpgNodeKind, GraphProjection, Language, MethodSignature, NodeId, ProjectionKind, SourceRange,
    Visibility,
};

const NODE_COUNT: usize = 100_000;
const SMALL_STACK_BYTES: usize = 256 * 1024;

fn function_kind() -> CpgNodeKind {
    CpgNodeKind::Function {
        signature: Box::new(MethodSignature {
            name: "deep_cfg".into(),
            params: Default::default(),
            return_type: None,
            is_static: false,
            is_async: false,
            visibility: Visibility::Private,
        }),
    }
}

#[test]
fn dominators_and_post_dominators_are_stack_safe_at_one_hundred_thousand_nodes() {
    let worker = std::thread::Builder::new()
        .name("dominator-small-stack".into())
        .stack_size(SMALL_STACK_BYTES)
        .spawn(|| {
            let nodes: Vec<_> = (0..NODE_COUNT as u32).map(NodeId::new).collect();
            let projection = GraphProjection::from_edges(
                ProjectionKind::External,
                nodes.iter().copied(),
                nodes.windows(2).map(|pair| (pair[0], pair[1], 1.0)),
            );
            let dominators =
                dominator_tree(&projection, NodeId::new(0)).expect("deep dominator path");
            assert_eq!(dominators.idom.len(), NODE_COUNT - 1);
            assert_eq!(
                dominators.immediate_dominator(NodeId::new((NODE_COUNT - 1) as u32)),
                Some(NodeId::new((NODE_COUNT - 2) as u32))
            );
            drop(dominators);
            drop(projection);

            let wide_projection = GraphProjection::from_edges(
                ProjectionKind::External,
                nodes.iter().copied(),
                nodes[1..]
                    .iter()
                    .copied()
                    .map(|target| (nodes[0], target, 1.0)),
            );
            let wide = dominator_tree(&wide_projection, nodes[0]).expect("wide dominator tree");
            assert_eq!(wide.children[&nodes[0]].len(), NODE_COUNT - 1);
            drop(wide);
            drop(wide_projection);

            let mut cpg = CodePropertyGraph::new(Language::Rust);
            let function = cpg.add_node(CpgNode::new(
                NodeId::new(0),
                function_kind(),
                SourceRange::default(),
            ));
            let exit = cpg.add_node(CpgNode::new(
                NodeId::new(0),
                CpgNodeKind::Return,
                SourceRange::default(),
            ));
            cpg.connect(
                function,
                exit,
                CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
            );
            let mut first_infinite = None;
            let mut previous = function;
            for _ in 2..NODE_COUNT {
                let current = cpg.add_node(CpgNode::new(
                    NodeId::new(0),
                    CpgNodeKind::Unknown { kind: "cfg".into() },
                    SourceRange::default(),
                ));
                first_infinite.get_or_insert(current);
                cpg.connect(
                    previous,
                    current,
                    CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
                );
                previous = current;
            }
            cpg.connect(
                previous,
                first_infinite.expect("infinite region head"),
                CpgEdgeKind::ControlFlow(CfgEdgeKind::LoopBack),
            );
            let post_dominators =
                post_dominator_tree(&cpg, function).expect("deep infinite post-dominator region");
            assert_eq!(post_dominators.exits, vec![exit]);
            assert!(post_dominators.ipdom.is_empty());
            assert_eq!(post_dominators.virtual_children.len(), NODE_COUNT);
            assert!(post_dominators.post_dominates(previous, previous));
            assert!(!post_dominators.post_dominates(exit, function));
            drop(post_dominators);
            drop(cpg);

            let mut wide_cpg = CodePropertyGraph::new(Language::Rust);
            let wide_function = wide_cpg.add_node(CpgNode::new(
                NodeId::new(0),
                function_kind(),
                SourceRange::default(),
            ));
            for _ in 1..NODE_COUNT {
                let wide_exit = wide_cpg.add_node(CpgNode::new(
                    NodeId::new(0),
                    CpgNodeKind::Return,
                    SourceRange::default(),
                ));
                wide_cpg.connect(
                    wide_function,
                    wide_exit,
                    CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
                );
            }
            let wide_post_dominators = post_dominator_tree(&wide_cpg, wide_function)
                .expect("wide function post-dominator tree");
            assert_eq!(wide_post_dominators.exits.len(), NODE_COUNT - 1);
            assert_eq!(wide_post_dominators.virtual_children.len(), NODE_COUNT);
        })
        .expect("spawn small-stack dominator worker");

    worker.join().expect("small-stack dominator worker");
}
