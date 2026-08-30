use std::sync::Arc;

use proptest::prelude::*;

use super::*;
use crate::{
    CodePropertyGraph, CpgEdgeKind, CpgNode, CpgNodeKind, ExpressionEffects, ExpressionFact,
    ExpressionFactTable, Language, LiteralKind, ScopeId, SourceRange,
};

#[cfg(feature = "serde")]
static_assertions::assert_impl_all!(ExpressionEquivalenceProof: serde::Serialize);
#[cfg(feature = "serde")]
static_assertions::assert_not_impl_any!(ExpressionEquivalenceProof: serde::de::DeserializeOwned);

struct Fixture {
    cpg: CodePropertyGraph,
    facts: Vec<(NodeId, ExpressionFact)>,
}

impl Fixture {
    fn new() -> Self {
        Self {
            cpg: CodePropertyGraph::new(Language::Rust),
            facts: Vec::new(),
        }
    }

    fn add_node(&mut self, kind: CpgNodeKind) -> NodeId {
        self.cpg
            .add_node(CpgNode::new(NodeId::new(0), kind, SourceRange::default()))
    }

    fn integer(&mut self, value: i128, value_type: ExpressionType) -> NodeId {
        let source = self.add_node(CpgNodeKind::Literal {
            kind: LiteralKind::Integer(i64::try_from(value).expect("test literal fits i64")),
        });
        self.facts.push((
            source,
            ExpressionFact::known(
                ExpressionFactKind::Literal(ExpressionLiteral::Integer(value)),
                Vec::new(),
                value_type,
                ExpressionEffects::PURE,
            ),
        ));
        source
    }

    fn boolean(&mut self, value: bool) -> NodeId {
        let source = self.add_node(CpgNodeKind::Literal {
            kind: LiteralKind::Bool(value),
        });
        self.facts.push((
            source,
            ExpressionFact::known(
                ExpressionFactKind::Literal(ExpressionLiteral::Bool(value)),
                Vec::new(),
                ExpressionType::Bool,
                ExpressionEffects::PURE,
            ),
        ));
        source
    }

    fn binding(
        &mut self,
        name: &str,
        scope: u32,
        value_type: ExpressionType,
    ) -> (NodeId, BindingIdentity) {
        let definition = self.add_node(CpgNodeKind::Variable {
            name: Arc::from(format!("{name}-definition")),
            var_type: None,
            scope: ScopeId::new(scope),
            is_mutable: false,
        });
        let source = self.add_node(CpgNodeKind::Identifier {
            name: Arc::from(name),
            definition: Some(definition),
        });
        let identity = BindingIdentity { definition, scope };
        self.facts.push((
            source,
            ExpressionFact::known(
                ExpressionFactKind::Binding(identity),
                Vec::new(),
                value_type,
                ExpressionEffects::PURE,
            ),
        ));
        (source, identity)
    }

    fn binary(
        &mut self,
        operator: BinaryOperator,
        left: NodeId,
        right: NodeId,
        value_type: ExpressionType,
    ) -> NodeId {
        let source = self.add_node(CpgNodeKind::BinaryOp {
            operator: Arc::from(format!("{operator:?}")),
        });
        self.cpg.connect(source, left, CpgEdgeKind::AstChild);
        self.cpg.connect(source, right, CpgEdgeKind::AstChild);
        self.facts.push((
            source,
            ExpressionFact::known(
                ExpressionFactKind::Binary(operator),
                vec![left, right],
                value_type,
                ExpressionEffects::PURE,
            ),
        ));
        source
    }

    fn unary(
        &mut self,
        operator: UnaryOperator,
        operand: NodeId,
        value_type: ExpressionType,
    ) -> NodeId {
        let source = self.add_node(CpgNodeKind::UnaryOp {
            operator: Arc::from(format!("{operator:?}")),
        });
        self.cpg.connect(source, operand, CpgEdgeKind::AstChild);
        self.facts.push((
            source,
            ExpressionFact::known(
                ExpressionFactKind::Unary(operator),
                vec![operand],
                value_type,
                ExpressionEffects::PURE,
            ),
        ));
        source
    }

    fn lower(&self, root: NodeId, profile: &SemanticProfile) -> ExpressionIr {
        let facts = ExpressionFactTable::try_new(Language::Rust, self.facts.clone())
            .expect("fixture facts are unique");
        let ir = super::super::expression_ir::lower_expression_ir(
            &self.cpg,
            root,
            &facts,
            profile,
            super::super::expression_ir::ExpressionIrLimits::default(),
        );
        assert_eq!(ir.completion(), &ExpressionIrCompletion::Complete);
        ir
    }
}

fn wrapping_i32() -> ExpressionType {
    ExpressionType::Integer(
        IntegerType::fixed(32, true, IntegerOverflow::Wrapping, ShiftBehavior::Masked)
            .expect("valid wrapping i32"),
    )
}

fn checked_i32() -> ExpressionType {
    ExpressionType::Integer(
        IntegerType::fixed(32, true, IntegerOverflow::Checked, ShiftBehavior::Checked)
            .expect("valid checked i32"),
    )
}

fn analyze<'ir>(
    profile: &SemanticProfile,
    candidates: &'ir [(NodeId, NodeId, &'ir ExpressionIr)],
) -> ExpressionEquivalenceReport {
    let candidates: Vec<_> = candidates
        .iter()
        .map(|(source, context, ir)| ExpressionEquivalenceCandidate::new(*source, *context, ir))
        .collect();
    analyze_expression_equivalence(
        NodeId::new(50_000),
        profile,
        &candidates,
        ExpressionEquivalenceLimits::default(),
    )
    .expect("equivalence analysis succeeds")
}

