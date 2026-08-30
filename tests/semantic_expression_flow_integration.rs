use libcpg::analysis::{
    analyze_semantic_expression_diff, cpg_diff, CpgDiffOptions, ExpressionEquivalenceCandidate,
    SemanticExpressionDiffLimits, SemanticExpressionDiffStatus,
};
use libcpg::{
    lower_expression_ir, BinaryOperator, CodePropertyGraph, CpgBuilder, CpgNodeKind,
    ExpressionEffects, ExpressionFact, ExpressionFactKind, ExpressionFactTable,
    ExpressionIrCompletion, ExpressionIrLimits, ExpressionLiteral, ExpressionType, FloatMode,
    FloatType, FloatWidth, IntegerOverflow, IntegerType, Language, LiteralKind, NodeId,
    SemanticProfile, ShiftBehavior, TreeSitterCpgBuilder,
};

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
            .expect("valid checked integer"),
    }
}

fn value_type(language: Language) -> ExpressionType {
    if matches!(language, Language::JavaScript | Language::TypeScript) {
        ExpressionType::Float(FloatType {
            width: FloatWidth::F64,
            mode: FloatMode::StrictIeee754,
        })
    } else {
        ExpressionType::Integer(integer_semantics(language))
    }
}

fn literal(language: Language, value: i64) -> ExpressionLiteral {
    if matches!(language, Language::JavaScript | Language::TypeScript) {
        ExpressionLiteral::Float64((value as f64).to_bits())
    } else {
        ExpressionLiteral::Integer(i128::from(value))
    }
}

fn find_binary(cpg: &CodePropertyGraph) -> (NodeId, [NodeId; 2]) {
    cpg.nodes()
        .find_map(|node| {
            let children = cpg.ast_children(node.id);
            (matches!(node.kind, CpgNodeKind::BinaryOp { .. }) && children.len() == 2)
                .then(|| (node.id, [children[0], children[1]]))
        })
        .expect("old revision contains one binary expression")
}

fn find_integer_literal(cpg: &CodePropertyGraph, expected: i64) -> NodeId {
    cpg.nodes()
        .find_map(|node| match &node.kind {
            CpgNodeKind::Literal {
                kind: LiteralKind::Integer(value),
            } if *value == expected => Some(node.id),
            _ => None,
        })
        .expect("revision contains the expected literal")
}

fn assert_parser_revision_diff(
    old_source: &str,
    new_source: &str,
    language: Language,
    expected: SemanticExpressionDiffStatus,
) {
    let old = TreeSitterCpgBuilder::new()
        .build(old_source, language)
        .expect("old revision parses");
    let new = TreeSitterCpgBuilder::new()
        .build(new_source, language)
        .expect("new revision parses");
    let (old_root, old_operands) = find_binary(&old);
    let new_root = find_integer_literal(&new, 3);
    let value_type = value_type(language);
    let old_facts = ExpressionFactTable::try_new(
        language,
        [
            (
                old_operands[0],
                ExpressionFact::known(
                    ExpressionFactKind::Literal(literal(language, 1)),
                    Vec::new(),
                    value_type,
                    ExpressionEffects::PURE,
                ),
            ),
            (
                old_operands[1],
                ExpressionFact::known(
                    ExpressionFactKind::Literal(literal(language, 2)),
                    Vec::new(),
                    value_type,
                    ExpressionEffects::PURE,
                ),
            ),
            (
                old_root,
                ExpressionFact::known(
                    ExpressionFactKind::Binary(BinaryOperator::Add),
                    old_operands.to_vec(),
                    value_type,
                    ExpressionEffects::PURE,
                ),
            ),
        ],
    )
    .expect("old facts are unique");
    let new_facts = ExpressionFactTable::try_new(
        language,
        [(
            new_root,
            ExpressionFact::known(
                ExpressionFactKind::Literal(literal(language, 3)),
                Vec::new(),
                value_type,
                ExpressionEffects::PURE,
            ),
        )],
    )
    .expect("new facts are unique");
    let profile = SemanticProfile::for_language(language);
    let old_ir = lower_expression_ir(
        &old,
        old_root,
        &old_facts,
        &profile,
        ExpressionIrLimits::default(),
    );
    let new_ir = lower_expression_ir(
        &new,
        new_root,
        &new_facts,
        &profile,
        ExpressionIrLimits::default(),
    );
    assert_eq!(old_ir.completion(), &ExpressionIrCompletion::Complete);
    assert_eq!(new_ir.completion(), &ExpressionIrCompletion::Complete);

    let old_before = format!("{old:?}");
    let new_before = format!("{new:?}");
    let structural_before =
        cpg_diff(&old, &new, &CpgDiffOptions::default()).expect("legacy structural diff succeeds");
    let semantic = analyze_semantic_expression_diff(
        &profile,
        ExpressionEquivalenceCandidate::new(old_root, NodeId::new(1), &old_ir),
        ExpressionEquivalenceCandidate::new(new_root, NodeId::new(2), &new_ir),
        SemanticExpressionDiffLimits::default(),
    )
    .expect("semantic revision diff succeeds");
    let structural_after = cpg_diff(&old, &new, &CpgDiffOptions::default())
        .expect("legacy structural diff remains available");

    assert_eq!(semantic.status, expected);
    if expected == SemanticExpressionDiffStatus::ProvenEquivalent {
        semantic
            .proof
            .as_ref()
            .expect("positive semantic diff carries a proof")
            .replay()
            .expect("parser-backed semantic proof replays");
    } else {
        assert!(semantic.proof.is_none());
        assert!(semantic.limitations.is_empty());
    }
    assert_eq!(semantic.old, old_root);
    assert_eq!(semantic.new, new_root);
    assert_eq!(structural_before, structural_after);
    assert_eq!(format!("{old:?}"), old_before);
    assert_eq!(format!("{new:?}"), new_before);
}

