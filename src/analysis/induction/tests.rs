use std::collections::BTreeSet;

use proptest::prelude::*;

use super::*;
use crate::{
    loop_forest, CfgEdgeKind, CpgEdgeKind, CpgNode, Language, MethodSignature, ScopeId,
    SourceRange, Visibility,
};

const FUNCTION: NodeId = NodeId(0);
const INITIAL_I: NodeId = NodeId(1);
const WHILE_HEADER: NodeId = NodeId(7);
const COMPARISON: NodeId = NodeId(8);
const BOUND_USE: NodeId = NodeId(10);
const INVARIANT_ASSIGNMENT: NodeId = NodeId(12);
const DERIVED_UPDATE: NodeId = NodeId(17);
const BASIC_UPDATE: NodeId = NodeId(24);

struct ExplicitFixture {
    cpg: CodePropertyGraph,
    function: NodeId,
}

fn node(id: u32, kind: CpgNodeKind) -> CpgNode {
    CpgNode::new(NodeId::new(id), kind, SourceRange::default())
}

fn function(name: &str) -> CpgNodeKind {
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
        is_mutable: true,
    }
}

fn identifier(name: &str) -> CpgNodeKind {
    CpgNodeKind::Identifier {
        name: name.into(),
        definition: None,
    }
}

fn integer(value: i64) -> CpgNodeKind {
    CpgNodeKind::Literal {
        kind: LiteralKind::Integer(value),
    }
}

fn binary(operator: &str) -> CpgNodeKind {
    CpgNodeKind::BinaryOp {
        operator: operator.into(),
    }
}

fn assignment(operator: &str) -> CpgNodeKind {
    CpgNodeKind::Assignment {
        operator: operator.into(),
    }
}

fn connect_ast(cpg: &mut CodePropertyGraph, parent: NodeId, child: NodeId) {
    cpg.connect(parent, child, CpgEdgeKind::AstChild)
        .expect("fixture AST endpoints exist");
    cpg.node_mut(child).expect("fixture child exists").parent = Some(parent);
}

fn connect_cfg(cpg: &mut CodePropertyGraph, source: NodeId, target: NodeId, kind: CfgEdgeKind) {
    cpg.connect(source, target, CpgEdgeKind::ControlFlow(kind))
        .expect("fixture CFG endpoints exist");
}

fn connect_def_use(cpg: &mut CodePropertyGraph, definition: NodeId, use_site: NodeId) {
    cpg.connect(
        definition,
        use_site,
        CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse),
    )
    .expect("fixture DFG endpoints exist");
}

fn explicit_fixture(step: i64, coefficient: i64, reverse_guard: bool) -> ExplicitFixture {
    let increasing = step > 0;
    let comparison_operator = match (increasing, reverse_guard) {
        (true, false) => "<",
        (true, true) => ">",
        (false, false) => ">",
        (false, true) => "<",
    };
    let initial = if increasing { 0 } else { 100 };
    let bound = if increasing { 100 } else { 0 };

    let mut cpg = CodePropertyGraph::new(Language::Rust);
    for (id, kind) in [
        (0, function("counted")),
        (1, variable("i")),
        (2, identifier("i")),
        (3, integer(initial)),
        (4, variable("j")),
        (5, identifier("j")),
        (6, integer(0)),
        (7, CpgNodeKind::While),
        (8, binary(comparison_operator)),
        (9, identifier("i")),
        (10, identifier("limit")),
        (11, block()),
        (12, assignment("=")),
        (13, identifier("x")),
        (14, binary("+")),
        (15, identifier("limit")),
        (16, integer(2)),
        (17, assignment("=")),
        (18, identifier("j")),
        (19, binary("+")),
        (20, binary("*")),
        (21, integer(coefficient)),
        (22, identifier("i")),
        (23, integer(3)),
        (24, assignment("=")),
        (25, identifier("i")),
        (26, binary("+")),
        (27, identifier("i")),
        (28, integer(step)),
        (29, CpgNodeKind::Return),
        (30, variable("limit")),
        (31, identifier("limit")),
        (32, integer(bound)),
    ] {
        cpg.add_node_with_id(node(id, kind));
    }

    for (parent, child) in [(30, 31), (30, 32), (1, 2), (1, 3), (4, 5), (4, 6)] {
        connect_ast(&mut cpg, NodeId::new(parent), NodeId::new(child));
    }
    for child in [30, 1, 4, 7, 29] {
        connect_ast(&mut cpg, FUNCTION, NodeId::new(child));
    }
    if reverse_guard {
        connect_ast(&mut cpg, COMPARISON, BOUND_USE);
        connect_ast(&mut cpg, COMPARISON, NodeId::new(9));
    } else {
        connect_ast(&mut cpg, COMPARISON, NodeId::new(9));
        connect_ast(&mut cpg, COMPARISON, BOUND_USE);
    }
    connect_ast(&mut cpg, WHILE_HEADER, COMPARISON);
    connect_ast(&mut cpg, WHILE_HEADER, NodeId::new(11));
    for child in [12, 17, 24] {
        connect_ast(&mut cpg, NodeId::new(11), NodeId::new(child));
    }
    connect_ast(&mut cpg, INVARIANT_ASSIGNMENT, NodeId::new(13));
    connect_ast(&mut cpg, INVARIANT_ASSIGNMENT, NodeId::new(14));
    connect_ast(&mut cpg, NodeId::new(14), NodeId::new(15));
    connect_ast(&mut cpg, NodeId::new(14), NodeId::new(16));
    connect_ast(&mut cpg, DERIVED_UPDATE, NodeId::new(18));
    connect_ast(&mut cpg, DERIVED_UPDATE, NodeId::new(19));
    connect_ast(&mut cpg, NodeId::new(19), NodeId::new(20));
    connect_ast(&mut cpg, NodeId::new(19), NodeId::new(23));
    connect_ast(&mut cpg, NodeId::new(20), NodeId::new(21));
    connect_ast(&mut cpg, NodeId::new(20), NodeId::new(22));
    connect_ast(&mut cpg, BASIC_UPDATE, NodeId::new(25));
    connect_ast(&mut cpg, BASIC_UPDATE, NodeId::new(26));
    connect_ast(&mut cpg, NodeId::new(26), NodeId::new(27));
    connect_ast(&mut cpg, NodeId::new(26), NodeId::new(28));

    for (source, target, kind) in [
        (0, 30, CfgEdgeKind::Sequential),
        (30, 1, CfgEdgeKind::Sequential),
        (1, 4, CfgEdgeKind::Sequential),
        (4, 7, CfgEdgeKind::Sequential),
        (7, 12, CfgEdgeKind::ConditionalTrue),
        (12, 17, CfgEdgeKind::Sequential),
        (17, 24, CfgEdgeKind::Sequential),
        (24, 7, CfgEdgeKind::LoopBack),
        (7, 29, CfgEdgeKind::LoopExit),
    ] {
        connect_cfg(&mut cpg, NodeId::new(source), NodeId::new(target), kind);
    }
    cpg.add_cfg_entry(FUNCTION);
    cpg.add_cfg_exit(NodeId::new(29));

    for use_site in [10, 15] {
        connect_def_use(&mut cpg, NodeId::new(30), NodeId::new(use_site));
    }
    for definition in [INITIAL_I, BASIC_UPDATE] {
        for use_site in [9, 22, 27] {
            connect_def_use(&mut cpg, definition, NodeId::new(use_site));
        }
    }

    ExplicitFixture {
        cpg,
        function: FUNCTION,
    }
}

