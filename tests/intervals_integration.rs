//! Public-API integration coverage for interval value-range analysis.

use libcpg::{
    value_ranges, Bound, CfgEdgeKind, CodePropertyGraph, CpgEdgeKind, CpgNode, CpgNodeKind,
    Interval, IntervalEnv, Language, LiteralKind, MethodSignature, NodeId, SourceRange, Visibility,
};

fn node(id: u32, kind: CpgNodeKind) -> CpgNode {
    CpgNode::new(NodeId::new(id), kind, SourceRange::default())
}

fn connect_ast(cpg: &mut CodePropertyGraph, parent: NodeId, child: NodeId) {
    cpg.connect(parent, child, CpgEdgeKind::AstChild);
}

fn connect_cfg(cpg: &mut CodePropertyGraph, source: NodeId, target: NodeId, kind: CfgEdgeKind) {
    cpg.connect(source, target, CpgEdgeKind::ControlFlow(kind));
}

#[test]
fn public_value_range_pipeline_refines_both_integer_comparison_edges() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = cpg.add_node_with_id(node(
        10,
        CpgNodeKind::Function {
            signature: MethodSignature {
                name: "classify".into(),
                params: Default::default(),
                return_type: None,
                is_static: false,
                is_async: false,
                visibility: Visibility::Private,
            },
        },
    ));
    let parameter = cpg.add_node_with_id(node(
        20,
        CpgNodeKind::Parameter {
            name: "x".into(),
            param_type: None,
            is_variadic: false,
        },
    ));
    let comparison = cpg.add_node_with_id(node(
        30,
        CpgNodeKind::BinaryOp {
            operator: "<".into(),
        },
    ));
    let use_site = cpg.add_node_with_id(node(
        31,
        CpgNodeKind::Identifier {
            name: "x".into(),
            definition: None,
        },
    ));
    let constant = cpg.add_node_with_id(node(
        32,
        CpgNodeKind::Literal {
            kind: LiteralKind::Integer(10),
        },
    ));
    let less = cpg.add_node_with_id(node(40, CpgNodeKind::Return));
    let greater_or_equal = cpg.add_node_with_id(node(50, CpgNodeKind::Return));
    for child in [parameter, comparison, less, greater_or_equal] {
        connect_ast(&mut cpg, function, child);
    }
    connect_ast(&mut cpg, comparison, use_site);
    connect_ast(&mut cpg, comparison, constant);
    connect_cfg(&mut cpg, function, parameter, CfgEdgeKind::Sequential);
    connect_cfg(&mut cpg, parameter, comparison, CfgEdgeKind::Sequential);
    connect_cfg(&mut cpg, comparison, less, CfgEdgeKind::ConditionalTrue);
    connect_cfg(
        &mut cpg,
        comparison,
        greater_or_equal,
        CfgEdgeKind::ConditionalFalse,
    );

    let graph_size = (cpg.node_count(), cpg.edge_count());
    let report = value_ranges(&cpg, function).expect("public interval analysis");
    assert_eq!(
        report.interval_at(less, "x"),
        Some(Interval {
            lo: Bound::NegInf,
            hi: Bound::Finite(9),
        })
    );
    assert_eq!(
        report.interval_at(greater_or_equal, "x"),
        Some(Interval {
            lo: Bound::Finite(10),
            hi: Bound::PosInf,
        })
    );
    assert_eq!((cpg.node_count(), cpg.edge_count()), graph_size);
}

#[test]
fn public_interval_domain_exposes_canonical_lattice_operations() {
    let left = Interval::new(Bound::Finite(-3), Bound::Finite(4)).expect("ordered interval");
    let right = Interval::new(Bound::Finite(2), Bound::Finite(9)).expect("ordered interval");
    assert_eq!(
        left.hull(right),
        Interval {
            lo: Bound::Finite(-3),
            hi: Bound::Finite(9),
        }
    );
    assert_eq!(
        left.meet(right),
        Interval::new(Bound::Finite(2), Bound::Finite(4))
    );
    assert!(left.widen(right).contains(i128::MAX));
    assert!(Interval::top().narrow(left).is_subset_of(Interval::top()));

    let environment = IntervalEnv::from_vars([(3, right), (1, left)]);
    assert_eq!(environment.vars[0], (1, left));
    assert_eq!(environment.get(3), Some(right));
}

#[cfg(feature = "lang-rust")]
#[test]
fn parser_built_rust_comparison_refines_the_public_report() {
    use libcpg::{CpgBuilder, TreeSitterCpgBuilder};

    let source = r#"
        fn classify(x: i64) -> i64 {
            if x < 5 {
                return 1;
            } else {
                return 2;
            }
        }
    "#;
    let cpg = TreeSitterCpgBuilder::new()
        .build(source, Language::Rust)
        .expect("build parser-backed CPG");
    let function = cpg.functions().next().expect("classify function").id;
    let report = value_ranges(&cpg, function).expect("parser-backed interval analysis");
    let comparison = cpg
        .nodes()
        .find(|node| matches!(&node.kind, CpgNodeKind::BinaryOp { operator } if &**operator == "<"))
        .expect("comparison")
        .id;
    let successors = cpg.cfg_successors(comparison);
    let true_target = successors
        .iter()
        .find_map(|(target, kind)| (*kind == CfgEdgeKind::ConditionalTrue).then_some(*target))
        .expect("true branch target");
    let false_target = successors
        .iter()
        .find_map(|(target, kind)| (*kind == CfgEdgeKind::ConditionalFalse).then_some(*target))
        .expect("false branch target");

    assert_eq!(
        report.interval_at(true_target, "x"),
        Some(Interval {
            lo: Bound::NegInf,
            hi: Bound::Finite(4),
        })
    );
    assert_eq!(
        report.interval_at(false_target, "x"),
        Some(Interval {
            lo: Bound::Finite(5),
            hi: Bound::PosInf,
        })
    );
}
