use std::{
    collections::hash_map::DefaultHasher,
    fmt::Write as _,
    hash::{Hash, Hasher},
    sync::Arc,
};

use proptest::prelude::*;

use super::*;
use crate::{CpgEdgeKind, CpgNode, SourceRange};

#[cfg(feature = "serde")]
static_assertions::assert_impl_all!(RewriteCertificate: serde::Serialize);
#[cfg(feature = "serde")]
static_assertions::assert_not_impl_any!(RewriteCertificate: serde::de::DeserializeOwned);

fn node(kind: CpgNodeKind) -> CpgNode {
    CpgNode::new(NodeId::new(0), kind, SourceRange::default())
}

fn integer_type(width: u16, overflow: IntegerOverflow) -> ExpressionType {
    ExpressionType::Integer(
        IntegerType::fixed(width, true, overflow, ShiftBehavior::Checked)
            .expect("valid signed integer type"),
    )
}

fn effects_from_bits(bits: u8) -> ExpressionEffects {
    ExpressionEffects {
        reads_memory: bits & 1 != 0,
        writes_memory: bits & 2 != 0,
        calls: bits & 4 != 0,
        may_fail: bits & 8 != 0,
        volatile: bits & 16 != 0,
        atomic: bits & 32 != 0,
        nondeterministic: bits & 64 != 0,
        suspends: bits & 128 != 0,
    }
}

fn integer_literal(value: i128, value_type: ExpressionType) -> ExpressionFact {
    ExpressionFact::known(
        ExpressionFactKind::Literal(ExpressionLiteral::Integer(value)),
        Vec::new(),
        value_type,
        ExpressionEffects::PURE,
    )
}

fn binary_fact(
    operator: BinaryOperator,
    operands: Vec<NodeId>,
    value_type: ExpressionType,
) -> ExpressionFact {
    ExpressionFact::known(
        ExpressionFactKind::Binary(operator),
        operands,
        value_type,
        ExpressionEffects::PURE,
    )
}

fn three_node_expression(
    language: Language,
    operator: BinaryOperator,
    left: i128,
    right: i128,
    value_type: ExpressionType,
    fact_order: [usize; 3],
) -> (CodePropertyGraph, NodeId, ExpressionFactTable) {
    let mut cpg = CodePropertyGraph::new(language);
    let root = cpg.add_node(node(CpgNodeKind::BinaryOp {
        operator: Arc::from("frontend-owned-and-ignored"),
    }));
    let left_id = cpg.add_node(node(CpgNodeKind::Literal {
        kind: LiteralKind::Integer(left as i64),
    }));
    let right_id = cpg.add_node(node(CpgNodeKind::Literal {
        kind: LiteralKind::Integer(right as i64),
    }));
    cpg.connect(root, left_id, CpgEdgeKind::AstChild);
    cpg.connect(root, right_id, CpgEdgeKind::AstChild);
    let entries = [
        (
            root,
            binary_fact(operator, vec![left_id, right_id], value_type),
        ),
        (left_id, integer_literal(left, value_type)),
        (right_id, integer_literal(right, value_type)),
    ];
    let facts =
        ExpressionFactTable::try_new(language, fact_order.map(|index| entries[index].clone()))
            .expect("unique facts");
    (cpg, root, facts)
}

fn lower_three(
    operator: BinaryOperator,
    left: i128,
    right: i128,
    value_type: ExpressionType,
) -> ExpressionIr {
    let (cpg, root, facts) =
        three_node_expression(Language::Rust, operator, left, right, value_type, [0, 1, 2]);
    lower_expression_ir(
        &cpg,
        root,
        &facts,
        &SemanticProfile::for_language(Language::Rust),
        ExpressionIrLimits::default(),
    )
}

#[test]
fn complete_postorder_lowering_certifies_checked_additive_identity() {
    let value_type = integer_type(8, IntegerOverflow::Checked);
    let ir = lower_three(BinaryOperator::Add, 37, 0, value_type);
    assert_eq!(ir.completion(), &ExpressionIrCompletion::Complete);
    let root = ir.root().expect("complete root");
    assert_eq!(
        ir.operands(root).expect("root operands"),
        &[ExpressionIrId(0), ExpressionIrId(1)]
    );
    assert!(ir
        .operands(root)
        .expect("operands")
        .iter()
        .all(|child| child.0 < root.0));
    let certificate = ir
        .certify(root, RewriteLaw::AdditiveIdentity)
        .expect("adding zero cannot overflow");
    assert_eq!(certificate.node(), root);
    assert_eq!(certificate.law(), RewriteLaw::AdditiveIdentity);
    assert_eq!(certificate.language(), Language::Rust);
}

#[test]
fn checked_overflow_division_zero_and_shift_bounds_deny_constant_folding() {
    let value_type = integer_type(8, IntegerOverflow::Checked);
    for (operator, left, right) in [
        (BinaryOperator::Add, 127, 1),
        (BinaryOperator::Subtract, -128, 1),
        (BinaryOperator::Multiply, 64, 2),
        (BinaryOperator::Divide, 1, 0),
        (BinaryOperator::Divide, -128, -1),
        (BinaryOperator::Remainder, 1, 0),
        (BinaryOperator::ShiftLeft, 1, 8),
        (BinaryOperator::ShiftRight, 1, -1),
    ] {
        let ir = lower_three(operator, left, right, value_type);
        assert_eq!(
            ir.certify(ir.root().expect("root"), RewriteLaw::ConstantFold),
            Err(RewriteDenial::PreconditionFailed),
            "{operator:?}({left}, {right})"
        );
    }
}

