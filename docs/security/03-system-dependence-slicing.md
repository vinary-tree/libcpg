# Security boundary for interprocedural slicing

System Dependence Graph (SDG) slices are useful for defensive review: they can
explain which represented statements may influence a security-sensitive call,
or which statements may be influenced by a selected source. They are evidence,
not vulnerability, exploitability, authorization, or non-interference verdicts.

![SDG construction and evidence boundaries](../diagrams/sdg-summary-overlay.svg)

## 1. Assets and trust boundaries

| Actor or asset | Role |
|---|---|
| Maintainer | selects criterion, caps, and review policy |
| CPG frontend | supplies AST, call, DFG, and type evidence |
| Call resolver | emits only supported call targets |
| Effect analysis | supplies parameter-to-return may-dependence masks |
| Summary builder | appends labeled caller-local dependence edges |
| Two-pass slicer | returns a bounded ordered evidence set |
| Analyzed repository | potentially malformed or adversarial input |

The graph and effect summaries are inputs. The slicer does not reparse source
or infer unrepresented language behavior.

## 2. Threats and controls

| Threat | Control | Residual risk |
|---|---|---|
| Wrong-call-site path inflates a slice | HRB two-pass edge masks | imprecise call targets can still merge contexts |
| Missing callee hides influence | only resolved callees create summary edges | unresolved runtime dispatch can cause false negatives |
| Capped effect summary drops dependencies | capped summaries connect every argument | wider edge set can increase false positives |
| Signature exceeds 64-bit mask | truncated positions beyond bit 63 connect conservatively | upstream summary remains incomplete |
| Adversarial graph exhausts resources | explicit node and call-depth caps | bounded slice can omit relevant nodes |
| Edge insertion changes a report | sorted neighbors and ordered result | changed node identity legitimately changes order |
| Summary build destroys PDG evidence | labeled identity and append-only connection | stale summaries remain if upstream evidence changes |
| Slice is mistaken for feasible execution | documentation states graph-reachability boundary | downstream tools can still overstate findings |

## 3. Safe handling

- Preserve `SliceCaps` next to every stored or displayed result.
- If the result size equals `max_nodes`, treat it as potentially partial.
- Preserve cap/issue evidence from the `EffectAnalysis` used to build summary
  edges.
- Rebuild from a fresh CPG when call targets or effect summaries change;
  append-only construction does not delete stale summary edges.
- Review control feasibility, aliasing, exceptions, reflection, native code,
  and dispatch precision before escalating a candidate.
- Do not use an empty slice as proof that a data flow, vulnerability, or
  side-channel is absent.

## 4. Stale-edge consideration

Idempotency assumes monotone construction on one evidence snapshot. If a
consumer mutates call topology or replaces an effect summary with a narrower
one, existing summary edges are intentionally not removed. This preserves the
global append-only invariant but means the graph is a conservative union of
materialized evidence. Build a new graph for a clean recomputation.

## 5. Defensive uses

Appropriate uses include narrowing code review around a taint sink, explaining
a maybe-null dereference, selecting statements for impact analysis, and
providing context to a later authorization or information-flow checker. The
slice contains source locations and program structure; protect it with the same
access and retention controls as source code and security findings.

## References

1. Horwitz, S., Reps, T., Binkley, D. (1990). *Interprocedural Slicing Using
   Dependence Graphs.* DOI:
   [10.1145/77606.77608](https://doi.org/10.1145/77606.77608).
2. Ferrante, J., Ottenstein, K. J., Warren, J. D. (1987). *The Program
   Dependence Graph and Its Use in Optimization.* DOI:
   [10.1145/24039.24041](https://doi.org/10.1145/24039.24041).
