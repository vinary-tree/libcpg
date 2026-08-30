# Use bounded expression equivalence

This guide starts after a frontend has produced live, complete, pure
[`ExpressionIr`](../api/expression-ir-reference.md) values. The analysis does
not parse source or infer missing types, effects, calls, bindings, or language
semantics.

## 1. Prepare candidates

Each candidate has a source node, a binding-context namespace, and a borrowed
typed IR. The namespace distinguishes otherwise identical local binding ids
from separate functions or scopes.

```rust,ignore
use libcpg::analysis::{
    analyze_expression_equivalence, ExpressionEquivalenceCandidate,
    ExpressionEquivalenceLimits,
};
use libcpg::NodeId;

let candidates = [
    ExpressionEquivalenceCandidate::new(left_source, left_function, &left_ir),
    ExpressionEquivalenceCandidate::new(right_source, right_function, &right_ir),
];
let report = analyze_expression_equivalence(
    NodeId::new(9_001),
    &profile,
    &candidates,
    ExpressionEquivalenceLimits::default(),
)?;
```

The first argument identifies this analysis run. It is not a substitute for a
candidate's binding context.

## 2. Inspect completion before interpreting absence

Exact and alpha passes can complete or stop independently. Retain both typed
completion values in downstream results.

```rust,ignore
use libcpg::analysis::ExpressionEquivalenceCompletion;

let exact_complete = matches!(
    report.exact_completion,
    ExpressionEquivalenceCompletion::Complete,
);
let alpha_complete = matches!(
    report.alpha_completion,
    ExpressionEquivalenceCompletion::Complete,
);
if !exact_complete || !alpha_complete {
    // Preserve the exact limit or unsupported reason. An omitted pair is not
    // evidence of inequivalence when its pass is incomplete.
}
```

Choose `ExpressionEquivalenceLimits` from service policy. The candidate,
input-node, e-node, work, match, iteration, pair, and proof-term caps bound
different resources and should not be merged into one ambiguous budget.

## 3. Interpret qualified pairs

```rust,ignore
use libcpg::analysis::ExpressionEquivalenceKind;

for pair in &report.pairs {
    match pair.kind {
        ExpressionEquivalenceKind::Exact => {
            // Same represented expression semantics and binding identity.
        }
        ExpressionEquivalenceKind::AlphaRenamedType4Clone => {
            // Structurally equivalent under this explicit binding bijection.
            assert!(!pair.alpha_bindings.is_empty());
        }
    }
    pair.proof.replay()?;
}
```

Exact pairs may be justified by typed identity, commutativity, associativity,
authorized identities, constant folding, or a composition of those laws.
Alpha pairs additionally retain their resolved binding bijection. Alpha
qualification never silently becomes exact equality.

## Worked meanings

| Source pair | Possible result | Required premise |
|---|---|---|
| `x + 0`, `x` | exact | additive identity is legal for the exact type/profile |
| `a + b`, `b + a` | exact | commutativity is authorized and effects are absent |
| `(a + b) + c`, `a + (c + b)` | exact | associativity and commutativity are both authorized |
| `first + 1`, `second + 1` | alpha | one total bijection maps `first` to `second` |
| `a + a`, `x + y` | no alpha pair | the proposed binding relation is not bijective |
| `(2 + 3) * 4`, `20` | exact | the closed typed evaluator reaches the exact literal |

For AC operators, normalization preserves the multiset of leaves. In
particular, `a + a + b` and `a + b + b` are not equivalent merely because they
contain the same distinct names.

## 4. Handle unsupported candidates explicitly

Read `&report.unsupported` alongside the pair list. Root mismatch, duplicate
source identity, profile mismatch, incomplete lowering, persisted-only
provenance, effectful roots, malformed operators, and cap exhaustion are
different operational diagnoses. None should be rewritten as a Boolean “not
equivalent.”

## 5. Compose with other analyses without changing their semantics

Expression equality is additive evidence. It does not replace structural
`CloneType`, a Datalog relation, data-flow reachability, approximate similarity,
or a security verdict. A consumer may correlate these results, but it must
retain each evidence kind and its completion state.

Replete remains a standalone crate. libcpg owns the typed adapter and replay of
libcpg-specific laws; Replete owns generic e-classes, congruence, saturation,
and provenance. This workflow does not modify or integrate MeTTaIL.

## 6. Keep deep workloads stack-safe

Do not recursively walk `ExpressionIr` or proof terms in consumers. Use their
flat arenas, iterators, and public report interfaces. The production analysis,
proof replay, formatting, serialization, cloning, and drop paths are validated
at 100,000 levels on a 256 KiB worker stack.

## Related documentation

- [API reference](../api/expression-equivalence-reference.md)
- [Component contract](../components/graph/expression-equivalence.md)
- [ADR-0075](../design/0075-proof-producing-expression-equivalence.md)
- [Security boundary](../security/33-expression-equivalence-boundary.md)
- [Scientific validation](../scientific/70-expression-equivalence-validation.md)
- [Datalog, e-graphs, and typed semantics](../theory/57-datalog-egraphs-and-typed-expression-semantics.md)

## Reference

Tate, R., Stepp, M., Tatlock, Z., Lerner, S. (2009). *Equality Saturation: A
New Approach to Optimization.* DOI:
[10.1145/1480881.1480915](https://doi.org/10.1145/1480881.1480915).
