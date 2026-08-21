use proptest::prelude::*;

use super::*;
use crate::testutil::{arb_well_formed_cpg, node_ids};
use crate::{
    CfgEdgeKind, CpgEdgeKind, CpgNode, Language, LiteralKind, MethodSignature, ScopeId,
    SourceRange, Visibility,
};

#[derive(Clone)]
struct NodeSpec {
    id: NodeId,
    kind: CpgNodeKind,
    parent: Option<NodeId>,
}

#[derive(Clone, Copy)]
struct CfgSpec {
    source: NodeId,
    target: NodeId,
    kind: CfgEdgeKind,
}

struct CountedFixture {
    cpg: CodePropertyGraph,
    function: NodeId,
    headers: Vec<NodeId>,
    bounds: Vec<NodeId>,
}

fn function_kind(name: &str) -> CpgNodeKind {
    CpgNodeKind::Function {
        signature: MethodSignature {
            name: name.into(),
            params: Default::default(),
            return_type: None,
            is_static: false,
            is_async: false,
            visibility: Visibility::Private,
        },
    }
}

fn block() -> CpgNodeKind {
    CpgNodeKind::Block {
        scope: ScopeId::GLOBAL,
    }
}

fn variable(name: &str) -> CpgNodeKind {
    CpgNodeKind::Variable {
        name: name.into(),
        var_type: None,
        scope: ScopeId::GLOBAL,
        is_mutable: false,
    }
}

fn identifier(name: &str) -> CpgNodeKind {
    CpgNodeKind::Identifier {
        name: name.into(),
        definition: None,
    }
}

fn binary(operator: &str) -> CpgNodeKind {
    CpgNodeKind::BinaryOp {
        operator: operator.into(),
    }
}

fn integer(value: i64) -> CpgNodeKind {
    CpgNodeKind::Literal {
        kind: LiteralKind::Integer(value),
    }
}

fn push_node(nodes: &mut Vec<NodeSpec>, kind: CpgNodeKind, parent: Option<NodeId>) -> NodeId {
    let id = NodeId::new(u32::try_from(nodes.len()).expect("small test fixture"));
    nodes.push(NodeSpec { id, kind, parent });
    id
}

fn counted_nested_fixture(depth: usize, reverse_storage: bool) -> CountedFixture {
    assert!(depth > 0);
    let mut nodes = Vec::new();
    let function = push_node(&mut nodes, function_kind("counted"), None);
    let mut parent = function;
    let mut headers = Vec::with_capacity(depth);
    let mut bounds = Vec::with_capacity(depth);
    let mut latches = Vec::with_capacity(depth);

    for level in 0..depth {
        let header = push_node(&mut nodes, CpgNodeKind::For, Some(parent));
        let binder = format!("i{level}");
        push_node(&mut nodes, variable(&binder), Some(header));
        let range = push_node(&mut nodes, binary(".."), Some(header));
        push_node(&mut nodes, integer(0), Some(range));
        let bound = push_node(&mut nodes, integer(100 + level as i64), Some(range));
        let body = push_node(&mut nodes, block(), Some(header));
        let latch = push_node(
            &mut nodes,
            CpgNodeKind::Unknown {
                kind: format!("latch_{level}").into(),
            },
            Some(body),
        );
        headers.push(header);
        bounds.push(bound);
        latches.push(latch);
        parent = body;
    }
    let exit = push_node(&mut nodes, CpgNodeKind::Return, Some(function));

    let mut cfg = Vec::with_capacity(depth * 3 + 1);
    cfg.push(CfgSpec {
        source: function,
        target: headers[0],
        kind: CfgEdgeKind::Sequential,
    });
    for level in 0..depth {
        cfg.push(CfgSpec {
            source: headers[level],
            target: headers.get(level + 1).copied().unwrap_or(latches[level]),
            kind: CfgEdgeKind::ConditionalTrue,
        });
        cfg.push(CfgSpec {
            source: headers[level],
            target: if level == 0 { exit } else { latches[level - 1] },
            kind: CfgEdgeKind::LoopExit,
        });
        cfg.push(CfgSpec {
            source: latches[level],
            target: headers[level],
            kind: CfgEdgeKind::LoopBack,
        });
    }

    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let node_order: Box<dyn Iterator<Item = &NodeSpec>> = if reverse_storage {
        Box::new(nodes.iter().rev())
    } else {
        Box::new(nodes.iter())
    };
    for spec in node_order {
        cpg.add_node_with_id(CpgNode::new(
            spec.id,
            spec.kind.clone(),
            SourceRange::default(),
        ));
    }
    for spec in &nodes {
        let Some(parent) = spec.parent else {
            continue;
        };
        cpg.connect(parent, spec.id, CpgEdgeKind::AstChild)
            .expect("fixture AST endpoints");
    }
    let cfg_order: Box<dyn Iterator<Item = &CfgSpec>> = if reverse_storage {
        Box::new(cfg.iter().rev())
    } else {
        Box::new(cfg.iter())
    };
    for edge in cfg_order {
        cpg.connect(
            edge.source,
            edge.target,
            CpgEdgeKind::ControlFlow(edge.kind),
        )
        .expect("fixture CFG endpoints");
    }
    cpg.add_cfg_entry(function);
    cpg.add_cfg_exit(exit);

    CountedFixture {
        cpg,
        function,
        headers,
        bounds,
    }
}

