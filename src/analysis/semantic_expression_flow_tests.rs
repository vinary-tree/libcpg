use std::sync::Arc;

use proptest::prelude::*;

use super::*;
use crate::{
    CfgEdgeKind, CpgEdgeKind, CpgNode, ExpressionEffects, ExpressionFact, ExpressionFactTable,
    ExpressionIr, ExpressionIrCompletion, ExpressionIrLimits, IntegerOverflow, IntegerType,
    Language, LiteralKind, MethodSignature, ScopeId, ShiftBehavior, SourceRange, Visibility,
};

struct Fixture {
    cpg: CodePropertyGraph,
    function: NodeId,
    facts: Vec<(NodeId, ExpressionFact)>,
    profile: SemanticProfile,
}

impl Fixture {
    fn new() -> Self {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let function = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Function {
                signature: Box::new(MethodSignature {
                    name: "semantic_flow".into(),
                    params: Default::default(),
                    return_type: None,
                    is_static: false,
                    is_async: false,
                    visibility: Visibility::Private,
                }),
            },
            SourceRange::default(),
        ));
        Self {
            cpg,
            function,
            facts: Vec::new(),
            profile: SemanticProfile::for_language(Language::Rust),
        }
    }

    fn add(&mut self, kind: CpgNodeKind) -> NodeId {
        self.cpg
            .add_node(CpgNode::new(NodeId::new(0), kind, SourceRange::default()))
    }

    fn attach(&mut self, child: NodeId) {
        self.cpg
            .connect(self.function, child, CpgEdgeKind::AstChild);
    }

    fn definition(&mut self, name: &str, scope: u32) -> NodeId {
        let definition = self.add(CpgNodeKind::Variable {
            name: Arc::from(name),
            var_type: None,
            scope: ScopeId::new(scope),
            is_mutable: true,
        });
        self.attach(definition);
        definition
    }

    fn binding(&mut self, name: &str, definition: NodeId, scope: u32) -> NodeId {
        let source = self.add(CpgNodeKind::Identifier {
            name: Arc::from(name),
            definition: Some(definition),
        });
        self.facts.push((
            source,
            ExpressionFact::known(
                ExpressionFactKind::Binding(super::super::BindingIdentity { definition, scope }),
                Vec::new(),
                wrapping_i32(),
                ExpressionEffects::PURE,
            ),
        ));
        source
    }

    fn integer(&mut self, value: i128) -> NodeId {
        let source = self.add(CpgNodeKind::Literal {
            kind: LiteralKind::Integer(i64::try_from(value).expect("test value fits i64")),
        });
        self.facts.push((
            source,
            ExpressionFact::known(
                ExpressionFactKind::Literal(ExpressionLiteral::Integer(value)),
                Vec::new(),
                wrapping_i32(),
                ExpressionEffects::PURE,
            ),
        ));
        source
    }

    fn add_expression(&mut self, left: NodeId, right: NodeId) -> NodeId {
        let source = self.add(CpgNodeKind::BinaryOp {
            operator: Arc::from("+"),
        });
        self.cpg.connect(source, left, CpgEdgeKind::AstChild);
        self.cpg.connect(source, right, CpgEdgeKind::AstChild);
        self.facts.push((
            source,
            ExpressionFact::known(
                ExpressionFactKind::Binary(super::super::BinaryOperator::Add),
                vec![left, right],
                wrapping_i32(),
                ExpressionEffects::PURE,
            ),
        ));
        source
    }

    fn assignment(&mut self, name: &str, definition: Option<NodeId>) -> NodeId {
        let assignment = self.add(CpgNodeKind::Assignment {
            operator: Arc::from("="),
        });
        let target = self.add(CpgNodeKind::Identifier {
            name: Arc::from(name),
            definition,
        });
        self.cpg.connect(assignment, target, CpgEdgeKind::AstChild);
        self.attach(assignment);
        assignment
    }

    fn return_node(&mut self) -> NodeId {
        let node = self.add(CpgNodeKind::Return);
        self.attach(node);
        node
    }

    fn cfg(&mut self, source: NodeId, target: NodeId, kind: CfgEdgeKind) {
        self.cpg
            .connect(source, target, CpgEdgeKind::ControlFlow(kind));
    }

    fn lower(&self, root: NodeId) -> ExpressionIr {
        let facts = ExpressionFactTable::try_new(Language::Rust, self.facts.clone())
            .expect("fixture expression facts are unique");
        let ir = super::super::super::lower_expression_ir(
            &self.cpg,
            root,
            &facts,
            &self.profile,
            ExpressionIrLimits::default(),
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

fn analyze_linear_assignment_with_order(
    kill_same_binding: bool,
    reverse_candidates: bool,
) -> SemanticExpressionFlowReport {
    let mut fixture = Fixture::new();
    let x = fixture.definition("x", 1);
    let y = fixture.definition("y", 2);
    let x_first = fixture.binding("x", x, 1);
    let zero = fixture.integer(0);
    let first = fixture.add_expression(x_first, zero);
    fixture.attach(first);
    let assignment = fixture.assignment(
        if kill_same_binding { "x" } else { "y" },
        Some(if kill_same_binding { x } else { y }),
    );
    let second = fixture.binding("x", x, 1);
    fixture.attach(second);
    let return_node = fixture.return_node();
    fixture.cfg(fixture.function, first, CfgEdgeKind::Sequential);
    fixture.cfg(first, assignment, CfgEdgeKind::Sequential);
    fixture.cfg(assignment, second, CfgEdgeKind::Sequential);
    fixture.cfg(second, return_node, CfgEdgeKind::Sequential);

    let first_ir = fixture.lower(first);
    let second_ir = fixture.lower(second);
    let mut candidates = vec![
        ExpressionEquivalenceCandidate::new(first, fixture.function, &first_ir),
        ExpressionEquivalenceCandidate::new(second, fixture.function, &second_ir),
    ];
    if reverse_candidates {
        candidates.reverse();
    }
    analyze_semantic_expression_flow(
        &fixture.cpg,
        fixture.function,
        fixture.function,
        &fixture.profile,
        &candidates,
        SemanticExpressionFlowLimits::default(),
    )
    .expect("semantic flow succeeds")
}

fn analyze_linear_assignment(kill_same_binding: bool) -> SemanticExpressionFlowReport {
    analyze_linear_assignment_with_order(kill_same_binding, false)
}

#[test]
fn cse_requires_resolved_kill_freedom_not_identifier_spelling() {
    let killed = analyze_linear_assignment(true);
    assert!(killed.cse_candidates.is_empty());
    assert_eq!(killed.classes.len(), 1);
    assert_eq!(killed.classes[0].dependencies.len(), 1);

    let shadow_safe = analyze_linear_assignment(false);
    assert_eq!(
        shadow_safe.completion,
        SemanticExpressionFlowCompletion::Complete
    );
    assert_eq!(shadow_safe.cse_candidates.len(), 1);
    let candidate = &shadow_safe.cse_candidates[0];
    shadow_safe.equivalence.pairs[candidate.proof_pair]
        .proof
        .replay()
        .expect("CSE proof replays");
    assert_eq!(shadow_safe.pre_candidates.len(), 1);
}

#[test]
fn candidate_insertion_order_does_not_change_public_evidence() {
    let forward = analyze_linear_assignment_with_order(false, false);
    let reverse = analyze_linear_assignment_with_order(false, true);
    assert_eq!(forward.completion, reverse.completion);
    assert_eq!(forward.classes, reverse.classes);
    assert_eq!(forward.available_in, reverse.available_in);
    assert_eq!(forward.busy_out, reverse.busy_out);
    assert_eq!(forward.cse_candidates, reverse.cse_candidates);
    assert_eq!(forward.pre_candidates, reverse.pre_candidates);
    assert_eq!(forward.excluded, reverse.excluded);
    assert_eq!(forward.metrics, reverse.metrics);
    let pair_metadata = |report: &SemanticExpressionFlowReport| {
        report
            .equivalence
            .pairs
            .iter()
            .map(|pair| {
                (
                    pair.left,
                    pair.right,
                    pair.kind,
                    pair.alpha_bindings.clone(),
                    pair.proof.encoded_len(),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(pair_metadata(&forward), pair_metadata(&reverse));
}

#[test]
fn diamond_availability_does_not_invent_a_dominating_producer() {
    let mut fixture = Fixture::new();
    let x = fixture.definition("x", 1);
    let branch = fixture.add(CpgNodeKind::If);
    fixture.attach(branch);
    let mut roots = Vec::new();
    for _ in 0..3 {
        let binding = fixture.binding("x", x, 1);
        let zero = fixture.integer(0);
        let root = fixture.add_expression(binding, zero);
        fixture.attach(root);
        roots.push(root);
    }
    let join = fixture.add(CpgNodeKind::Block {
        scope: ScopeId::new(9),
    });
    fixture.attach(join);
    let return_node = fixture.return_node();
    fixture.cfg(fixture.function, branch, CfgEdgeKind::Sequential);
    fixture.cfg(branch, roots[0], CfgEdgeKind::ConditionalTrue);
    fixture.cfg(branch, roots[1], CfgEdgeKind::ConditionalFalse);
    fixture.cfg(roots[0], join, CfgEdgeKind::Sequential);
    fixture.cfg(roots[1], join, CfgEdgeKind::Sequential);
    fixture.cfg(join, roots[2], CfgEdgeKind::Sequential);
    fixture.cfg(roots[2], return_node, CfgEdgeKind::Sequential);
    let irs = roots
        .iter()
        .map(|&root| fixture.lower(root))
        .collect::<Vec<_>>();
    let candidates = roots
        .iter()
        .zip(&irs)
        .map(|(&root, ir)| ExpressionEquivalenceCandidate::new(root, fixture.function, ir))
        .collect::<Vec<_>>();
    let report = analyze_semantic_expression_flow(
        &fixture.cpg,
        fixture.function,
        fixture.function,
        &fixture.profile,
        &candidates,
        SemanticExpressionFlowLimits::default(),
    )
    .expect("diamond semantic flow succeeds");

    assert_eq!(report.classes.len(), 1);
    assert!(report.available_in[&roots[2]].contains(&report.classes[0].representative));
    assert!(report.cse_candidates.is_empty());
    assert_eq!(report.pre_candidates.len(), 1);
    assert_eq!(report.pre_candidates[0].placement, branch);
}

#[test]
fn unknown_call_is_a_conservative_flow_barrier() {
    let mut fixture = Fixture::new();
    let x = fixture.definition("x", 1);
    let first_binding = fixture.binding("x", x, 1);
    let zero = fixture.integer(0);
    let first = fixture.add_expression(first_binding, zero);
    fixture.attach(first);
    let call = fixture.add(CpgNodeKind::Call {
        target: None,
        is_method: false,
    });
    fixture.attach(call);
    let second = fixture.binding("x", x, 1);
    fixture.attach(second);
    let return_node = fixture.return_node();
    fixture.cfg(fixture.function, first, CfgEdgeKind::Sequential);
    fixture.cfg(first, call, CfgEdgeKind::Sequential);
    fixture.cfg(call, second, CfgEdgeKind::Sequential);
    fixture.cfg(second, return_node, CfgEdgeKind::Sequential);
    let first_ir = fixture.lower(first);
    let second_ir = fixture.lower(second);
    let report = analyze_semantic_expression_flow(
        &fixture.cpg,
        fixture.function,
        fixture.function,
        &fixture.profile,
        &[
            ExpressionEquivalenceCandidate::new(first, fixture.function, &first_ir),
            ExpressionEquivalenceCandidate::new(second, fixture.function, &second_ir),
        ],
        SemanticExpressionFlowLimits::default(),
    )
    .expect("call-barrier semantic flow succeeds");
    assert!(report.cse_candidates.is_empty());
}

#[test]
fn loop_reaches_fixed_point_and_visit_cap_remains_explicit() {
    let mut fixture = Fixture::new();
    let x = fixture.definition("x", 1);
    let first_binding = fixture.binding("x", x, 1);
    let first_zero = fixture.integer(0);
    let first = fixture.add_expression(first_binding, first_zero);
    fixture.attach(first);
    let header = fixture.add(CpgNodeKind::While);
    fixture.attach(header);
    let second_binding = fixture.binding("x", x, 1);
    let second_zero = fixture.integer(0);
    let second = fixture.add_expression(second_binding, second_zero);
    fixture.attach(second);
    let return_node = fixture.return_node();
    fixture.cfg(fixture.function, first, CfgEdgeKind::Sequential);
    fixture.cfg(first, header, CfgEdgeKind::Sequential);
    fixture.cfg(header, second, CfgEdgeKind::ConditionalTrue);
    fixture.cfg(second, header, CfgEdgeKind::LoopBack);
    fixture.cfg(header, return_node, CfgEdgeKind::ConditionalFalse);
    let first_ir = fixture.lower(first);
    let second_ir = fixture.lower(second);
    let candidates = [
        ExpressionEquivalenceCandidate::new(first, fixture.function, &first_ir),
        ExpressionEquivalenceCandidate::new(second, fixture.function, &second_ir),
    ];
    let complete = analyze_semantic_expression_flow(
        &fixture.cpg,
        fixture.function,
        fixture.function,
        &fixture.profile,
        &candidates,
        SemanticExpressionFlowLimits::default(),
    )
    .expect("loop reaches fixed point");
    assert_eq!(
        complete.completion,
        SemanticExpressionFlowCompletion::Complete
    );
    assert_eq!(complete.cse_candidates.len(), 1);

    let limited = analyze_semantic_expression_flow(
        &fixture.cpg,
        fixture.function,
        fixture.function,
        &fixture.profile,
        &candidates,
        SemanticExpressionFlowLimits {
            max_visits_per_block: 1,
            ..SemanticExpressionFlowLimits::default()
        },
    )
    .expect("visit limit is an honest report, not an error");
    assert!(!limited.completion.is_complete());
    assert!(
        !limited.available_completion.is_complete() || !limited.very_busy_completion.is_complete()
    );
}

#[test]
fn exceptional_exit_blocks_pre_but_not_normal_path_cse() {
    let mut fixture = Fixture::new();
    let x = fixture.definition("x", 1);
    let branch = fixture.add(CpgNodeKind::Try);
    fixture.attach(branch);
    let first_binding = fixture.binding("x", x, 1);
    let first_zero = fixture.integer(0);
    let first = fixture.add_expression(first_binding, first_zero);
    fixture.attach(first);
    let second_binding = fixture.binding("x", x, 1);
    let second_zero = fixture.integer(0);
    let second = fixture.add_expression(second_binding, second_zero);
    fixture.attach(second);
    let third_binding = fixture.binding("x", x, 1);
    let third_zero = fixture.integer(0);
    let third = fixture.add_expression(third_binding, third_zero);
    fixture.attach(third);
    let thrown = fixture.add(CpgNodeKind::Throw);
    fixture.attach(thrown);
    let return_node = fixture.return_node();
    fixture.cfg(fixture.function, branch, CfgEdgeKind::Sequential);
    fixture.cfg(branch, first, CfgEdgeKind::ConditionalTrue);
    fixture.cfg(branch, third, CfgEdgeKind::ConditionalFalse);
    fixture.cfg(branch, thrown, CfgEdgeKind::Throw);
    fixture.cfg(first, second, CfgEdgeKind::Sequential);
    fixture.cfg(second, return_node, CfgEdgeKind::Sequential);
    fixture.cfg(third, return_node, CfgEdgeKind::Sequential);
    let first_ir = fixture.lower(first);
    let second_ir = fixture.lower(second);
    let third_ir = fixture.lower(third);
    let report = analyze_semantic_expression_flow(
        &fixture.cpg,
        fixture.function,
        fixture.function,
        &fixture.profile,
        &[
            ExpressionEquivalenceCandidate::new(first, fixture.function, &first_ir),
            ExpressionEquivalenceCandidate::new(second, fixture.function, &second_ir),
            ExpressionEquivalenceCandidate::new(third, fixture.function, &third_ir),
        ],
        SemanticExpressionFlowLimits::default(),
    )
    .expect("exception CFG semantic flow succeeds");
    assert_eq!(report.cse_candidates.len(), 1);
    assert!(report.pre_candidates.is_empty());
    assert!(!report.busy_out[&branch].contains(&report.classes[0].representative));
}

#[test]
fn must_solver_matches_exhaustive_diamond_path_oracle() {
    for left_generate in [false, true] {
        for right_generate in [false, true] {
            for left_kill in [false, true] {
                for right_kill in [false, true] {
                    let mut fixture = Fixture::new();
                    let branch = fixture.add(CpgNodeKind::If);
                    let left = fixture.add(CpgNodeKind::Block {
                        scope: ScopeId::new(1),
                    });
                    let right = fixture.add(CpgNodeKind::Block {
                        scope: ScopeId::new(2),
                    });
                    let join = fixture.add(CpgNodeKind::Block {
                        scope: ScopeId::new(3),
                    });
                    let exit = fixture.return_node();
                    for node in [branch, left, right, join] {
                        fixture.attach(node);
                    }
                    fixture.cfg(fixture.function, branch, CfgEdgeKind::Sequential);
                    fixture.cfg(branch, left, CfgEdgeKind::ConditionalTrue);
                    fixture.cfg(branch, right, CfgEdgeKind::ConditionalFalse);
                    fixture.cfg(left, join, CfgEdgeKind::Sequential);
                    fixture.cfg(right, join, CfgEdgeKind::Sequential);
                    fixture.cfg(join, exit, CfgEdgeKind::Sequential);
                    let generated = [
                        left_generate.then_some((left, vec![0])),
                        right_generate.then_some((right, vec![0])),
                    ]
                    .into_iter()
                    .flatten()
                    .collect();
                    let killed = [
                        left_kill.then_some((left, vec![0])),
                        right_kill.then_some((right, vec![0])),
                    ]
                    .into_iter()
                    .flatten()
                    .collect();
                    let transfer = SemanticTransfer {
                        width: 1,
                        generated,
                        killed,
                    };
                    let cfg = block_cfg(&fixture.cpg, fixture.function).expect("block CFG");
                    let solution = solve(
                        &fixture.cpg,
                        &cfg,
                        &SemanticAvailable {
                            transfer: &transfer,
                        },
                        &SolverConfig::default(),
                        None,
                    )
                    .expect("must solver reaches a fixed point");
                    let at_join = solution
                        .state_at(&cfg, join, Direction::Forward, |node, state| {
                            transfer.transfer_node(node, state);
                        })
                        .expect("join is reachable")
                        .contains(0);
                    // Transfer order is kill then generate, so a generation at
                    // the same program point establishes the outgoing fact.
                    let left_oracle = left_generate || (!left_kill && false);
                    let right_oracle = right_generate || (!right_kill && false);
                    assert_eq!(at_join, left_oracle && right_oracle);
                }
            }
        }
    }
}

fn lower_closed_add(left: i128, right: i128) -> (NodeId, ExpressionIr, SemanticProfile) {
    let mut fixture = Fixture::new();
    let left = fixture.integer(left);
    let right = fixture.integer(right);
    let root = fixture.add_expression(left, right);
    fixture.attach(root);
    let ir = fixture.lower(root);
    (root, ir, fixture.profile)
}

fn lower_literal(value: i128) -> (NodeId, ExpressionIr, SemanticProfile) {
    let mut fixture = Fixture::new();
    let root = fixture.integer(value);
    fixture.attach(root);
    let ir = fixture.lower(root);
    (root, ir, fixture.profile)
}

fn lower_open_add_scoped(
    name: &str,
    scope: u32,
    constant: i128,
) -> (NodeId, ExpressionIr, SemanticProfile) {
    let mut fixture = Fixture::new();
    let definition = fixture.definition(name, scope);
    let binding = fixture.binding(name, definition, scope);
    let literal = fixture.integer(constant);
    let root = fixture.add_expression(binding, literal);
    fixture.attach(root);
    let ir = fixture.lower(root);
    (root, ir, fixture.profile)
}

fn lower_open_add(constant: i128) -> (NodeId, ExpressionIr, SemanticProfile) {
    lower_open_add_scoped("x", 1, constant)
}

#[test]
fn semantic_diff_distinguishes_equivalent_different_unknown_and_limited() {
    let (old_source, old_ir, profile) = lower_closed_add(1, 2);
    let (new_source, new_ir, _) = lower_literal(3);
    let equivalent = analyze_semantic_expression_diff(
        &profile,
        ExpressionEquivalenceCandidate::new(old_source, NodeId::new(10), &old_ir),
        ExpressionEquivalenceCandidate::new(new_source, NodeId::new(20), &new_ir),
        SemanticExpressionDiffLimits::default(),
    )
    .expect("constant-fold diff succeeds");
    assert_eq!(
        equivalent.status,
        SemanticExpressionDiffStatus::ProvenEquivalent
    );
    equivalent
        .proof
        .as_ref()
        .expect("equivalent diff has proof")
        .replay()
        .expect("diff proof replays");

    let (old_source, old_ir, profile) = lower_literal(1);
    let (new_source, new_ir, _) = lower_literal(2);
    let different = analyze_semantic_expression_diff(
        &profile,
        ExpressionEquivalenceCandidate::new(old_source, NodeId::new(10), &old_ir),
        ExpressionEquivalenceCandidate::new(new_source, NodeId::new(20), &new_ir),
        SemanticExpressionDiffLimits::default(),
    )
    .expect("unequal-value diff succeeds");
    assert_eq!(
        different.status,
        SemanticExpressionDiffStatus::ProvenDifferent
    );
    assert_ne!(different.old_value, different.new_value);

    let (old_source, old_ir, profile) = lower_open_add(1);
    let (new_source, new_ir, _) = lower_open_add(2);
    let unknown = analyze_semantic_expression_diff(
        &profile,
        ExpressionEquivalenceCandidate::new(old_source, NodeId::new(10), &old_ir),
        ExpressionEquivalenceCandidate::new(new_source, NodeId::new(20), &new_ir),
        SemanticExpressionDiffLimits::default(),
    )
    .expect("open call diff succeeds");
    assert_eq!(unknown.status, SemanticExpressionDiffStatus::Unknown);
    assert!(unknown.limitations.is_empty());

    let (old_source, old_ir, profile) = lower_closed_add(1, 2);
    let (new_source, new_ir, _) = lower_literal(4);
    let limited = analyze_semantic_expression_diff(
        &profile,
        ExpressionEquivalenceCandidate::new(old_source, NodeId::new(10), &old_ir),
        ExpressionEquivalenceCandidate::new(new_source, NodeId::new(20), &new_ir),
        SemanticExpressionDiffLimits {
            max_evaluation_work: 1,
            ..SemanticExpressionDiffLimits::default()
        },
    )
    .expect("bounded diff succeeds");
    assert_eq!(limited.status, SemanticExpressionDiffStatus::Limited);
    assert!(limited
        .limitations
        .contains(&SemanticExpressionDiffLimitation::OldEvaluationWork));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn unequal_closed_literals_always_produce_exact_difference(
        left in -10_000_i32..10_000,
        delta in 1_i32..1_000,
    ) {
        let right = left.saturating_add(delta);
        prop_assume!(left != right);
        let (old_source, old_ir, profile) = lower_literal(i128::from(left));
        let (new_source, new_ir, _) = lower_literal(i128::from(right));
        let report = analyze_semantic_expression_diff(
            &profile,
            ExpressionEquivalenceCandidate::new(old_source, NodeId::new(10), &old_ir),
            ExpressionEquivalenceCandidate::new(new_source, NodeId::new(20), &new_ir),
            SemanticExpressionDiffLimits::default(),
        ).expect("literal property diff succeeds");
        prop_assert_eq!(report.status, SemanticExpressionDiffStatus::ProvenDifferent);
        prop_assert!(report.proof.is_none());
        prop_assert!(report.limitations.is_empty());
    }

    #[test]
    fn alpha_renaming_is_bijective_and_independent_of_names_and_scope_numbers(
        left_scope in 0_u32..1_000,
        right_scope in 1_000_u32..2_000,
    ) {
        let (old_source, old_ir, profile) = lower_open_add_scoped("left_name", left_scope, 7);
        let (new_source, new_ir, _) = lower_open_add_scoped("right_name", right_scope, 7);
        let report = analyze_semantic_expression_diff(
            &profile,
            ExpressionEquivalenceCandidate::new(old_source, NodeId::new(10), &old_ir),
            ExpressionEquivalenceCandidate::new(new_source, NodeId::new(20), &new_ir),
            SemanticExpressionDiffLimits::default(),
        ).expect("alpha property diff succeeds");
        prop_assert_eq!(report.status, SemanticExpressionDiffStatus::ProvenEquivalent);
        prop_assert_eq!(report.alpha_bindings.len(), 1);
        prop_assert_eq!(report.alpha_bindings[0].slot, 0);
        prop_assert_ne!(
            report.alpha_bindings[0].left.scope,
            report.alpha_bindings[0].right.scope,
        );
        report.proof.as_ref().expect("alpha proof").replay()
            .expect("alpha proof replays");
    }
}

#[test]
fn depth_100_000_cfg_composition_fits_a_small_native_stack() {
    const NODE_COUNT: u32 = 100_000;
    std::thread::Builder::new()
        .name("semantic-expression-flow-small-stack".into())
        .stack_size(256 * 1024)
        .spawn(|| {
            let mut fixture = Fixture::new();
            let first = fixture.integer(7);
            fixture.attach(first);
            let mut previous = first;
            for scope in 0..NODE_COUNT - 2 {
                let node = fixture.add(CpgNodeKind::Block {
                    scope: ScopeId::new(scope),
                });
                fixture.attach(node);
                fixture.cfg(previous, node, CfgEdgeKind::Sequential);
                previous = node;
            }
            let second = fixture.integer(7);
            fixture.attach(second);
            fixture.cfg(previous, second, CfgEdgeKind::Sequential);
            let exit = fixture.return_node();
            fixture.cfg(fixture.function, first, CfgEdgeKind::Sequential);
            fixture.cfg(second, exit, CfgEdgeKind::Sequential);
            let first_ir = fixture.lower(first);
            let second_ir = fixture.lower(second);
            let report = analyze_semantic_expression_flow(
                &fixture.cpg,
                fixture.function,
                fixture.function,
                &fixture.profile,
                &[
                    ExpressionEquivalenceCandidate::new(first, fixture.function, &first_ir),
                    ExpressionEquivalenceCandidate::new(second, fixture.function, &second_ir),
                ],
                SemanticExpressionFlowLimits {
                    max_candidate_work: MAX_SEMANTIC_FLOW_WORK,
                    ..SemanticExpressionFlowLimits::default()
                },
            )
            .expect("deep CFG semantic flow succeeds");
            assert_eq!(report.cse_candidates.len(), 1);
            assert!(report.cse_candidates[0].producer < report.cse_candidates[0].use_node);
        })
        .expect("small-stack semantic-flow thread starts")
        .join()
        .expect("deep semantic-flow lifecycle remains stack safe");
}
