use std::collections::{BTreeMap, BTreeSet, VecDeque};

use libcpg::{projection_sccs, GraphProjection, NodeId, ProjectionKind, SccDecomposition};
use proptest::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq)]
struct TestIsomorphism {
    forward: BTreeMap<NodeId, NodeId>,
    backward: BTreeMap<NodeId, NodeId>,
}

impl TestIsomorphism {
    fn new(pairs: impl IntoIterator<Item = (NodeId, NodeId)>) -> Result<Self, &'static str> {
        let mut forward = BTreeMap::new();
        let mut backward = BTreeMap::new();
        for (source, target) in pairs {
            if forward.insert(source, target).is_some() {
                return Err("an isomorphism source must be unique");
            }
            if backward.insert(target, source).is_some() {
                return Err("an isomorphism target must be unique");
            }
        }
        Ok(Self { forward, backward })
    }

    fn identity(nodes: impl IntoIterator<Item = NodeId>) -> Self {
        Self::new(nodes.into_iter().map(|node| (node, node))).expect("identity pairs are bijective")
    }

    fn inverse(&self) -> Self {
        Self {
            forward: self.backward.clone(),
            backward: self.forward.clone(),
        }
    }

    fn compose(&self, after: &Self) -> Result<Self, &'static str> {
        if self.backward.keys().ne(after.forward.keys()) {
            return Err("isomorphism composition endpoints must agree");
        }
        Self::new(self.forward.iter().map(|(&source, middle)| {
            let target = after
                .forward
                .get(middle)
                .copied()
                .expect("equal composition endpoints contain every middle node");
            (source, target)
        }))
    }

    fn map_node(&self, node: NodeId) -> Result<NodeId, &'static str> {
        self.forward
            .get(&node)
            .copied()
            .ok_or("isomorphism does not contain the source node")
    }

    fn map_kind(&self, kind: ProjectionKind) -> Result<ProjectionKind, &'static str> {
        Ok(match kind {
            ProjectionKind::ControlFlow { function } => ProjectionKind::ControlFlow {
                function: self.map_node(function)?,
            },
            ProjectionKind::DataFlow { function } => ProjectionKind::DataFlow {
                function: function.map(|node| self.map_node(node)).transpose()?,
            },
            ProjectionKind::ProgramDependence { function } => ProjectionKind::ProgramDependence {
                function: self.map_node(function)?,
            },
            unchanged => unchanged,
        })
    }

    fn map_projection(
        &self,
        projection: &GraphProjection,
    ) -> Result<GraphProjection, &'static str> {
        if projection
            .nodes
            .iter()
            .any(|node| !self.forward.contains_key(node))
            || projection.nodes.len() != self.forward.len()
        {
            return Err("isomorphism domain must equal the projection node set");
        }
        let nodes = projection
            .nodes
            .iter()
            .map(|&node| self.map_node(node))
            .collect::<Result<Vec<_>, _>>()?;
        let edges = projection
            .edges()
            .map(|(source, target, weight)| {
                Ok((
                    self.map_node(projection.node_id(source))?,
                    self.map_node(projection.node_id(target))?,
                    weight,
                ))
            })
            .collect::<Result<Vec<_>, &'static str>>()?;
        Ok(GraphProjection::from_edges(
            self.map_kind(projection.kind)?,
            nodes,
            edges,
        ))
    }
}

fn ranked_ids(keys: &[u16], node_count: usize, offset: u32, stride: u32) -> Vec<NodeId> {
    let mut logical = (0..node_count).collect::<Vec<_>>();
    logical.sort_unstable_by_key(|&index| (keys[index], index));
    let mut ids = vec![NodeId::new(0); node_count];
    for (rank, index) in logical.into_iter().enumerate() {
        ids[index] = NodeId::new(
            offset
                .checked_add(u32::try_from(rank).expect("generated rank fits u32") * stride)
                .expect("generated sparse node ID fits u32"),
        );
    }
    ids
}

fn logical_partition(
    decomposition: &SccDecomposition,
    logical_by_node: &BTreeMap<NodeId, usize>,
) -> Vec<Vec<usize>> {
    let mut components = decomposition
        .components
        .iter()
        .map(|component| {
            let mut nodes = component
                .nodes
                .iter()
                .map(|node| {
                    logical_by_node
                        .get(node)
                        .copied()
                        .expect("every decomposition node belongs to the witness")
                })
                .collect::<Vec<_>>();
            nodes.sort_unstable();
            nodes
        })
        .collect::<Vec<_>>();
    components.sort_unstable();
    components
}