fn add_node(cpg: &mut CodePropertyGraph, kind: CpgNodeKind) -> NodeId {
    cpg.add_node(CpgNode::new(NodeId::new(0), kind, SourceRange::default()))
}

fn connect_ast(cpg: &mut CodePropertyGraph, parent: NodeId, child: NodeId) {
    cpg.connect(parent, child, CpgEdgeKind::AstChild)
        .expect("fixture AST endpoints");
}

fn connect_cfg(cpg: &mut CodePropertyGraph, source: NodeId, target: NodeId, kind: CfgEdgeKind) {
    cpg.connect(source, target, CpgEdgeKind::ControlFlow(kind))
        .expect("fixture CFG endpoints");
}

fn unknown_while_fixture() -> (CodePropertyGraph, NodeId, NodeId) {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = add_node(&mut cpg, function_kind("unknown"));
    let header = add_node(&mut cpg, CpgNodeKind::While);
    let condition = add_node(
        &mut cpg,
        CpgNodeKind::Literal {
            kind: LiteralKind::Bool(true),
        },
    );
    let body = add_node(&mut cpg, block());
    let latch = add_node(
        &mut cpg,
        CpgNodeKind::Unknown {
            kind: "latch".into(),
        },
    );
    let exit = add_node(&mut cpg, CpgNodeKind::Return);
    for child in [header, exit] {
        connect_ast(&mut cpg, function, child);
    }
    for child in [condition, body] {
        connect_ast(&mut cpg, header, child);
    }
    connect_ast(&mut cpg, body, latch);
    connect_cfg(&mut cpg, function, header, CfgEdgeKind::Sequential);
    connect_cfg(&mut cpg, header, latch, CfgEdgeKind::ConditionalTrue);
    connect_cfg(&mut cpg, latch, header, CfgEdgeKind::LoopBack);
    connect_cfg(&mut cpg, header, exit, CfgEdgeKind::LoopExit);
    cpg.add_cfg_entry(function);
    cpg.add_cfg_exit(exit);
    (cpg, function, header)
}

fn irreducible_fixture() -> (CodePropertyGraph, NodeId) {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = add_node(&mut cpg, function_kind("irreducible"));
    let left = add_node(&mut cpg, block());
    let right = add_node(&mut cpg, block());
    let exit = add_node(&mut cpg, CpgNodeKind::Return);
    for child in [left, right, exit] {
        connect_ast(&mut cpg, function, child);
    }
    connect_cfg(&mut cpg, function, left, CfgEdgeKind::ConditionalTrue);
    connect_cfg(&mut cpg, function, right, CfgEdgeKind::ConditionalFalse);
    connect_cfg(&mut cpg, left, right, CfgEdgeKind::Sequential);
    connect_cfg(&mut cpg, right, left, CfgEdgeKind::LoopBack);
    connect_cfg(&mut cpg, left, exit, CfgEdgeKind::LoopExit);
    connect_cfg(&mut cpg, right, exit, CfgEdgeKind::LoopExit);
    cpg.add_cfg_entry(function);
    cpg.add_cfg_exit(exit);
    (cpg, function)
}

fn assert_same_estimate(left: &ComplexityEstimate, right: &ComplexityEstimate) {
    assert_eq!(left.class, right.class);
    assert_eq!(left.confidence.to_bits(), right.confidence.to_bits());
    assert_eq!(left.justification, right.justification);
}

#[test]
fn counted_nests_use_exact_forest_depth_and_complete_node_evidence() {
    let analyzer = ComplexityAnalyzer::new();
    for depth in 1..=5 {
        let fixture = counted_nested_fixture(depth, false);
        let before = (fixture.cpg.node_count(), fixture.cpg.edge_count());
        let heuristic = analyzer.estimate_time_complexity(&fixture.cpg, fixture.function);
        let evidence =
            analyzer.estimate_time_complexity_with_evidence(&fixture.cpg, fixture.function);
        assert_eq!(evidence.class, complexity_for_polynomial_degree(depth));
        assert_eq!(evidence.confidence, 1.0);
        assert!(evidence.confidence > heuristic.confidence);
        for (level, (&header, &bound)) in fixture.headers.iter().zip(&fixture.bounds).enumerate() {
            assert!(evidence.justification.contains(&format!("{header:?}")));
            assert!(evidence.justification.contains(&format!("{bound:?}")));
            assert!(evidence.justification.contains(&format!("i{level}")));
        }
        assert!(evidence
            .justification
            .contains("all reducible trips counted"));
        assert_eq!(
            (fixture.cpg.node_count(), fixture.cpg.edge_count()),
            before,
            "the evidence consumer is out-of-graph"
        );
    }
}

