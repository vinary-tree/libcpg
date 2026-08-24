//! Public integration coverage for relational abstract domains.

use libcpg::{
    block_cfg, solve, BlockCfg, BlockEdge, BlockId, Bound, CfgEdgeKind, CodePropertyGraph,
    Confluence, CpgEdgeKind, CpgNode, CpgNodeKind, DataflowAnalysis, Direction, Interval,
    IntervalEnv, IntervalOctagonReduction, Language, MethodSignature, NodeId, Octagon,
    OctagonConstraint, OctagonTerm, ReducedProduct, SolverConfig, SourceRange, Visibility,
};

#[cfg(feature = "abstract-domains")]
use libcpg::{Lattice, LinearConstraint, Polyhedron};

type RelationalState = ReducedProduct<IntervalEnv, Octagon, IntervalOctagonReduction>;

fn node(id: u32, kind: CpgNodeKind) -> CpgNode {
    CpgNode::new(NodeId::new(id), kind, SourceRange::default())
}

fn connect_ast(cpg: &mut CodePropertyGraph, parent: NodeId, child: NodeId) {
    cpg.connect(parent, child, CpgEdgeKind::AstChild);
}

fn connect_cfg(cpg: &mut CodePropertyGraph, source: NodeId, target: NodeId, kind: CfgEdgeKind) {
    cpg.connect(source, target, CpgEdgeKind::ControlFlow(kind));
}

fn equality_octagon() -> Octagon {
    let mut octagon = Octagon::top(2);
    octagon
        .assume_all([
            OctagonConstraint::binary(OctagonTerm::positive(0), OctagonTerm::negative(1), 0),
            OctagonConstraint::binary(OctagonTerm::positive(1), OctagonTerm::negative(0), 0),
        ])
        .expect("close x equals y");
    octagon
}

struct BranchRelation;

impl DataflowAnalysis for BranchRelation {
    type Domain = RelationalState;

    const NAME: &'static str = "public-branch-relation";
    const DIRECTION: Direction = Direction::Forward;
    const CONFLUENCE: Confluence = Confluence::May;

    fn bottom(&self, _cfg: &BlockCfg) -> Self::Domain {
        RelationalState::new(IntervalEnv::bottom(), Octagon::bottom(2))
    }

    fn boundary(&self, _cfg: &BlockCfg) -> Self::Domain {
        RelationalState::new(
            IntervalEnv::from_vars([(0, Interval::top()), (1, Interval::top())]),
            equality_octagon(),
        )
    }

    fn transfer_block(
        &self,
        _cpg: &CodePropertyGraph,
        _cfg: &BlockCfg,
        _block: BlockId,
        _state: &mut Self::Domain,
    ) {
    }

    fn transfer_edge(
        &self,
        _cpg: &CodePropertyGraph,
        _cfg: &BlockCfg,
        edge: &BlockEdge,
        state: &Self::Domain,
    ) -> Option<Self::Domain> {
        let value = match edge.kind {
            CfgEdgeKind::ConditionalTrue => 0,
            CfgEdgeKind::ConditionalFalse => 4,
            _ => return None,
        };
        let mut transferred = state.clone();
        transferred.transform(|intervals, _octagon| {
            intervals.set(0, Interval::singleton(value));
        });
        Some(transferred)
    }
}

#[test]
fn public_reduced_product_is_reduced_by_the_real_cfg_merge() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = cpg.add_node_with_id(node(
        10,
        CpgNodeKind::Function {
            signature: Box::new(MethodSignature {
                name: "merge_relation".into(),
                params: Default::default(),
                return_type: None,
                is_static: false,
                is_async: false,
                visibility: Visibility::Private,
            }),
        },
    ));
    let condition = cpg.add_node_with_id(node(
        20,
        CpgNodeKind::Identifier {
            name: "condition".into(),
            definition: None,
        },
    ));
    let left = cpg.add_node_with_id(node(
        30,
        CpgNodeKind::Unknown {
            kind: "left-branch".into(),
        },
    ));
    let right = cpg.add_node_with_id(node(
        40,
        CpgNodeKind::Unknown {
            kind: "right-branch".into(),
        },
    ));
    let merge = cpg.add_node_with_id(node(50, CpgNodeKind::Return));
    for child in [condition, left, right, merge] {
        connect_ast(&mut cpg, function, child);
    }
    connect_cfg(&mut cpg, function, condition, CfgEdgeKind::Sequential);
    connect_cfg(&mut cpg, condition, left, CfgEdgeKind::ConditionalTrue);
    connect_cfg(&mut cpg, condition, right, CfgEdgeKind::ConditionalFalse);
    connect_cfg(&mut cpg, left, merge, CfgEdgeKind::Sequential);
    connect_cfg(&mut cpg, right, merge, CfgEdgeKind::Sequential);

    let graph_size = (cpg.node_count(), cpg.edge_count());
    let cfg = block_cfg(&cpg, function).expect("build public relational block CFG");
    let solution = solve(&cpg, &cfg, &BranchRelation, &SolverConfig::default(), None)
        .expect("solve public reduced-product analysis");
    let merge_block = cfg.block_of_node[&merge];
    let merged = &solution.before[merge_block.as_u32() as usize];
    let zero_to_four = Interval::new(Bound::Finite(0), Bound::Finite(4));
    assert_eq!(merged.first().get(0), zero_to_four);
    assert_eq!(merged.first().get(1), zero_to_four);
    assert!(merged.second().contains(&[0, 0]));
    assert!(merged.second().contains(&[4, 4]));
    assert!(!merged.second().contains(&[0, 4]));
    assert!(solution.visits.iter().all(|visits| *visits < 256));

    assert!(block_cfg(&cpg, NodeId::new(999_999)).is_err());
    assert_eq!((cpg.node_count(), cpg.edge_count()), graph_size);
}

#[cfg(feature = "abstract-domains")]
#[test]
fn public_exact_polyhedron_composes_in_the_reduced_product() {
    let left = Polyhedron::integer_point(&[0, 0]).expect("left exact point");
    let right = Polyhedron::integer_point(&[4, 4]).expect("right exact point");
    let hull = left
        .closed_convex_hull(&right)
        .expect("exact public closed convex hull");
    assert!(hull.contains_integers(&[2, 2]));
    assert!(!hull.contains_integers(&[2, 1]));

    let half_space =
        Polyhedron::from_constraints(2, [LinearConstraint::from_integers([(0, 1), (1, -1)], 0)])
            .expect("public x minus y half-space");
    let precise: ReducedProduct<_, _> = ReducedProduct::new(hull, equality_octagon());
    let components: ReducedProduct<_, _> = ReducedProduct::new(half_space, Octagon::top(2));
    assert!(precise.leq(&components));
    assert!(!components.leq(&precise));
}
