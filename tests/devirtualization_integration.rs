//! Public-API integration coverage for closed-world virtual-call refinement.

use std::collections::BTreeSet;

use libcpg::{
    devirtualize, resolve_calls, CodePropertyGraph, Confidence, CpgEdgeKind, CpgNode, CpgNodeKind,
    DevirtualizationMode, Language, MethodSignature, NodeId, ResolveConfig, ScopeId, SourceRange,
    TypeInfo, Visibility,
};

fn node(kind: CpgNodeKind) -> CpgNode {
    CpgNode::new(NodeId::new(0), kind, SourceRange::default())
}

fn child(cpg: &mut CodePropertyGraph, parent: NodeId, kind: CpgNodeKind) -> NodeId {
    let child = cpg.add_node(node(kind).with_parent(parent));
    cpg.connect(parent, child, CpgEdgeKind::AstChild);
    child
}

fn method(cpg: &mut CodePropertyGraph, owner: NodeId, name: &str) -> NodeId {
    child(
        cpg,
        owner,
        CpgNodeKind::Function {
            signature: MethodSignature {
                name: name.into(),
                params: Default::default(),
                return_type: None,
                is_static: false,
                is_async: false,
                visibility: Visibility::Public,
            },
        },
    )
}

fn class(cpg: &mut CodePropertyGraph, root: NodeId, name: &str) -> NodeId {
    child(
        cpg,
        root,
        CpgNodeKind::Class {
            name: name.into(),
            is_abstract: false,
        },
    )
}

#[test]
fn public_c1_resolution_refines_monotonically_without_mutating_topology() {
    let mut cpg = CodePropertyGraph::new(Language::Java);
    let root = cpg.add_node(node(CpgNodeKind::Root));
    let base = class(&mut cpg, root, "Base");
    let mid = class(&mut cpg, root, "Mid");
    let leaf = class(&mut cpg, root, "Leaf");
    cpg.connect(mid, base, CpgEdgeKind::Inherits);
    cpg.connect(leaf, mid, CpgEdgeKind::Inherits);
    let base_run = method(&mut cpg, base, "run");
    let mid_run = method(&mut cpg, mid, "run");
    let leaf_run = method(&mut cpg, leaf, "run");

    let definition = child(
        &mut cpg,
        root,
        CpgNodeKind::Variable {
            name: "service".into(),
            var_type: Some(TypeInfo::new("Base")),
            scope: ScopeId::GLOBAL,
            is_mutable: true,
        },
    );
    let leaf_allocation = child(
        &mut cpg,
        definition,
        CpgNodeKind::Call {
            target: None,
            is_method: false,
        },
    );
    cpg.node_mut(leaf_allocation)
        .expect("leaf allocation")
        .allocation_type = Some(TypeInfo::new("Leaf"));
    let mid_allocation = child(
        &mut cpg,
        root,
        CpgNodeKind::Call {
            target: None,
            is_method: false,
        },
    );
    cpg.node_mut(mid_allocation)
        .expect("mid allocation")
        .allocation_type = Some(TypeInfo::new("Mid"));

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
            name: "service".into(),
            definition: Some(definition),
        },
    );

    let c1 = resolve_calls(&mut cpg, &ResolveConfig::default());
    assert_eq!(
        c1.resolutions[&call].confidence,
        Confidence::BareNameAmbiguous
    );
    let topology = (cpg.node_count(), cpg.edge_count());
    let cha = devirtualize(&cpg, &c1, DevirtualizationMode::Cha);
    let rta = devirtualize(&cpg, &c1, DevirtualizationMode::Rta);
    let vta = devirtualize(&cpg, &c1, DevirtualizationMode::Vta);

    assert_eq!(
        cha.resolutions[&call].candidates,
        vec![base_run, mid_run, leaf_run]
    );
    assert_eq!(rta.resolutions[&call].candidates, vec![mid_run, leaf_run]);
    assert_eq!(vta.resolutions[&call].candidates, vec![leaf_run]);
    let cha: BTreeSet<_> = cha.resolutions[&call].candidates.iter().copied().collect();
    let rta: BTreeSet<_> = rta.resolutions[&call].candidates.iter().copied().collect();
    let vta: BTreeSet<_> = vta.resolutions[&call].candidates.iter().copied().collect();
    assert!(vta.is_subset(&rta));
    assert!(rta.is_subset(&cha));
    assert_eq!((cpg.node_count(), cpg.edge_count()), topology);
}

#[test]
fn implements_edges_support_default_method_dispatch() {
    let mut cpg = CodePropertyGraph::new(Language::Java);
    let root = cpg.add_node(node(CpgNodeKind::Root));
    let interface = child(
        &mut cpg,
        root,
        CpgNodeKind::Trait {
            name: "Runner".into(),
        },
    );
    let default_run = method(&mut cpg, interface, "run");
    let worker = class(&mut cpg, root, "Worker");
    cpg.connect(worker, interface, CpgEdgeKind::Implements);
    let definition = child(
        &mut cpg,
        root,
        CpgNodeKind::Variable {
            name: "runner".into(),
            var_type: Some(TypeInfo::new("Runner")),
            scope: ScopeId::GLOBAL,
            is_mutable: false,
        },
    );
    let allocation = child(
        &mut cpg,
        definition,
        CpgNodeKind::Call {
            target: None,
            is_method: false,
        },
    );
    cpg.node_mut(allocation)
        .expect("allocation")
        .allocation_type = Some(TypeInfo::new("Worker"));
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
            name: "runner".into(),
            definition: Some(definition),
        },
    );

    let c1 = resolve_calls(&mut cpg, &ResolveConfig::default());
    let refined = devirtualize(&cpg, &c1, DevirtualizationMode::Rta);
    assert_eq!(refined.resolutions[&call].target, Some(default_run));
    assert_eq!(refined.resolutions[&call].confidence, Confidence::RapidType);
}
