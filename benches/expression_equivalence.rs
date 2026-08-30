use std::sync::Arc;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use libcpg::analysis::{
    analyze_expression_equivalence, ExpressionEquivalenceCandidate, ExpressionEquivalenceLimits,
};
use libcpg::{
    lower_expression_ir, BinaryOperator, BindingIdentity, CodePropertyGraph, CpgEdgeKind, CpgNode,
    CpgNodeKind, ExpressionEffects, ExpressionFact, ExpressionFactKind, ExpressionFactTable,
    ExpressionIr, ExpressionIrLimits, ExpressionLiteral, ExpressionType, IntegerOverflow,
    IntegerType, Language, LiteralKind, NodeId, ScopeId, SemanticProfile, ShiftBehavior,
    SourceRange,
};

struct Workload {
    profile: SemanticProfile,
    roots: Vec<NodeId>,
    expressions: Vec<ExpressionIr>,
}

impl Workload {
    fn candidates(&self) -> Vec<ExpressionEquivalenceCandidate<'_>> {
        self.roots
            .iter()
            .copied()
            .zip(&self.expressions)
            .map(|(source, expression)| {
                ExpressionEquivalenceCandidate::new(source, NodeId::new(1_000_000), expression)
            })
            .collect()
    }
}

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

    fn node(&mut self, kind: CpgNodeKind) -> NodeId {
        self.cpg
            .add_node(CpgNode::new(NodeId::new(0), kind, SourceRange::default()))
    }

    fn literal(&mut self, value: i128, value_type: ExpressionType) -> NodeId {
        let source = self.node(CpgNodeKind::Literal {
            kind: LiteralKind::Integer(i64::try_from(value).expect("benchmark literal fits i64")),
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

    fn binding(&mut self, index: usize, value_type: ExpressionType) -> NodeId {
        let definition = self.node(CpgNodeKind::Variable {
            name: Arc::from(format!("v{index}-definition")),
            var_type: None,
            scope: ScopeId::new(1),
            is_mutable: false,
        });
        let source = self.node(CpgNodeKind::Identifier {
            name: Arc::from(format!("v{index}")),
            definition: Some(definition),
        });
        self.facts.push((
            source,
            ExpressionFact::known(
                ExpressionFactKind::Binding(BindingIdentity {
                    definition,
                    scope: 1,
                }),
                Vec::new(),
                value_type,
                ExpressionEffects::PURE,
            ),
        ));
        source
    }

    fn add(&mut self, left: NodeId, right: NodeId, value_type: ExpressionType) -> NodeId {
        let source = self.node(CpgNodeKind::BinaryOp {
            operator: Arc::from("Add"),
        });
        self.cpg.connect(source, left, CpgEdgeKind::AstChild);
        self.cpg.connect(source, right, CpgEdgeKind::AstChild);
        self.facts.push((
            source,
            ExpressionFact::known(
                ExpressionFactKind::Binary(BinaryOperator::Add),
                vec![left, right],
                value_type,
                ExpressionEffects::PURE,
            ),
        ));
        source
    }

    fn finish(self, roots: Vec<NodeId>) -> Workload {
        let profile = SemanticProfile::for_language(Language::Rust);
        let facts = ExpressionFactTable::try_new(Language::Rust, self.facts)
            .expect("benchmark facts are unique");
        let expressions = roots
            .iter()
            .map(|root| {
                lower_expression_ir(
                    &self.cpg,
                    *root,
                    &facts,
                    &profile,
                    ExpressionIrLimits::default(),
                )
            })
            .collect();
        Workload {
            profile,
            roots,
            expressions,
        }
    }
}

fn wrapping_i32() -> ExpressionType {
    ExpressionType::Integer(
        IntegerType::fixed(32, true, IntegerOverflow::Wrapping, ShiftBehavior::Masked)
            .expect("valid wrapping i32"),
    )
}

fn repeated_literal_workload(candidates: usize) -> Workload {
    let value_type = wrapping_i32();
    let mut fixture = Fixture::new();
    let roots = (0..candidates)
        .map(|_| fixture.literal(7, value_type))
        .collect();
    fixture.finish(roots)
}

fn ac_workload(operands: usize) -> Workload {
    let value_type = wrapping_i32();
    let mut fixture = Fixture::new();
    let bindings: Vec<_> = (0..operands)
        .map(|index| fixture.binding(index, value_type))
        .collect();
    let mut left = bindings[0];
    for &binding in &bindings[1..] {
        left = fixture.add(left, binding, value_type);
    }
    let mut right = *bindings.last().expect("nonempty AC workload");
    for &binding in bindings[..bindings.len() - 1].iter().rev() {
        right = fixture.add(binding, right, value_type);
    }
    fixture.finish(vec![left, right])
}

fn benchmark_expression_equivalence(c: &mut Criterion) {
    let mut pairs = c.benchmark_group("expression_equivalence/pair_output");
    for count in [2usize, 8, 32] {
        let workload = repeated_literal_workload(count);
        let candidates = workload.candidates();
        pairs.throughput(Throughput::Elements(count as u64));
        pairs.bench_with_input(BenchmarkId::from_parameter(count), &count, |bench, _| {
            bench.iter(|| {
                black_box(
                    analyze_expression_equivalence(
                        NodeId::new(1_000_001),
                        &workload.profile,
                        &candidates,
                        ExpressionEquivalenceLimits::default(),
                    )
                    .expect("benchmark analysis succeeds"),
                )
            });
        });
    }
    pairs.finish();

    let mut ac = c.benchmark_group("expression_equivalence/ac_normalization");
    for operands in [4usize, 16, 64] {
        let workload = ac_workload(operands);
        let candidates = workload.candidates();
        ac.throughput(Throughput::Elements(operands as u64));
        ac.bench_with_input(
            BenchmarkId::from_parameter(operands),
            &operands,
            |bench, _| {
                bench.iter(|| {
                    black_box(
                        analyze_expression_equivalence(
                            NodeId::new(1_000_002),
                            &workload.profile,
                            &candidates,
                            ExpressionEquivalenceLimits::default(),
                        )
                        .expect("benchmark analysis succeeds"),
                    )
                });
            },
        );
    }
    ac.finish();
}

criterion_group!(benches, benchmark_expression_equivalence);
criterion_main!(benches);