fn assert_replayable(report: &ExpressionEquivalenceReport) {
    for pair in &report.pairs {
        pair.proof.replay().expect("positive proof replays");
        assert!(pair.proof.encoded_len() > 0);
    }
}

#[test]
fn exact_pass_proves_identity_commutativity_associativity_and_constant_folding() {
    let profile = SemanticProfile::for_language(Language::Rust);
    let value_type = wrapping_i32();
    let context = NodeId::new(40_000);
    let mut fixture = Fixture::new();
    let (x, _) = fixture.binding("x", 1, value_type);
    let (y, _) = fixture.binding("y", 1, value_type);
    let (z, _) = fixture.binding("z", 1, value_type);
    let zero = fixture.integer(0, value_type);
    let one = fixture.integer(1, value_type);
    let two = fixture.integer(2, value_type);
    let three = fixture.integer(3, value_type);

    let x_plus_zero = fixture.binary(BinaryOperator::Add, x, zero, value_type);
    let xy = fixture.binary(BinaryOperator::Add, x, y, value_type);
    let yx = fixture.binary(BinaryOperator::Add, y, x, value_type);
    let xy_then_z = fixture.binary(BinaryOperator::Add, xy, z, value_type);
    let yz = fixture.binary(BinaryOperator::Add, y, z, value_type);
    let x_then_yz = fixture.binary(BinaryOperator::Add, x, yz, value_type);
    let one_plus_two = fixture.binary(BinaryOperator::Add, one, two, value_type);

    let roots = [
        x,
        x_plus_zero,
        xy,
        yx,
        xy_then_z,
        x_then_yz,
        one_plus_two,
        three,
    ];
    let irs: Vec<_> = roots
        .iter()
        .map(|root| fixture.lower(*root, &profile))
        .collect();
    let candidates: Vec<_> = roots
        .iter()
        .copied()
        .zip(&irs)
        .map(|(root, ir)| (root, context, ir))
        .collect();
    let report = analyze(&profile, &candidates);

    assert_eq!(report.completion, ExpressionEquivalenceCompletion::Complete);
    assert!(report.unsupported.is_empty());
    for expected in [
        (x, x_plus_zero),
        (xy, yx),
        (xy_then_z, x_then_yz),
        (one_plus_two, three),
    ] {
        let expected = (expected.0.min(expected.1), expected.0.max(expected.1));
        assert!(
            report.pairs.iter().any(|pair| {
                (pair.left, pair.right) == expected
                    && pair.kind == ExpressionEquivalenceKind::Exact
                    && pair.left_binding_context == context
                    && pair.right_binding_context == context
                    && pair.alpha_bindings.is_empty()
            }),
            "missing exact pair {expected:?}"
        );
    }
    assert_replayable(&report);
}

#[test]
fn alpha_pass_reports_a_bijection_without_claiming_cross_context_exactness() {
    let profile = SemanticProfile::for_language(Language::Rust);
    let value_type = wrapping_i32();
    let mut fixture = Fixture::new();
    let (x, x_identity) = fixture.binding("x", 1, value_type);
    let (y, y_identity) = fixture.binding("y", 2, value_type);
    let left = fixture.binary(BinaryOperator::Add, x, x, value_type);
    let right = fixture.binary(BinaryOperator::Add, y, y, value_type);
    let left_ir = fixture.lower(left, &profile);
    let right_ir = fixture.lower(right, &profile);
    let report = analyze(
        &profile,
        &[
            (left, NodeId::new(100), &left_ir),
            (right, NodeId::new(200), &right_ir),
        ],
    );

    assert_eq!(report.pairs.len(), 1);
    let pair = &report.pairs[0];
    assert_eq!(pair.kind, ExpressionEquivalenceKind::AlphaRenamedType4Clone);
    assert_eq!(
        pair.alpha_bindings,
        vec![AlphaBindingCorrespondence {
            slot: 0,
            left: x_identity,
            right: y_identity,
        }]
    );
    assert!(!report
        .pairs
        .iter()
        .any(|pair| pair.kind == ExpressionEquivalenceKind::Exact));
    assert_replayable(&report);
}

#[test]
fn alpha_pass_rejects_non_bijective_binding_shapes() {
    let profile = SemanticProfile::for_language(Language::Rust);
    let value_type = wrapping_i32();
    let mut fixture = Fixture::new();
    let (x, _) = fixture.binding("x", 1, value_type);
    let (y, _) = fixture.binding("y", 2, value_type);
    let (z, _) = fixture.binding("z", 2, value_type);
    let left = fixture.binary(BinaryOperator::Add, x, x, value_type);
    let right = fixture.binary(BinaryOperator::Add, y, z, value_type);
    let left_ir = fixture.lower(left, &profile);
    let right_ir = fixture.lower(right, &profile);
    let report = analyze(
        &profile,
        &[
            (left, NodeId::new(100), &left_ir),
            (right, NodeId::new(200), &right_ir),
        ],
    );

    assert_eq!(report.completion, ExpressionEquivalenceCompletion::Complete);
    assert!(report.pairs.is_empty());
}

