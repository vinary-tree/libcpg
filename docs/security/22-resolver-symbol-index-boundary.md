# Security 22 -- resolver symbol-index boundary

The call resolver is a graph-integrity boundary. A name index is permitted to
accelerate exact evidence, but it cannot infer identity, authorize fuzzy
correction, or conceal resource exhaustion.

## Assets and untrusted inputs

The protected assets are exact call topology, deterministic evidence records,
bounded process memory, bounded native stack, and isolation between graph
builds. Source spellings, import aliases, graph containment, duplicate
declarations, call counts, name length, and branching may be adversarial.

![Exact names reach the retained call-local index; semantic and resource failures terminate without graph mutation.](../diagrams/resolver-index-decision.svg)

*Figure -- integrity-preserving index admission. Source:
[`diagrams/resolver-index-decision.puml`](../diagrams/resolver-index-decision.puml).*

## Integrity controls

1. Lookup compares complete `str` values. No hash, prefix, normalization, case
   fold, or edit score establishes equality.
2. The nearest represented lexical scope and import binding remain part of the
   evidence; a globally unique spelling does not override stronger ambiguous
   local evidence.
3. Ambiguity uses the full pre-cap `SymbolCandidates::total`, never retained
   vector length.
4. Candidates and calls are canonically ordered by `NodeId`; insertion order
   and hash iteration cannot select a target.
5. Only a C1 confidence tier whose `creates_topology()` contract is true may
   mutate the call target and emit edges.
6. Prefix and fuzzy candidate sets are not resolver inputs. An exact miss
   remains `Unresolved` or `External` as documented.

## Availability and memory controls

`ResolveConfig` bounds scope/callee traversal, retained candidates, and
processed calls. Visited sets stop malformed graph cycles. Every graph walk is
an explicit heap worklist, so graph/name depth does not consume native call
stack.

The symbol index is call-local and dropped with the resolver invocation. There
is no process-global table, cross-tenant sharing, or invalidation race. Its
ordered-map space is linear in represented symbol/name storage.

The rejected fuzzy transducer is not linked. Its evaluated dense generated
target table retained $`O(q^2)`$ heap for query length $`q`$ and reached a 4
GiB enforced ceiling. Keeping that code outside libcpg is the availability
control; input truncation, silent fallback, or an unbounded service allocation
would not be an acceptable substitute.

## Unicode and byte boundary

CPG names are valid Rust `str` values. Equality is case-sensitive and compares
their exact Unicode scalar encoding. Canonically equivalent but differently
encoded strings remain different names unless a frontend explicitly emits the
same normalized property. libcpg does not normalize because doing so could
merge distinct language identifiers. Invalid UTF-8 cannot enter this API as a
name and is never repaired into a candidate.

## Fail-closed table

| Condition | Behavior |
|---|---|
| no exact represented name | `Unresolved` or `External`; no edge |
| two or more exact candidates | `BareNameAmbiguous`; no edge |
| candidate retention cap below full count | preserve full ambiguity count and truncation evidence |
| scope-depth or call cap | bounded documented result; no fabricated target |
| parent/AST cycle | visited-set termination |
| differently normalized or cased spelling | exact miss |
| prefix or near edit | no resolver lookup |
| repeated resolver invocation | idempotent unique edges and deterministic report |
| extremely long/deep represented exact name | iterative table lookup/construction/drop |
| proposed fuzzy dense-table exhaustion | dependency absent; no production exposure |

## Deployment guidance

- Pair `ResolveConfig` with Mode-A file-size and service request limits.
- Treat `candidates_truncated` or a call-processing cap as incomplete evidence,
  never as permission to select the retained first node.
- Keep graphs and resolution reports in the same request/tenant boundary.
- Do not pre-normalize identifiers independently of the language frontend.
- Profile or benchmark only inside an explicit RSS-limited, no-swap systemd
  scope when evaluating third-party dictionary structures.

## Non-claims

- Exact source spelling does not prove semantic dispatch in a dynamic language.
- `BareNameUnique` is a represented-graph fallback, not whole-program linkage.
- Deterministic order is not evidence strength.
- A fast fuzzy candidate is not an exact target.
- A 256 KiB native-stack pass does not imply bounded heap; both dimensions
  require independent evidence.

The [component contract](../components/graph/call-resolution.md),
[usage guide](../usage/32-exact-call-resolution.md),
[ADR-0064](../design/0064-retain-ordered-resolver-symbol-index.md), and
[scientific validation 59](../scientific/59-resolver-symbol-index-validation.md)
provide the semantic, operational, decision, and experimental details.