fn range_fixture() -> ExplicitFixture {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    for (id, kind) in [
        (0, function("range")),
        (1, CpgNodeKind::For),
        (2, block()),
        (3, variable("i")),
        (4, binary("..")),
        (5, integer(0)),
        (6, identifier("x")),
        (7, identifier("i")),
        (8, variable("x")),
        (9, CpgNodeKind::Return),
        (10, identifier("x")),
        (11, integer(20)),
    ] {
        cpg.add_node_with_id(node(id, kind));
    }
    connect_ast(&mut cpg, NodeId::new(8), NodeId::new(10));
    connect_ast(&mut cpg, NodeId::new(8), NodeId::new(11));
    for child in [8, 1, 9] {
        connect_ast(&mut cpg, FUNCTION, NodeId::new(child));
    }
    for child in [3, 4, 2] {
        connect_ast(&mut cpg, NodeId::new(1), NodeId::new(child));
    }
    connect_ast(&mut cpg, NodeId::new(4), NodeId::new(5));
    connect_ast(&mut cpg, NodeId::new(4), NodeId::new(6));
    connect_ast(&mut cpg, NodeId::new(2), NodeId::new(7));

    for (source, target, kind) in [
        (0, 8, CfgEdgeKind::Sequential),
        (8, 1, CfgEdgeKind::Sequential),
        (1, 2, CfgEdgeKind::ConditionalTrue),
        (2, 1, CfgEdgeKind::LoopBack),
        (1, 9, CfgEdgeKind::LoopExit),
    ] {
        connect_cfg(&mut cpg, NodeId::new(source), NodeId::new(target), kind);
    }
    connect_def_use(&mut cpg, NodeId::new(8), NodeId::new(6));
    connect_def_use(&mut cpg, NodeId::new(3), NodeId::new(7));
    cpg.add_cfg_entry(FUNCTION);
    cpg.add_cfg_exit(NodeId::new(9));
    ExplicitFixture {
        cpg,
        function: FUNCTION,
    }
}

fn hoist_fixture(duplicate_definition: bool) -> ExplicitFixture {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    for (id, kind) in [
        (0, function("hoist")),
        (1, assignment("=")),
        (2, block()),
        (3, variable("limit")),
        (4, identifier("x")),
        (5, binary("+")),
        (6, identifier("limit")),
        (7, integer(1)),
        (8, identifier("x")),
        (9, CpgNodeKind::Return),
    ] {
        cpg.add_node_with_id(node(id, kind));
    }
    if duplicate_definition {
        for (id, kind) in [
            (10, assignment("=")),
            (11, identifier("x")),
            (12, integer(2)),
        ] {
            cpg.add_node_with_id(node(id, kind));
        }
    }
    for child in [3, 1, 2, 9] {
        connect_ast(&mut cpg, FUNCTION, NodeId::new(child));
    }
    connect_ast(&mut cpg, NodeId::new(1), NodeId::new(4));
    connect_ast(&mut cpg, NodeId::new(1), NodeId::new(5));
    connect_ast(&mut cpg, NodeId::new(5), NodeId::new(6));
    connect_ast(&mut cpg, NodeId::new(5), NodeId::new(7));
    if duplicate_definition {
        connect_ast(&mut cpg, NodeId::new(2), NodeId::new(10));
        connect_ast(&mut cpg, NodeId::new(10), NodeId::new(11));
        connect_ast(&mut cpg, NodeId::new(10), NodeId::new(12));
    }
    connect_ast(&mut cpg, NodeId::new(2), NodeId::new(8));
    for (source, target, kind) in [
        (0, 3, CfgEdgeKind::Sequential),
        (3, 1, CfgEdgeKind::Sequential),
        (1, 2, CfgEdgeKind::Sequential),
        (2, 1, CfgEdgeKind::LoopBack),
        (2, 9, CfgEdgeKind::LoopExit),
    ] {
        connect_cfg(&mut cpg, NodeId::new(source), NodeId::new(target), kind);
    }
    connect_def_use(&mut cpg, NodeId::new(3), NodeId::new(6));
    connect_def_use(&mut cpg, NodeId::new(1), NodeId::new(8));
    if duplicate_definition {
        connect_def_use(&mut cpg, NodeId::new(10), NodeId::new(8));
    }
    ExplicitFixture {
        cpg,
        function: FUNCTION,
    }
}