#[test]
fn boolean_identities_and_folds_are_proof_producing() {
    let profile = SemanticProfile::for_language(Language::Rust);
    let context = NodeId::new(91);
    let mut fixture = Fixture::new();
    let (condition, _) = fixture.binding("condition", 3, ExpressionType::Bool);
    let truth = fixture.boolean(true);
    let falsity = fixture.boolean(false);
    let with_identity = fixture.binary(
        BinaryOperator::LogicalAnd,
        condition,
        truth,
        ExpressionType::Bool,
    );
    let folded = fixture.binary(
        BinaryOperator::LogicalOr,
        falsity,
        truth,
        ExpressionType::Bool,
    );
    let roots = [condition, with_identity, folded, truth];
    let irs: Vec<_> = roots
        .iter()
        .map(|root| fixture.lower(*root, &profile))
        .collect();
    let candidates: Vec<_> = roots
        .iter()
        .copied()
        .zip(&irs)
        .map(|(root, ir)| (root, context, ir))
        .collect();
    let report = analyze(&profile, &candidates);

    assert!(report.pairs.iter().any(|pair| {
        (pair.left, pair.right) == (condition, with_identity)
            && pair.kind == ExpressionEquivalenceKind::Exact
    }));
    assert!(report.pairs.iter().any(|pair| {
        (pair.left, pair.right) == (truth, folded) && pair.kind == ExpressionEquivalenceKind::Exact
    }));
    assert_replayable(&report);
}

#[test]
fn nested_constant_folding_reaches_a_proof_producing_fixed_point() {
    let profile = SemanticProfile::for_language(Language::Rust);
    let value_type = wrapping_i32();
    let context = NodeId::new(92);
    let mut fixture = Fixture::new();
    let one = fixture.integer(1, value_type);
    let two = fixture.integer(2, value_type);
    let three = fixture.integer(3, value_type);
    let six = fixture.integer(6, value_type);
    let one_plus_two = fixture.binary(BinaryOperator::Add, one, two, value_type);
    let nested = fixture.binary(BinaryOperator::Add, one_plus_two, three, value_type);
    let nested_ir = fixture.lower(nested, &profile);
    let six_ir = fixture.lower(six, &profile);
    let report = analyze(
        &profile,
        &[(nested, context, &nested_ir), (six, context, &six_ir)],
    );

    assert_eq!(report.completion, ExpressionEquivalenceCompletion::Complete);
    assert_eq!(report.pairs.len(), 1);
    assert_eq!(report.pairs[0].kind, ExpressionEquivalenceKind::Exact);
    assert_replayable(&report);

    let mut wrong_target = report.pairs[0].proof.clone();
    let operators = Arc::make_mut(&mut wrong_target.operators);
    let (header, mut operator) = operators
        .iter()
        .find(|(_, operator)| {
            operator.kind == EquivalenceOperatorKind::Literal(ExpressionLiteral::Integer(6))
        })
        .map(|(header, operator)| (header.clone(), operator.clone()))
        .expect("proof records its target literal");
    operator.kind = EquivalenceOperatorKind::Literal(ExpressionLiteral::Integer(7));
    operators.insert(header, operator);
    assert!(wrong_target.replay().is_err());

    let mut open_source = report.pairs[0].proof.clone();
    let operators = Arc::make_mut(&mut open_source.operators);
    let (header, mut operator) = operators
        .iter()
        .find(|(_, operator)| {
            operator.kind == EquivalenceOperatorKind::Literal(ExpressionLiteral::Integer(1))
        })
        .map(|(header, operator)| (header.clone(), operator.clone()))
        .expect("proof records its first source literal");
    operator.kind = EquivalenceOperatorKind::Binding(EquivalenceBinding::Alpha(0));
    operators.insert(header, operator);
    assert!(open_source.replay().is_err());
}

#[test]
fn ac_normalization_is_exhaustive_over_three_operand_permutations_and_associations() {
    let profile = SemanticProfile::for_language(Language::Rust);
    let value_type = wrapping_i32();
    let context = NodeId::new(93);
    let mut fixture = Fixture::new();
    let (a, _) = fixture.binding("a", 4, value_type);
    let (b, _) = fixture.binding("b", 4, value_type);
    let (c, _) = fixture.binding("c", 4, value_type);
    let variables = [a, b, c];
    let mut roots = Vec::new();
    for permutation in [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ] {
        let [first, second, third] = permutation.map(|index| variables[index]);
        let left_pair = fixture.binary(BinaryOperator::Add, first, second, value_type);
        roots.push(fixture.binary(BinaryOperator::Add, left_pair, third, value_type));
        let right_pair = fixture.binary(BinaryOperator::Add, second, third, value_type);
        roots.push(fixture.binary(BinaryOperator::Add, first, right_pair, value_type));
    }
    let irs: Vec<_> = roots
        .iter()
        .map(|root| fixture.lower(*root, &profile))
        .collect();
    let candidates: Vec<_> = roots
        .iter()
        .copied()
        .zip(&irs)
        .map(|(root, ir)| (root, context, ir))
        .collect();
    let report = analyze(&profile, &candidates);

    assert_eq!(report.completion, ExpressionEquivalenceCompletion::Complete);
    assert_eq!(report.pairs.len(), 12 * 11 / 2);
    assert_eq!(report.metrics.exact_pairs, report.pairs.len());
    assert!(report
        .pairs
        .iter()
        .all(|pair| pair.kind == ExpressionEquivalenceKind::Exact));
    assert_replayable(&report);
}