#[test]
fn integer_descriptors_and_shift_modes_obey_formal_boundaries() {
    let invalid = [
        IntegerType {
            width: Some(0),
            signed: true,
            overflow: IntegerOverflow::Checked,
            shift: ShiftBehavior::Checked,
        },
        IntegerType {
            width: Some(129),
            signed: true,
            overflow: IntegerOverflow::Checked,
            shift: ShiftBehavior::Checked,
        },
        IntegerType {
            width: Some(32),
            signed: true,
            overflow: IntegerOverflow::ArbitraryPrecision,
            shift: ShiftBehavior::Checked,
        },
        IntegerType {
            width: None,
            signed: true,
            overflow: IntegerOverflow::Checked,
            shift: ShiftBehavior::Checked,
        },
        IntegerType {
            width: None,
            signed: true,
            overflow: IntegerOverflow::ArbitraryPrecision,
            shift: ShiftBehavior::Masked,
        },
    ];
    for integer in invalid {
        assert!(!integer.is_valid());
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let literal = cpg.add_node(node(CpgNodeKind::Literal {
            kind: LiteralKind::Integer(1),
        }));
        let facts = ExpressionFactTable::try_new(
            Language::Rust,
            [(
                literal,
                integer_literal(1, ExpressionType::Integer(integer)),
            )],
        )
        .expect("unique malformed fact");
        assert_eq!(
            lower_expression_ir(
                &cpg,
                literal,
                &facts,
                &SemanticProfile::for_language(Language::Rust),
                ExpressionIrLimits::default(),
            )
            .completion(),
            &ExpressionIrCompletion::Malformed(ExpressionFactError::LiteralTypeMismatch(literal))
        );
    }

    let masked = ExpressionType::Integer(
        IntegerType::fixed(32, true, IntegerOverflow::Wrapping, ShiftBehavior::Masked)
            .expect("valid masked integer"),
    );
    let ir = lower_three(BinaryOperator::ShiftLeft, 1, -1, masked);
    ir.certify(ir.root().expect("root"), RewriteLaw::ConstantFold)
        .expect("masked shifts accept every signed count");
}

#[test]
fn finite_reassociation_rejects_contradictory_float_literals() {
    let finite_type = ExpressionType::Float(FloatType {
        width: FloatWidth::F64,
        mode: FloatMode::FiniteReassociation,
    });
    for bits in [
        (-0.0_f64).to_bits(),
        f64::INFINITY.to_bits(),
        f64::NEG_INFINITY.to_bits(),
        f64::NAN.to_bits(),
    ] {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let literal = cpg.add_node(node(CpgNodeKind::Literal {
            kind: LiteralKind::Float(f64::from_bits(bits)),
        }));
        let facts = ExpressionFactTable::try_new(
            Language::Rust,
            [(
                literal,
                ExpressionFact::known(
                    ExpressionFactKind::Literal(ExpressionLiteral::Float64(bits)),
                    Vec::new(),
                    finite_type,
                    ExpressionEffects::PURE,
                ),
            )],
        )
        .expect("unique float fact");
        assert_eq!(
            lower_expression_ir(
                &cpg,
                literal,
                &facts,
                &SemanticProfile::for_language(Language::Rust),
                ExpressionIrLimits::default(),
            )
            .completion(),
            &ExpressionIrCompletion::Malformed(ExpressionFactError::LiteralTypeMismatch(literal))
        );
    }
    for value in [0.0_f64, 1.0, -1.0, f64::MIN_POSITIVE, f64::MAX] {
        assert!(float_literal_is_valid(
            value,
            value.to_bits() == (-0.0_f64).to_bits(),
            match finite_type {
                ExpressionType::Float(float_type) => float_type,
                _ => unreachable!(),
            }
        ));
    }
}

#[test]
fn strict_float_nan_and_signed_zero_deny_algebraic_laws() {
    let float_type = ExpressionType::Float(FloatType {
        width: FloatWidth::F64,
        mode: FloatMode::StrictIeee754,
    });
    for (left, right) in [
        ((-0.0_f64).to_bits(), 0.0_f64.to_bits()),
        (f64::NAN.to_bits(), 1.0_f64.to_bits()),
        (f64::INFINITY.to_bits(), 0.0_f64.to_bits()),
    ] {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let root = cpg.add_node(node(CpgNodeKind::BinaryOp {
            operator: Arc::from("+"),
        }));
        let left_id = cpg.add_node(node(CpgNodeKind::Literal {
            kind: LiteralKind::Float(f64::from_bits(left)),
        }));
        let right_id = cpg.add_node(node(CpgNodeKind::Literal {
            kind: LiteralKind::Float(f64::from_bits(right)),
        }));
        cpg.connect(root, left_id, CpgEdgeKind::AstChild);
        cpg.connect(root, right_id, CpgEdgeKind::AstChild);
        let facts = ExpressionFactTable::try_new(
            Language::Rust,
            [
                (
                    root,
                    binary_fact(BinaryOperator::Add, vec![left_id, right_id], float_type),
                ),
                (
                    left_id,
                    ExpressionFact::known(
                        ExpressionFactKind::Literal(ExpressionLiteral::Float64(left)),
                        Vec::new(),
                        float_type,
                        ExpressionEffects::PURE,
                    ),
                ),
                (
                    right_id,
                    ExpressionFact::known(
                        ExpressionFactKind::Literal(ExpressionLiteral::Float64(right)),
                        Vec::new(),
                        float_type,
                        ExpressionEffects::PURE,
                    ),
                ),
            ],
        )
        .expect("float facts");
        let ir = lower_expression_ir(
            &cpg,
            root,
            &facts,
            &SemanticProfile::for_language(Language::Rust),
            ExpressionIrLimits::default(),
        );
        for law in [
            RewriteLaw::AdditiveIdentity,
            RewriteLaw::Commutativity,
            RewriteLaw::Associativity,
            RewriteLaw::ConstantFold,
        ] {
            assert_eq!(
                ir.certify(ir.root().expect("float root"), law),
                Err(RewriteDenial::PreconditionFailed)
            );
        }
    }
}

