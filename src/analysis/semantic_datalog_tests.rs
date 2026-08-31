use std::sync::Arc;

use proptest::prelude::*;

use super::*;
use crate::{
    CfgEdgeKind, CodePropertyGraph, CpgEdgeKind, CpgNode, CpgNodeKind, ExpressionEffects,
    ExpressionFact, ExpressionFactKind, ExpressionFactTable, ExpressionIr, ExpressionIrCompletion,
    ExpressionIrLimits, ExpressionLiteral, IntegerOverflow, IntegerType, Language, LiteralKind,
    MethodSignature, ScopeId, SemanticProfile, ShiftBehavior, SourceRange, Visibility,
};

use super::super::{
    analyze_semantic_expression_flow, BinaryOperator, BindingIdentity,
    ExpressionEquivalenceCandidate, ExpressionType, SemanticExpressionClass,
    SemanticExpressionFlowLimits,
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
                    name: "semantic_datalog".into(),
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
                ExpressionFactKind::Binding(BindingIdentity { definition, scope }),
                Vec::new(),
                wrapping_i32(),
                ExpressionEffects::PURE,
            ),
        ));
        source
    }

    fn integer(&mut self, value: i64) -> NodeId {
        let source = self.add(CpgNodeKind::Literal {
            kind: LiteralKind::Integer(value),
        });
        self.facts.push((
            source,
            ExpressionFact::known(
                ExpressionFactKind::Literal(ExpressionLiteral::Integer(i128::from(value))),
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
                ExpressionFactKind::Binary(BinaryOperator::Add),
                vec![left, right],
                wrapping_i32(),
                ExpressionEffects::PURE,
            ),
        ));
        source
    }

    fn assignment(&mut self, definition: NodeId) -> NodeId {
        let assignment = self.add(CpgNodeKind::Assignment {
            operator: Arc::from("="),
        });
        let target = self.add(CpgNodeKind::Identifier {
            name: Arc::from("y"),
            definition: Some(definition),
        });
        self.cpg.connect(assignment, target, CpgEdgeKind::AstChild);
        self.attach(assignment);
        assignment
    }

    fn lower(&self, root: NodeId) -> ExpressionIr {
        let facts = ExpressionFactTable::try_new(Language::Rust, self.facts.clone())
            .expect("fixture facts are unique");
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
            .expect("valid wrapping integer"),
    )
}

