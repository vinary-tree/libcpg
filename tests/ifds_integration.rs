use std::collections::BTreeSet;

use libcpg::{
    call_graph_sccs, solve_summaries, solve_summaries_with_resolver, CalleeRef, CodePropertyGraph,
    CpgEdgeKind, CpgNode, CpgNodeKind, IfdsCaps, IfdsLattice, Language, MethodSignature, NodeId,
    SourceRange, SummaryCtx, SummaryDomain, Visibility,
};

#[derive(Clone, Default)]
struct ReachSummary(BTreeSet<NodeId>);

impl IfdsLattice for ReachSummary {
    fn join(&mut self, other: &Self) -> bool {
        let before = self.0.len();
        self.0.extend(other.0.iter().copied());
        self.0.len() != before
    }

    fn subsumes(&self, other: &Self) -> bool {
        self.0.is_superset(&other.0)
    }
}

struct PublicReachability;

impl SummaryDomain for PublicReachability {
    type Fact = NodeId;
    type Summary = ReachSummary;

    fn seed_facts(&self, _cpg: &CodePropertyGraph, function: NodeId) -> Vec<(NodeId, Self::Fact)> {
        vec![(function, function)]
    }

    fn summarize(
        &self,
        cpg: &CodePropertyGraph,
        function: NodeId,
        ctx: &SummaryCtx<'_, Self>,
    ) -> Self::Summary {
        let mut summary = ReachSummary(BTreeSet::from([function]));
        let mut scope = cpg.ast_descendants(function);
        scope.sort_unstable();
        for call in scope.into_iter().filter(|&node| {
            cpg.node(node)
                .is_some_and(|node| matches!(node.kind, CpgNodeKind::Call { .. }))
        }) {
            if let CalleeRef::Resolved(callee) = (ctx.resolve)(call) {
                summary.0.insert(callee);
                if let Some(callee_summary) = ctx.summaries.get(&callee) {
                    summary.0.extend(callee_summary.0.iter().copied());
                }
            }
        }
        summary
    }
}

fn function(name: &str) -> CpgNodeKind {
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

fn child(cpg: &mut CodePropertyGraph, parent: NodeId, kind: CpgNodeKind) -> NodeId {
    let child = add(cpg, kind);
    cpg.connect(parent, child, CpgEdgeKind::AstChild)
        .expect("public fixture AST endpoints exist");
    cpg.node_mut(child)
        .expect("public fixture child exists")
        .parent = Some(parent);
    child
}

#[test]
fn public_api_solves_a_recursive_call_graph_end_to_end() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let entry = add(&mut cpg, function("entry"));
    let worker = add(&mut cpg, function("worker"));
    let leaf = add(&mut cpg, function("leaf"));
    child(
        &mut cpg,
        entry,
        CpgNodeKind::Call {
            target: Some(worker),
            is_method: false,
        },
    );
    child(
        &mut cpg,
        worker,
        CpgNodeKind::Call {
            target: Some(leaf),
            is_method: false,
        },
    );
    child(
        &mut cpg,
        leaf,
        CpgNodeKind::Call {
            target: Some(worker),
            is_method: false,
        },
    );

    let output = solve_summaries(
        &cpg,
        &PublicReachability,
        &call_graph_sccs(&cpg),
        &IfdsCaps::default(),
    );

    assert!(output.is_complete());
    assert_eq!(
        output.summaries[&entry].0,
        BTreeSet::from([entry, worker, leaf])
    );
    assert_eq!(output.summaries[&worker].0, BTreeSet::from([worker, leaf]));
    assert_eq!(output.summaries[&leaf].0, BTreeSet::from([worker, leaf]));
}

#[test]
fn public_custom_resolver_handles_an_external_boundary() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let entry = add(&mut cpg, function("entry"));
    let call = child(
        &mut cpg,
        entry,
        CpgNodeKind::Call {
            target: None,
            is_method: false,
        },
    );
    let resolver = |site: NodeId| {
        if site == call {
            CalleeRef::External("system-library".into())
        } else {
            CalleeRef::Unresolved("unknown".into())
        }
    };

    let output = solve_summaries_with_resolver(
        &cpg,
        &PublicReachability,
        &call_graph_sccs(&cpg),
        &IfdsCaps::default(),
        &resolver,
    );

    assert!(output.is_complete());
    assert_eq!(output.summaries[&entry].0, BTreeSet::from([entry]));
}
