# Use the typed expression IR

This workflow assumes a frontend has already resolved types, calls, bindings,
effects, and language rules. The IR layer does not parse or infer them.

## 1. Build or receive a CPG

Use the normal parser builder or caller-supplied tree. Find the expression root
and retain its ordered CPG operand identities. The nine parser-backed
integration fixtures demonstrate Rust, C, C++, Go, Java, Python, JavaScript,
TypeScript, and Ruby.

## 2. Emit exact facts

For an integer expression `x + 0`, a frontend emits:

```rust,ignore
use libcpg::{
    BinaryOperator, ExpressionEffects, ExpressionFact, ExpressionFactKind,
    ExpressionFactTable, ExpressionLiteral, ExpressionType, IntegerOverflow,
    IntegerType, Language, ShiftBehavior,
};

let i32_type = ExpressionType::Integer(IntegerType::fixed(
    32,
    true,
    IntegerOverflow::Checked,
    ShiftBehavior::Checked,
)?);
let facts = ExpressionFactTable::try_new(
    Language::Rust,
    [
        (
            root,
            ExpressionFact::known(
                ExpressionFactKind::Binary(BinaryOperator::Add),
                vec![x, zero],
                i32_type,
                ExpressionEffects::PURE,
            ),
        ),
        (
            x,
            ExpressionFact::known(
                ExpressionFactKind::Binding(x_identity),
                Vec::new(),
                i32_type,
                ExpressionEffects::PURE,
            ),
        ),
        (
            zero,
            ExpressionFact::known(
                ExpressionFactKind::Literal(ExpressionLiteral::Integer(0)),
                Vec::new(),
                i32_type,
                ExpressionEffects::PURE,
            ),
        ),
    ],
)?;
```

The binding identity contains both its resolved definition node and the
frontend-assigned lexical scope. Identifier spelling is not identity.

## 3. Lower under explicit limits

```rust,ignore
use libcpg::{lower_expression_ir, ExpressionIrCompletion, ExpressionIrLimits,
             SemanticProfile};

let profile = SemanticProfile::for_language(Language::Rust);
let ir = lower_expression_ir(
    &cpg,
    root,
    &facts,
    &profile,
    ExpressionIrLimits {
        max_nodes: 10_000,
        max_operands: 40_000,
        max_work: 50_000,
    },
);
if ir.completion() != &ExpressionIrCompletion::Complete {
    // Preserve and report the typed reason; do not claim no optimization.
    return Ok(());
}
```

Choose caps from the containing service's policy. A work frame is one processed
`Enter` or `Exit` continuation; shared-source reuse still charges its `Enter`
frame.

## 4. Request one law

```rust,ignore
use libcpg::RewriteLaw;

let root_ir = ir.root().expect("completion was checked");
let certificate = ir.certify(root_ir, RewriteLaw::AdditiveIdentity)?;
assert_eq!(certificate.node(), root_ir);
```

Keep the certificate adjacent to the rewrite action. Do not convert it to a
Boolean and discard the named law/language context.

## 5. Handle calls and memory conservatively

- Use `CallIdentity::Resolved` only after exact call resolution and include all
  transitive call effects.
- Use `MemoryAccess::Immutable` only for frontend-proved immutable storage.
- Shared, mutable, volatile, atomic, unresolved, and external operations remain
  identity-only.
- Mark exceptions, traps, panic, allocation failure, suspension, ambient
  state, and nondeterminism in `ExpressionEffects`.

An effect on any descendant prevents a parent certificate.

## 6. Persist for diagnostics, not authority

With `serde`, structural IR bytes can be stored for audit and replay. A decoded
artifact has `PersistedIdentityOnly` provenance and cannot certify a law. Re-run
the frontend and lowering against the current CPG to regain live authority.

## 7. Integrate with standalone Replete later

The future adapter may translate a complete typed IR term into Replete and
admit only certified equations. Replete remains its own crate and repository.
Do not add a Replete dependency to this IR module, and do not make Replete
depend on libcpg.

## Related documentation

- [Component contract](../components/graph/expression-ir.md)
- [API reference](../api/expression-ir-reference.md)
- [Theory 57](../theory/57-datalog-egraphs-and-typed-expression-semantics.md)
- [ADR-0074](../design/0074-typed-pure-expression-ir.md)
- [Security boundary](../security/32-expression-rewrite-boundary.md)
- [Scientific validation](../scientific/69-expression-ir-validation.md)