fn nested_range_fixture() -> ExplicitFixture {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    for (id, kind) in [
        (0, function("nested_ranges")),
        (1, CpgNodeKind::For),
        (2, CpgNodeKind::For),
        (3, block()),
        (4, block()),
        (5, CpgNodeKind::Return),
        (6, variable("x")),
        (7, variable("i")),
        (8, binary("..")),
        (9, integer(0)),
        (10, identifier("x")),
        (11, block()),
        (12, variable("j")),
        (13, binary("..=")),
        (14, integer(0)),
        (15, identifier("x")),
    ] {
        cpg.add_node_with_id(node(id, kind));
    }
    for child in [6, 1, 5] {
        connect_ast(&mut cpg, FUNCTION, NodeId::new(child));
    }
    for child in [7, 8, 11] {
        connect_ast(&mut cpg, NodeId::new(1), NodeId::new(child));
    }
    connect_ast(&mut cpg, NodeId::new(8), NodeId::new(9));
    connect_ast(&mut cpg, NodeId::new(8), NodeId::new(10));
    connect_ast(&mut cpg, NodeId::new(11), NodeId::new(2));
    connect_ast(&mut cpg, NodeId::new(11), NodeId::new(4));
    for child in [12, 13, 3] {
        connect_ast(&mut cpg, NodeId::new(2), NodeId::new(child));
    }
    connect_ast(&mut cpg, NodeId::new(13), NodeId::new(14));
    connect_ast(&mut cpg, NodeId::new(13), NodeId::new(15));
    for (source, target, kind) in [
        (0, 6, CfgEdgeKind::Sequential),
        (6, 1, CfgEdgeKind::Sequential),
        (1, 2, CfgEdgeKind::ConditionalTrue),
        (2, 3, CfgEdgeKind::ConditionalTrue),
        (3, 2, CfgEdgeKind::LoopBack),
        (2, 4, CfgEdgeKind::LoopExit),
        (4, 1, CfgEdgeKind::LoopBack),
        (1, 5, CfgEdgeKind::LoopExit),
    ] {
        connect_cfg(&mut cpg, NodeId::new(source), NodeId::new(target), kind);
    }
    connect_def_use(&mut cpg, NodeId::new(6), NodeId::new(10));
    connect_def_use(&mut cpg, NodeId::new(6), NodeId::new(15));
    ExplicitFixture {
        cpg,
        function: FUNCTION,
    }
}

fn permuted_copy(original: &CodePropertyGraph) -> CodePropertyGraph {
    let mut copy = CodePropertyGraph::new(original.language());
    let mut nodes: Vec<_> = original.nodes().cloned().collect();
    nodes.reverse();
    for node in nodes {
        copy.add_node_with_id(node);
    }
    let mut edges: Vec<_> = original.edges().cloned().collect();
    edges.reverse();
    for edge in edges {
        copy.add_edge_with_id(edge)
            .expect("permuted edge endpoints exist");
    }
    for &entry in original.cfg_entries().iter().rev() {
        copy.add_cfg_entry(entry);
    }
    for &exit in original.cfg_exits().iter().rev() {
        copy.add_cfg_exit(exit);
    }
    copy
}

fn assert_analysis_contracts(
    cpg: &CodePropertyGraph,
    function: NodeId,
    forest: &LoopForest,
    analysis: &InductionAnalysis,
) {
    let facts = variable_facts(cpg, function);
    let scope = function_scope_nodes(cpg, function);
    assert_eq!(analysis.invariants.len(), forest.loops.len());
    assert_eq!(analysis.bounds.len(), forest.loops.len());
    for (natural_loop, report) in forest.loops.iter().zip(&analysis.invariants) {
        let body: BTreeSet<_> = natural_loop.body.iter().copied().collect();
        let region = loop_region_nodes(cpg, &body, &scope);
        let invariant: BTreeSet<_> = report.invariant_nodes.iter().copied().collect();
        assert!(report.hoistable.iter().all(|node| invariant.contains(node)));
        for &candidate in &report.invariant_nodes {
            for use_site in candidate_uses(cpg, candidate, &facts) {
                let predecessors: Vec<_> = cpg
                    .dfg_predecessors(use_site)
                    .into_iter()
                    .filter_map(|(source, kind)| (kind == DfgEdgeKind::DefUse).then_some(source))
                    .collect();
                assert!(!predecessors.is_empty());
                assert!(predecessors
                    .iter()
                    .all(|source| !region.contains(source) || invariant.contains(source)));
            }
        }
    }
    assert!(analysis
        .ivs
        .iter()
        .all(|iv| forest.loops[iv.loop_id].body.contains(&iv.update_node)));
}

