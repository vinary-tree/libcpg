# Usage 19 — Review clone and path-feasibility advisories

I8 and I9 are feature-free, read-only analyses over an existing CPG. They do
not parse source text. Build the graph through any supported frontend, retain
the qualified report, display exact evidence, and treat findings as review
candidates.

The complete feature-free fixture is
[`examples/clone_feasibility_advisories.rs`](../../examples/clone_feasibility_advisories.rs):

```sh
cargo run --example clone_feasibility_advisories --no-default-features
```

![The public APIs preserve evidence and qualification for both advisory paths](../diagrams/clone-feasibility-advisory-pipeline.svg)

## 1. Review inconsistent clones

The default policy admits Type-2/3 pairs with at most one structural edit per
five tape nodes and requires a dominant rename mapping at four-fifths support
over at least three aligned occurrences:

```rust
use libcpg::{
    analyze_inconsistent_clones, CloneBugConfig, CodePropertyGraph,
    InconsistentCloneKind,
};

fn review_clone_divergences(
    cpg: &CodePropertyGraph,
) -> Result<(), libcpg::AnalysisError> {
    let report = analyze_inconsistent_clones(cpg, &CloneBugConfig::default())?;
    for finding in &report.findings {
        let diagnosis = match &finding.kind {
            InconsistentCloneKind::IdentifierRename {
                left_name,
                right_name,
                expected,
                support,
                ..
            } => format!(
                "rename {left_name}->{right_name}; expected {expected}; support={}/{}",
                support.supporting, support.population,
            ),
            InconsistentCloneKind::GuardDivergence { left, right } => {
                format!("guard payload {left:?} differs from {right:?}")
            }
        };
        println!(
            "{:?} vs {:?}: {diagnosis}; edits={}/{}; GED={}",
            finding.left_function,
            finding.right_function,
            finding.structural_edits,
            finding.structural_size,
            finding.ged.upper,
        );
    }
    if !report.is_complete() {
        eprintln!("partial clone evidence: {:?}", report.issues);
    }
    Ok(())
}
```

`analyzed_pairs` records complete pair evidence even when no typed divergence
is found. It is useful for auditing threshold choices and distinguishing “no
suspicious payload” from “pair could not be evaluated.”

## 2. Tune I8 evidence bounds

```rust
use libcpg::{CloneBugConfig, PopulationThreshold};

let config = CloneBugConfig {
    maximum_structural_edit_fraction: PopulationThreshold::new(1, 10),
    minimum_rename_consistency: PopulationThreshold::new(9, 10),
    minimum_rename_occurrences: 10,
    max_ast_nodes: 100_000,
    max_alignment_cells: 2_000_000,
    ..CloneBugConfig::default()
};
```

The edit and rename thresholds are exact fractions. Clone similarity and GED
remain evidence but do not replace the typed diagnosis. Raising a cap permits
more work; it does not repair absent or malformed AST/clone evidence.

## 3. Review infeasible paths

```rust
use libcpg::{
    analyze_infeasible_paths, CodePropertyGraph, InfeasiblePathConfig,
    InfeasiblePathKind,
};

fn review_path_feasibility(
    cpg: &CodePropertyGraph,
) -> Result<(), libcpg::AnalysisError> {
    let report = analyze_infeasible_paths(cpg, &InfeasiblePathConfig::default())?;
    for finding in &report.findings {
        let diagnosis = match &finding.kind {
            InfeasiblePathKind::InfeasibleBranch { branch, target } => {
                format!("{branch:?} edge to {target:?} is infeasible")
            }
            InfeasiblePathKind::RedundantCondition { value } => {
                format!("condition is always {value}")
            }
            InfeasiblePathKind::ContradictoryCondition => {
                "no integer state satisfies the condition".to_owned()
            }
        };
        println!(
            "guard {:?}: {diagnosis}; proof={:?}; entry={:?}",
            finding.condition, finding.proof, finding.entry_intervals,
        );
        for empty in &finding.interval_contradictions {
            println!("  contradictory predicates: {:?}", empty.predicates);
        }
    }
    if !report.is_complete() {
        eprintln!("partial feasibility evidence: {:?}", report.issues);
    }
    Ok(())
}
```

