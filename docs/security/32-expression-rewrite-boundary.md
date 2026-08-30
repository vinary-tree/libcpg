# Security boundary -- Typed expression rewrite evidence

## Security claim

libcpg can certify one named local rewrite only when live lowering has checked a
complete semantic profile, a pure typed subtree, immutable CPG correspondence,
and the law's exact preconditions. It does not certify whole-program
equivalence, source-language correctness outside represented facts, or the
safety/profitability of a later extracted program.

## Trusted and untrusted inputs

| Input | Treatment |
|---|---|
| CPG node identity and ordered AST children | immutable structural evidence |
| frontend type/effect/call/binding facts | untrusted until cross-checked against representable CPG identity and topology |
| semantic profile | explicit policy; any unknown dimension fails closed |
| operator spelling and source text | ignored by the IR layer |
| serialized IR | inspectable but identity-only; never rewrite authority |
| rewrite certificate | live process-local construction; serializable audit output, not deserializable input |
| Replete rule set | separate standalone consumer responsibility |

## Failure and incompleteness semantics

`WorkLimited`, `NodeLimited`, `OperandLimited`, and `Malformed` cannot expose a
root for certification. An identity-only or unknown node cannot expose a
certificate even when the surrounding lowering is complete. No empty or
partial artifact means “safe,” “equivalent,” or “no defect.”

## Numeric hazards

- Public integer descriptors are validated before range arithmetic, preventing
  zero-width underflow and incoherent fixed/arbitrary-precision modes.
- Checked and undefined shifts require a count in the represented width;
  masked shifts accept signed counts because masking defines the effective
  count.
- Division/remainder by zero and non-representable checked results are denied.
- Strict IEEE 754 mode preserves NaN, infinities, rounding, and signed zero by
  denying algebraic rewrite certificates.
- Finite-reassociation mode rejects NaN, infinities, and negative zero at the
  literal boundary, so its premise cannot contradict represented constants.

## Effects, calls, memory, and ordering

Every explicit effect bit propagates non-purity through ancestors. A resolved
call can be pure only with explicit empty transitive effects. External and
unresolved calls remain identity-only. Only an immutable load can be pure;
shared, mutable, volatile, and atomic loads remain identity-only regardless of
an empty effect bitmap. Ordered operands are retained even when a language
profile declares their evaluation order unspecified.

## Resource exhaustion

Callers must set `ExpressionIrLimits` appropriate to the service. Native-stack
safety does not authorize unbounded heap growth. Run untrusted workloads under
process-level resident-memory, no-swap, task, and time caps in addition to the
library's node, operand, and work bounds.

## Persistence and replay

Serialization omits live provenance. A decoded artifact returns
`PersistedIdentityOnly`, and certification is denied before examining the node
or law. To regain authority, reconstruct facts from the current frontend and
call `lower_expression_ir` against the current CPG and profile.

Do not add a deserialization constructor for `RewriteCertificate`, a public
provenance setter, or a bypass around `certify`. Those changes would cross the
formal trust boundary and require a new model and security decision first.

## Operational checklist

1. Match CPG, fact-table, and profile languages.
2. Populate all ten profile dimensions explicitly.
3. Carry exact types and all eight effect dimensions per fact.
4. Inspect `ExpressionIrCompletion` before using the root.
5. Request the exact named law and retain its certificate with the action.
6. Treat persisted artifacts as diagnostic identity only.
7. Revalidate at any frontend, CPG, profile, or rule-set boundary.
8. Keep Replete rule admission and extraction policy outside libcpg.

See [ADR-0074](../design/0074-typed-pure-expression-ir.md), the
[component guide](../components/graph/expression-ir.md), and the
[formal verification plan](../verification/t4-1-expression-ir-formal-plan.md).