#[test]
fn every_named_law_has_positive_and_denied_examples() {
    let wrapping = integer_type(8, IntegerOverflow::Wrapping);
    let addition = lower_three(BinaryOperator::Add, 1, 0, wrapping);
    let addition_root = addition.root().expect("addition root");
    for law in [
        RewriteLaw::ConstantFold,
        RewriteLaw::Commutativity,
        RewriteLaw::Associativity,
        RewriteLaw::AdditiveIdentity,
    ] {
        addition
            .certify(addition_root, law)
            .unwrap_or_else(|error| panic!("{law:?} should be admitted: {error}"));
    }

    let multiplication = lower_three(BinaryOperator::Multiply, 7, 1, wrapping);
    multiplication
        .certify(
            multiplication.root().expect("multiplication root"),
            RewriteLaw::MultiplicativeIdentity,
        )
        .expect("integer multiplication by one");

    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let root = cpg.add_node(node(CpgNodeKind::BinaryOp {
        operator: Arc::from("&&"),
    }));
    let left = cpg.add_node(node(CpgNodeKind::Literal {
        kind: LiteralKind::Bool(false),
    }));
    let right = cpg.add_node(node(CpgNodeKind::Literal {
        kind: LiteralKind::Bool(true),
    }));
    cpg.connect(root, left, CpgEdgeKind::AstChild);
    cpg.connect(root, right, CpgEdgeKind::AstChild);
    let bool_literal = |value| {
        ExpressionFact::known(
            ExpressionFactKind::Literal(ExpressionLiteral::Bool(value)),
            Vec::new(),
            ExpressionType::Bool,
            ExpressionEffects::PURE,
        )
    };
    let facts = ExpressionFactTable::try_new(
        Language::Rust,
        [
            (
                root,
                binary_fact(
                    BinaryOperator::LogicalAnd,
                    vec![left, right],
                    ExpressionType::Bool,
                ),
            ),
            (left, bool_literal(false)),
            (right, bool_literal(true)),
        ],
    )
    .expect("Boolean facts");
    let bool_ir = lower_expression_ir(
        &cpg,
        root,
        &facts,
        &SemanticProfile::for_language(Language::Rust),
        ExpressionIrLimits::default(),
    );
    bool_ir
        .certify(
            bool_ir.root().expect("Boolean root"),
            RewriteLaw::BooleanIdentity,
        )
        .expect("x && true has a pure Boolean identity");

    for law in RewriteLaw::ALL {
        let denied = SemanticProfile::for_language(Language::Rust).with_rewrite_law(law, false);
        let (cpg, root, facts) = three_node_expression(
            Language::Rust,
            BinaryOperator::Add,
            1,
            0,
            wrapping,
            [0, 1, 2],
        );
        let ir = lower_expression_ir(&cpg, root, &facts, &denied, ExpressionIrLimits::default());
        assert_eq!(
            ir.certify(ir.root().expect("root"), law),
            Err(RewriteDenial::LawDisabled)
        );
    }
}

#[test]
fn calls_memory_aliasing_and_unknown_nodes_propagate_evidence_modes() {
    let value_type = integer_type(32, IntegerOverflow::Wrapping);

    for (identity, target, expected) in [
        (
            CallIdentity::Resolved(NodeId(1)),
            Some(NodeId(1)),
            ExpressionEvidenceMode::Pure,
        ),
        (
            CallIdentity::External(Arc::from("foreign")),
            None,
            ExpressionEvidenceMode::IdentityOnly,
        ),
        (
            CallIdentity::Unresolved,
            None,
            ExpressionEvidenceMode::IdentityOnly,
        ),
    ] {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let call = cpg.add_node(node(CpgNodeKind::Call {
            target,
            is_method: false,
        }));
        if target.is_some() {
            let target_node = cpg.add_node(node(CpgNodeKind::Variable {
                name: Arc::from("callee"),
                var_type: None,
                scope: crate::ScopeId::GLOBAL,
                is_mutable: false,
            }));
            assert_eq!(target_node, NodeId(1));
        }
        let facts = ExpressionFactTable::try_new(
            Language::Rust,
            [(
                call,
                ExpressionFact::known(
                    ExpressionFactKind::Call(identity),
                    Vec::new(),
                    value_type,
                    ExpressionEffects::PURE,
                ),
            )],
        )
        .expect("call fact");
        let ir = lower_expression_ir(
            &cpg,
            call,
            &facts,
            &SemanticProfile::for_language(Language::Rust),
            ExpressionIrLimits::default(),
        );
        assert_eq!(
            ir.node(ir.root().expect("call root")).expect("call").mode,
            expected
        );
    }

    for (access, expected) in [
        (MemoryAccess::Immutable, ExpressionEvidenceMode::Pure),
        (MemoryAccess::Shared, ExpressionEvidenceMode::IdentityOnly),
        (MemoryAccess::Mutable, ExpressionEvidenceMode::IdentityOnly),
        (MemoryAccess::Volatile, ExpressionEvidenceMode::IdentityOnly),
        (MemoryAccess::Atomic, ExpressionEvidenceMode::IdentityOnly),
    ] {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let load = cpg.add_node(node(CpgNodeKind::IndexAccess));
        let index = cpg.add_node(node(CpgNodeKind::Literal {
            kind: LiteralKind::Integer(0),
        }));
        cpg.connect(load, index, CpgEdgeKind::AstChild);
        let facts = ExpressionFactTable::try_new(
            Language::Rust,
            [
                (
                    load,
                    ExpressionFact::known(
                        ExpressionFactKind::Load {
                            location: NodeId(99),
                            access,
                        },
                        vec![index],
                        value_type,
                        ExpressionEffects::PURE,
                    ),
                ),
                (index, integer_literal(0, value_type)),
            ],
        )
        .expect("load facts");
        let ir = lower_expression_ir(
            &cpg,
            load,
            &facts,
            &SemanticProfile::for_language(Language::Rust),
            ExpressionIrLimits::default(),
        );
        assert_eq!(
            ir.node(ir.root().expect("load root")).expect("load").mode,
            expected
        );
    }

    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let opaque = cpg.add_node(node(CpgNodeKind::Await));
    let facts = ExpressionFactTable::try_new(
        Language::Rust,
        [(
            opaque,
            ExpressionFact::known(
                ExpressionFactKind::Opaque,
                Vec::new(),
                value_type,
                ExpressionEffects::PURE,
            ),
        )],
    )
    .expect("opaque fact");
    let ir = lower_expression_ir(
        &cpg,
        opaque,
        &facts,
        &SemanticProfile::for_language(Language::Rust),
        ExpressionIrLimits::default(),
    );
    assert_eq!(
        ir.node(ir.root().expect("opaque root"))
            .expect("opaque")
            .mode,
        ExpressionEvidenceMode::IdentityOnly
    );
}

