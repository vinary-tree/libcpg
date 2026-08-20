//! Public-API stack-safety gate for hierarchy-based call refinement.

use libcpg::{
    devirtualize, CallGraphResolution, CallResolution, CodePropertyGraph, Confidence, CpgEdgeKind,
    CpgNode, CpgNodeKind, DevirtualizationMode, Language, MethodSignature, NodeId, ScopeId,
    SourceRange, TypeInfo, Visibility,
};

fn node(kind: CpgNodeKind) -> CpgNode {
    CpgNode::new(NodeId::new(0), kind, SourceRange::default())
}

fn child(cpg: &mut CodePropertyGraph, parent: NodeId, kind: CpgNodeKind) -> NodeId {
    let child = cpg.add_node(node(kind).with_parent(parent));
    cpg.node_mut(parent).expect("parent").children.push(child);
    cpg.connect(parent, child, CpgEdgeKind::AstChild);
    child
}

#[test]
fn public_cha_crosses_depth_100_000_on_a_small_native_stack() {
    const DEPTH: usize = 100_000;

    std::thread::Builder::new()
        .name("devirtualization-small-stack".into())
        .stack_size(256 * 1024)
        .spawn(|| {
            let mut cpg = CodePropertyGraph::new(Language::Java);
            let root = cpg.add_node(node(CpgNodeKind::Root));
            let base = child(
                &mut cpg,
                root,
                CpgNodeKind::Class {
                    name: "Type0".into(),
                    is_abstract: false,
                },
            );
            let base_method = child(
                &mut cpg,
                base,
                CpgNodeKind::Function {
                    signature: MethodSignature {
                        name: "run".into(),
                        params: Default::default(),
                        return_type: None,
                        is_static: false,
                        is_async: false,
                        visibility: Visibility::Public,
                    },
                },
            );
            let mut parent = base;
            for depth in 1..DEPTH {
                let subtype = child(
                    &mut cpg,
                    root,
                    CpgNodeKind::Class {
                        name: format!("Type{depth}").into(),
                        is_abstract: false,
                    },
                );
                cpg.connect(subtype, parent, CpgEdgeKind::Inherits);
                parent = subtype;
            }

            let definition = child(
                &mut cpg,
                root,
                CpgNodeKind::Variable {
                    name: "receiver".into(),
                    var_type: Some(TypeInfo::new("Type0")),
                    scope: ScopeId::GLOBAL,
                    is_mutable: false,
                },
            );
            let call = child(
                &mut cpg,
                root,
                CpgNodeKind::Call {
                    target: None,
                    is_method: true,
                },
            );
            let member = child(
                &mut cpg,
                call,
                CpgNodeKind::MemberAccess {
                    member: "run".into(),
                },
            );
            child(
                &mut cpg,
                member,
                CpgNodeKind::Identifier {
                    name: "receiver".into(),
                    definition: Some(definition),
                },
            );
            let mut input = CallGraphResolution::default();
            input.resolutions.insert(
                call,
                CallResolution {
                    callee_name: Some("run".into()),
                    scope: Some(root),
                    confidence: Confidence::BareNameUnique,
                    target: Some(base_method),
                    candidates: vec![base_method],
                    candidate_count: 1,
                    candidates_truncated: false,
                },
            );

            let before = (cpg.node_count(), cpg.edge_count());
            let output = devirtualize(&cpg, &input, DevirtualizationMode::Cha);
            assert_eq!(output.resolutions[&call].target, Some(base_method));
            assert_eq!(output.resolutions[&call].candidate_count, 1);
            assert_eq!((cpg.node_count(), cpg.edge_count()), before);
        })
        .expect("spawn devirtualization small-stack test")
        .join()
        .expect("worklist devirtualization completes on a small native stack");
}