fn logical_condensation_edges(
    decomposition: &SccDecomposition,
    logical_by_node: &BTreeMap<NodeId, usize>,
) -> BTreeSet<(Vec<usize>, Vec<usize>)> {
    let components = decomposition
        .components
        .iter()
        .map(|component| {
            let mut nodes = component
                .nodes
                .iter()
                .map(|node| {
                    logical_by_node
                        .get(node)
                        .copied()
                        .expect("every decomposition node belongs to the witness")
                })
                .collect::<Vec<_>>();
            nodes.sort_unstable();
            nodes
        })
        .collect::<Vec<_>>();
    decomposition
        .condensation_edges
        .iter()
        .map(|edge| {
            (
                components[edge.source].clone(),
                components[edge.target].clone(),
            )
        })
        .collect()
}

fn reachable(adjacency: &[Vec<usize>], source: usize, target: usize) -> bool {
    let mut seen = vec![false; adjacency.len()];
    let mut queue = VecDeque::from([source]);
    seen[source] = true;
    while let Some(node) = queue.pop_front() {
        if node == target {
            return true;
        }
        for &successor in &adjacency[node] {
            if !seen[successor] {
                seen[successor] = true;
                queue.push_back(successor);
            }
        }
    }
    false
}

fn projection_adjacency(projection: &GraphProjection) -> Vec<Vec<usize>> {
    (0..projection.node_count())
        .map(|source| {
            projection
                .successors(u32::try_from(source).expect("test projection fits u32"))
                .iter()
                .map(|&target| target as usize)
                .collect()
        })
        .collect()
}

fn assert_quotient_laws(projection: &GraphProjection, decomposition: &SccDecomposition) {
    let graph = projection_adjacency(projection);
    let mut quotient = vec![Vec::new(); decomposition.components.len()];
    for edge in &decomposition.condensation_edges {
        assert_ne!(edge.source, edge.target, "quotient edges cannot be loops");
        quotient[edge.source].push(edge.target);
    }

    for source in 0..projection.node_count() {
        for target in 0..projection.node_count() {
            let source_node = projection.node_id(source as u32);
            let target_node = projection.node_id(target as u32);
            let source_component = decomposition.component_by_node[&source_node];
            let target_component = decomposition.component_by_node[&target_node];
            assert_eq!(
                reachable(&graph, source, target),
                source_component == target_component
                    || reachable(&quotient, source_component, target_component),
                "condensation must preserve reachability"
            );
        }
    }

    let quotient_projection = decomposition.condensation_projection();
    let quotient_decomposition = projection_sccs(&quotient_projection);
    assert!(
        quotient_decomposition.is_acyclic(),
        "a condensation must be acyclic"
    );
    assert!(
        quotient_decomposition
            .components
            .iter()
            .all(|component| component.nodes.len() == 1 && !component.cyclic),
        "a second quotient cannot contract another component"
    );
    assert_eq!(
        quotient_decomposition.condensation_projection(),
        quotient_projection,
        "condensation must be idempotent up to its canonical representatives"
    );
}

