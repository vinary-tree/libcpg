use std::collections::BTreeSet;

use libcpg::analysis::{
    analyze_expression_equivalence, ExpressionEquivalenceCandidate,
    ExpressionEquivalenceCompletion, ExpressionEquivalenceKind, ExpressionEquivalenceLimits,
};
use libcpg::{
    lower_expression_ir, BinaryOperator, CodePropertyGraph, CpgBuilder, CpgNodeKind,
    ExpressionEffects, ExpressionFact, ExpressionFactKind, ExpressionFactTable,
    ExpressionIrCompletion, ExpressionIrLimits, ExpressionLiteral, ExpressionType, FloatMode,
    FloatType, FloatWidth, IntegerOverflow, IntegerType, Language, NodeId, SemanticProfile,
    ShiftBehavior, TreeSitterCpgBuilder,
};

fn parser_built_binaries(cpg: &CodePropertyGraph) -> Vec<(NodeId, Vec<NodeId>)> {
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
            .then(|| (node.id, operands.to_vec()))
        })
        .take(2)
        .collect()
}

fn integer_semantics(language: Language) -> IntegerType {
    match language {
        Language::Python | Language::Ruby => IntegerType::arbitrary_precision(true),
        Language::C | Language::Cpp => IntegerType::fixed(
            32,
            true,
            IntegerOverflow::Undefined,
            ShiftBehavior::Undefined,
        )
        .expect("valid C-family integer"),
        Language::Java => {
            IntegerType::fixed(32, true, IntegerOverflow::Wrapping, ShiftBehavior::Masked)
                .expect("valid Java integer")
        }
        _ => IntegerType::fixed(32, true, IntegerOverflow::Checked, ShiftBehavior::Checked)
            .expect("valid integer"),
    }
}

fn assert_parser_equivalence(source: &str, language: Language) {
    let cpg = TreeSitterCpgBuilder::new()
        .build(source, language)
        .expect("configured upstream parser builds the CPG");
    let expressions = parser_built_binaries(&cpg);
    assert_eq!(expressions.len(), 2, "{language:?} fixture has two terms");
    let javascript_number = matches!(language, Language::JavaScript | Language::TypeScript);
    let value_type = if javascript_number {
        ExpressionType::Float(FloatType {
            width: FloatWidth::F64,
            mode: FloatMode::StrictIeee754,
        })
    } else {
        ExpressionType::Integer(integer_semantics(language))
    };
    let mut facts = Vec::with_capacity(6);
    let mut seen = BTreeSet::new();
    for (root, operands) in &expressions {
        assert!(seen.insert(*root));
        facts.push((
            *root,
            ExpressionFact::known(
                ExpressionFactKind::Binary(BinaryOperator::Add),
                operands.clone(),
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
                other => panic!("expected integer-syntax literal, found {other:?}"),
            };
            let literal = if javascript_number {
                ExpressionLiteral::Float64((value as f64).to_bits())
            } else {
                ExpressionLiteral::Integer(i128::from(value))
            };
            facts.push((
                *operand,
                ExpressionFact::known(
                    ExpressionFactKind::Literal(literal),
                    Vec::new(),
                    value_type,
                    ExpressionEffects::PURE,
                ),
            ));
        }
    }
    let facts = ExpressionFactTable::try_new(language, facts).expect("unique parser facts");
    let profile = SemanticProfile::for_language(language);
    let before = (cpg.node_count(), cpg.edge_count());
    let left = lower_expression_ir(
        &cpg,
        expressions[0].0,
        &facts,
        &profile,
        ExpressionIrLimits::default(),
    );
    let right = lower_expression_ir(
        &cpg,
        expressions[1].0,
        &facts,
        &profile,
        ExpressionIrLimits::default(),
    );
    assert_eq!(left.completion(), &ExpressionIrCompletion::Complete);
    assert_eq!(right.completion(), &ExpressionIrCompletion::Complete);
    let context = NodeId::new(u32::MAX - 1);
    let report = analyze_expression_equivalence(
        NodeId::new(u32::MAX),
        &profile,
        &[
            ExpressionEquivalenceCandidate::new(expressions[0].0, context, &left),
            ExpressionEquivalenceCandidate::new(expressions[1].0, context, &right),
        ],
        ExpressionEquivalenceLimits::default(),
    )
    .expect("parser-backed exact analysis succeeds");

    assert_eq!(before, (cpg.node_count(), cpg.edge_count()));
    assert_eq!(report.completion, ExpressionEquivalenceCompletion::Complete);
    assert_eq!(report.pairs.len(), 1);
    assert_eq!(report.pairs[0].kind, ExpressionEquivalenceKind::Exact);
    report.pairs[0]
        .proof
        .replay()
        .expect("parser-backed proof replays independently");
}

#[cfg(feature = "lang-rust")]
#[test]
fn rust_parser_equivalence_integration() {
    assert_parser_equivalence(
        "fn f() -> i32 { let a = 1 + 2; let b = 1 + 2; a + b }",
        Language::Rust,
    );
}

#[cfg(feature = "lang-c")]
#[test]
fn c_parser_equivalence_integration() {
    assert_parser_equivalence(
        "int f(void) { int a = 1 + 2; int b = 1 + 2; return a + b; }",
        Language::C,
    );
}

#[cfg(feature = "lang-cpp")]
#[test]
fn cpp_parser_equivalence_integration() {
    assert_parser_equivalence(
        "int f() { int a = 1 + 2; int b = 1 + 2; return a + b; }",
        Language::Cpp,
    );
}

#[cfg(feature = "lang-go")]
#[test]
fn go_parser_equivalence_integration() {
    assert_parser_equivalence(
        "package p\nfunc f() int { a := 1 + 2; b := 1 + 2; return a + b }",
        Language::Go,
    );
}

#[cfg(feature = "lang-java")]
#[test]
fn java_parser_equivalence_integration() {
    assert_parser_equivalence(
        "class A { int f() { int a = 1 + 2; int b = 1 + 2; return a + b; } }",
        Language::Java,
    );
}

#[cfg(feature = "lang-python")]
#[test]
fn python_parser_equivalence_integration() {
    assert_parser_equivalence(
        "def f():\n    a = 1 + 2\n    b = 1 + 2\n    return a + b\n",
        Language::Python,
    );
}

#[cfg(feature = "lang-javascript")]
#[test]
fn javascript_parser_equivalence_integration() {
    assert_parser_equivalence(
        "function f() { const a = 1 + 2; const b = 1 + 2; return a + b; }",
        Language::JavaScript,
    );
}

#[cfg(feature = "lang-typescript")]
#[test]
fn typescript_parser_equivalence_integration() {
    assert_parser_equivalence(
        "function f(): number { const a = 1 + 2; const b = 1 + 2; return a + b; }",
        Language::TypeScript,
    );
}

#[cfg(feature = "lang-ruby")]
#[test]
fn ruby_parser_equivalence_integration() {
    assert_parser_equivalence(
        "def f\n  a = 1 + 2\n  b = 1 + 2\n  a + b\nend\n",
        Language::Ruby,
    );
}