fn report_with_order(reverse_candidates: bool) -> SemanticExpressionFlowReport {
    let mut fixture = Fixture::new();
    let x = fixture.definition("x", 1);
    let y = fixture.definition("y", 2);
    let binding = fixture.binding("x", x, 1);
    let zero = fixture.integer(0);
    let first = fixture.add_expression(binding, zero);
    fixture.attach(first);
    let assignment = fixture.assignment(y);
    let second = fixture.binding("x", x, 1);
    fixture.attach(second);
    let return_node = fixture.add(CpgNodeKind::Return);
    fixture.attach(return_node);
    fixture.cpg.connect(
        fixture.function,
        first,
        CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
    );
    fixture.cpg.connect(
        first,
        assignment,
        CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
    );
    fixture.cpg.connect(
        assignment,
        second,
        CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
    );
    fixture.cpg.connect(
        second,
        return_node,
        CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
    );

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

fn report() -> SemanticExpressionFlowReport {
    report_with_order(false)
}

#[test]
fn exports_owned_proof_qualified_canonical_relations() {
    let report = report();
    let facts = export_semantic_datalog(&report, SemanticDatalogLimits::default())
        .expect("valid report exports");

    assert_eq!(facts.class_members().len(), 2);
    assert_eq!(facts.proof_edges().len(), 1);
    assert_eq!(facts.cse_candidates().len(), 1);
    assert_eq!(facts.pre_candidates().len(), 2);
    assert_eq!(facts.completions().len(), 5);
    for edge in facts.proof_edges() {
        let source = &report.equivalence.pairs[edge.proof_pair as usize];
        assert_eq!((source.left, source.right), (edge.left, edge.right));
        source.proof.replay().expect("exported proof replays");
    }

    let files = facts.souffle_files().expect("canonical rendering fits");
    assert_eq!(
        files.iter().map(|file| file.relation).collect::<Vec<_>>(),
        vec![
            "semantic_class",
            "semantic_proof",
            "semantic_available",
            "semantic_anticipable",
            "semantic_cse_candidate",
            "semantic_pre_candidate",
            "semantic_completion",
        ]
    );
    assert!(files.iter().all(|file| !file.contents.contains("replete")));
}

#[test]
fn candidate_insertion_order_cannot_change_exported_evidence() {
    let forward =
        export_semantic_datalog(&report_with_order(false), SemanticDatalogLimits::default())
            .expect("forward report exports");
    let reverse =
        export_semantic_datalog(&report_with_order(true), SemanticDatalogLimits::default())
            .expect("reverse report exports");
    assert_eq!(forward, reverse);
    assert_eq!(
        forward.souffle_files().unwrap(),
        reverse.souffle_files().unwrap()
    );
}

#[test]
fn valid_positive_evidence_survives_an_honestly_limited_report() {
    let mut report = report();
    report.equivalence.exact_completion = ExpressionEquivalenceCompletion::WorkLimited;
    report.completion = SemanticExpressionFlowCompletion::EquivalenceLimited(
        ExpressionEquivalenceCompletion::WorkLimited,
    );
    let facts = export_semantic_datalog(&report, SemanticDatalogLimits::default())
        .expect("limited positive report exports");
    assert_eq!(facts.proof_edges().len(), 1);
    assert_eq!(
        facts
            .query_completion(
                SemanticDatalogDimension::ExactEquality,
                DatalogEvaluationCompletion::Complete,
            )
            .semantic,
        SemanticDatalogCompletion::WorkLimited
    );
    assert!(!facts
        .query_completion(
            SemanticDatalogDimension::ExactEquality,
            DatalogEvaluationCompletion::Complete,
        )
        .can_conclude_absence());
}

#[test]
fn every_limit_dimension_preserves_replayed_positive_evidence() {
    let equality_limits = [
        ExpressionEquivalenceCompletion::UnsupportedInputs,
        ExpressionEquivalenceCompletion::CandidateLimited,
        ExpressionEquivalenceCompletion::InputNodeLimited,
        ExpressionEquivalenceCompletion::NodeLimited,
        ExpressionEquivalenceCompletion::WorkLimited,
        ExpressionEquivalenceCompletion::MatchLimited,
        ExpressionEquivalenceCompletion::IterationLimited,
        ExpressionEquivalenceCompletion::PairLimited,
        ExpressionEquivalenceCompletion::ProofLimited,
    ];
    for limit in equality_limits {
        let mut limited = report();
        limited.equivalence.exact_completion = limit;
        limited.completion = if limit == ExpressionEquivalenceCompletion::UnsupportedInputs {
            SemanticExpressionFlowCompletion::UnsupportedInputs
        } else {
            SemanticExpressionFlowCompletion::EquivalenceLimited(limit)
        };
        let facts = export_semantic_datalog(&limited, SemanticDatalogLimits::default())
            .expect("a limit does not invalidate an existing proof");
        assert_eq!(facts.proof_edges().len(), 1);
        assert_eq!(
            facts.query_completion(
                SemanticDatalogDimension::ExactEquality,
                DatalogEvaluationCompletion::Complete,
            ),
            SemanticDatalogQueryCompletion {
                dimension: SemanticDatalogDimension::ExactEquality,
                semantic: equality_completion(limit),
                datalog: DatalogEvaluationCompletion::Complete,
            }
        );
    }

    let mut availability_limited = report();
    availability_limited.available_completion = SemanticDataflowCompletion::IterationLimited;
    availability_limited.completion = SemanticExpressionFlowCompletion::AvailableDataflowLimited;
    let availability =
        export_semantic_datalog(&availability_limited, SemanticDatalogLimits::default()).unwrap();
    assert_eq!(availability.proof_edges().len(), 1);
    assert_eq!(
        availability
            .query_completion(
                SemanticDatalogDimension::CseCandidates,
                DatalogEvaluationCompletion::Complete,
            )
            .semantic,
        SemanticDatalogCompletion::AvailableDataflowLimited
    );

    let mut anticipability_limited = report();
    anticipability_limited.very_busy_completion = SemanticDataflowCompletion::IterationLimited;
    anticipability_limited.completion = SemanticExpressionFlowCompletion::VeryBusyDataflowLimited;
    let anticipability =
        export_semantic_datalog(&anticipability_limited, SemanticDatalogLimits::default()).unwrap();
    assert_eq!(anticipability.proof_edges().len(), 1);
    assert_eq!(
        anticipability
            .query_completion(
                SemanticDatalogDimension::PreCandidates,
                DatalogEvaluationCompletion::Complete,
            )
            .semantic,
        SemanticDatalogCompletion::VeryBusyDataflowLimited
    );

    let mut candidate_limited = report();
    candidate_limited.completion = SemanticExpressionFlowCompletion::CandidateWorkLimited;
    let candidates =
        export_semantic_datalog(&candidate_limited, SemanticDatalogLimits::default()).unwrap();
    assert_eq!(candidates.proof_edges().len(), 1);
    for dimension in [
        SemanticDatalogDimension::CseCandidates,
        SemanticDatalogDimension::PreCandidates,
    ] {
        assert_eq!(
            candidates
                .query_completion(dimension, DatalogEvaluationCompletion::Complete)
                .semantic,
            SemanticDatalogCompletion::CandidateWorkLimited
        );
    }
}

#[test]
fn facts_remain_owned_and_usable_after_the_source_report_is_dropped() {
    let report = report();
    let before = format!("{report:?}");
    let facts = export_semantic_datalog(&report, SemanticDatalogLimits::default()).unwrap();
    assert_eq!(format!("{report:?}"), before);
    drop(report);

    let clone = facts.clone();
    assert_eq!(clone, facts);
    assert_eq!(
        clone.souffle_files().unwrap(),
        facts.souffle_files().unwrap()
    );
    #[cfg(feature = "serde")]
    assert_eq!(
        serde_json::to_vec(&clone).unwrap(),
        serde_json::to_vec(&facts).unwrap()
    );
}

#[test]
fn malformed_report_evidence_is_rejected_at_the_boundary() {
    let mut noncanonical = report();
    noncanonical.classes[0].occurrences.reverse();
    assert!(matches!(
        export_semantic_datalog(&noncanonical, SemanticDatalogLimits::default()),
        Err(SemanticDatalogError::Invariant(_))
    ));

    let mut missing_cse_proof = report();
    missing_cse_proof.cse_candidates[0].proof_pair = usize::MAX;
    assert!(matches!(
        export_semantic_datalog(&missing_cse_proof, SemanticDatalogLimits::default()),
        Err(SemanticDatalogError::Invariant(_))
    ));

    let mut disconnected_class = report();
    disconnected_class.equivalence.pairs.clear();
    assert!(matches!(
        export_semantic_datalog(&disconnected_class, SemanticDatalogLimits::default()),
        Err(SemanticDatalogError::Invariant(_))
    ));

    let mut disconnected_pre = report();
    disconnected_pre.pre_candidates[0].proof_pairs.clear();
    assert!(matches!(
        export_semantic_datalog(&disconnected_pre, SemanticDatalogLimits::default()),
        Err(SemanticDatalogError::Invariant(_))
    ));

    let mut unknown_class = report();
    unknown_class
        .available_in
        .insert(NodeId::new(900), vec![NodeId::new(901)]);
    assert!(matches!(
        export_semantic_datalog(&unknown_class, SemanticDatalogLimits::default()),
        Err(SemanticDatalogError::Invariant(_))
    ));

    let mut duplicate_member = report();
    let representative = duplicate_member.classes[0].representative;
    duplicate_member.classes[0].occurrences = vec![representative, representative];
    assert!(matches!(
        export_semantic_datalog(&duplicate_member, SemanticDatalogLimits::default()),
        Err(SemanticDatalogError::Invariant(_))
    ));

    let mut duplicate_point = report();
    let representative = duplicate_point.classes[0].representative;
    duplicate_point
        .available_in
        .insert(NodeId::new(902), vec![representative, representative]);
    assert!(matches!(
        export_semantic_datalog(&duplicate_point, SemanticDatalogLimits::default()),
        Err(SemanticDatalogError::Invariant(_))
    ));

    let mut duplicate_proof = report();
    duplicate_proof
        .equivalence
        .pairs
        .push(duplicate_proof.equivalence.pairs[0].clone());
    assert!(matches!(
        export_semantic_datalog(&duplicate_proof, SemanticDatalogLimits::default()),
        Err(SemanticDatalogError::Invariant(_))
    ));

    let mut duplicate_cse = report();
    duplicate_cse
        .cse_candidates
        .push(duplicate_cse.cse_candidates[0].clone());
    assert!(matches!(
        export_semantic_datalog(&duplicate_cse, SemanticDatalogLimits::default()),
        Err(SemanticDatalogError::Invariant(_))
    ));

    let mut duplicate_pre = report();
    duplicate_pre
        .pre_candidates
        .push(duplicate_pre.pre_candidates[0].clone());
    assert!(matches!(
        export_semantic_datalog(&duplicate_pre, SemanticDatalogLimits::default()),
        Err(SemanticDatalogError::Invariant(_))
    ));

    let mut inconsistent_completion = report();
    inconsistent_completion.equivalence.exact_completion =
        ExpressionEquivalenceCompletion::WorkLimited;
    assert!(matches!(
        export_semantic_datalog(&inconsistent_completion, SemanticDatalogLimits::default()),
        Err(SemanticDatalogError::Invariant(_))
    ));
}

#[test]
fn fact_and_rendering_limits_fail_closed() {
    let report = report();
    assert!(matches!(
        export_semantic_datalog(
            &report,
            SemanticDatalogLimits {
                max_facts: 0,
                ..SemanticDatalogLimits::default()
            }
        ),
        Err(SemanticDatalogError::InvalidLimit("max_facts"))
    ));
    assert!(matches!(
        export_semantic_datalog(
            &report,
            SemanticDatalogLimits {
                max_rendered_bytes: MAX_SEMANTIC_DATALOG_BYTES + 1,
                ..SemanticDatalogLimits::default()
            }
        ),
        Err(SemanticDatalogError::InvalidLimit("max_rendered_bytes"))
    ));

    let facts = export_semantic_datalog(&report, SemanticDatalogLimits::default()).unwrap();
    let row_limit = facts.fact_count() - 1;
    assert!(matches!(
        export_semantic_datalog(
            &report,
            SemanticDatalogLimits {
                max_facts: row_limit,
                ..SemanticDatalogLimits::default()
            }
        ),
        Err(SemanticDatalogError::FactLimit { .. })
    ));

    let rendered_bytes = facts
        .souffle_files()
        .unwrap()
        .iter()
        .map(|file| file.contents.len())
        .sum::<usize>();
    let byte_limited = export_semantic_datalog(
        &report,
        SemanticDatalogLimits {
            max_rendered_bytes: rendered_bytes - 1,
            ..SemanticDatalogLimits::default()
        },
    )
    .unwrap();
    assert!(matches!(
        byte_limited.souffle_files(),
        Err(SemanticDatalogError::RenderedByteLimit { .. })
    ));
}

#[test]
fn completion_rendering_is_exact_and_canonical() {
    let facts = export_semantic_datalog(&report(), SemanticDatalogLimits::default()).unwrap();
    let files = facts.souffle_files().unwrap();
    let completion = files
        .iter()
        .find(|file| file.relation == "semantic_completion")
        .expect("completion relation exists");
    let function = facts.completions()[0].function.as_u32();
    assert_eq!(
        completion.contents,
        format!(
            "{function}\t\"exact_equality\"\t\"complete\"\n\
             {function}\t\"availability\"\t\"complete\"\n\
             {function}\t\"anticipability\"\t\"complete\"\n\
             {function}\t\"cse_candidates\"\t\"complete\"\n\
             {function}\t\"pre_candidates\"\t\"complete\"\n"
        )
    );
}

fn semantic_completions() -> [SemanticDatalogCompletion; 13] {
    [
        SemanticDatalogCompletion::Complete,
        SemanticDatalogCompletion::UnsupportedInputs,
        SemanticDatalogCompletion::CandidateLimited,
        SemanticDatalogCompletion::InputNodeLimited,
        SemanticDatalogCompletion::NodeLimited,
        SemanticDatalogCompletion::WorkLimited,
        SemanticDatalogCompletion::MatchLimited,
        SemanticDatalogCompletion::IterationLimited,
        SemanticDatalogCompletion::PairLimited,
        SemanticDatalogCompletion::ProofLimited,
        SemanticDatalogCompletion::AvailableDataflowLimited,
        SemanticDatalogCompletion::VeryBusyDataflowLimited,
        SemanticDatalogCompletion::CandidateWorkLimited,
    ]
}

fn datalog_completions() -> [DatalogEvaluationCompletion; 7] {
    [
        DatalogEvaluationCompletion::Complete,
        DatalogEvaluationCompletion::WorkLimited,
        DatalogEvaluationCompletion::IterationLimited,
        DatalogEvaluationCompletion::MemoryLimited,
        DatalogEvaluationCompletion::TimeLimited,
        DatalogEvaluationCompletion::Cancelled,
        DatalogEvaluationCompletion::Unknown,
    ]
}

proptest! {
    #[test]
    fn absence_is_conclusive_exactly_when_both_fixed_points_complete(
        semantic_index in 0usize..13,
        datalog_index in 0usize..7,
    ) {
        let semantic = semantic_completions()[semantic_index];
        let datalog = datalog_completions()[datalog_index];
        let completion = SemanticDatalogQueryCompletion {
            dimension: SemanticDatalogDimension::ExactEquality,
            semantic,
            datalog,
        };
        let expected = semantic == SemanticDatalogCompletion::Complete
            && datalog == DatalogEvaluationCompletion::Complete;
        prop_assert_eq!(completion.is_complete(), expected);
        prop_assert_eq!(completion.can_conclude_absence(), expected);
    }
}

#[cfg(feature = "serde")]
#[test]
fn serialization_is_deterministic_but_does_not_restore_proof_authority() {
    let facts = export_semantic_datalog(&report(), SemanticDatalogLimits::default()).unwrap();
    let first = serde_json::to_vec(&facts).expect("semantic facts serialize");
    let second = serde_json::to_vec(&facts).expect("semantic facts reserialize");
    assert_eq!(first, second);
    assert!(!first.is_empty());
}

#[test]
fn hundred_thousand_rows_are_stack_safe_on_a_small_thread_stack() {
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            let mut report = report();
            report.classes = (0..100_000)
                .map(|index| {
                    let occurrence = NodeId::new(index + 1_000);
                    SemanticExpressionClass {
                        representative: occurrence,
                        occurrences: vec![occurrence],
                        dependencies: Vec::new(),
                    }
                })
                .collect();
            report.available_in.clear();
            report.busy_out.clear();
            report.cse_candidates.clear();
            report.pre_candidates.clear();
            report.equivalence.pairs.clear();
            let facts = export_semantic_datalog(
                &report,
                SemanticDatalogLimits {
                    max_facts: 100_005,
                    max_rendered_bytes: 16 * 1024 * 1024,
                },
            )
            .expect("large flat export succeeds");
            assert_eq!(facts.class_members().len(), 100_000);
            let cloned = facts.clone();
            assert_eq!(cloned.fact_count(), 100_005);
            let rendered = cloned.souffle_files().expect("large rendering succeeds");
            assert_eq!(rendered.len(), 7);
            #[cfg(feature = "serde")]
            assert!(!serde_json::to_vec(&cloned)
                .expect("large flat facts serialize")
                .is_empty());
            drop(rendered);
            drop(cloned);
            drop(facts);
            drop(report);
        })
        .expect("small-stack worker starts")
        .join()
        .expect("small-stack worker completes");
}