#[test]
fn ac_normalization_preserves_operand_multiplicity() {
    let profile = SemanticProfile::for_language(Language::Rust);
    let value_type = wrapping_i32();
    let context = NodeId::new(94);
    let mut fixture = Fixture::new();
    let (a, _) = fixture.binding("a", 5, value_type);
    let (b, _) = fixture.binding("b", 5, value_type);
    let aa = fixture.binary(BinaryOperator::Add, a, a, value_type);
    let left = fixture.binary(BinaryOperator::Add, aa, b, value_type);
    let ab = fixture.binary(BinaryOperator::Add, a, b, value_type);
    let right = fixture.binary(BinaryOperator::Add, ab, b, value_type);
    let left_ir = fixture.lower(left, &profile);
    let right_ir = fixture.lower(right, &profile);
    let report = analyze(
        &profile,
        &[(left, context, &left_ir), (right, context, &right_ir)],
    );

    assert_eq!(report.completion, ExpressionEquivalenceCompletion::Complete);
    assert!(report.pairs.is_empty());
}

#[test]
fn candidate_input_pair_and_node_limits_are_honest() {
    let profile = SemanticProfile::for_language(Language::Rust);
    let value_type = wrapping_i32();
    let mut fixture = Fixture::new();
    let one = fixture.integer(1, value_type);
    let another_one = fixture.integer(1, value_type);
    let one_ir = fixture.lower(one, &profile);
    let another_ir = fixture.lower(another_one, &profile);
    let candidates = [
        ExpressionEquivalenceCandidate::new(one, NodeId::new(1), &one_ir),
        ExpressionEquivalenceCandidate::new(another_one, NodeId::new(2), &another_ir),
    ];

    for (limits, expected) in [
        (
            ExpressionEquivalenceLimits {
                max_candidates: 1,
                ..ExpressionEquivalenceLimits::default()
            },
            ExpressionEquivalenceCompletion::CandidateLimited,
        ),
        (
            ExpressionEquivalenceLimits {
                max_input_nodes: 1,
                ..ExpressionEquivalenceLimits::default()
            },
            ExpressionEquivalenceCompletion::InputNodeLimited,
        ),
        (
            ExpressionEquivalenceLimits {
                max_pairs: 0,
                ..ExpressionEquivalenceLimits::default()
            },
            ExpressionEquivalenceCompletion::PairLimited,
        ),
        (
            ExpressionEquivalenceLimits {
                max_egraph_nodes: 1,
                ..ExpressionEquivalenceLimits::default()
            },
            ExpressionEquivalenceCompletion::NodeLimited,
        ),
    ] {
        let report = analyze_expression_equivalence(NodeId::new(2), &profile, &candidates, limits)
            .expect("resource exhaustion is data, not an error");
        assert_eq!(report.completion, expected);
        assert!(!report.completion.is_complete());
    }
}

#[test]
fn work_match_iteration_and_proof_limits_have_distinct_terminal_states() {
    let profile = SemanticProfile::for_language(Language::Rust);
    let value_type = checked_i32();
    let context = NodeId::new(95);
    let mut fixture = Fixture::new();
    let (x, _) = fixture.binding("x", 6, value_type);
    let (y, _) = fixture.binding("y", 6, value_type);
    let xy = fixture.binary(BinaryOperator::Add, x, y, value_type);
    let yx = fixture.binary(BinaryOperator::Add, y, x, value_type);
    let xy_ir = fixture.lower(xy, &profile);
    let yx_ir = fixture.lower(yx, &profile);
    let candidates = [
        ExpressionEquivalenceCandidate::new(xy, context, &xy_ir),
        ExpressionEquivalenceCandidate::new(yx, context, &yx_ir),
    ];

    for (limits, expected) in [
        (
            ExpressionEquivalenceLimits {
                max_work: 0,
                ..ExpressionEquivalenceLimits::default()
            },
            ExpressionEquivalenceCompletion::WorkLimited,
        ),
        (
            ExpressionEquivalenceLimits {
                max_matches: 0,
                ..ExpressionEquivalenceLimits::default()
            },
            ExpressionEquivalenceCompletion::MatchLimited,
        ),
        (
            ExpressionEquivalenceLimits {
                max_iterations: 0,
                ..ExpressionEquivalenceLimits::default()
            },
            ExpressionEquivalenceCompletion::IterationLimited,
        ),
        (
            ExpressionEquivalenceLimits {
                max_proof_nodes: 0,
                ..ExpressionEquivalenceLimits::default()
            },
            ExpressionEquivalenceCompletion::ProofLimited,
        ),
    ] {
        let report = analyze_expression_equivalence(NodeId::new(96), &profile, &candidates, limits)
            .expect("cap exhaustion is typed report data");
        assert_eq!(report.completion, expected);
        assert!(!report.completion.is_complete());
        assert!(report.pairs.is_empty());
    }
}

#[test]
fn pair_limit_returns_a_deterministic_prefix_of_the_complete_relation() {
    let profile = SemanticProfile::for_language(Language::Rust);
    let value_type = wrapping_i32();
    let context = NodeId::new(97);
    let mut fixture = Fixture::new();
    let first = fixture.integer(11, value_type);
    let second = fixture.integer(11, value_type);
    let third = fixture.integer(11, value_type);
    let roots = [first, second, third];
    let irs: Vec<_> = roots
        .iter()
        .map(|root| fixture.lower(*root, &profile))
        .collect();
    let candidates: Vec<_> = roots
        .iter()
        .copied()
        .zip(&irs)
        .map(|(source, ir)| ExpressionEquivalenceCandidate::new(source, context, ir))
        .collect();
    let complete = analyze_expression_equivalence(
        NodeId::new(98),
        &profile,
        &candidates,
        ExpressionEquivalenceLimits::default(),
    )
    .expect("complete relation succeeds");
    let limited = analyze_expression_equivalence(
        NodeId::new(98),
        &profile,
        &candidates,
        ExpressionEquivalenceLimits {
            max_pairs: 1,
            ..ExpressionEquivalenceLimits::default()
        },
    )
    .expect("pair cap is report data");

    assert_eq!(complete.pairs.len(), 3);
    assert_eq!(
        limited.completion,
        ExpressionEquivalenceCompletion::PairLimited
    );
    assert_eq!(limited.pairs.len(), 1);
    assert_eq!(
        (limited.pairs[0].left, limited.pairs[0].right),
        (complete.pairs[0].left, complete.pairs[0].right)
    );
    assert_replayable(&limited);
}

