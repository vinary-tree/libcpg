# Expression-IR API reference

The expression IR is feature-free. Enable Cargo feature `serde` only when
facts, profiles, structural artifacts, or audit certificates must be
serialized.

## Construct typed facts

```rust,ignore
let fact = libcpg::ExpressionFact::known(
    libcpg::ExpressionFactKind::Binary(libcpg::BinaryOperator::Add),
    vec![left_id, right_id],
    libcpg::ExpressionType::Integer(integer_type),
    libcpg::ExpressionEffects::PURE,
);
let facts = libcpg::ExpressionFactTable::try_new(
    libcpg::Language::Rust,
    [(root_id, fact), (left_id, left_fact), (right_id, right_fact)],
)?;
```

`try_new` rejects duplicate source identities and stores facts canonically.
Every reachable CPG operand needs a fact. Fact operands must equal the CPG's
ordered AST children exactly.

## Integer and floating-point types

```rust,ignore
let checked_i32 = libcpg::IntegerType::fixed(
    32,
    true,
    libcpg::IntegerOverflow::Checked,
    libcpg::ShiftBehavior::Checked,
)?;
assert!(checked_i32.is_valid());

let binary64 = libcpg::FloatType {
    width: libcpg::FloatWidth::F64,
    mode: libcpg::FloatMode::StrictIeee754,
};
```

`IntegerType::fixed` accepts widths 1 through 128 and rejects
`ArbitraryPrecision` overflow. `IntegerType::arbitrary_precision` creates the
widthless form. Public struct literals are revalidated during lowering.

Float literals carry exact `to_bits()` representations. `StrictIeee754`
preserves exceptional values and signed zero. `FiniteReassociation` requires
finite values and rejects negative zero.

## Semantic profiles

```rust,ignore
let profile = libcpg::SemanticProfile::for_language(libcpg::Language::Rust)
    .with_rewrite_law(libcpg::RewriteLaw::Associativity, false);
assert!(profile.is_complete());
assert!(!profile.allows(libcpg::RewriteLaw::Associativity));
```

`for_language` is complete only for the nine values in
`BUILTIN_EXPRESSION_PROFILE_LANGUAGES`. `unknown(language)` constructs the
explicit fail-closed form. Callers may disable individual laws, but cannot
bypass the ten semantic dimensions.

## Lowering

```rust,ignore
let ir = libcpg::lower_expression_ir(
    &cpg,
    root_id,
    &facts,
    &profile,
    libcpg::ExpressionIrLimits {
        max_nodes: 100_000,
        max_operands: 400_000,
        max_work: 500_000,
    },
);
```

Inspect `ir.completion()` before `ir.root()`. `root()` returns `None` for every
limited or malformed artifact. `metrics()` reports exact emitted nodes,
operands, and processed work frames. `node`, `operands`, and `source_id` are
bounds-checked immutable queries.

## Completion and malformed facts

| Outcome | Meaning |
|---|---|
| `Complete` | every reachable fact was validated and emitted |
| `WorkLimited` | the next work frame was not processed |
| `NodeLimited` | the next node was not emitted |
| `OperandLimited` | the next node's operand range was not emitted |
| `Malformed(error)` | facts, types, identities, topology, language, cycle, identifier domain, or allocation were invalid |

Every non-complete outcome is identity/investigation evidence only.

## Certification

```rust,ignore
let root = ir.root().expect("complete lowering");
let certificate = ir.certify(root, libcpg::RewriteLaw::AdditiveIdentity)?;
assert_eq!(certificate.node(), root);
assert_eq!(certificate.language(), libcpg::Language::Rust);
```

`certify` can return `PersistedArtifact`, `IncompleteLowering`, `UnknownNode`,
`NonPureEvidence`, `IncompleteProfile`, `LawDisabled`, or
`PreconditionFailed`. A certificate's fields are private and only accessible
through read-only methods.

## Persistence

```rust,ignore
let bytes = serde_json::to_vec(&ir)?;
let replayed: libcpg::ExpressionIr = serde_json::from_slice(&bytes)?;
assert_eq!(
    replayed.provenance(),
    libcpg::ExpressionIrProvenance::PersistedIdentityOnly,
);
assert_eq!(
    replayed.certify(libcpg::ExpressionIrId(0), libcpg::RewriteLaw::ConstantFold),
    Err(libcpg::RewriteDenial::PersistedArtifact),
);
```

Stable bytes deliberately omit provenance. `RewriteCertificate` is
serializable but not deserializable.

See [Usage 42](../usage/42-typed-expression-ir.md) for an end-to-end example
and [Security 32](../security/32-expression-rewrite-boundary.md) before using a
certificate across a trust boundary.
