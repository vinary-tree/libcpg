# Typed pure-expression IR

The expression intermediate representation (IR) is a flat, immutable adapter
between Code Property Graph (CPG) expressions and consumers that need exact
rewrite preconditions. It is neither a parser nor an optimizer.

## End-to-end flow

![Typed facts and profiles reach certification only through complete, pure, live lowering evidence.](../../diagrams/expression-ir-trust-boundary.svg)

1. A language frontend emits `ExpressionFact` values for one expression root
   and every reachable ordered operand.
2. `SemanticProfile` declares all language behaviors used by rewriting.
3. `lower_expression_ir` cross-checks facts against the immutable CPG and emits
   one flat postorder arena.
4. `ExpressionIr::certify` checks a named law at a node.
5. A later standalone Replete adapter may consume the certificate; libcpg does
   not own or mutate a Replete e-graph.

## Data model

| Type | Meaning |
|---|---|
| `ExpressionFactTable` | canonical source-node-to-fact relation for one language |
| `ExpressionFactKind` | literal, binding, unary/binary operation, call, load, or opaque identity |
| `ExpressionType` | Boolean, exact integer, exact binary float, string, character, unit, or dynamic result |
| `ExpressionEffects` | eight explicit effect bits, including failure, memory, calls, volatile/atomic access, nondeterminism, and suspension |
| `SemanticProfile` | ten required language-semantic dimensions and enabled rewrite laws |
| `ExpressionIrNode` | flat typed node plus an operand-tape range and evidence mode |
| `ExpressionIrCompletion` | complete, exactly limited, or malformed result |
| `RewriteCertificate` | successful check for one node, law, and profile language |

`ExpressionIrId` is an arena index. If node $`p`$ has operand $`c`$, the
postorder invariant is:

```math
0 \leq c < p < |N|,
```

where $`N`$ is the emitted node vector. Shared CPG nodes map to one arena node,
so the representation is a directed acyclic graph rather than an expanded
tree.

## Semantic profiles

A profile is complete only when all ten dimensions are `Known`. Per-fact types
and effects remain mandatory even under a complete built-in profile. The
built-in set is deliberately closed:

| Languages | Evaluation order represented by the profile |
|---|---|
| Rust, Java, Python, JavaScript, TypeScript, Ruby | left to right |
| C, C++, Go | unspecified where the language permits it |
| every other `Language` | unknown; no rewrite certificate |

The profile never supplies a convenient default for an unsupported language.

## Evidence propagation

- `Pure` means the profile, type, effects, operation class, and all descendants
  are complete and effect-free.
- `IdentityOnly` means identity is retained, but calls, aliasable memory,
  volatile/atomic access, explicit effects, or opaque operations forbid a
  rewrite.
- `Unknown` means at least one profile, type, effect, or descendant premise is
  absent.

Resolved calls and immutable loads can be pure only when their complete
transitive effect evidence is explicitly empty. External/unresolved calls and
shared/mutable/volatile/atomic loads remain identity-only even if a malformed
frontend attempts to mark their effect bits empty.

## Lowering machine

The literate algorithm uses heap-resident frames:

```text
LOWER(root, facts, profile, limits)
    pending <- [Enter(root)]
    active <- empty ordered set
    source_to_ir <- empty ordered map
    nodes, operands <- empty flat tapes

    while pending is not empty
        charge one work unit or return WorkLimited
        frame <- pop(pending)
        if frame = Enter(source)
            reject an active source as a cycle
            skip a source already mapped
            validate CPG identity, topology, type, literal, call, and binding
            push Exit(source), then ordered Enter(operand) frames
        else
            require every operand to have an earlier IR identifier
            enforce node and operand caps exactly
            append operands and one node; propagate evidence mode

    return Complete with the mapped root
```

No logical expression depth becomes native call depth. Allocation uses fallible
reserve operations where the standard collections expose them, and all public
growth has explicit node, operand, and work caps.

## Rewrite-law boundary

The initial closed law vocabulary is constant folding, commutativity,
associativity, additive identity, multiplicative identity, and Boolean
identity. Each law has separate type/value checks. Examples include:

- checked and undefined integer overflow must remain representable;
- division and remainder by zero are denied;
- masked shift counts differ from checked/undefined shift counts;
- strict IEEE 754 floats do not receive algebraic identities;
- finite reassociation rejects contradictory exceptional literals; and
- Boolean identities require pure Boolean operands.

## Persistence

With feature `serde`, facts, profiles, IR structure, limits, completion, and
metrics have owned wire forms. `ExpressionIrProvenance` is intentionally
omitted. Deserialization always yields `PersistedIdentityOnly`, and
`certify` returns `RewriteDenial::PersistedArtifact` before any law check.

`RewriteCertificate` implements `Serialize` but not `Deserialize`. This
asymmetry makes a certificate auditable without making private construction
forgeable through bytes.

## Related documentation

- [ADR-0074](../../design/0074-typed-pure-expression-ir.md)
- [Theory 57](../../theory/57-datalog-egraphs-and-typed-expression-semantics.md)
- [API reference](../../api/expression-ir-reference.md)
- [Usage 42](../../usage/42-typed-expression-ir.md)
- [Security boundary](../../security/32-expression-rewrite-boundary.md)
- [Scientific validation](../../scientific/69-expression-ir-validation.md)
- [Formal plan](../../verification/t4-1-expression-ir-formal-plan.md)