#[test]
fn duplicate_source_and_profile_mismatch_are_explicitly_unsupported() {
    let rust_profile = SemanticProfile::for_language(Language::Rust);
    let python_profile = SemanticProfile::for_language(Language::Python);
    let value_type = wrapping_i32();
    let mut fixture = Fixture::new();
    let one = fixture.integer(1, value_type);
    let ir = fixture.lower(one, &rust_profile);
    let candidates = [
        ExpressionEquivalenceCandidate::new(one, NodeId::new(1), &ir),
        ExpressionEquivalenceCandidate::new(one, NodeId::new(2), &ir),
    ];
    let duplicate = analyze_expression_equivalence(
        NodeId::new(3),
        &rust_profile,
        &candidates,
        ExpressionEquivalenceLimits::default(),
    )
    .expect("duplicate source is classified");
    assert_eq!(
        duplicate.unsupported,
        vec![UnsupportedExpression {
            source: one,
            reason: UnsupportedExpressionReason::DuplicateSource,
        }]
    );
    assert_eq!(
        duplicate.completion,
        ExpressionEquivalenceCompletion::UnsupportedInputs
    );

    let mismatch = analyze_expression_equivalence(
        NodeId::new(3),
        &python_profile,
        &[ExpressionEquivalenceCandidate::new(
            one,
            NodeId::new(1),
            &ir,
        )],
        ExpressionEquivalenceLimits::default(),
    )
    .expect("profile mismatch is classified");
    assert_eq!(
        mismatch.unsupported[0].reason,
        UnsupportedExpressionReason::ProfileMismatch
    );
}

#[test]
fn unsupported_lowering_root_and_effect_states_never_enter_an_egraph() {
    let profile = SemanticProfile::for_language(Language::Rust);
    let value_type = wrapping_i32();
    let mut fixture = Fixture::new();
    let one = fixture.integer(1, value_type);
    let zero = fixture.integer(0, value_type);
    let root = fixture.binary(BinaryOperator::Add, one, zero, value_type);
    let complete = fixture.lower(root, &profile);
    let facts = ExpressionFactTable::try_new(Language::Rust, fixture.facts.clone())
        .expect("fixture facts are unique");
    let incomplete = super::super::expression_ir::lower_expression_ir(
        &fixture.cpg,
        root,
        &facts,
        &profile,
        super::super::expression_ir::ExpressionIrLimits {
            max_nodes: 0,
            ..super::super::expression_ir::ExpressionIrLimits::default()
        },
    );
    let effectful = fixture.integer(2, value_type);
    fixture.facts.last_mut().expect("effectful fact").1.effects =
        SemanticKnowledge::Known(ExpressionEffects {
            writes_memory: true,
            ..ExpressionEffects::PURE
        });
    let effectful_ir = fixture.lower(effectful, &profile);

    let report = analyze_expression_equivalence(
        NodeId::new(9),
        &profile,
        &[
            ExpressionEquivalenceCandidate::new(one, NodeId::new(1), &complete),
            ExpressionEquivalenceCandidate::new(root, NodeId::new(1), &incomplete),
            ExpressionEquivalenceCandidate::new(effectful, NodeId::new(1), &effectful_ir),
        ],
        ExpressionEquivalenceLimits::default(),
    )
    .expect("unsupported inputs are classified");

    assert_eq!(
        report.unsupported,
        vec![
            UnsupportedExpression {
                source: one,
                reason: UnsupportedExpressionReason::RootMismatch,
            },
            UnsupportedExpression {
                source: root,
                reason: UnsupportedExpressionReason::IncompleteLowering(
                    incomplete.completion().clone()
                ),
            },
            UnsupportedExpression {
                source: effectful,
                reason: UnsupportedExpressionReason::NonPureEvidence,
            },
        ]
    );
    assert_eq!(report.metrics.admitted, 0);
    assert_eq!(
        report.completion,
        ExpressionEquivalenceCompletion::UnsupportedInputs
    );
}

#[cfg(feature = "serde")]
#[test]
fn persisted_ir_is_identity_only_and_reports_serialize_without_restoring_authority() {
    let profile = SemanticProfile::for_language(Language::Rust);
    let value_type = wrapping_i32();
    let mut fixture = Fixture::new();
    let one = fixture.integer(1, value_type);
    let live = fixture.lower(one, &profile);
    let encoded = serde_json::to_vec(&live).expect("serialize live flat IR");
    let persisted: ExpressionIr = serde_json::from_slice(&encoded).expect("deserialize flat IR");
    let report = analyze_expression_equivalence(
        NodeId::new(12),
        &profile,
        &[ExpressionEquivalenceCandidate::new(
            one,
            NodeId::new(3),
            &persisted,
        )],
        ExpressionEquivalenceLimits::default(),
    )
    .expect("persisted authority is classified");

    assert_eq!(
        report.unsupported,
        vec![UnsupportedExpression {
            source: one,
            reason: UnsupportedExpressionReason::PersistedArtifact,
        }]
    );
    let report_json = serde_json::to_vec(&report).expect("serialize evidence report");
    assert!(!report_json.is_empty());
}

