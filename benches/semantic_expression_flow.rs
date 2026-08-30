use std::sync::Arc;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use libcpg::analysis::{
    analyze_semantic_expression_diff, analyze_semantic_expression_flow,
    ExpressionEquivalenceCandidate, SemanticExpressionDiffLimits, SemanticExpressionFlowLimits,
};
use libcpg::{
    lower_expression_ir, BinaryOperator, CodePropertyGraph, CpgEdgeKind, CpgNode, CpgNodeKind,
    ExpressionEffects, ExpressionFact, ExpressionFactKind, ExpressionFactTable, ExpressionIr,
    ExpressionIrLimits, ExpressionLiteral, ExpressionType, IntegerOverflow, IntegerType, Language,
    LiteralKind, MethodSignature, NodeId, SemanticProfile, ShiftBehavior, SourceRange, Visibility,
};

struct FlowWorkload {
    cpg: CodePropertyGraph,
    function: NodeId,
    profile: SemanticProfile,
    roots: Vec<NodeId>,
    expressions: Vec<ExpressionIr>,
}

impl FlowWorkload {
    fn candidates(&self) -> Vec<ExpressionEquivalenceCandidate<'_>> {
        self.roots
            .iter()
            .copied()
            .zip(&self.expressions)
            .map(|(source, ir)| ExpressionEquivalenceCandidate::new(source, self.function, ir))
            .collect()
    }
}

fn node(kind: CpgNodeKind) -> CpgNode {
    CpgNode::new(NodeId::new(0), kind, SourceRange::default())
}

fn wrapping_i32() -> ExpressionType {
    ExpressionType::Integer(
        IntegerType::fixed(32, true, IntegerOverflow::Wrapping, ShiftBehavior::Masked)
            .expect("valid wrapping i32"),
    )
}

fn repeated_literal_flow(count: usize) -> FlowWorkload {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = cpg.add_node(node(CpgNodeKind::Function {
        signature: Box::new(MethodSignature {
            name: "flow_benchmark".into(),
            params: Default::default(),
            return_type: None,
            is_static: false,
            is_async: false,
            visibility: Visibility::Private,
        }),
    }));
    let mut roots = Vec::with_capacity(count);
    let mut facts = Vec::with_capacity(count);
    let mut previous = function;
    for _ in 0..count {
        let root = cpg.add_node(node(CpgNodeKind::Literal {
            kind: LiteralKind::Integer(7),
        }));
        cpg.connect(function, root, CpgEdgeKind::AstChild);
        cpg.connect(
            previous,
            root,
            CpgEdgeKind::ControlFlow(libcpg::CfgEdgeKind::Sequential),
        );
        facts.push((
            root,
            ExpressionFact::known(
                ExpressionFactKind::Literal(ExpressionLiteral::Integer(7)),
                Vec::new(),
                wrapping_i32(),
                ExpressionEffects::PURE,
            ),
        ));
        roots.push(root);
        previous = root;
    }
    let exit = cpg.add_node(node(CpgNodeKind::Return));
    cpg.connect(function, exit, CpgEdgeKind::AstChild);
    cpg.connect(
        previous,
        exit,
        CpgEdgeKind::ControlFlow(libcpg::CfgEdgeKind::Sequential),
    );
    let facts =
        ExpressionFactTable::try_new(Language::Rust, facts).expect("benchmark facts are unique");
    let profile = SemanticProfile::for_language(Language::Rust);
    let expressions = roots
        .iter()
        .map(|root| {
            lower_expression_ir(&cpg, *root, &facts, &profile, ExpressionIrLimits::default())
        })
        .collect();
    FlowWorkload {
        cpg,
        function,
        profile,
        roots,
        expressions,
    }
}

struct DiffWorkload {
    profile: SemanticProfile,
    old_source: NodeId,
    old_ir: ExpressionIr,
    new_source: NodeId,
    new_ir: ExpressionIr,
}

