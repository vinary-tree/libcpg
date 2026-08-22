//! Small-native-stack integration coverage for public WL analysis.

use libcpg::{structural_clone_classes, wl_labels, GraphProjection, NodeId, ProjectionKind};

const NODE_COUNT: usize = 100_000;
const SMALL_STACK_BYTES: usize = 256 * 1024;

#[test]
fn public_wl_handles_deep_and_wide_hundred_thousand_node_graphs() {
    let worker = std::thread::Builder::new()
        .name("wl-small-stack".into())
        .stack_size(SMALL_STACK_BYTES)
        .spawn(|| {
            let nodes = (0..NODE_COUNT as u32).map(NodeId::new).collect::<Vec<_>>();

            let deep = GraphProjection::from_edges(
                ProjectionKind::External,
                nodes.iter().copied(),
                nodes.windows(2).map(|pair| (pair[0], pair[1], 1.0)),
            );
            let deep_labels = wl_labels(&deep, 4);
            assert_eq!(deep_labels.len(), NODE_COUNT);
            assert_ne!(deep_labels[0], deep_labels[NODE_COUNT / 2]);
            assert_eq!(deep_labels, wl_labels(&deep, 4));
            drop(deep_labels);
            drop(deep);

            let wide = GraphProjection::from_edges(
                ProjectionKind::External,
                nodes.iter().copied(),
                nodes[1..]
                    .iter()
                    .copied()
                    .map(|target| (nodes[0], target, 1.0)),
            );
            let wide_labels = wl_labels(&wide, 4);
            assert_eq!(wide_labels.len(), NODE_COUNT);
            assert_ne!(wide_labels[0], wide_labels[1]);
            assert!(wide_labels[1..]
                .iter()
                .all(|&label| label == wide_labels[1]));
            assert_eq!(
                structural_clone_classes(&wide, 4, NODE_COUNT - 1),
                vec![nodes[1..].to_vec()]
            );
            drop(wide_labels);
            drop(wide);
            drop(nodes);
        })
        .expect("spawn 256 KiB WL worker");

    worker
        .join()
        .expect("iterative WL lifecycle completes without stack overflow");
}
