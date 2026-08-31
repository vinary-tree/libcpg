#![cfg(feature = "lang-rust")]

use std::collections::BTreeSet;

use libcpg::analysis::{
    analyze_semantic_expression_flow, export_semantic_datalog, ExpressionEquivalenceCandidate,
    SemanticDatalogLimits, SemanticExpressionFlowLimits,
};
use libcpg::{
    export_datalog, lower_expression_ir, BinaryOperator, CodePropertyGraph, CpgBuilder,
    CpgNodeKind, ExpressionEffects, ExpressionFact, ExpressionFactKind, ExpressionFactTable,
    ExpressionIrCompletion, ExpressionIrLimits, ExpressionLiteral, ExpressionType, IntegerOverflow,
    IntegerType, Language, NodeId, SemanticProfile, ShiftBehavior, TreeSitterCpgBuilder,
};

fn literal_binaries(cpg: &CodePropertyGraph) -> Vec<(NodeId, [NodeId; 2])> {
    cpg.nodes()
        .filter_map(|node| {
            let operands = cpg.ast_children(node.id);
            (matches!(node.kind, CpgNodeKind::BinaryOp { .. })
                && operands.len() == 2
                && operands.iter().all(|operand| {
                    matches!(
                        cpg.node(*operand).map(|node| &node.kind),
                        Some(CpgNodeKind::Literal { .. })
                    )
                }))
            .then(|| (node.id, [operands[0], operands[1]]))
        })
        .take(2)
        .collect()
}

#[test]
fn parser_cpg_semantic_evidence_is_an_owned_additive_datalog_input() {
    let cpg = TreeSitterCpgBuilder::new()
        .build(
            "fn repeated() -> i32 { let a = 1 + 2; let b = 1 + 2; a + b }",
            Language::Rust,
        )
        .expect("Rust fixture parses");
    let function = cpg
        .nodes()
        .find(|node| matches!(node.kind, CpgNodeKind::Function { .. }))
        .map(|node| node.id)
        .expect("fixture has a function");
    let expressions = literal_binaries(&cpg);
    assert_eq!(expressions.len(), 2);

    let value_type = ExpressionType::Integer(
        IntegerType::fixed(32, true, IntegerOverflow::Checked, ShiftBehavior::Checked)
            .expect("valid Rust i32 semantics"),
    );
    let mut seen = BTreeSet::new();
    let mut facts = Vec::new();
    for (root, operands) in &expressions {
        assert!(seen.insert(*root));
        facts.push((
            *root,
            ExpressionFact::known(
                ExpressionFactKind::Binary(BinaryOperator::Add),
                operands.to_vec(),
                value_type,
                ExpressionEffects::PURE,
            ),
        ));
        for operand in operands {
            if !seen.insert(*operand) {
                continue;
            }
            let value = match cpg.node(*operand).map(|node| &node.kind) {
                Some(CpgNodeKind::Literal {
                    kind: libcpg::LiteralKind::Integer(value),
                }) => *value,
                other => panic!("expected integer literal, found {other:?}"),
            };
            facts.push((
                *operand,
                ExpressionFact::known(
                    ExpressionFactKind::Literal(ExpressionLiteral::Integer(i128::from(value))),
                    Vec::new(),
                    value_type,
                    ExpressionEffects::PURE,
                ),
            ));
        }
    }
    let facts = ExpressionFactTable::try_new(Language::Rust, facts).expect("unique facts");
    let profile = SemanticProfile::for_language(Language::Rust);
    let lowered = expressions
        .iter()
        .map(|(root, _)| {
            let ir =
                lower_expression_ir(&cpg, *root, &facts, &profile, ExpressionIrLimits::default());
            assert_eq!(ir.completion(), &ExpressionIrCompletion::Complete);
            ir
        })
        .collect::<Vec<_>>();
    let candidates = expressions
        .iter()
        .zip(&lowered)
        .map(|((source, _), ir)| ExpressionEquivalenceCandidate::new(*source, function, ir))
        .collect::<Vec<_>>();

    let graph_before = (cpg.node_count(), cpg.edge_count(), format!("{cpg:?}"));
    let ordinary_before = export_datalog(&cpg);
    let report = analyze_semantic_expression_flow(
        &cpg,
        function,
        function,
        &profile,
        &candidates,
        SemanticExpressionFlowLimits::default(),
    )
    .expect("parser-backed semantic flow succeeds");
    let semantic = export_semantic_datalog(&report, SemanticDatalogLimits::default())
        .expect("proof-qualified semantic facts export");
    let ordinary_after = export_datalog(&cpg);

    assert_eq!(ordinary_before, ordinary_after);
    assert_eq!(
        graph_before,
        (cpg.node_count(), cpg.edge_count(), format!("{cpg:?}"))
    );
    assert_eq!(semantic.class_members().len(), 2);
    assert_eq!(semantic.proof_edges().len(), 1);
    report.equivalence.pairs[semantic.proof_edges()[0].proof_pair as usize]
        .proof
        .replay()
        .expect("exported proof reference replays");
    assert!(semantic
        .souffle_files()
        .expect("semantic fact rendering succeeds")
        .iter()
        .all(|file| !file.contents.is_empty()));
}