#[test]
fn every_profile_dimension_and_every_nonempty_effect_fail_closed() {
    let profile = SemanticProfile::for_language(Language::Rust);
    assert!(profile.is_complete());
    let mut incomplete_profiles = Vec::new();
    macro_rules! omit {
        ($field:ident) => {{
            let mut candidate = profile.clone();
            candidate.$field = SemanticKnowledge::Unknown;
            incomplete_profiles.push(candidate);
        }};
    }
    omit!(effects);
    omit!(evaluation_order);
    omit!(integers);
    omit!(floating_point);
    omit!(exceptions);
    omit!(short_circuit);
    omit!(aliasing);
    omit!(calls);
    omit!(volatile_atomic);
    omit!(bindings);
    assert_eq!(incomplete_profiles.len(), 10);
    assert!(incomplete_profiles
        .iter()
        .all(|candidate| !candidate.is_complete()));

    let value_type = integer_type(8, IntegerOverflow::Wrapping);
    let (profile_cpg, profile_root, profile_facts) = three_node_expression(
        Language::Rust,
        BinaryOperator::Add,
        1,
        0,
        value_type,
        [0, 1, 2],
    );
    for candidate in &incomplete_profiles {
        let ir = lower_expression_ir(
            &profile_cpg,
            profile_root,
            &profile_facts,
            candidate,
            ExpressionIrLimits::default(),
        );
        let root = ir.root().expect("identity-preserving root");
        assert_eq!(
            ir.node(root).expect("node").mode,
            ExpressionEvidenceMode::Unknown
        );
        assert_eq!(
            ir.certify(root, RewriteLaw::AdditiveIdentity),
            Err(RewriteDenial::NonPureEvidence)
        );
    }

    for bits in 1..=u8::MAX {
        let (cpg, root, mut facts) = three_node_expression(
            Language::Rust,
            BinaryOperator::Add,
            1,
            0,
            value_type,
            [0, 1, 2],
        );
        facts.facts.get_mut(&root).expect("root fact").effects =
            SemanticKnowledge::Known(effects_from_bits(bits));
        let ir = lower_expression_ir(&cpg, root, &facts, &profile, ExpressionIrLimits::default());
        assert_eq!(
            ir.node(ir.root().expect("identity root"))
                .expect("node")
                .mode,
            ExpressionEvidenceMode::IdentityOnly
        );
        assert_eq!(
            ir.certify(
                ir.root().expect("identity root"),
                RewriteLaw::AdditiveIdentity
            ),
            Err(RewriteDenial::NonPureEvidence)
        );
    }

    let (cpg, root, mut facts) = three_node_expression(
        Language::Rust,
        BinaryOperator::Add,
        1,
        0,
        value_type,
        [0, 1, 2],
    );
    let child = facts.get(root).expect("root").operands[0];
    facts.facts.get_mut(&child).expect("child").effects =
        SemanticKnowledge::Known(ExpressionEffects {
            nondeterministic: true,
            ..ExpressionEffects::PURE
        });
    let ir = lower_expression_ir(&cpg, root, &facts, &profile, ExpressionIrLimits::default());
    assert_eq!(
        ir.node(ir.root().expect("root")).expect("root node").mode,
        ExpressionEvidenceMode::IdentityOnly,
        "descendant effects propagate to the root"
    );
}

#[test]
fn shared_subexpressions_emit_once_and_incomplete_limits_mint_no_certificate() {
    let value_type = integer_type(8, IntegerOverflow::Wrapping);
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let root = cpg.add_node(node(CpgNodeKind::BinaryOp {
        operator: Arc::from("+"),
    }));
    let shared = cpg.add_node(node(CpgNodeKind::Literal {
        kind: LiteralKind::Integer(1),
    }));
    cpg.connect(root, shared, CpgEdgeKind::AstChild);
    cpg.connect(root, shared, CpgEdgeKind::AstChild);
    let facts = ExpressionFactTable::try_new(
        Language::Rust,
        [
            (
                root,
                binary_fact(BinaryOperator::Add, vec![shared, shared], value_type),
            ),
            (shared, integer_literal(1, value_type)),
        ],
    )
    .expect("shared facts");
    let profile = SemanticProfile::for_language(Language::Rust);
    let ir = lower_expression_ir(&cpg, root, &facts, &profile, ExpressionIrLimits::default());
    assert_eq!(ir.metrics().nodes, 2);
    assert_eq!(
        ir.operands(ir.root().expect("root")).expect("operands"),
        &[ExpressionIrId(0), ExpressionIrId(0)]
    );

    for (limits, expected) in [
        (
            ExpressionIrLimits {
                max_work: 0,
                ..ExpressionIrLimits::default()
            },
            ExpressionIrCompletion::WorkLimited {
                actual: 0,
                maximum: 0,
            },
        ),
        (
            ExpressionIrLimits {
                max_nodes: 1,
                ..ExpressionIrLimits::default()
            },
            ExpressionIrCompletion::NodeLimited {
                actual: 1,
                maximum: 1,
            },
        ),
        (
            ExpressionIrLimits {
                max_operands: 1,
                ..ExpressionIrLimits::default()
            },
            ExpressionIrCompletion::OperandLimited {
                actual: 2,
                maximum: 1,
            },
        ),
    ] {
        let limited = lower_expression_ir(&cpg, root, &facts, &profile, limits);
        assert_eq!(limited.completion(), &expected);
        assert_eq!(
            limited.certify(ExpressionIrId(0), RewriteLaw::ConstantFold),
            Err(RewriteDenial::IncompleteLowering)
        );
    }
}