#[test]
fn explicit_loop_reports_invariants_basic_and_derived_ivs_and_counted_bound() {
    let fixture = explicit_fixture(3, 2, false);
    let before = (fixture.cpg.node_count(), fixture.cpg.edge_count());
    let forest = loop_forest(&fixture.cpg, fixture.function).expect("natural loop forest");
    let analysis =
        loop_induction(&fixture.cpg, fixture.function, &forest).expect("induction analysis");

    assert_eq!(forest.loops.len(), 1);
    assert!(analysis.invariants[0]
        .invariant_nodes
        .contains(&INVARIANT_ASSIGNMENT));
    assert!(!analysis.invariants[0]
        .invariant_nodes
        .contains(&BASIC_UPDATE));
    assert_eq!(
        analysis.ivs,
        vec![
            InductionVariable {
                loop_id: 0,
                variable: "i".into(),
                update_node: BASIC_UPDATE,
                step: Some(3),
                direction: IvDirection::Increasing,
                init_defs: vec![INITIAL_I],
            },
            InductionVariable {
                loop_id: 0,
                variable: "j".into(),
                update_node: DERIVED_UPDATE,
                step: Some(6),
                direction: IvDirection::Increasing,
                init_defs: vec![NodeId::new(4)],
            },
        ]
    );
    assert_eq!(
        analysis.bounds[0].trip,
        TripEvidence::Counted {
            iv: "i".into(),
            comparison: COMPARISON,
            bound_node: BOUND_USE,
        }
    );
    assert_analysis_contracts(&fixture.cpg, fixture.function, &forest, &analysis);
    assert_eq!((fixture.cpg.node_count(), fixture.cpg.edge_count()), before);
}

#[test]
fn semantic_range_loop_reports_implicit_step_and_counted_evidence() {
    let fixture = range_fixture();
    let forest = loop_forest(&fixture.cpg, fixture.function).expect("range loop forest");
    let analysis = loop_induction(&fixture.cpg, fixture.function, &forest).expect("range analysis");

    assert_eq!(
        analysis.ivs,
        vec![InductionVariable {
            loop_id: 0,
            variable: "i".into(),
            update_node: NodeId::new(1),
            step: Some(1),
            direction: IvDirection::Increasing,
            init_defs: vec![NodeId::new(3)],
        }]
    );
    assert_eq!(
        analysis.bounds[0].trip,
        TripEvidence::Counted {
            iv: "i".into(),
            comparison: NodeId::new(4),
            bound_node: NodeId::new(6),
        }
    );
    assert_analysis_contracts(&fixture.cpg, fixture.function, &forest, &analysis);
}

#[test]
fn hoistability_requires_unique_definitions_and_dominated_uses() {
    let fixture = hoist_fixture(false);
    let forest = loop_forest(&fixture.cpg, fixture.function).expect("hoist forest");
    let analysis = loop_induction(&fixture.cpg, fixture.function, &forest).expect("hoist analysis");
    assert!(analysis.invariants[0].hoistable.contains(&NodeId::new(1)));
    assert!(analysis.invariants[0].hoistable.contains(&NodeId::new(5)));
    assert_eq!(analysis.bounds[0].trip, TripEvidence::Unknown);

    let duplicate = hoist_fixture(true);
    let forest = loop_forest(&duplicate.cpg, duplicate.function).expect("duplicate forest");
    let analysis =
        loop_induction(&duplicate.cpg, duplicate.function, &forest).expect("duplicate analysis");
    assert!(!analysis.invariants[0].hoistable.contains(&NodeId::new(1)));
    assert!(!analysis.invariants[0].hoistable.contains(&NodeId::new(10)));
}

#[test]
fn nested_range_loops_keep_induction_and_bounds_loop_local() {
    let fixture = nested_range_fixture();
    let forest = loop_forest(&fixture.cpg, fixture.function).expect("nested forest");
    let analysis =
        loop_induction(&fixture.cpg, fixture.function, &forest).expect("nested analysis");
    assert_eq!(forest.loops.len(), 2);
    assert_eq!(analysis.invariants.len(), 2);
    assert_eq!(analysis.bounds.len(), 2);
    let outer = forest
        .loops
        .iter()
        .find(|natural_loop| natural_loop.header == NodeId::new(1))
        .expect("outer loop");
    let inner = forest
        .loops
        .iter()
        .find(|natural_loop| natural_loop.header == NodeId::new(2))
        .expect("inner loop");
    let outer_iv = analysis
        .ivs
        .iter()
        .find(|iv| &*iv.variable == "i")
        .expect("outer IV");
    let inner_iv = analysis
        .ivs
        .iter()
        .find(|iv| &*iv.variable == "j")
        .expect("inner IV");
    assert_eq!(outer_iv.loop_id, outer.id);
    assert_eq!(inner_iv.loop_id, inner.id);
    assert_eq!(outer_iv.step, Some(1));
    assert_eq!(inner_iv.step, Some(1));
    assert!(matches!(
        analysis.bounds[outer.id].trip,
        TripEvidence::Counted { ref iv, .. } if &**iv == "i"
    ));
    assert!(matches!(
        analysis.bounds[inner.id].trip,
        TripEvidence::Counted { ref iv, .. } if &**iv == "j"
    ));
    assert_analysis_contracts(&fixture.cpg, fixture.function, &forest, &analysis);
}

