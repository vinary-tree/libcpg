# Security boundary for clone and feasibility advisories

An inconsistent security check copied across handlers can leave one path less
restrictive. A contradictory authorization guard can make protection
unreachable, while a redundant condition can conceal dead recovery logic.
libcpg reports represented evidence for review; it does not prove a
vulnerability, exploitability, or safe remediation.

![Untrusted graph evidence crosses bounded analysis into qualified reports](../diagrams/clone-feasibility-advisory-pipeline.svg)

## 1. Assets and actors

| Actor or asset | Security role |
|---|---|
| analyzed repository/CPG | untrusted, potentially huge, cyclic, malformed, incomplete, or adversarial evidence |
| frontend and graph builders | trusted syntax/CFG/DFG normalization boundary |
| clone/GED/SCCP/interval analyses | supporting evidence providers with their own completeness contracts |
| policy author | selects clone thresholds and resource caps |
| advisory detector | bounded read-only evidence composer |
| reviewer | decides intent, runtime feasibility, correctness, and impact |
| downstream automation | must preserve issues and proof provenance |

## 2. Threats and controls

| Threat | Control | Residual risk |
|---|---|---|
| comments/strings imitate syntax | typed nodes and edges only; no detector source parser | faulty frontend evidence can still be wrong |
| transitive clone class fabricates a pair | consume exact pair relation | upstream pair classification can be imprecise |
| every small clone edit is labeled a bug | require typed rename or guard-payload divergence | deliberate divergence still appears advisory |
| rename threshold rounding flips a result | exact integer cross multiplication | policy can choose a poor threshold |
| missing/cyclic AST becomes a clean negative | typed issue excludes affected pair | completely absent evidence is not observable |
| capped GED is treated as exact | `GedCapped` disqualifies the pair | a larger cap can consume substantial CPU/heap |
| arbitrary descendant is mistaken for guard | exact recognized guard-root contract | frontend wrapper shape can omit a guard |
| dead CFG island supplies a false proof | exclude SCCP-unreachable condition sources | upstream CFG reachability can over-approximate |
| compound false-edge refinement is unsound | exact short-circuit path partitions | unsupported language semantics remain issues |
| disequality convexification admits excluded value | retain two disjoint regions | path-state growth can hit a visible cap |
| integer extrema overflow | checked predecessor/successor; empty region | non-integer arithmetic is unsupported |
| literal false is labeled contradiction | contradiction requires predicate-backed empty true alternatives | other semantic contradictions can remain unrecognized |
| deep syntax overflows native stack | flat arenas/tapes and explicit continuations | allowed heap/work can still be large |
| cap exhaustion looks clean | typed issues make `is_complete()` false | consumers can discard report qualification |
| findings mutate shared graph state | read-only APIs plus integration invariance tests | cached report becomes stale after caller mutation |
| serialized evidence loses provenance | typed serde report retains proof and witnesses | transport integrity is caller-owned |

## 3. Safe consumption

1. Use `analyze_inconsistent_clones` or `analyze_infeasible_paths` when an
   empty result influences a decision. Require `is_complete()` and retain every
   issue. Convenience vectors are for positive review only.
2. Display the exact function pair, witness nodes, clone type, edit fraction,
   GED evidence, and rename support before judging I8.
3. Display the condition/source, branch target/outcome, SCCP value, entry
   intervals, proof source, and ordered contradiction predicates before judging
   I9.
4. Inspect the original source and runtime contract. A divergent clone may
   intentionally encode a different policy; a redundant branch may be
   defensive documentation or generated code.
5. Confirm frontend, CFG, DFG, call, and type coverage. External effects,
   aliases, integer conversions, overflow modes, exceptions, concurrency, and
   language-specific truth semantics can invalidate an operational conclusion.
6. Never delete a branch or unify clone bodies automatically from advisory
   evidence alone. Require tests/specifications or a separate verified
   transformation proof.
7. Recompute after any graph, frontend, clone policy, GED policy, or cap change.

## 4. Resource and stack controls

I8 inherits the clone engine's pair/PDG/VF2 bounds and adds AST-node,
alignment-cell, and GED-node boundaries. I9 bounds unique condition nodes,
alternative states, and machine transitions. Invalid public values are rejected
before analysis; reached evidence bounds are reported at their exact scope.

No input-sized operation uses native recursion. Typed AST tapes, edit matrices,
condition arenas, alternative queues, explicit frames, active-node sets, and
witness vectors live on the heap. Tests cross 100,000 represented nodes on a
64 KiB native stack. Hostile repositories should additionally run under
caller-owned wall-time and memory isolation.

![Explicit condition frames prevent source depth from becoming native call depth](../diagrams/boolean-condition-pushdown-machine.svg)

## 5. Non-goals

The analyzers do not prove clone equivalence, developer intent, defect
causality, runtime path feasibility, branch safety, absence of side effects,
complete integer semantics, exploitability, or absence of clone/path defects.

## References

1. Juergens, E., Deissenboeck, F., Hummel, B., Wagner, S. (2009). *Do Code
   Clones Matter?* DOI:
   [10.1109/ICSE.2009.5070547](https://doi.org/10.1109/ICSE.2009.5070547).
2. Wegman, M. N., Zadeck, F. K. (1991). *Constant Propagation with Conditional
   Branches.* DOI:
   [10.1145/103135.103136](https://doi.org/10.1145/103135.103136).
3. Cousot, P., Cousot, R. (1977). *Abstract Interpretation.* DOI:
   [10.1145/512950.512973](https://doi.org/10.1145/512950.512973).