#[test]
fn unknown_trip_keeps_the_legacy_heuristic_confidence() {
    let (cpg, function, header) = unknown_while_fixture();
    let analyzer = ComplexityAnalyzer::new();
    let heuristic = analyzer.estimate_time_complexity(&cpg, function);
    let evidence = analyzer.estimate_time_complexity_with_evidence(&cpg, function);
    assert_eq!(evidence.class, ComplexityClass::Linear);
    assert_eq!(
        evidence.confidence.to_bits(),
        heuristic.confidence.to_bits()
    );
    assert_eq!(evidence.confidence, 0.6);
    assert!(evidence.justification.contains(&format!("{header:?}")));
    assert!(evidence.justification.contains("trip evidence unknown"));
    assert!(evidence
        .justification
        .contains("unknown-trip heuristic confidence ceiling"));
}

#[test]
fn irreducible_region_is_one_low_confidence_linear_factor() {
    let (cpg, function) = irreducible_fixture();
    let forest = loop_forest(&cpg, function).expect("irreducible forest");
    assert_eq!(forest.loops.len(), 1);
    assert!(forest.loops[0].irreducible);
    let evidence = ComplexityAnalyzer::new().estimate_time_complexity_with_evidence(&cpg, function);
    assert_eq!(evidence.class, ComplexityClass::Linear);
    assert_eq!(evidence.confidence, IRREDUCIBLE_CONFIDENCE);
    assert!(evidence.justification.contains("irreducible region"));
    assert!(evidence
        .justification
        .contains(&format!("{:?}", forest.loops[0].header)));
}

#[test]
fn cfg_free_and_invalid_inputs_return_the_exact_legacy_estimate() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = add_node(&mut cpg, function_kind("legacy"));
    let loop_node = add_node(&mut cpg, CpgNodeKind::For);
    connect_ast(&mut cpg, function, loop_node);
    let analyzer = ComplexityAnalyzer::new();
    let heuristic = analyzer.estimate_time_complexity(&cpg, function);
    let evidence = analyzer.estimate_time_complexity_with_evidence(&cpg, function);
    assert_same_estimate(&heuristic, &evidence);

    let absent = NodeId::new(u32::MAX);
    let heuristic = analyzer.estimate_time_complexity(&cpg, absent);
    let evidence = analyzer.estimate_time_complexity_with_evidence(&cpg, absent);
    assert_same_estimate(&heuristic, &evidence);
}

#[test]
fn dominant_recursion_retains_subordinate_loop_evidence() {
    let mut fixture = counted_nested_fixture(1, false);
    let call = add_node(
        &mut fixture.cpg,
        CpgNodeKind::Call {
            target: None,
            is_method: false,
        },
    );
    let callee = add_node(&mut fixture.cpg, identifier("counted"));
    let body = fixture
        .cpg
        .ast_children(fixture.headers[0])
        .iter()
        .copied()
        .find(|node| {
            matches!(
                fixture.cpg.node(*node).map(|node| &node.kind),
                Some(CpgNodeKind::Block { .. })
            )
        })
        .expect("range-loop body");
    connect_ast(&mut fixture.cpg, body, call);
    connect_ast(&mut fixture.cpg, call, callee);
    let evidence = ComplexityAnalyzer::new()
        .estimate_time_complexity_with_evidence(&fixture.cpg, fixture.function);
    assert_eq!(evidence.class, ComplexityClass::Linear);
    assert!(evidence
        .justification
        .contains("Recursion without clear base case"));
    assert!(evidence.justification.contains("subordinate loop evidence"));
    assert!(evidence
        .justification
        .contains(&format!("{:?}", fixture.bounds[0])));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn evidence_reports_are_deterministic_under_storage_permutations(
        depth in 1usize..=8,
        reverse_storage in any::<bool>(),
    ) {
        let canonical = counted_nested_fixture(depth, false);
        let permuted = counted_nested_fixture(depth, reverse_storage);
        let analyzer = ComplexityAnalyzer::new();
        let left = analyzer.estimate_time_complexity_with_evidence(
            &canonical.cpg,
            canonical.function,
        );
        let right = analyzer.estimate_time_complexity_with_evidence(
            &permuted.cpg,
            permuted.function,
        );
        prop_assert_eq!(left.class, right.class);
        prop_assert_eq!(left.confidence.to_bits(), right.confidence.to_bits());
        prop_assert_eq!(left.justification, right.justification);
        prop_assert_eq!(left.class, complexity_for_polynomial_degree(depth));
    }

    #[test]
    fn every_cfg_free_generated_function_falls_back_exactly(
        cpg in arb_well_formed_cpg(),
    ) {
        let function = node_ids(&cpg)
            .into_iter()
            .find(|node| matches!(cpg.node(*node).map(|node| &node.kind), Some(CpgNodeKind::Function { .. })))
            .expect("generated function root");
        let analyzer = ComplexityAnalyzer::new();
        let heuristic = analyzer.estimate_time_complexity(&cpg, function);
        let evidence = analyzer.estimate_time_complexity_with_evidence(&cpg, function);
        prop_assert_eq!(heuristic.class, evidence.class);
        prop_assert_eq!(heuristic.confidence.to_bits(), evidence.confidence.to_bits());
        prop_assert_eq!(heuristic.justification, evidence.justification);
    }
}
