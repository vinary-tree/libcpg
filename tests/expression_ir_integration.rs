use libcpg::{
    lower_expression_ir, BinaryOperator, CodePropertyGraph, CpgBuilder, CpgNodeKind,
    ExpressionEffects, ExpressionFact, ExpressionFactKind, ExpressionFactTable,
    ExpressionIrCompletion, ExpressionIrLimits, ExpressionLiteral, ExpressionType, FloatMode,
    FloatType, FloatWidth, IntegerOverflow, IntegerType, Language, RewriteDenial, RewriteLaw,
    SemanticProfile, ShiftBehavior, TreeSitterCpgBuilder,
};

fn parser_built_binary(cpg: &CodePropertyGraph) -> (libcpg::NodeId, Vec<libcpg::NodeId>) {
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
        .next()
        .expect("parser emits a binary node with two literal operands")
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

fn assert_parser_profile(source: &str, language: Language) {
    let cpg = TreeSitterCpgBuilder::new()
        .build(source, language)
        .expect("configured upstream parser builds the CPG");
    let (root, operands) = parser_built_binary(&cpg);
    let javascript_number = matches!(language, Language::JavaScript | Language::TypeScript);
    let value_type = if javascript_number {
        ExpressionType::Float(FloatType {
            width: FloatWidth::F64,
            mode: FloatMode::StrictIeee754,
        })
    } else {
        ExpressionType::Integer(integer_semantics(language))
    };
    let mut facts = Vec::with_capacity(3);
    facts.push((
        root,
        ExpressionFact::known(
            ExpressionFactKind::Binary(BinaryOperator::Add),
            operands.clone(),
            value_type,
            ExpressionEffects::PURE,
        ),
    ));
    for operand in &operands {
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
    let facts = ExpressionFactTable::try_new(language, facts).expect("unique typed facts");
    let profile = SemanticProfile::for_language(language);
    assert!(profile.is_complete(), "{language:?} profile is explicit");
    let before = (cpg.node_count(), cpg.edge_count());
    let ir = lower_expression_ir(&cpg, root, &facts, &profile, ExpressionIrLimits::default());
    assert_eq!(ir.completion(), &ExpressionIrCompletion::Complete);
    assert_eq!(ir.metrics().nodes, 3);
    assert_eq!(before, (cpg.node_count(), cpg.edge_count()));
    let root = ir.root().expect("complete root");
    if javascript_number {
        assert_eq!(
            ir.certify(root, RewriteLaw::AdditiveIdentity),
            Err(RewriteDenial::PreconditionFailed),
            "strict IEEE signed-zero semantics deny x + 0"
        );
    } else {
        ir.certify(root, RewriteLaw::AdditiveIdentity)
            .expect("typed integer addition by zero is admissible");
    }
}

#[cfg(feature = "lang-rust")]
#[test]
fn rust_parser_profile_integration() {
    assert_parser_profile("fn f() -> i32 { 1 + 0 }", Language::Rust);
}

#[cfg(feature = "lang-c")]
#[test]
fn c_parser_profile_integration() {
    assert_parser_profile("int f(void) { return 1 + 0; }", Language::C);
}

#[cfg(feature = "lang-cpp")]
#[test]
fn cpp_parser_profile_integration() {
    assert_parser_profile("int f() { return 1 + 0; }", Language::Cpp);
}

#[cfg(feature = "lang-go")]
#[test]
fn go_parser_profile_integration() {
    assert_parser_profile("package p\nfunc f() int { return 1 + 0 }", Language::Go);
}

#[cfg(feature = "lang-java")]
#[test]
fn java_parser_profile_integration() {
    assert_parser_profile("class A { int f() { return 1 + 0; } }", Language::Java);
}

#[cfg(feature = "lang-python")]
#[test]
fn python_parser_profile_integration() {
    assert_parser_profile("def f():\n    return 1 + 0\n", Language::Python);
}

#[cfg(feature = "lang-javascript")]
#[test]
fn javascript_parser_profile_integration() {
    assert_parser_profile("function f() { return 1 + 0; }", Language::JavaScript);
}

#[cfg(feature = "lang-typescript")]
#[test]
fn typescript_parser_profile_integration() {
    assert_parser_profile(
        "function f(): number { return 1 + 0; }",
        Language::TypeScript,
    );
}

#[cfg(feature = "lang-ruby")]
#[test]
fn ruby_parser_profile_integration() {
    assert_parser_profile("def f\n  1 + 0\nend\n", Language::Ruby);
}