fn constant_fold_diff() -> DiffWorkload {
    let profile = SemanticProfile::for_language(Language::Rust);
    let value_type = wrapping_i32();
    let mut old = CodePropertyGraph::new(Language::Rust);
    let one = old.add_node(node(CpgNodeKind::Literal {
        kind: LiteralKind::Integer(1),
    }));
    let two = old.add_node(node(CpgNodeKind::Literal {
        kind: LiteralKind::Integer(2),
    }));
    let old_source = old.add_node(node(CpgNodeKind::BinaryOp {
        operator: Arc::from("+"),
    }));
    old.connect(old_source, one, CpgEdgeKind::AstChild);
    old.connect(old_source, two, CpgEdgeKind::AstChild);
    let old_facts = ExpressionFactTable::try_new(
        Language::Rust,
        [
            (
                one,
                ExpressionFact::known(
                    ExpressionFactKind::Literal(ExpressionLiteral::Integer(1)),
                    Vec::new(),
                    value_type,
                    ExpressionEffects::PURE,
                ),
            ),
            (
                two,
                ExpressionFact::known(
                    ExpressionFactKind::Literal(ExpressionLiteral::Integer(2)),
                    Vec::new(),
                    value_type,
                    ExpressionEffects::PURE,
                ),
            ),
            (
                old_source,
                ExpressionFact::known(
                    ExpressionFactKind::Binary(BinaryOperator::Add),
                    vec![one, two],
                    value_type,
                    ExpressionEffects::PURE,
                ),
            ),
        ],
    )
    .expect("old benchmark facts are unique");
    let old_ir = lower_expression_ir(
        &old,
        old_source,
        &old_facts,
        &profile,
        ExpressionIrLimits::default(),
    );

    let mut new = CodePropertyGraph::new(Language::Rust);
    let new_source = new.add_node(node(CpgNodeKind::Literal {
        kind: LiteralKind::Integer(3),
    }));
    let new_facts = ExpressionFactTable::try_new(
        Language::Rust,
        [(
            new_source,
            ExpressionFact::known(
                ExpressionFactKind::Literal(ExpressionLiteral::Integer(3)),
                Vec::new(),
                value_type,
                ExpressionEffects::PURE,
            ),
        )],
    )
    .expect("new benchmark facts are unique");
    let new_ir = lower_expression_ir(
        &new,
        new_source,
        &new_facts,
        &profile,
        ExpressionIrLimits::default(),
    );
    DiffWorkload {
        profile,
        old_source,
        old_ir,
        new_source,
        new_ir,
    }
}

fn benchmark_semantic_expression_flow(c: &mut Criterion) {
    let mut flow = c.benchmark_group("semantic_expression_flow/linear_reuse");
    for count in [2usize, 8, 32] {
        let workload = repeated_literal_flow(count);
        let candidates = workload.candidates();
        flow.throughput(Throughput::Elements(count as u64));
        flow.bench_with_input(BenchmarkId::from_parameter(count), &count, |bench, _| {
            bench.iter(|| {
                black_box(
                    analyze_semantic_expression_flow(
                        &workload.cpg,
                        workload.function,
                        workload.function,
                        &workload.profile,
                        &candidates,
                        SemanticExpressionFlowLimits::default(),
                    )
                    .expect("flow benchmark succeeds"),
                )
            });
        });
    }
    flow.finish();

    let workload = constant_fold_diff();
    c.bench_function("semantic_expression_flow/cross_revision_fold", |bench| {
        bench.iter(|| {
            black_box(
                analyze_semantic_expression_diff(
                    &workload.profile,
                    ExpressionEquivalenceCandidate::new(
                        workload.old_source,
                        NodeId::new(10),
                        &workload.old_ir,
                    ),
                    ExpressionEquivalenceCandidate::new(
                        workload.new_source,
                        NodeId::new(20),
                        &workload.new_ir,
                    ),
                    SemanticExpressionDiffLimits::default(),
                )
                .expect("semantic diff benchmark succeeds"),
            )
        });
    });
}

criterion_group!(benches, benchmark_semantic_expression_flow);
criterion_main!(benches);
