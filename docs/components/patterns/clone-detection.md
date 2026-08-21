# Function clone detection

`libcpg::pattern::detect_clones` is an always-on, read-only detector for
Type-1 through Type-4 function clone classes. It composes the CPG's ordered AST,
PDG projection, WL refinement, and existing VF2 matcher; it does not parse
source itself.

![Function roots pass through exact and abstract AST fingerprints, multiset similarity, and bounded PDG verification.](../../diagrams/clone-detection-pipeline.svg)

*Figure — end-to-end clone detection. Source:
[`diagrams/clone-detection-pipeline.puml`](../../diagrams/clone-detection-pipeline.puml).*

## API

```rust
use libcpg::pattern::{detect_clones, CloneClass, CloneOptions};
use libcpg::{AnalysisError, CodePropertyGraph};

fn clones(cpg: &CodePropertyGraph) -> Result<Vec<CloneClass>, AnalysisError> {
    detect_clones(cpg, &CloneOptions::default())
}
```

`CloneClass` exposes `clone_type`, sorted function-root `members`, and a finite
`similarity`. `CloneType` is ordered `Type1`, `Type2`, `Type3`, `Type4` for
stable result ordering; that enum order is not a statement that Type-4 is more
desirable than Type-1.

## Options and bounds

| Field | Default | Contract |
|---|---:|---|
| `type3_threshold` | `0.8` | finite value in $`[0,1]`$ |
| `type4_wl_threshold` | `0.8` | finite value in $`[0,1]`$ |
| `wl_iterations` | `3` | at most `MAX_WL_ITERATIONS` (`16`) |
| `max_pairs` | `100_000` | size-qualified candidate pairs; at most `MAX_CLONE_PAIRS` (`100_000`) |
| `max_pdg_nodes` | `300` | at most `MAX_CLONE_PDG_NODES` (`300`) |
| `max_vf2_invocations` | `10_000` | at most `MAX_CLONE_VF2_INVOCATIONS` (`100_000`) |

Values outside static contracts return `AnalysisError::InvalidInput`. A live
search exceeding a configured cap returns `AnalysisError::BudgetExceeded`; no
partial clone vector is returned.

## Example classification

Given two function subtrees with the same declaration name:

```text
f: Identifier("x"), Literal(1)
g: Identifier("y"), Literal(2)
```

their Type-1 fingerprints differ but their ordered tag streams are both
`Function, Identifier, Literal`, so they form a Type-2 class. Inserting one
statement changes the ordered fingerprint and may form Type-3 when multiset
Jaccard reaches the threshold. Reordering unlike but dependence-independent
statements preserves an equal feature bag and can form Type-4 after PDG WL/VF2
verification.

## Evidence and interpretation

- Type-1 ignores comments and source locations.
- Type-2 abstracts names, literal values, and other payloads to `NodeKindTag`.
- Type-3 is a bag relation; it does not establish order or control equivalence.
- Type-4 needs PDG edges. Build the PDG before treating absence as meaningful.
- Nested functions are excluded from an enclosing function and analyzed as
  independent roots.
- Classes are connected components of pair relations. `similarity` is the
  minimum joining-edge score, not a new all-pairs recomputation.

With `serde`, all public option, taxonomy, and result types serialize and
deserialize. See [Theory 23](../../theory/23-function-clone-detection.md) for
formulae and citations, [ADR-0030](../../design/0030-bounded-function-clone-detection.md)
for tradeoffs, and [Scientific validation 25](../../scientific/25-function-clone-validation.md)
for the evidence matrix.

## Failure handling

```rust
use libcpg::pattern::{detect_clones, CloneOptions};
use libcpg::AnalysisError;

match detect_clones(&cpg, &CloneOptions::default()) {
    Ok(classes) => println!("{} complete clone classes", classes.len()),
    Err(AnalysisError::BudgetExceeded { unit, needed, cap, .. }) => {
        eprintln!("clone analysis needs {needed} {unit}; configured cap is {cap}");
    }
    Err(error) => return Err(error),
}
# Ok::<(), libcpg::AnalysisError>(())
```

## References

The implementation follows the taxonomy/evaluation tradition of Bellon et al.
([DOI 10.1109/TSE.2007.70725](https://doi.org/10.1109/TSE.2007.70725)), AST
clone detection of Baxter et al.
([DOI 10.1109/ICSM.1998.738528](https://doi.org/10.1109/ICSM.1998.738528)), and
dependence-based detection of Zou et al.
([DOI 10.1145/3324884.3416541](https://doi.org/10.1145/3324884.3416541)) and
Komondoor–Horwitz
([DOI 10.1007/3-540-47764-0_3](https://doi.org/10.1007/3-540-47764-0_3)).