#[cfg(feature = "serde")]
#[test]
fn canonical_report_bytes_are_candidate_order_invariant() {
    let profile = SemanticProfile::for_language(Language::Rust);
    let value_type = wrapping_i32();
    let context = NodeId::new(13);
    let mut fixture = Fixture::new();
    let first = fixture.integer(5, value_type);
    let second = fixture.integer(5, value_type);
    let first_ir = fixture.lower(first, &profile);
    let second_ir = fixture.lower(second, &profile);
    let forward = analyze(
        &profile,
        &[(first, context, &first_ir), (second, context, &second_ir)],
    );
    let reverse = analyze(
        &profile,
        &[(second, context, &second_ir), (first, context, &first_ir)],
    );

    assert_eq!(
        serde_json::to_vec(&forward).expect("serialize forward report"),
        serde_json::to_vec(&reverse).expect("serialize reverse report")
    );
}

#[derive(Default)]
struct TestCanonicalSink(Vec<u8>);

impl CanonicalSink for TestCanonicalSink {
    fn write(&mut self, bytes: &[u8]) -> Result<(), CanonicalValueError> {
        self.0.extend_from_slice(bytes);
        Ok(())
    }
}

fn assert_operator_round_trip(operator: EquivalenceOperator) {
    let mut sink = TestCanonicalSink::default();
    operator
        .encode_canonical(&mut sink)
        .expect("operator encodes");
    assert_eq!(EquivalenceOperator::decode_canonical(&sink.0), Ok(operator));
}

#[test]
fn canonical_operator_encoding_round_trips_every_discriminant_family() {
    let value_types = [
        ExpressionType::Bool,
        ExpressionType::String,
        ExpressionType::Char,
        ExpressionType::Unit,
        ExpressionType::Dynamic,
        ExpressionType::Float(FloatType {
            width: FloatWidth::F32,
            mode: FloatMode::StrictIeee754,
        }),
        ExpressionType::Float(FloatType {
            width: FloatWidth::F64,
            mode: FloatMode::FiniteReassociation,
        }),
        wrapping_i32(),
        ExpressionType::Integer(IntegerType::arbitrary_precision(true)),
    ];
    for value_type in value_types {
        assert_operator_round_trip(EquivalenceOperator {
            namespace: Some(NodeId::new(17)),
            kind: EquivalenceOperatorKind::Opaque,
            value_type,
        });
    }

    let kinds = [
        EquivalenceOperatorKind::Literal(ExpressionLiteral::Integer(-17)),
        EquivalenceOperatorKind::Literal(ExpressionLiteral::Float32(u32::MAX)),
        EquivalenceOperatorKind::Literal(ExpressionLiteral::Float64(u64::MAX)),
        EquivalenceOperatorKind::Literal(ExpressionLiteral::Bool(true)),
        EquivalenceOperatorKind::Literal(ExpressionLiteral::String(Arc::from("nul\0λ"))),
        EquivalenceOperatorKind::Literal(ExpressionLiteral::Char('🦀')),
        EquivalenceOperatorKind::Literal(ExpressionLiteral::Null),
        EquivalenceOperatorKind::Binding(EquivalenceBinding::Exact(BindingIdentity {
            definition: NodeId::new(18),
            scope: 19,
        })),
        EquivalenceOperatorKind::Binding(EquivalenceBinding::Alpha(20)),
        EquivalenceOperatorKind::Unary(UnaryOperator::Negate),
        EquivalenceOperatorKind::Unary(UnaryOperator::LogicalNot),
        EquivalenceOperatorKind::Unary(UnaryOperator::BitwiseNot),
        EquivalenceOperatorKind::Call(CallIdentity::Resolved(NodeId::new(21))),
        EquivalenceOperatorKind::Call(CallIdentity::External(Arc::from("crate::symbol"))),
        EquivalenceOperatorKind::Call(CallIdentity::Unresolved),
        EquivalenceOperatorKind::Opaque,
    ];
    for kind in kinds {
        assert_operator_round_trip(EquivalenceOperator {
            namespace: None,
            kind,
            value_type: ExpressionType::Dynamic,
        });
    }
    for operator in [
        BinaryOperator::Add,
        BinaryOperator::Subtract,
        BinaryOperator::Multiply,
        BinaryOperator::Divide,
        BinaryOperator::Remainder,
        BinaryOperator::ShiftLeft,
        BinaryOperator::ShiftRight,
        BinaryOperator::BitwiseAnd,
        BinaryOperator::BitwiseOr,
        BinaryOperator::BitwiseXor,
        BinaryOperator::LogicalAnd,
        BinaryOperator::LogicalOr,
        BinaryOperator::Equal,
        BinaryOperator::NotEqual,
        BinaryOperator::Less,
        BinaryOperator::LessEqual,
        BinaryOperator::Greater,
        BinaryOperator::GreaterEqual,
    ] {
        assert_operator_round_trip(EquivalenceOperator {
            namespace: None,
            kind: EquivalenceOperatorKind::Binary(operator),
            value_type: ExpressionType::Dynamic,
        });
    }
    for access in [
        MemoryAccess::Immutable,
        MemoryAccess::Shared,
        MemoryAccess::Mutable,
        MemoryAccess::Volatile,
        MemoryAccess::Atomic,
    ] {
        assert_operator_round_trip(EquivalenceOperator {
            namespace: None,
            kind: EquivalenceOperatorKind::Load {
                location: NodeId::new(22),
                access,
            },
            value_type: ExpressionType::Dynamic,
        });
    }
}