#[test]
fn recurrence_requires_an_invariant_operand_unique_definition_and_all_latches() {
    let mut non_invariant = explicit_fixture(3, 2, false);
    non_invariant
        .cpg
        .node_mut(NodeId::new(28))
        .expect("step operand")
        .kind = identifier("delta");
    let forest = loop_forest(&non_invariant.cpg, non_invariant.function).expect("forest");
    let analysis = loop_induction(&non_invariant.cpg, non_invariant.function, &forest)
        .expect("non-invariant step analysis");
    assert!(analysis.ivs.is_empty());

    let mut duplicate = explicit_fixture(3, 2, false);
    for (id, kind) in [
        (33, assignment("=")),
        (34, identifier("i")),
        (35, integer(0)),
    ] {
        duplicate.cpg.add_node_with_id(node(id, kind));
    }
    connect_ast(&mut duplicate.cpg, NodeId::new(11), NodeId::new(33));
    connect_ast(&mut duplicate.cpg, NodeId::new(33), NodeId::new(34));
    connect_ast(&mut duplicate.cpg, NodeId::new(33), NodeId::new(35));
    let forest = loop_forest(&duplicate.cpg, duplicate.function).expect("duplicate forest");
    let analysis = loop_induction(&duplicate.cpg, duplicate.function, &forest)
        .expect("duplicate definition analysis");
    assert!(analysis.ivs.iter().all(|iv| &*iv.variable != "i"));

    let mut conditional = explicit_fixture(3, 2, false);
    connect_cfg(
        &mut conditional.cpg,
        DERIVED_UPDATE,
        WHILE_HEADER,
        CfgEdgeKind::LoopBack,
    );
    let forest = loop_forest(&conditional.cpg, conditional.function).expect("two-latch forest");
    assert_eq!(forest.loops[0].latches, vec![DERIVED_UPDATE, BASIC_UPDATE]);
    let analysis = loop_induction(&conditional.cpg, conditional.function, &forest)
        .expect("two-latch analysis");
    assert!(analysis.ivs.iter().all(|iv| &*iv.variable != "i"));
}

#[test]
fn subtraction_unknown_coefficients_and_overflow_remain_conservative() {
    let mut subtraction = explicit_fixture(3, 2, false);
    subtraction
        .cpg
        .node_mut(NodeId::new(26))
        .expect("update expression")
        .kind = binary("-");
    let forest = loop_forest(&subtraction.cpg, subtraction.function).expect("subtraction forest");
    let analysis = loop_induction(&subtraction.cpg, subtraction.function, &forest)
        .expect("subtraction analysis");
    let basic = analysis
        .ivs
        .iter()
        .find(|iv| &*iv.variable == "i")
        .expect("subtractive IV");
    assert_eq!(basic.step, Some(-3));
    assert_eq!(basic.direction, IvDirection::Decreasing);
    assert_eq!(analysis.bounds[0].trip, TripEvidence::Unknown);

    let mut symbolic = explicit_fixture(3, 2, false);
    symbolic
        .cpg
        .node_mut(NodeId::new(21))
        .expect("coefficient")
        .kind = identifier("scale");
    symbolic.cpg.add_node_with_id(node(33, variable("scale")));
    connect_ast(&mut symbolic.cpg, FUNCTION, NodeId::new(33));
    connect_def_use(&mut symbolic.cpg, NodeId::new(33), NodeId::new(21));
    let forest = loop_forest(&symbolic.cpg, symbolic.function).expect("symbolic forest");
    let analysis = loop_induction(&symbolic.cpg, symbolic.function, &forest)
        .expect("symbolic coefficient analysis");
    let derived = analysis
        .ivs
        .iter()
        .find(|iv| &*iv.variable == "j")
        .expect("symbolic derived IV");
    assert_eq!(derived.step, None);
    assert_eq!(derived.direction, IvDirection::Unknown);

    let overflow = explicit_fixture(i64::MAX, 2, false);
    let forest = loop_forest(&overflow.cpg, overflow.function).expect("overflow forest");
    let analysis =
        loop_induction(&overflow.cpg, overflow.function, &forest).expect("overflow analysis");
    assert_eq!(
        analysis
            .ivs
            .iter()
            .find(|iv| &*iv.variable == "j")
            .expect("overflow-derived IV")
            .step,
        None
    );
}

#[test]
fn update_shapes_directions_and_checked_arithmetic_cover_supported_forms() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    for (id, kind) in [
        (0, assignment("+=")),
        (1, identifier("i")),
        (2, integer(4)),
        (3, assignment("-=")),
        (4, identifier("i")),
        (5, integer(i64::MIN)),
        (6, assignment("=")),
        (7, identifier("i")),
        (8, binary("+")),
        (9, integer(2)),
        (10, identifier("i")),
        (11, assignment("=")),
        (12, identifier("i")),
        (13, binary("-")),
        (14, integer(2)),
        (15, identifier("i")),
    ] {
        cpg.add_node_with_id(node(id, kind));
    }
    for (parent, child) in [
        (0, 1),
        (0, 2),
        (3, 4),
        (3, 5),
        (6, 7),
        (6, 8),
        (8, 9),
        (8, 10),
        (11, 12),
        (11, 13),
        (13, 14),
        (13, 15),
    ] {
        connect_ast(&mut cpg, NodeId::new(parent), NodeId::new(child));
    }

    assert_eq!(
        basic_update_shape(&cpg, NodeId::new(0), "i"),
        Some((NodeId::new(2), true))
    );
    assert_eq!(
        basic_update_shape(&cpg, NodeId::new(3), "i"),
        Some((NodeId::new(5), false))
    );
    assert_eq!(
        basic_update_shape(&cpg, NodeId::new(6), "i"),
        Some((NodeId::new(9), true))
    );
    assert_eq!(basic_update_shape(&cpg, NodeId::new(11), "i"), None);
    assert_eq!(integer_constant(&cpg, NodeId::new(5)), Some(i64::MIN));
    assert_eq!(i64::MIN.checked_neg(), None);
    assert_eq!(direction_from_step(Some(4)), IvDirection::Increasing);
    assert_eq!(direction_from_step(Some(-4)), IvDirection::Decreasing);
    assert_eq!(direction_from_step(Some(0)), IvDirection::Stationary);
    assert_eq!(direction_from_step(None), IvDirection::Unknown);
}

