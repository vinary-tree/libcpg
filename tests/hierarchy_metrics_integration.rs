//! Public-API coverage for stack-safe hierarchy metrics.

use libcpg::{hierarchy_metrics, HierarchyMetrics, NodeId};

#[test]
fn public_hierarchy_api_condenses_cycles_and_preserves_direct_children() {
    let nodes = (10..=14).map(NodeId::new).collect::<Vec<_>>();
    let result = hierarchy_metrics(
        &nodes,
        &[
            (NodeId::new(11), NodeId::new(10)),
            (NodeId::new(12), NodeId::new(11)),
            (NodeId::new(11), NodeId::new(12)),
            (NodeId::new(13), NodeId::new(12)),
            (NodeId::new(14), NodeId::new(13)),
        ],
    );

    assert_eq!(
        result,
        vec![
            HierarchyMetrics {
                node: NodeId::new(10),
                depth: 0,
                direct_children: 1,
            },
            HierarchyMetrics {
                node: NodeId::new(11),
                depth: 1,
                direct_children: 1,
            },
            HierarchyMetrics {
                node: NodeId::new(12),
                depth: 1,
                direct_children: 2,
            },
            HierarchyMetrics {
                node: NodeId::new(13),
                depth: 2,
                direct_children: 1,
            },
            HierarchyMetrics {
                node: NodeId::new(14),
                depth: 3,
                direct_children: 0,
            },
        ]
    );
}

#[test]
fn public_hierarchy_api_is_total_for_empty_and_out_of_domain_relations() {
    assert!(hierarchy_metrics(&[], &[]).is_empty());
    assert_eq!(
        hierarchy_metrics(&[NodeId::new(7)], &[(NodeId::new(7), NodeId::new(99))]),
        vec![HierarchyMetrics {
            node: NodeId::new(7),
            depth: 0,
            direct_children: 0,
        }]
    );
}