#[test]
fn canonical_operator_decoder_rejects_truncation_trailing_bytes_and_bad_tags() {
    let operator = EquivalenceOperator {
        namespace: None,
        kind: EquivalenceOperatorKind::Literal(ExpressionLiteral::Bool(false)),
        value_type: ExpressionType::Bool,
    };
    let mut sink = TestCanonicalSink::default();
    operator
        .encode_canonical(&mut sink)
        .expect("operator encodes");
    assert!(EquivalenceOperator::decode_canonical(&sink.0[..sink.0.len() - 1]).is_err());
    sink.0.push(0);
    assert_eq!(
        EquivalenceOperator::decode_canonical(&sink.0),
        Err(CanonicalValueError::InvalidLength)
    );
    assert_eq!(
        EquivalenceOperator::decode_canonical(&[2]),
        Err(CanonicalValueError::InvalidDiscriminant)
    );
}

#[test]
fn exact_relation_is_transitive_canonical_and_unaffected_by_irrelevant_candidates() {
    let profile = SemanticProfile::for_language(Language::Rust);
    let value_type = wrapping_i32();
    let context = NodeId::new(33);
    let mut fixture = Fixture::new();
    let first = fixture.integer(7, value_type);
    let second = fixture.integer(7, value_type);
    let third = fixture.integer(7, value_type);
    let irrelevant = fixture.integer(8, value_type);
    let roots = [first, second, third, irrelevant];
    let irs: Vec<_> = roots
        .iter()
        .map(|root| fixture.lower(*root, &profile))
        .collect();
    let candidates: Vec<_> = roots
        .iter()
        .copied()
        .zip(&irs)
        .map(|(root, ir)| (root, context, ir))
        .collect();
    let report = analyze(&profile, &candidates);

    let exact: Vec<_> = report
        .pairs
        .iter()
        .filter(|pair| pair.kind == ExpressionEquivalenceKind::Exact)
        .map(|pair| (pair.left, pair.right))
        .collect();
    assert_eq!(
        exact,
        vec![(first, second), (first, third), (second, third)]
    );
    assert!(report
        .pairs
        .iter()
        .all(|pair| pair.left < pair.right && pair.left != irrelevant && pair.right != irrelevant));
    assert_replayable(&report);
}