#[test]
fn affine_recognition_and_malformed_semantic_shapes_cover_every_operator_family() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    for (id, kind) in [
        (0, identifier("i")),
        (1, integer(2)),
        (2, binary("+")),
        (3, binary("+")),
        (4, binary("-")),
        (5, binary("+")),
        (6, binary("-")),
        (7, binary("*")),
        (
            8,
            CpgNodeKind::UnaryOp {
                operator: "-".into(),
            },
        ),
        (
            9,
            CpgNodeKind::UnaryOp {
                operator: "+".into(),
            },
        ),
        (
            10,
            CpgNodeKind::UnaryOp {
                operator: "!".into(),
            },
        ),
        (11, binary("/")),
        (12, binary("+")),
        (13, identifier("other")),
        (14, binary("*")),
        (15, identifier("scale")),
        (16, binary("*")),
        (
            17,
            CpgNodeKind::UnaryOp {
                operator: "-".into(),
            },
        ),
        (
            18,
            CpgNodeKind::UnaryOp {
                operator: "+".into(),
            },
        ),
        (19, integer(5)),
        (
            20,
            CpgNodeKind::UnaryOp {
                operator: "-".into(),
            },
        ),
        (21, integer(5)),
        (
            22,
            CpgNodeKind::UnaryOp {
                operator: "!".into(),
            },
        ),
        (23, integer(5)),
        (
            24,
            CpgNodeKind::UnaryOp {
                operator: "-".into(),
            },
        ),
        (25, integer(i64::MIN)),
        (
            26,
            CpgNodeKind::Literal {
                kind: LiteralKind::Bool(true),
            },
        ),
        (30, CpgNodeKind::For),
        (31, identifier("i")),
        (32, binary("..=")),
        (33, integer(0)),
        (34, integer(10)),
        (35, CpgNodeKind::For),
        (36, block()),
        (37, CpgNodeKind::For),
        (38, identifier("i")),
        (39, binary("..")),
        (40, integer(0)),
        (41, CpgNodeKind::For),
        (
            42,
            CpgNodeKind::Unknown {
                kind: "tuple_pattern".into(),
            },
        ),
        (43, variable("i")),
        (44, CpgNodeKind::For),
        (45, identifier("i")),
        (50, variable("v")),
        (51, identifier("v")),
        (52, integer(7)),
        (53, variable("empty")),
        (54, identifier("empty")),
        (60, block()),
        (61, block()),
        (70, assignment("=")),
        (71, identifier("i")),
        (72, integer(1)),
        (73, assignment("=")),
        (74, identifier("i")),
        (75, binary("+")),
        (76, identifier("i")),
        (77, assignment("*=")),
        (78, identifier("i")),
        (79, integer(2)),
        (80, assignment("=")),
        (81, identifier("i")),
    ] {
        cpg.add_node_with_id(node(id, kind));
    }
    for (parent, child) in [
        (2, 0),
        (2, 1),
        (3, 1),
        (3, 0),
        (4, 1),
        (4, 0),
        (5, 0),
        (5, 0),
        (6, 0),
        (6, 0),
        (7, 0),
        (7, 1),
        (8, 0),
        (9, 0),
        (10, 0),
        (11, 0),
        (11, 1),
        (12, 0),
        (14, 0),
        (14, 15),
        (16, 0),
        (16, 0),
        (18, 19),
        (20, 21),
        (22, 23),
        (24, 25),
        (30, 31),
        (30, 32),
        (32, 33),
        (32, 34),
        (35, 36),
        (37, 38),
        (37, 39),
        (39, 40),
        (41, 42),
        (42, 43),
        (44, 45),
        (50, 51),
        (50, 52),
        (53, 54),
        (60, 61),
        (61, 60),
        (70, 71),
        (70, 72),
        (73, 74),
        (73, 75),
        (75, 76),
        (77, 78),
        (77, 79),
        (80, 81),
    ] {
        connect_ast(&mut cpg, NodeId::new(parent), NodeId::new(child));
    }

    let base = InductionVariable {
        loop_id: 0,
        variable: "i".into(),
        update_node: NodeId::new(90),
        step: Some(3),
        direction: IvDirection::Increasing,
        init_defs: Vec::new(),
    };
    let basics = BTreeMap::from([(base.variable.clone(), &base)]);
    let invariant = BTreeSet::from([
        NodeId::new(1),
        NodeId::new(15),
        NodeId::new(19),
        NodeId::new(21),
        NodeId::new(23),
        NodeId::new(25),
    ]);
    let coefficient = |node| {
        affine_in_basic(
            &cpg,
            NodeId::new(node),
            &basics,
            &invariant,
            3,
            &mut BTreeSet::new(),
        )
        .map(|affine| affine.coefficient)
    };
    assert_eq!(coefficient(2), Some(Some(1)));
    assert_eq!(coefficient(3), Some(Some(1)));
    assert_eq!(coefficient(4), Some(Some(-1)));
    assert_eq!(coefficient(5), Some(Some(2)));
    assert_eq!(coefficient(6), Some(Some(0)));
    assert_eq!(coefficient(7), Some(Some(2)));
    assert_eq!(coefficient(8), Some(Some(-1)));
    assert_eq!(coefficient(9), Some(Some(1)));
    assert_eq!(coefficient(10), None);
    assert_eq!(coefficient(11), None);
    assert_eq!(coefficient(12), None);
    assert_eq!(coefficient(13), None);
    assert_eq!(coefficient(14), Some(None));
    assert_eq!(coefficient(16), None);
    assert_eq!(coefficient(17), None);
    assert!(affine_in_basic(
        &cpg,
        NodeId::new(2),
        &basics,
        &invariant,
        0,
        &mut BTreeSet::new(),
    )
    .is_none());
    let mut already_seen = BTreeSet::from([NodeId::new(0)]);
    assert!(affine_in_basic(
        &cpg,
        NodeId::new(0),
        &basics,
        &invariant,
        3,
        &mut already_seen
    )
    .is_none());

    assert_eq!(integer_constant(&cpg, NodeId::new(18)), Some(5));
    assert_eq!(integer_constant(&cpg, NodeId::new(20)), Some(-5));
    assert_eq!(integer_constant(&cpg, NodeId::new(22)), None);
    assert_eq!(integer_constant(&cpg, NodeId::new(24)), None);
    assert_eq!(integer_constant(&cpg, NodeId::new(26)), None);
    assert_eq!(multiply_coefficient(&cpg, Some(3), NodeId::new(1)), Some(6));
    assert_eq!(multiply_coefficient(&cpg, None, NodeId::new(1)), None);
    assert_eq!(multiply_coefficient(&cpg, Some(3), NodeId::new(15)), None);

    assert_eq!(
        for_range_parts(&cpg, NodeId::new(30)),
        Some((
            NodeId::new(31),
            NodeId::new(32),
            NodeId::new(33),
            NodeId::new(34)
        ))
    );
    assert_eq!(for_range_parts(&cpg, NodeId::new(35)), None);
    assert_eq!(for_range_parts(&cpg, NodeId::new(37)), None);
    assert_eq!(for_range_parts(&cpg, NodeId::new(44)), None);
    assert!(is_for_pattern_binding(&cpg, NodeId::new(31)));
    assert!(is_for_pattern_binding(&cpg, NodeId::new(43)));
    assert!(!is_for_pattern_binding(&cpg, NodeId::new(50)));
    assert!(operand_is_invariant(
        &cpg,
        NodeId::new(34),
        &BTreeSet::new()
    ));
    assert_eq!(definition_rhs(&cpg, NodeId::new(50)), Some(NodeId::new(52)));
    assert_eq!(definition_rhs(&cpg, NodeId::new(53)), None);
    assert_eq!(definition_rhs(&cpg, NodeId::new(26)), None);
    assert_eq!(comparison_expression(&cpg, NodeId::new(60)), None);
    assert_eq!(reverse_comparison("?"), None);
    assert_eq!(basic_update_shape(&cpg, NodeId::new(70), "i"), None);
    assert_eq!(basic_update_shape(&cpg, NodeId::new(73), "i"), None);
    assert_eq!(basic_update_shape(&cpg, NodeId::new(77), "i"), None);
    assert_eq!(basic_update_shape(&cpg, NodeId::new(80), "i"), None);
    assert_eq!(basic_update_shape(&cpg, NodeId::new(72), "i"), None);

    let fixture = explicit_fixture(1, 1, false);
    let facts = variable_facts(&fixture.cpg, fixture.function);
    assert!(external_definitions(&facts, &"missing".into(), &BTreeSet::new()).is_empty());
    let mut duplicates = vec![base.clone(), base];
    canonicalize_ivs(&mut duplicates);
    assert_eq!(duplicates.len(), 1);
}

