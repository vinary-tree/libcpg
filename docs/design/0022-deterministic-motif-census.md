# ADR 0022 — Use a complete bounded directed-motif census

- **Status:** Accepted
- **Date:** 2026-08-12
- **Decision owners:** `libcpg` maintainers

## Context

The upstream pgmcp motif implementation exposed a useful shape but had two
scientific-completeness problems. Its classifier collapsed `111D` with `111U`
and collapsed the three `120D`/`120U`/`120C` orientations. Its four-node pass
also disappeared silently above a size threshold, leaving callers unable to
distinguish a complete zero count from an omitted computation.

`libcpg` needs feature-free motif evidence with stable output, explicit
resource bounds, and testable completeness. Projection vertex identifiers can
be sparse, edges can be inserted in any order, and malformed public CSR must
be rejected before indexing.

## Decision

1. Validate the public forward/reverse CSR before doing any work.
2. Convert non-loop directed topology into dense row bitsets. Ignore weights
   and parallel multiplicity because motifs classify adjacency.
3. Enumerate every unordered triple and classify its six-bit tricode through a
   complete 64-entry Davis–Leinhardt lookup table.
4. Enumerate every unordered quadruple for the two selected graphlet
   predicates: complete bidirected clique and source-compatible out-star.
5. Apply hard limits of 512 vertices for triads and 50 for graphlets, honoring
   smaller caller limits. A call must fit both because it requests both result
   families.
6. Return a typed budget error rather than a partial result when either limit
   is exceeded.
7. Use fixed arrays and a public name array so class ordering is deterministic
   and allocation-free at the result boundary.

## Alternatives considered

### Preserve the collapsed classifier

Rejected. The result would not be a complete sixteen-class census, and
directionally distinct program structures would remain indistinguishable.

### Use adjacency hash sets

Rejected for this bounded implementation. Hash membership has larger constant
factors and introduces irrelevant hash-state/order concerns. Dense row bitsets
are small at the admitted sizes and make each directed edge query constant
time.

### Port the sparse subquadratic algorithm directly

Deferred as a different API for substantially larger sparse graphs. Its
boundary accounting is more complex, while exhaustive enumeration provides a
simple conservation law and independent oracle at the current hard limit.

### Skip graphlets when the input is too large

Rejected. Silent omission makes a zero count ambiguous and violates the
repository rule that bounded analyses report their first unmet budget.

### Return all directed four-node isomorphism classes

Rejected for this contract. The epic requests two source-compatible graphlets;
a complete four-node directed taxonomy would require a separate named result
and validation surface.

## Consequences

The result is deterministic, complete within its admitted domain, and easy to
audit. Triads obey `sum(counts) = C(n,3)`. Costs are deliberately explicit:
`O(n³ + n⁴)` time and `O(n ceil(n/64))` words of adjacency storage. The
50-vertex graphlet ceiling is therefore the binding default limit for the
combined call.

The out-star definition permits reverse spoke arcs for compatibility with the
source algorithm. This nonstandard detail is documented in the public API and
tested explicitly.

## Verification obligations

- one hand fixture for each of the sixteen classes;
- all 64 labeled tricodes checked against vertex-permutation isomorphism;
- generated graphs through eight vertices checked against an independent
  brute-force census;
- the `C(n,3)` conservation law and input-order determinism;
- exact graphlet, self-loop, weight, malformed-CSR, and both-budget examples;
- feature-free and parser-backed public-API integration tests; and
- benchmark oracles before every timed family.

See the [component guide](../components/graph/motif-analysis.md) and
[scientific validation](../scientific/17-motif-census-validation.md).