#[cfg(feature = "lang-rust")]
#[test]
fn rust_parser_revision_semantic_diff() {
    assert_parser_revision_diff(
        "fn f() -> i32 { 1 + 2 }",
        "fn f() -> i32 { 3 }",
        Language::Rust,
        SemanticExpressionDiffStatus::ProvenEquivalent,
    );
}

#[cfg(feature = "lang-c")]
#[test]
fn c_parser_revision_semantic_diff() {
    assert_parser_revision_diff(
        "int f(void) { return 1 + 2; }",
        "int f(void) { return 3; }",
        Language::C,
        SemanticExpressionDiffStatus::ProvenEquivalent,
    );
}

#[cfg(feature = "lang-cpp")]
#[test]
fn cpp_parser_revision_semantic_diff() {
    assert_parser_revision_diff(
        "int f() { return 1 + 2; }",
        "int f() { return 3; }",
        Language::Cpp,
        SemanticExpressionDiffStatus::ProvenEquivalent,
    );
}

#[cfg(feature = "lang-go")]
#[test]
fn go_parser_revision_semantic_diff() {
    assert_parser_revision_diff(
        "package p\nfunc f() int { return 1 + 2 }",
        "package p\nfunc f() int { return 3 }",
        Language::Go,
        SemanticExpressionDiffStatus::ProvenEquivalent,
    );
}

#[cfg(feature = "lang-java")]
#[test]
fn java_parser_revision_semantic_diff() {
    assert_parser_revision_diff(
        "class A { int f() { return 1 + 2; } }",
        "class A { int f() { return 3; } }",
        Language::Java,
        SemanticExpressionDiffStatus::ProvenEquivalent,
    );
}

#[cfg(feature = "lang-python")]
#[test]
fn python_parser_revision_semantic_diff() {
    assert_parser_revision_diff(
        "def f():\n    return 1 + 2\n",
        "def f():\n    return 3\n",
        Language::Python,
        SemanticExpressionDiffStatus::ProvenEquivalent,
    );
}

#[cfg(feature = "lang-javascript")]
#[test]
fn javascript_parser_revision_semantic_diff() {
    assert_parser_revision_diff(
        "function f() { return 1 + 2; }",
        "function f() { return 3; }",
        Language::JavaScript,
        SemanticExpressionDiffStatus::Unknown,
    );
}

#[cfg(feature = "lang-typescript")]
#[test]
fn typescript_parser_revision_semantic_diff() {
    assert_parser_revision_diff(
        "function f(): number { return 1 + 2; }",
        "function f(): number { return 3; }",
        Language::TypeScript,
        SemanticExpressionDiffStatus::Unknown,
    );
}

#[cfg(feature = "lang-ruby")]
#[test]
fn ruby_parser_revision_semantic_diff() {
    assert_parser_revision_diff(
        "def f\n  1 + 2\nend\n",
        "def f\n  3\nend\n",
        Language::Ruby,
        SemanticExpressionDiffStatus::ProvenEquivalent,
    );
}