#[test]
fn unknown_and_irreducible_loops_do_not_claim_counted_trip_evidence() {
    let fixture = explicit_fixture(0, 2, false);
    let mut forest = loop_forest(&fixture.cpg, fixture.function).expect("stationary forest");
    let stationary =
        loop_induction(&fixture.cpg, fixture.function, &forest).expect("stationary analysis");
    assert_eq!(stationary.ivs[0].direction, IvDirection::Stationary);
    assert_eq!(stationary.bounds[0].trip, TripEvidence::Unknown);

    forest.loops[0].irreducible = true;
    let irreducible =
        loop_induction(&fixture.cpg, fixture.function, &forest).expect("irreducible analysis");
    assert_eq!(irreducible.bounds[0].trip, TripEvidence::Unknown);
}

#[test]
fn malformed_selectors_foreign_forests_and_ast_cycles_are_bounded() {
    let mut fixture = explicit_fixture(1, 2, false);
    let forest = loop_forest(&fixture.cpg, fixture.function).expect("valid forest");
    assert_eq!(
        loop_induction(&fixture.cpg, NodeId::new(999), &forest),
        Err(AnalysisError::UnknownNode(NodeId::new(999)))
    );
    assert_eq!(
        loop_induction(&fixture.cpg, NodeId::new(29), &forest),
        Err(AnalysisError::NotAFunction(NodeId::new(29)))
    );

    let mut foreign = forest.clone();
    foreign.loops[0].body.push(NodeId::new(999));
    assert_eq!(
        loop_induction(&fixture.cpg, fixture.function, &foreign),
        Err(AnalysisError::UnknownNode(NodeId::new(999)))
    );

    fixture
        .cpg
        .connect(NodeId::new(28), BASIC_UPDATE, CpgEdgeKind::AstChild)
        .expect("cycle endpoints exist");
    fixture
        .cpg
        .node_mut(BASIC_UPDATE)
        .expect("update exists")
        .parent = Some(NodeId::new(28));
    let cycled = loop_induction(&fixture.cpg, fixture.function, &forest)
        .expect("cycle guards bound every traversal");
    assert_eq!(cycled.function, fixture.function);

    let empty = loop_induction(&fixture.cpg, fixture.function, &LoopForest::default())
        .expect("empty supplied forest");
    assert!(empty.invariants.is_empty());
    assert!(empty.ivs.is_empty());
    assert!(empty.bounds.is_empty());

    let mut foreign_header = forest.clone();
    foreign_header.loops[0].header = NodeId::new(998);
    assert_eq!(
        loop_induction(&fixture.cpg, fixture.function, &foreign_header),
        Err(AnalysisError::UnknownNode(NodeId::new(998)))
    );
    let mut foreign_exit = forest.clone();
    foreign_exit.loops[0].exits[0].target = NodeId::new(997);
    assert_eq!(
        loop_induction(&fixture.cpg, fixture.function, &foreign_exit),
        Err(AnalysisError::UnknownNode(NodeId::new(997)))
    );
}