#[test]
fn malformed_cycles_topology_types_literals_bindings_and_calls_are_rejected() {
    let value_type = integer_type(8, IntegerOverflow::Wrapping);

    let mut cyclic = CodePropertyGraph::new(Language::Rust);
    let cycle = cyclic.add_node(node(CpgNodeKind::UnaryOp {
        operator: Arc::from("~"),
    }));
    cyclic.connect(cycle, cycle, CpgEdgeKind::AstChild);
    let cycle_facts = ExpressionFactTable::try_new(
        Language::Rust,
        [(
            cycle,
            ExpressionFact::known(
                ExpressionFactKind::Unary(UnaryOperator::BitwiseNot),
                vec![cycle],
                value_type,
                ExpressionEffects::PURE,
            ),
        )],
    )
    .expect("cycle fact");
    let cycle_ir = lower_expression_ir(
        &cyclic,
        cycle,
        &cycle_facts,
        &SemanticProfile::for_language(Language::Rust),
        ExpressionIrLimits::default(),
    );
    assert_eq!(
        cycle_ir.completion(),
        &ExpressionIrCompletion::Malformed(ExpressionFactError::Cycle(cycle))
    );

    let (cpg, root, mut facts) = three_node_expression(
        Language::Rust,
        BinaryOperator::Add,
        1,
        2,
        value_type,
        [0, 1, 2],
    );
    facts.facts.get_mut(&root).expect("root").operands.reverse();
    assert!(matches!(
        lower_expression_ir(
            &cpg,
            root,
            &facts,
            &SemanticProfile::for_language(Language::Rust),
            ExpressionIrLimits::default()
        )
        .completion(),
        ExpressionIrCompletion::Malformed(ExpressionFactError::OperandTopologyMismatch(_))
    ));

    let (cpg, root, mut facts) = three_node_expression(
        Language::Rust,
        BinaryOperator::Add,
        1,
        2,
        value_type,
        [0, 1, 2],
    );
    let first = facts.get(root).expect("root").operands[0];
    facts.facts.get_mut(&first).expect("first").value_type =
        SemanticKnowledge::Known(ExpressionType::Bool);
    assert!(matches!(
        lower_expression_ir(
            &cpg,
            root,
            &facts,
            &SemanticProfile::for_language(Language::Rust),
            ExpressionIrLimits::default()
        )
        .completion(),
        ExpressionIrCompletion::Malformed(ExpressionFactError::TypeMismatch(_))
    ));

    let mut literal_cpg = CodePropertyGraph::new(Language::Rust);
    let literal = literal_cpg.add_node(node(CpgNodeKind::Literal {
        kind: LiteralKind::Integer(1),
    }));
    let mismatch =
        ExpressionFactTable::try_new(Language::Rust, [(literal, integer_literal(2, value_type))])
            .expect("fact");
    assert_eq!(
        lower_expression_ir(
            &literal_cpg,
            literal,
            &mismatch,
            &SemanticProfile::for_language(Language::Rust),
            ExpressionIrLimits::default()
        )
        .completion(),
        &ExpressionIrCompletion::Malformed(ExpressionFactError::SourceLiteralMismatch(literal))
    );

    let mut binding_cpg = CodePropertyGraph::new(Language::Rust);
    let definition = binding_cpg.add_node(node(CpgNodeKind::Variable {
        name: Arc::from("x"),
        var_type: None,
        scope: crate::ScopeId::GLOBAL,
        is_mutable: false,
    }));
    let binding = binding_cpg.add_node(node(CpgNodeKind::Identifier {
        name: Arc::from("x"),
        definition: Some(definition),
    }));
    let binding_facts = ExpressionFactTable::try_new(
        Language::Rust,
        [(
            binding,
            ExpressionFact::known(
                ExpressionFactKind::Binding(BindingIdentity {
                    definition: NodeId(definition.0 + 1),
                    scope: 0,
                }),
                Vec::new(),
                value_type,
                ExpressionEffects::PURE,
            ),
        )],
    )
    .expect("binding fact");
    assert_eq!(
        lower_expression_ir(
            &binding_cpg,
            binding,
            &binding_facts,
            &SemanticProfile::for_language(Language::Rust),
            ExpressionIrLimits::default()
        )
        .completion(),
        &ExpressionIrCompletion::Malformed(ExpressionFactError::BindingMismatch(binding))
    );

    let mut call_cpg = CodePropertyGraph::new(Language::Rust);
    let call = call_cpg.add_node(node(CpgNodeKind::Call {
        target: None,
        is_method: false,
    }));
    let call_facts = ExpressionFactTable::try_new(
        Language::Rust,
        [(
            call,
            ExpressionFact::known(
                ExpressionFactKind::Call(CallIdentity::Resolved(NodeId(42))),
                Vec::new(),
                value_type,
                ExpressionEffects::PURE,
            ),
        )],
    )
    .expect("call fact");
    assert_eq!(
        lower_expression_ir(
            &call_cpg,
            call,
            &call_facts,
            &SemanticProfile::for_language(Language::Rust),
            ExpressionIrLimits::default()
        )
        .completion(),
        &ExpressionIrCompletion::Malformed(ExpressionFactError::CallMismatch(call))
    );

    let empty = ExpressionFactTable::try_new(Language::Rust, []).expect("empty table");
    assert_eq!(
        lower_expression_ir(
            &literal_cpg,
            literal,
            &empty,
            &SemanticProfile::for_language(Language::Rust),
            ExpressionIrLimits::default(),
        )
        .completion(),
        &ExpressionIrCompletion::Malformed(ExpressionFactError::MissingFact(literal))
    );

    let absent = NodeId(u32::MAX);
    let absent_facts =
        ExpressionFactTable::try_new(Language::Rust, [(absent, integer_literal(1, value_type))])
            .expect("absent fact");
    assert_eq!(
        lower_expression_ir(
            &literal_cpg,
            absent,
            &absent_facts,
            &SemanticProfile::for_language(Language::Rust),
            ExpressionIrLimits::default(),
        )
        .completion(),
        &ExpressionIrCompletion::Malformed(ExpressionFactError::MissingCpgNode(absent))
    );

    let mut statement_cpg = CodePropertyGraph::new(Language::Rust);
    let statement = statement_cpg.add_node(node(CpgNodeKind::Return));
    let statement_facts = ExpressionFactTable::try_new(
        Language::Rust,
        [(statement, integer_literal(1, value_type))],
    )
    .expect("statement fact");
    assert_eq!(
        lower_expression_ir(
            &statement_cpg,
            statement,
            &statement_facts,
            &SemanticProfile::for_language(Language::Rust),
            ExpressionIrLimits::default(),
        )
        .completion(),
        &ExpressionIrCompletion::Malformed(ExpressionFactError::NonExpressionSource(statement))
    );

    let language_mismatch =
        ExpressionFactTable::try_new(Language::Java, [(literal, integer_literal(1, value_type))])
            .expect("foreign facts");
    assert_eq!(
        lower_expression_ir(
            &literal_cpg,
            literal,
            &language_mismatch,
            &SemanticProfile::for_language(Language::Rust),
            ExpressionIrLimits::default(),
        )
        .completion(),
        &ExpressionIrCompletion::Malformed(ExpressionFactError::LanguageMismatch {
            cpg: Language::Rust,
            facts: Language::Java,
            profile: Language::Rust,
        })
    );
}

