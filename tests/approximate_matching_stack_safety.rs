#![cfg(feature = "approximate-matching")]

use libcpg::{
    project_cpg_functions, ApproximateCompletion, ApproximateLimits, CodePropertyGraph, CpgNode,
    CpgNodeKind, Language, MethodSignature, NodeId, SourceRange, Visibility,
};

fn function(name: &str) -> CpgNodeKind {
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

fn project_chain(depth: usize) {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let root = cpg.add_node(CpgNode::new(
        NodeId::new(0),
        function("deep"),
        SourceRange::default(),
    ));
    let mut parent = root;
    for index in 0..depth {
        let child = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Identifier {
                name: format!("n{index}").into(),
                definition: None,
            },
            SourceRange::default(),
        ));
        cpg.node_mut(parent)
            .expect("chain parent")
            .children
            .push(child);
        parent = child;
    }
    let limits = ApproximateLimits {
        max_input_units: depth + 1,
        max_pair_cells: 1,
        max_work_units: (depth as u64 + 1) * 3,
        max_state_count: 1,
        max_memory_bytes: 128 * 1024 * 1024,
        ..ApproximateLimits::default()
    };
    let projection = project_cpg_functions(&cpg, 1, &limits).expect("bounded deep projection");
    assert_eq!(projection.completion, ApproximateCompletion::Complete);
    assert_eq!(projection.records.len(), 1);
}

#[test]
fn function_projection_is_stack_safe_at_20k_and_100k_depth() {
    std::thread::Builder::new()
        .name("approximate-projection-stack-gate".into())
        .stack_size(256 * 1024)
        .spawn(|| {
            project_chain(20_000);
            project_chain(100_000);
        })
        .expect("spawn small-stack gate")
        .join()
        .expect("iterative projection must not overflow the native stack");
}
