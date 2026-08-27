# Security boundary -- Approximate evidence

## Security claim

libcpg returns deterministic, bounded advisory candidates over immutable text
or token tapes. It does not certify symbol identity, call resolution, program
equivalence, clone class, protocol violation, vulnerability, exploitability,
authorization, or safety.

## Trust boundary

| Evidence | Required trust condition |
|---|---|
| stable candidate ID | caller-assigned corpus namespace; unique within one search corpus |
| vocabulary ID | reversible exact `libdictenstein` term relation, never hash identity |
| sequence hit | exact stored ID sequence and all associated candidate slots |
| rewrite path | complete typed relation with one explicit semiring |
| distance | named liblevenshtein method, bound, and exact rational cost |
| rank | deterministic cost and stable-ID total order within the admitted corpus |
| negative result | complete build and complete query under recorded limits |
| semantic claim | independent consumer-specific exact confirmation |

## False-positive and false-negative boundaries

- A low edit cost means only that two admitted tapes are near under the named
  operation set. It does not imply the same declaration, behavior, or security
  property.
- A complete no-match applies only to the recorded projection, vocabulary,
  method, and bound. It says nothing about omitted source facts.
- An incomplete empty report is unknown. Treating it as no-match creates a
  false negative.
- Unicode normalization is explicit policy. libcpg compares represented scalar
  or token equality and does not silently collapse homoglyphs or normalization
  forms.
- Approximate candidate generation can improve recall, but the top-k and
  distance bound can omit relevant candidates by design.

## Resource-exhaustion boundary

Input length, candidate count, pair work, total work, state count, memory, and
unique vocabulary size are independently bounded with checked arithmetic.
The reusable index also preflights corpus ordering, vocabulary text, encoded
sequences, directed acyclic word graph (DAWG) publication, candidate slots,
query-local unknown terms, traversal, and retained evidence.

Native-stack safety is not permission for unbounded heap use. Services must set
application-level limits and process-level resident-memory, swap, task, and
wall-time limits. Cancellation is cooperative; a currently executing bounded
iterator step finishes before cancellation is observed.

## Serialization and replay

With feature `serde`, reports and complete finite observations are canonical
owned values. Live dictionaries, caches, cancellation tokens, automata, and
graphs are not serialized. A receiver must validate the schema version,
configuration digest, query/corpus digests, method identity, completion,
limits, namespace, and exact-confirmation predicate. Deserialization restores
evidence, not authority.

## Cross-engine isolation

`duallity` finite-state rewrites cannot merge Replete e-classes.
`ApproximateTokenIndex` cannot mutate a CPG or Replete graph. Replete may later
confirm a pure-expression candidate through a separate exact adapter, but it
must not share mutable state with the approximate engine. Datalog, SCC,
dominance, data flow, taint, call resolution, and graph topology remain exact.

## Operational checklist

1. Validate unique stable IDs and the corpus namespace.
2. Choose the tape domain, edit method, distance, top-k, and every resource cap.
3. Require complete rewrite observation before using a composed normalization.
4. Branch explicitly on `ApproximateCompletion`.
5. Execute the recorded `ExactConfirmation` before a semantic claim.
6. Protect tapes, IDs, provenance digests, and candidate relations as
   source-confidential data.
7. Run untrusted workloads under resident-memory, no-swap, task, and time caps.

See [ADR-0073](../design/0073-bounded-composable-approximate-evidence.md), the
[component guide](../components/graph/approximate-matching.md),
[Usage 41](../usage/41-bounded-approximate-matching.md), and the
[validation dossier](../scientific/68-approximate-automata-validation.md).