#[test]
fn built_in_profile_set_is_exact_and_unsupported_languages_fail_closed() {
    assert!(BUILTIN_EXPRESSION_PROFILE_LANGUAGES
        .iter()
        .all(|language| SemanticProfile::for_language(*language).is_complete()));
    for language in [
        Language::Unknown,
        Language::Rholang,
        Language::MeTTa,
        Language::Json,
        Language::Bash,
    ] {
        assert!(!SemanticProfile::for_language(language).is_complete());
    }
}

#[test]
fn source_text_and_operator_spelling_do_not_affect_lowering() {
    let value_type = integer_type(32, IntegerOverflow::Wrapping);
    let (first_cpg, first_root, first_facts) = three_node_expression(
        Language::Rust,
        BinaryOperator::Add,
        1,
        0,
        value_type,
        [0, 1, 2],
    );
    let mut second_cpg = CodePropertyGraph::new(Language::Rust)
        .with_source_code("entirely unrelated retained source text");
    let second_root = second_cpg.add_node(node(CpgNodeKind::BinaryOp {
        operator: Arc::from("spelling-is-not-consumed"),
    }));
    let left = second_cpg.add_node(node(CpgNodeKind::Literal {
        kind: LiteralKind::Integer(1),
    }));
    let right = second_cpg.add_node(node(CpgNodeKind::Literal {
        kind: LiteralKind::Integer(0),
    }));
    second_cpg.connect(second_root, left, CpgEdgeKind::AstChild);
    second_cpg.connect(second_root, right, CpgEdgeKind::AstChild);
    let second_facts = ExpressionFactTable::try_new(
        Language::Rust,
        [
            (
                second_root,
                binary_fact(BinaryOperator::Add, vec![left, right], value_type),
            ),
            (left, integer_literal(1, value_type)),
            (right, integer_literal(0, value_type)),
        ],
    )
    .expect("second facts");
    let profile = SemanticProfile::for_language(Language::Rust);
    let first = lower_expression_ir(
        &first_cpg,
        first_root,
        &first_facts,
        &profile,
        ExpressionIrLimits::default(),
    );
    let second = lower_expression_ir(
        &second_cpg,
        second_root,
        &second_facts,
        &profile,
        ExpressionIrLimits::default(),
    );
    assert_eq!(first, second);
}

#[test]
fn binding_identity_uses_definition_and_scope_not_identifier_spelling() {
    fn lower_binding(name: &str, scope: u32) -> ExpressionIr {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let definition = cpg.add_node(node(CpgNodeKind::Variable {
            name: Arc::from("definition"),
            var_type: None,
            scope: crate::ScopeId::GLOBAL,
            is_mutable: false,
        }));
        let binding = cpg.add_node(node(CpgNodeKind::Identifier {
            name: Arc::from(name),
            definition: Some(definition),
        }));
        let facts = ExpressionFactTable::try_new(
            Language::Rust,
            [(
                binding,
                ExpressionFact::known(
                    ExpressionFactKind::Binding(BindingIdentity { definition, scope }),
                    Vec::new(),
                    integer_type(32, IntegerOverflow::Wrapping),
                    ExpressionEffects::PURE,
                ),
            )],
        )
        .expect("binding fact");
        lower_expression_ir(
            &cpg,
            binding,
            &facts,
            &SemanticProfile::for_language(Language::Rust),
            ExpressionIrLimits::default(),
        )
    }

    let original = lower_binding("x", 7);
    let alpha_renamed = lower_binding("renamed", 7);
    let shadowed = lower_binding("x", 8);
    assert_eq!(original, alpha_renamed);
    assert_ne!(original, shadowed);
}

fn checked_i8_oracle(operator: BinaryOperator, left: i8, right: i8) -> Option<i8> {
    match operator {
        BinaryOperator::Add => left.checked_add(right),
        BinaryOperator::Subtract => left.checked_sub(right),
        BinaryOperator::Multiply => left.checked_mul(right),
        BinaryOperator::Divide => left.checked_div(right),
        BinaryOperator::Remainder => left.checked_rem(right),
        _ => None,
    }
}

fn arithmetic_operator(tag: u8) -> BinaryOperator {
    match tag % 5 {
        0 => BinaryOperator::Add,
        1 => BinaryOperator::Subtract,
        2 => BinaryOperator::Multiply,
        3 => BinaryOperator::Divide,
        _ => BinaryOperator::Remainder,
    }
}