The analyzer scans represented functions and conditional CFG sources. Ensure
the frontend has built the AST, CFG, and DFG required by SCCP and interval
analysis. No PDG build is required for I9.

## 4. Interpret the three I9 categories

| Category | Meaning within represented evidence | Required review |
|---|---|---|
| `InfeasibleBranch` | one represented Boolean CFG outcome has no proven executable state | confirm CFG/frontend/runtime semantics |
| `RedundantCondition` | exactly one complete-guard outcome remains possible | determine whether redundancy is intentional |
| `ContradictoryCondition` | every true interval alternative is empty and predicate witnesses exist | inspect correlation, types, overflow, aliases, and intent |

For `x > 5 && x < 3`, expect all three: the true edge is infeasible, the guard
is always false, and the two range predicates contradict. For `if true`, expect
the first two only; a Boolean literal is not an interval contradiction.

## 5. Tune I9 resource policy

```rust
use libcpg::InfeasiblePathConfig;

let config = InfeasiblePathConfig {
    max_condition_nodes: 100_000,
    max_condition_states: 8_192,
    max_condition_steps: 4_000_000,
};
```

Alternative states can grow with nested Boolean disjunctions and disequalities.
The state and step caps are semantic completeness boundaries, not performance
hints. A reached cap becomes a typed issue and the affected condition produces
no negative claim.

![The condition evaluator exposes every expansion, meet, live alternative, contradiction, and issue](../diagrams/boolean-condition-pushdown-machine.svg)

## 6. Choose the report API

| Need | API |
|---|---|
| review positive clone divergences | `inconsistent_clone_bugs` |
| assert no represented clone divergence | `analyze_inconsistent_clones` plus `is_complete()` |
| review positive path findings | `infeasible_paths` |
| assert no represented infeasible/redundant/contradictory condition | `analyze_infeasible_paths` plus `is_complete()` |

An empty convenience vector is not a clean verdict because it discards issues
and analyzed-pair/condition context.

## 7. Parser-backed Rust example

Enable `lang-rust` only at graph construction:

```rust
use libcpg::{
    analyze_infeasible_paths, CpgBuilder, InfeasiblePathConfig, Language,
    TreeSitterCpgBuilder,
};

let source = "fn impossible(x: i32) { if x > 5 && x < 3 {} }";
let cpg = TreeSitterCpgBuilder::new().build(source, Language::Rust)?;
let report = analyze_infeasible_paths(&cpg, &InfeasiblePathConfig::default())?;
assert!(report.is_complete());
# Ok::<(), libcpg::AnalysisError>(())
```

The analyzer remains feature-free; the feature supplies only the Rust parser.

## 8. Serialize evidence

With `serde`, both configs and qualified reports support deterministic
serialization. Preserve the report rather than only the finding vector so
proof provenance, pair/condition inventory, and issues survive transport.

See [Theory 36](../theory/36-clone-consistency-and-abstract-feasibility.md),
[ADR-0046](../design/0046-evidence-bounded-clone-and-feasibility-advisories.md),
the [component guide](../components/graph/clone-feasibility-bug-detectors.md),
the [validation ledger](../scientific/41-clone-feasibility-validation.md), and
the [security boundary](../security/11-clone-feasibility-detectors.md).

## References

1. Juergens, E., Deissenboeck, F., Hummel, B., Wagner, S. (2009). *Do Code
   Clones Matter?* DOI:
   [10.1109/ICSE.2009.5070547](https://doi.org/10.1109/ICSE.2009.5070547).
2. Wegman, M. N., Zadeck, F. K. (1991). *Constant Propagation with Conditional
   Branches.* DOI:
   [10.1145/103135.103136](https://doi.org/10.1145/103135.103136).
3. Cousot, P., Cousot, R. (1977). *Abstract Interpretation.* DOI:
   [10.1145/512950.512973](https://doi.org/10.1145/512950.512973).