#[test]
fn invalid_public_limits_are_rejected_before_graph_construction() {
    let profile = SemanticProfile::for_language(Language::Rust);
    for (limits, name) in [
        (
            ExpressionEquivalenceLimits {
                max_egraph_nodes: 0,
                ..ExpressionEquivalenceLimits::default()
            },
            "max_egraph_nodes",
        ),
        (
            ExpressionEquivalenceLimits {
                max_candidates: MAX_EQUIVALENCE_CANDIDATES + 1,
                ..ExpressionEquivalenceLimits::default()
            },
            "max_candidates",
        ),
        (
            ExpressionEquivalenceLimits {
                max_input_nodes: MAX_EQUIVALENCE_INPUT_NODES + 1,
                ..ExpressionEquivalenceLimits::default()
            },
            "max_input_nodes",
        ),
        (
            ExpressionEquivalenceLimits {
                max_pairs: MAX_EQUIVALENCE_PAIRS + 1,
                ..ExpressionEquivalenceLimits::default()
            },
            "max_pairs",
        ),
    ] {
        assert!(matches!(
            analyze_expression_equivalence(NodeId::new(1), &profile, &[], limits),
            Err(ExpressionProofError::InvalidLimit(actual)) if actual == name
        ));
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn wrapping_addition_commutativity_is_insertion_order_invariant(
        left_value in any::<i32>(),
        right_value in any::<i32>(),
        reverse in any::<bool>(),
    ) {
        let profile = SemanticProfile::for_language(Language::Rust);
        let value_type = wrapping_i32();
        let context = NodeId::new(7_001);
        let mut fixture = Fixture::new();
        let left_literal = fixture.integer(i128::from(left_value), value_type);
        let right_literal = fixture.integer(i128::from(right_value), value_type);
        let left = fixture.binary(BinaryOperator::Add, left_literal, right_literal, value_type);
        let right = fixture.binary(BinaryOperator::Add, right_literal, left_literal, value_type);
        let left_ir = fixture.lower(left, &profile);
        let right_ir = fixture.lower(right, &profile);
        let ordered = if reverse {
            vec![(right, context, &right_ir), (left, context, &left_ir)]
        } else {
            vec![(left, context, &left_ir), (right, context, &right_ir)]
        };
        let report = analyze(&profile, &ordered);

        prop_assert_eq!(report.completion, ExpressionEquivalenceCompletion::Complete);
        let found = report.pairs.iter().any(|pair| {
            pair.left == left.min(right)
                && pair.right == left.max(right)
                && pair.kind == ExpressionEquivalenceKind::Exact
        });
        prop_assert!(found);
        for pair in &report.pairs {
            prop_assert!(pair.proof.replay().is_ok());
        }
    }

    #[test]
    fn canonical_integer_operator_encoding_round_trips(
        value in any::<i128>(),
        namespace in proptest::option::of(any::<u32>()),
        signed in any::<bool>(),
    ) {
        let operator = EquivalenceOperator {
            namespace: namespace.map(NodeId::new),
            kind: EquivalenceOperatorKind::Literal(ExpressionLiteral::Integer(value)),
            value_type: ExpressionType::Integer(IntegerType::arbitrary_precision(signed)),
        };
        let mut sink = TestCanonicalSink::default();
        operator.encode_canonical(&mut sink).expect("operator encodes");
        prop_assert_eq!(EquivalenceOperator::decode_canonical(&sink.0), Ok(operator));
    }

    #[test]
    fn checked_i8_literal_evaluator_matches_independent_oracle(
        left in any::<i8>(),
        right in any::<i8>(),
        tag in 0_u8..5,
    ) {
        let operator = match tag {
            0 => BinaryOperator::Add,
            1 => BinaryOperator::Subtract,
            2 => BinaryOperator::Multiply,
            3 => BinaryOperator::Divide,
            _ => BinaryOperator::Remainder,
        };
        let integer = IntegerType::fixed(
            8,
            true,
            IntegerOverflow::Checked,
            ShiftBehavior::Checked,
        ).expect("valid checked i8");
        let expected = match operator {
            BinaryOperator::Add => left.checked_add(right),
            BinaryOperator::Subtract => left.checked_sub(right),
            BinaryOperator::Multiply => left.checked_mul(right),
            BinaryOperator::Divide => left.checked_div(right),
            BinaryOperator::Remainder => left.checked_rem(right),
            _ => unreachable!(),
        }.map(i128::from);
        prop_assert_eq!(
            fold_integer_binary(operator, integer, i128::from(left), i128::from(right)),
            expected
        );
    }
}

#[test]
fn unary_constructor_remains_available_for_deep_stack_gate() {
    let profile = SemanticProfile::for_language(Language::Rust);
    let value_type = wrapping_i32();
    let mut fixture = Fixture::new();
    let literal = fixture.integer(1, value_type);
    let root = fixture.unary(UnaryOperator::BitwiseNot, literal, value_type);
    let ir = fixture.lower(root, &profile);
    let report = analyze(&profile, &[(root, NodeId::new(8), &ir)]);
    assert_eq!(report.completion, ExpressionEquivalenceCompletion::Complete);
}

#[test]
fn depth_100_000_full_equivalence_lifecycle_fits_a_small_native_stack() {
    const DEPTH: usize = 100_000;
    std::thread::Builder::new()
        .name("expression-equivalence-small-stack".into())
        .stack_size(256 * 1024)
        .spawn(|| {
            let profile = SemanticProfile::for_language(Language::Rust);
            let value_type = wrapping_i32();
            let context = NodeId::new(700_000);
            let mut fixture = Fixture::new();
            let (leaf, _) = fixture.binding("deep", 99, value_type);
            let mut left = leaf;
            for _ in 0..DEPTH {
                left = fixture.unary(UnaryOperator::BitwiseNot, left, value_type);
            }
            let mut right = leaf;
            for _ in 0..DEPTH {
                right = fixture.unary(UnaryOperator::BitwiseNot, right, value_type);
            }
            let left_ir = fixture.lower(left, &profile);
            let right_ir = fixture.lower(right, &profile);
            let report = analyze(
                &profile,
                &[(left, context, &left_ir), (right, context, &right_ir)],
            );
            assert_eq!(report.completion, ExpressionEquivalenceCompletion::Complete);
            assert_eq!(report.pairs.len(), 1);
            report.pairs[0]
                .proof
                .replay()
                .expect("deep proof replays iteratively");
            let cloned = report.clone();
            let rendered = format!("{cloned:?}");
            assert!(rendered.contains("ExpressionEquivalenceReport"));
            #[cfg(feature = "serde")]
            {
                let encoded = serde_json::to_vec(&cloned).expect("serialize deep flat proof");
                assert!(!encoded.is_empty());
            }
            drop((cloned, report, left_ir, right_ir, fixture));
        })
        .expect("small-stack worker starts")
        .join()
        .expect("full deep equivalence lifecycle remains stack safe");
}

#[test]
fn depth_100_000_closed_evaluation_proof_replay_fits_a_small_native_stack() {
    const DEPTH: usize = 100_000;
    std::thread::Builder::new()
        .name("closed-evaluation-proof-small-stack".into())
        .stack_size(256 * 1024)
        .spawn(|| {
            let value_type = wrapping_i32();
            let namespace = Some(NodeId::new(710_000));
            let literal = EquivalenceOperator {
                namespace,
                kind: EquivalenceOperatorKind::Literal(ExpressionLiteral::Integer(1)),
                value_type,
            };
            let bitwise_not = EquivalenceOperator {
                namespace,
                kind: EquivalenceOperatorKind::Unary(UnaryOperator::BitwiseNot),
                value_type,
            };
            let limits = ExpressionEquivalenceLimits::default();
            let mut builder = ProofBuilder::new(limits.proof_limits()).expect("proof builder");
            let leaf = builder.add_term(&literal, &[]).expect("literal term");
            let mut source = leaf;
            for _ in 0..DEPTH {
                source = builder
                    .add_term(&bitwise_not, &[source])
                    .expect("deep unary term");
            }
            let proof = builder
                .rewritten(
                    source,
                    leaf,
                    EvidenceLabel::new(law_label(RewriteLaw::ConstantFold)).expect("fold label"),
                    RewriteDirection::Forward,
                )
                .expect("deep fold proof");
            let tape = builder.finish(proof).expect("finish flat proof");
            let operators = BTreeMap::from([
                (operator_header(&literal, 0), literal),
                (operator_header(&bitwise_not, 1), bitwise_not),
            ]);
            let proof = ExpressionEquivalenceProof {
                tape,
                profile: Arc::new(SemanticProfile::for_language(Language::Rust)),
                operators: Arc::new(operators),
            };

            proof
                .replay()
                .expect("deep closed evaluation replays iteratively");
            let rendered = format!("{proof:?}");
            assert!(rendered.contains("ExpressionEquivalenceProof"));
            drop(proof);
        })
        .expect("small-stack worker starts")
        .join()
        .expect("deep closed evaluation remains stack safe");
}