#[test]
fn twenty_five_thousand_node_loop_region_is_stack_safe() {
    const LAST_DESCENDANT: u32 = 25_001;
    const RETURN: u32 = 25_002;
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    cpg.add_node_with_id(node(0, function("deep_region")));
    cpg.add_node_with_id(node(1, block()));
    for id in 2..=LAST_DESCENDANT {
        cpg.add_node_with_id(node(id, block()));
    }
    cpg.add_node_with_id(node(RETURN, CpgNodeKind::Return));
    connect_ast(&mut cpg, FUNCTION, NodeId::new(1));
    connect_ast(&mut cpg, FUNCTION, NodeId::new(RETURN));
    for id in 1..LAST_DESCENDANT {
        connect_ast(&mut cpg, NodeId::new(id), NodeId::new(id + 1));
    }
    connect_cfg(&mut cpg, FUNCTION, NodeId::new(1), CfgEdgeKind::Sequential);
    connect_cfg(
        &mut cpg,
        NodeId::new(1),
        NodeId::new(1),
        CfgEdgeKind::LoopBack,
    );
    connect_cfg(
        &mut cpg,
        NodeId::new(1),
        NodeId::new(RETURN),
        CfgEdgeKind::LoopExit,
    );

    let forest = loop_forest(&cpg, FUNCTION).expect("deep loop forest");
    let analysis = loop_induction(&cpg, FUNCTION, &forest).expect("stack-safe analysis");
    assert_eq!(analysis.invariants.len(), 1);
    assert!(analysis.invariants[0].invariant_nodes.is_empty());
    assert!(analysis.ivs.is_empty());
    assert_eq!(analysis.bounds[0].trip, TripEvidence::Unknown);
}

#[cfg(feature = "serde")]
#[test]
fn all_public_result_layers_round_trip_through_serde() {
    let fixture = explicit_fixture(-3, -2, true);
    let forest = loop_forest(&fixture.cpg, fixture.function).expect("forest");
    let analysis = loop_induction(&fixture.cpg, fixture.function, &forest).expect("analysis");
    let encoded = serde_json::to_string(&analysis).expect("serialize induction analysis");
    let decoded: InductionAnalysis =
        serde_json::from_str(&encoded).expect("deserialize induction analysis");
    assert_eq!(decoded, analysis);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(192))]

    #[test]
    fn analysis_is_insertion_order_deterministic_and_satisfies_contracts(
        step in (-32i64..=32).prop_filter("non-stationary", |step| *step != 0),
        coefficient in -16i64..=16,
        reverse_guard in any::<bool>(),
    ) {
        let fixture = explicit_fixture(step, coefficient, reverse_guard);
        let forest = loop_forest(&fixture.cpg, fixture.function).expect("forward forest");
        let analysis = loop_induction(&fixture.cpg, fixture.function, &forest)
            .expect("forward analysis");
        let permuted = permuted_copy(&fixture.cpg);
        let permuted_forest = loop_forest(&permuted, fixture.function).expect("permuted forest");
        let permuted_analysis = loop_induction(&permuted, fixture.function, &permuted_forest)
            .expect("permuted analysis");

        prop_assert_eq!(&forest, &permuted_forest);
        prop_assert_eq!(&analysis, &permuted_analysis);
        let basic = analysis.ivs.iter().find(|iv| &*iv.variable == "i").expect("basic IV");
        prop_assert_eq!(basic.step, Some(step));
        let derived = analysis.ivs.iter().find(|iv| &*iv.variable == "j").expect("derived IV");
        prop_assert_eq!(derived.step, step.checked_mul(coefficient));
        prop_assert!(
            matches!(analysis.bounds[0].trip, TripEvidence::Counted { .. }),
            "expected counted trip evidence"
        );
        assert_analysis_contracts(&fixture.cpg, fixture.function, &forest, &analysis);
        assert_analysis_contracts(&permuted, fixture.function, &permuted_forest, &permuted_analysis);
    }

    #[test]
    fn checked_direction_and_comparison_oracles_match_integer_semantics(
        step in any::<i64>(),
        operator in prop::sample::select(vec!["<", "<=", ">", ">=", "!="]),
    ) {
        let direction = direction_from_step(Some(step));
        let expected_direction = match step.cmp(&0) {
            std::cmp::Ordering::Greater => IvDirection::Increasing,
            std::cmp::Ordering::Less => IvDirection::Decreasing,
            std::cmp::Ordering::Equal => IvDirection::Stationary,
        };
        prop_assert_eq!(direction, expected_direction);
        let expected_match = match expected_direction {
            IvDirection::Increasing => matches!(operator, "<" | "<=" | "!="),
            IvDirection::Decreasing => matches!(operator, ">" | ">=" | "!="),
            IvDirection::Stationary | IvDirection::Unknown => false,
        };
        prop_assert_eq!(comparison_direction_matches(operator, direction), expected_match);
        let reversed = reverse_comparison(operator).expect("supported comparison");
        prop_assert_eq!(reverse_comparison(reversed), Some(operator));
    }
}