#[test]
fn isomorphism_validation_rejects_non_bijections_and_endpoint_mismatch() {
    let a = NodeId::new(1);
    let b = NodeId::new(2);
    let x = NodeId::new(10);
    let y = NodeId::new(20);

    assert_eq!(
        TestIsomorphism::new([(a, x), (a, y)]),
        Err("an isomorphism source must be unique")
    );
    assert_eq!(
        TestIsomorphism::new([(a, x), (b, x)]),
        Err("an isomorphism target must be unique")
    );

    let left = TestIsomorphism::new([(a, x), (b, y)]).expect("valid left witness");
    let incompatible = TestIsomorphism::new([(a, NodeId::new(30)), (b, NodeId::new(40))])
        .expect("valid but endpoint-incompatible right witness");
    assert_eq!(
        left.compose(&incompatible),
        Err("isomorphism composition endpoints must agree")
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(192))]

    #[test]
    fn graph_isomorphisms_obey_category_and_scc_quotient_laws(
        node_count in 1usize..24,
        raw_edges in prop::collection::vec((0usize..24, 0usize..24), 0..96),
        first_keys in prop::collection::vec(any::<u16>(), 24),
        second_keys in prop::collection::vec(any::<u16>(), 24),
    ) {
        let base_ids = (0..node_count)
            .map(|index| NodeId::new(100 + index as u32 * 7))
            .collect::<Vec<_>>();
        let first_ids = ranked_ids(&first_keys, node_count, 10_000, 11);
        let second_ids = ranked_ids(&second_keys, node_count, 20_000, 13);
        let logical_edges = raw_edges
            .into_iter()
            .filter(|&(source, target)| source < node_count && target < node_count)
            .collect::<Vec<_>>();
        let projection = GraphProjection::from_edges(
            ProjectionKind::External,
            base_ids.iter().copied(),
            logical_edges
                .iter()
                .map(|&(source, target)| (base_ids[source], base_ids[target], 1.0)),
        );
        let first = TestIsomorphism::new(
            (0..node_count).map(|index| (base_ids[index], first_ids[index])),
        ).expect("generated first witness is bijective");
        let second = TestIsomorphism::new(
            (0..node_count).map(|index| (first_ids[index], second_ids[index])),
        ).expect("generated second witness is bijective");
        let composite = first.compose(&second).expect("generated endpoints agree");
        let identity = TestIsomorphism::identity(base_ids.iter().copied());

        prop_assert_eq!(
            identity.map_projection(&projection).expect("identity domain agrees"),
            projection.clone(),
        );
        prop_assert_eq!(
            first
                .inverse()
                .map_projection(&first.map_projection(&projection).expect("first map"))
                .expect("inverse map"),
            projection.clone(),
        );
        prop_assert_eq!(
            second
                .map_projection(&first.map_projection(&projection).expect("first map"))
                .expect("second map"),
            composite.map_projection(&projection).expect("composite map"),
        );

        let first_projection = first.map_projection(&projection).expect("first relabeling");
        let base_sccs = projection_sccs(&projection);
        let first_sccs = projection_sccs(&first_projection);
        let base_logical = base_ids
            .iter()
            .enumerate()
            .map(|(logical, &node)| (node, logical))
            .collect::<BTreeMap<_, _>>();
        let first_logical = first_ids
            .iter()
            .enumerate()
            .map(|(logical, &node)| (node, logical))
            .collect::<BTreeMap<_, _>>();

        prop_assert_eq!(
            logical_partition(&base_sccs, &base_logical),
            logical_partition(&first_sccs, &first_logical),
        );
        prop_assert_eq!(
            logical_condensation_edges(&base_sccs, &base_logical),
            logical_condensation_edges(&first_sccs, &first_logical),
        );
        assert_quotient_laws(&projection, &base_sccs);
        assert_quotient_laws(&first_projection, &first_sccs);
    }
}

#[test]
fn semantic_depth_20_000_is_stack_safe_on_a_256_kib_thread() {
    const DEPTH: u32 = 20_000;
    std::thread::Builder::new()
        .name("compositional-laws-depth".into())
        .stack_size(256 * 1024)
        .spawn(|| {
            let nodes = (0..DEPTH).map(NodeId::new).collect::<Vec<_>>();
            let path = GraphProjection::from_edges(
                ProjectionKind::External,
                nodes.iter().copied(),
                (0..DEPTH - 1).map(|node| (NodeId::new(node), NodeId::new(node + 1), 1.0)),
            );
            let path_sccs = projection_sccs(&path);
            assert_eq!(path_sccs.components.len(), DEPTH as usize);
            assert!(path_sccs.is_acyclic());

            let cycle = GraphProjection::from_edges(
                ProjectionKind::External,
                nodes,
                (0..DEPTH).map(|node| (NodeId::new(node), NodeId::new((node + 1) % DEPTH), 1.0)),
            );
            let cycle_sccs = projection_sccs(&cycle);
            assert_eq!(cycle_sccs.components.len(), 1);
            assert_eq!(cycle_sccs.components[0].nodes.len(), DEPTH as usize);
            assert!(cycle_sccs.components[0].cyclic);
        })
        .expect("the operating system can create the small-stack test thread")
        .join()
        .expect("the small-stack semantic gate completes without a panic");
}

#[test]
fn lifecycle_100_000_build_rebuild_transpose_and_drop_is_stack_safe() {
    const NODE_COUNT: u32 = 100_000;
    std::thread::Builder::new()
        .name("compositional-laws-lifecycle".into())
        .stack_size(256 * 1024)
        .spawn(|| {
            let projection = GraphProjection::from_edges(
                ProjectionKind::External,
                (0..NODE_COUNT).map(NodeId::new),
                (0..NODE_COUNT - 1).map(|node| (NodeId::new(node), NodeId::new(node + 1), 1.0)),
            );
            assert!(projection.is_well_formed());
            let rebuilt = projection.transpose().transpose();
            assert_eq!(rebuilt, projection);
            let decomposition = projection_sccs(&rebuilt);
            assert_eq!(decomposition.node_count(), NODE_COUNT as usize);
            assert_eq!(decomposition.components.len(), NODE_COUNT as usize);
            assert_eq!(
                decomposition.condensation_edges.len(),
                NODE_COUNT as usize - 1
            );
        })
        .expect("the operating system can create the lifecycle test thread")
        .join()
        .expect("the small-stack lifecycle gate completes and drops iteratively");
}