proptest! {
    #[test]
    fn constant_fold_certificate_matches_independent_checked_i8_evaluator(
        left in any::<i8>(),
        right in any::<i8>(),
        tag in 0_u8..5,
    ) {
        let operator = arithmetic_operator(tag);
        let ir = lower_three(
            operator,
            i128::from(left),
            i128::from(right),
            integer_type(8, IntegerOverflow::Checked),
        );
        let actual = ir
            .certify(ir.root().expect("root"), RewriteLaw::ConstantFold)
            .is_ok();
        prop_assert_eq!(actual, checked_i8_oracle(operator, left, right).is_some());
    }

    #[test]
    fn fact_insertion_permutations_preserve_byte_ordered_ir(
        left in -100_i8..100,
        right in -100_i8..100,
        reverse in any::<bool>(),
    ) {
        let value_type = integer_type(8, IntegerOverflow::Wrapping);
        let order = if reverse { [2, 1, 0] } else { [0, 1, 2] };
        let (cpg, root, facts) = three_node_expression(
            Language::Rust,
            BinaryOperator::Add,
            i128::from(left),
            i128::from(right),
            value_type,
            order,
        );
        let first = lower_expression_ir(
            &cpg,
            root,
            &facts,
            &SemanticProfile::for_language(Language::Rust),
            ExpressionIrLimits::default(),
        );
        let reordered = ExpressionFactTable::try_new(
            Language::Rust,
            facts.facts.iter().rev().map(|(id, fact)| (*id, fact.clone())),
        ).expect("reordered facts");
        let second = lower_expression_ir(
            &cpg,
            root,
            &reordered,
            &SemanticProfile::for_language(Language::Rust),
            ExpressionIrLimits::default(),
        );
        prop_assert_eq!(first, second);
    }

    #[test]
    fn work_cap_is_exact_and_monotone(max_work in 0_usize..10) {
        let value_type = integer_type(8, IntegerOverflow::Wrapping);
        let (cpg, root, facts) = three_node_expression(
            Language::Rust,
            BinaryOperator::Add,
            1,
            2,
            value_type,
            [0, 1, 2],
        );
        let ir = lower_expression_ir(
            &cpg,
            root,
            &facts,
            &SemanticProfile::for_language(Language::Rust),
            ExpressionIrLimits {
                max_work,
                ..ExpressionIrLimits::default()
            },
        );
        if max_work >= 6 {
            prop_assert_eq!(ir.completion(), &ExpressionIrCompletion::Complete);
            prop_assert_eq!(ir.metrics().work, 6);
        } else {
            prop_assert_eq!(
                ir.completion(),
                &ExpressionIrCompletion::WorkLimited {
                    actual: max_work,
                    maximum: max_work,
                }
            );
        }
    }

    #[test]
    fn any_explicit_effect_bit_denies_rewrite(bits in 1_u8..=u8::MAX) {
        let value_type = integer_type(8, IntegerOverflow::Wrapping);
        let (cpg, root, mut facts) = three_node_expression(
            Language::Rust,
            BinaryOperator::Add,
            1,
            0,
            value_type,
            [0, 1, 2],
        );
        facts.facts.get_mut(&root).expect("root").effects =
            SemanticKnowledge::Known(effects_from_bits(bits));
        let ir = lower_expression_ir(
            &cpg,
            root,
            &facts,
            &SemanticProfile::for_language(Language::Rust),
            ExpressionIrLimits::default(),
        );
        prop_assert_eq!(
            ir.certify(ir.root().expect("root"), RewriteLaw::AdditiveIdentity),
            Err(RewriteDenial::NonPureEvidence)
        );
    }

    #[test]
    fn arbitrary_unary_chains_are_flat_deterministic_and_backward_referencing(
        depth in 0_usize..128,
    ) {
        let (cpg, root, facts) = build_unary_chain(depth);
        let profile = SemanticProfile::for_language(Language::Rust);
        let first = lower_expression_ir(
            &cpg,
            root,
            &facts,
            &profile,
            ExpressionIrLimits::default(),
        );
        let second = lower_expression_ir(
            &cpg,
            root,
            &facts,
            &profile,
            ExpressionIrLimits::default(),
        );
        prop_assert_eq!(&first, &second);
        prop_assert_eq!(first.metrics().nodes, depth + 1);
        for index in 0..first.metrics().nodes {
            let id = ExpressionIrId(u32::try_from(index).expect("generated depth fits u32"));
            for child in first.operands(id).expect("valid operand range") {
                prop_assert!(child.0 < id.0);
            }
        }
    }

    #[test]
    fn node_and_operand_caps_are_exact(depth in 1_usize..32, cap in 0_usize..32) {
        let (cpg, root, facts) = build_unary_chain(depth);
        let profile = SemanticProfile::for_language(Language::Rust);
        let node_ir = lower_expression_ir(
            &cpg,
            root,
            &facts,
            &profile,
            ExpressionIrLimits {
                max_nodes: cap,
                ..ExpressionIrLimits::default()
            },
        );
        if cap > depth {
            prop_assert_eq!(node_ir.completion(), &ExpressionIrCompletion::Complete);
        } else {
            prop_assert_eq!(
                node_ir.completion(),
                &ExpressionIrCompletion::NodeLimited { actual: cap, maximum: cap }
            );
        }

        let operand_ir = lower_expression_ir(
            &cpg,
            root,
            &facts,
            &profile,
            ExpressionIrLimits {
                max_operands: cap,
                ..ExpressionIrLimits::default()
            },
        );
        if cap >= depth {
            prop_assert_eq!(operand_ir.completion(), &ExpressionIrCompletion::Complete);
        } else {
            prop_assert_eq!(
                operand_ir.completion(),
                &ExpressionIrCompletion::OperandLimited { actual: cap + 1, maximum: cap }
            );
        }
    }

    #[test]
    fn masked_shift_admission_matches_the_signed_count_model(count in any::<i32>()) {
        let value_type = ExpressionType::Integer(
            IntegerType::fixed(
                32,
                true,
                IntegerOverflow::Wrapping,
                ShiftBehavior::Masked,
            ).expect("valid masked integer")
        );
        let ir = lower_three(BinaryOperator::ShiftLeft, 1, i128::from(count), value_type);
        prop_assert!(ir
            .certify(ir.root().expect("root"), RewriteLaw::ConstantFold)
            .is_ok());
    }

    #[test]
    fn finite_reassociation_bit_validation_matches_ieee_oracle(bits in any::<u64>()) {
        let value = f64::from_bits(bits);
        let actual = float_literal_is_valid(
            value,
            bits == (-0.0_f64).to_bits(),
            FloatType { width: FloatWidth::F64, mode: FloatMode::FiniteReassociation },
        );
        prop_assert_eq!(actual, value.is_finite() && bits != (-0.0_f64).to_bits());
    }

    #[cfg(feature = "serde")]
    #[test]
    fn persisted_random_artifacts_preserve_bytes_and_revoke_authority(
        left in any::<i8>(),
        right in any::<i8>(),
    ) {
        let ir = lower_three(
            BinaryOperator::Add,
            i128::from(left),
            i128::from(right),
            integer_type(8, IntegerOverflow::Wrapping),
        );
        let bytes = serde_json::to_vec(&ir).expect("serialize generated IR");
        let decoded: ExpressionIr = serde_json::from_slice(&bytes).expect("deserialize generated IR");
        prop_assert_eq!(serde_json::to_vec(&decoded).expect("reserialize"), bytes);
        prop_assert_eq!(decoded.provenance(), ExpressionIrProvenance::PersistedIdentityOnly);
        prop_assert_eq!(
            decoded.certify(ExpressionIrId(0), RewriteLaw::ConstantFold),
            Err(RewriteDenial::PersistedArtifact)
        );
    }
}

