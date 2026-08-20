//! Native small-stack regression for CPG diff and flat property snapshots.

use libcpg::{
    cpg_diff, ChangeKind, CodePropertyGraph, CpgDiffOptions, CpgEdge, CpgEdgeKind, CpgNode,
    CpgNodeKind, EdgeId, Language, NodeChangeField, NodeId, PropertyKey, PropertyValue,
    SourceRange,
};

const GRAPH_NODES: usize = 128;
const PROPERTY_DEPTH: usize = 100_000;

fn deep_property() -> PropertyValue {
    let mut value = PropertyValue::Int(7);
    for _ in 0..PROPERTY_DEPTH {
        value = PropertyValue::List(vec![value]);
    }
    value
}

fn chain(id_offset: u32, text: &str, property: PropertyValue) -> CodePropertyGraph {
    let mut graph = CodePropertyGraph::new(Language::Rust);
    let mut property = Some(property);
    for dense in 0..GRAPH_NODES {
        let id = NodeId::new(id_offset + dense as u32);
        let kind = match dense {
            0 => CpgNodeKind::Root,
            dense if dense + 1 == GRAPH_NODES => CpgNodeKind::Return,
            _ => CpgNodeKind::If,
        };
        let mut node = CpgNode::new(id, kind, SourceRange::default());
        if dense == 0 {
            node = node.with_text(text).with_property(
                PropertyKey::Custom("deep".into()),
                property.take().expect("root owns the deep property"),
            );
        }
        graph.add_node_with_id(node);
    }
    for dense in 0..GRAPH_NODES - 1 {
        graph
            .add_edge_with_id(CpgEdge::new(
                EdgeId::new(dense as u32),
                NodeId::new(id_offset + dense as u32),
                NodeId::new(id_offset + dense as u32 + 1),
                CpgEdgeKind::AstChild,
            ))
            .expect("chain endpoints exist");
    }
    graph
}

#[test]
fn deep_property_and_graph_alignment_fit_a_256_kib_native_stack() {
    std::thread::Builder::new()
        .name("cpg-diff-small-stack".into())
        .stack_size(256 * 1024)
        .spawn(|| {
            let property = deep_property();
            let old = chain(0, "before", property.clone());
            let new = chain(10_000, "after", property);
            let diff =
                cpg_diff(&old, &new, &CpgDiffOptions::default()).expect("bounded stack-safe diff");

            assert_eq!(diff.alignment.len(), GRAPH_NODES);
            assert!(diff.added.is_empty());
            assert!(diff.removed.is_empty());
            assert_eq!(diff.changed.len(), 1);
            assert_eq!(diff.classification, ChangeKind::Refactoring);
            assert!(matches!(
                &diff.changed[0],
                libcpg::CpgChange::Node { fields, .. }
                    if fields == &[NodeChangeField::Text]
            ));
        })
        .expect("spawn small-stack worker")
        .join()
        .expect("small-stack worker completed");
}