#[cfg(feature = "serde")]
#[test]
fn serde_round_trip_preserves_complete_and_incomplete_artifacts() {
    let value_type = integer_type(8, IntegerOverflow::Wrapping);
    let (cpg, root, facts) = three_node_expression(
        Language::Rust,
        BinaryOperator::Add,
        1,
        0,
        value_type,
        [0, 1, 2],
    );
    let profile = SemanticProfile::for_language(Language::Rust);
    for limits in [
        ExpressionIrLimits::default(),
        ExpressionIrLimits {
            max_work: 2,
            ..ExpressionIrLimits::default()
        },
    ] {
        let ir = lower_expression_ir(&cpg, root, &facts, &profile, limits);
        assert_eq!(ir.provenance(), ExpressionIrProvenance::LiveLowering);
        let bytes = serde_json::to_vec(&ir).expect("serialize flat IR");
        let decoded: ExpressionIr = serde_json::from_slice(&bytes).expect("deserialize flat IR");
        assert_eq!(
            decoded.provenance(),
            ExpressionIrProvenance::PersistedIdentityOnly
        );
        assert_eq!(decoded.nodes, ir.nodes);
        assert_eq!(decoded.operands, ir.operands);
        assert_eq!(decoded.sources, ir.sources);
        assert_eq!(decoded.root, ir.root);
        assert_eq!(decoded.profile, ir.profile);
        assert_eq!(decoded.completion, ir.completion);
        assert_eq!(decoded.metrics, ir.metrics);
        assert_eq!(
            serde_json::to_vec(&decoded).expect("reserialize persisted IR"),
            bytes,
            "provenance is intentionally absent from the stable bytes"
        );
        assert_eq!(
            decoded.certify(ExpressionIrId(0), RewriteLaw::AdditiveIdentity),
            Err(RewriteDenial::PersistedArtifact)
        );
    }
}

struct FormattingSink;

impl std::fmt::Write for FormattingSink {
    fn write_str(&mut self, _value: &str) -> std::fmt::Result {
        Ok(())
    }
}

fn build_unary_chain(depth: usize) -> (CodePropertyGraph, NodeId, ExpressionFactTable) {
    let value_type = integer_type(32, IntegerOverflow::Wrapping);
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let mut nodes = Vec::with_capacity(depth + 1);
    for _ in 0..depth {
        nodes.push(cpg.add_node(node(CpgNodeKind::UnaryOp {
            operator: Arc::from("typed"),
        })));
    }
    let leaf = cpg.add_node(node(CpgNodeKind::Literal {
        kind: LiteralKind::Integer(1),
    }));
    nodes.push(leaf);
    for pair in nodes.windows(2) {
        cpg.connect(pair[0], pair[1], CpgEdgeKind::AstChild);
    }
    let mut facts = Vec::with_capacity(depth + 1);
    for pair in nodes.windows(2) {
        facts.push((
            pair[0],
            ExpressionFact::known(
                ExpressionFactKind::Unary(UnaryOperator::BitwiseNot),
                vec![pair[1]],
                value_type,
                ExpressionEffects::PURE,
            ),
        ));
    }
    facts.push((leaf, integer_literal(1, value_type)));
    let root = nodes[0];
    (
        cpg,
        root,
        ExpressionFactTable::try_new(Language::Rust, facts).expect("chain facts"),
    )
}

#[test]
fn hundred_thousand_level_lifecycle_is_stack_safe_on_256_kib() {
    std::thread::Builder::new()
        .name("expression-ir-deep-lifecycle".into())
        .stack_size(256 * 1024)
        .spawn(|| {
            let (cpg, root, facts) = build_unary_chain(100_000);
            let before = (
                cpg.node_count(),
                cpg.edge_count(),
                format!("{:?}", cpg.language()),
            );
            let ir = lower_expression_ir(
                &cpg,
                root,
                &facts,
                &SemanticProfile::for_language(Language::Rust),
                ExpressionIrLimits::default(),
            );
            assert_eq!(ir.completion(), &ExpressionIrCompletion::Complete);
            assert_eq!(ir.metrics().nodes, 100_001);
            assert_eq!(
                before,
                (
                    cpg.node_count(),
                    cpg.edge_count(),
                    format!("{:?}", cpg.language())
                )
            );

            let cloned = ir.clone();
            assert_eq!(cloned, ir);
            let mut first_hash = DefaultHasher::new();
            ir.hash(&mut first_hash);
            let mut second_hash = DefaultHasher::new();
            cloned.hash(&mut second_hash);
            assert_eq!(first_hash.finish(), second_hash.finish());
            let mut sink = FormattingSink;
            write!(&mut sink, "{ir:?}").expect("flat debug formatting");
            #[cfg(feature = "serde")]
            {
                let bytes = serde_json::to_vec(&ir).expect("serialize deep flat IR");
                let persisted: ExpressionIr =
                    serde_json::from_slice(&bytes).expect("deserialize deep flat IR");
                assert_eq!(persisted.metrics(), ir.metrics());
                assert_eq!(
                    persisted.provenance(),
                    ExpressionIrProvenance::PersistedIdentityOnly
                );
                assert_eq!(
                    persisted.certify(ExpressionIrId(0), RewriteLaw::ConstantFold),
                    Err(RewriteDenial::PersistedArtifact)
                );
                drop(persisted);
                drop(bytes);
            }
            drop(cloned);
            drop(ir);
            drop(facts);
            drop(cpg);
        })
        .expect("spawn constrained-stack thread")
        .join()
        .expect("deep lifecycle completes");
}
