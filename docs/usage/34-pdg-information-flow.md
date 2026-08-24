# Check represented information flow and timing sites

Use the feature-free IFC APIs after the CPG contains the dependence edges your
policy needs. Initial source construction creates CFG and DFG evidence;
`PdgBuilder` adds intraprocedural data/control dependence on demand. SDG summary
construction is optional when caller-local interprocedural return influence is
required.

## 1. Build the PDG

```rust
use libcpg::{CodePropertyGraph, NodeId, PdgBuilder};

fn add_function_pdg(cpg: &mut CodePropertyGraph, function: NodeId) {
    PdgBuilder::new().build(cpg, function);
}
```

To cover every parsed function deterministically, call
`PdgBuilder::new().build_all(&mut cpg)`. Rebuilding is idempotent.

## 2. Declare the confidentiality policy

```rust
use libcpg::{CodePropertyGraph, FlowViolation, NodeId, SecurityLabels, noninterference};

fn confidentiality_findings(
    cpg: &CodePropertyGraph,
    secret_definition: NodeId,
    public_observation: NodeId,
    approved_release: NodeId,
) -> Vec<FlowViolation> {
    let labels = SecurityLabels::new([secret_definition], [public_observation])
        .with_declassifier(approved_release);
    noninterference(cpg, &labels)
}
```

High sources and Low sinks are policy roles, not inferred syntax categories.
They may name definitions, expressions, calls, or any other represented node.
A declassifier is a hard cut; do not label a node as a declassifier merely to
silence a finding.

## 3. Interpret flow findings

| `FlowViolationKind` | Meaning |
|---|---|
| `Explicit` | At least one shortest represented path uses data/summary dependence only. |
| `Implicit` | At least one shortest represented path contains control dependence. |
| `ExplicitAndImplicit` | Both path classes connect this source/sink pair; both witnesses are retained. |

Replay `InformationFlowWitness::steps` in order. Every step names its source,
target, and `DataDependence`, `Summary`, or `ControlDependence` class. An empty
explicit witness means the same node was intentionally classified both High
and Low.

## 4. Check timing sites separately

```rust
use std::collections::BTreeSet;

use libcpg::{CodePropertyGraph, LeakSite, NodeId, constant_time};

fn timing_findings(cpg: &CodePropertyGraph, secrets: [NodeId; 2]) -> Vec<LeakSite> {
    constant_time(cpg, &BTreeSet::from(secrets))
}
```

`ConditionalTrue` and `ConditionalFalse` findings retain the exact CFG target.
`IndexAccess` identifies the reached index expression. A branch controller or
index node supplied directly as a secret has a valid empty witness to itself.

Do not reuse confidentiality declassification here. Constant-time policy asks
whether control or addressing depends on the secret, even if some value is
authorized for later release.

## 5. Preserve the interpretation boundary

```text
zero findings
    means: no matching path/site in the supplied represented relation
    does not mean: the program or binary is proven secure

positive finding
    means: a canonical represented influence witness exists
    does not mean: the runtime path is feasible or exploitable
```

The functions ignore labels outside the graph, borrow the CPG immutably, sort
all output canonically, and do not truncate findings. Bound input graph size
and downstream response handling at the caller boundary.

## 6. Focused validation

Run the property/oracle and public parser gates under the evidence-backed RSS
configuration:

```bash
systemd-run --user --scope \
  -p MemoryMax=4G -p MemorySwapMax=0 -p TasksMax=256 \
  env CARGO_BUILD_JOBS=1 \
  cargo test -j 1 --all-features analysis::security::ifc::tests

systemd-run --user --scope \
  -p MemoryMax=4G -p MemorySwapMax=0 -p TasksMax=256 \
  env CARGO_BUILD_JOBS=1 \
  cargo test -j 1 --all-features --test ifc_integration
```

See the [component guide](../components/graph/information-flow-control.md),
[security boundary](../security/24-pdg-information-flow-boundary.md), and
[Scientific validation 61](../scientific/61-pdg-information-flow-validation.md).

## References

1. Hammer, C., Snelting, G. (2009). *Flow-sensitive, context-sensitive, and
   object-sensitive information flow control based on program dependence
   graphs.* DOI:
   [10.1007/s10207-009-0086-1](https://doi.org/10.1007/s10207-009-0086-1).
2. Barthe, G., Betarte, G., Campo, J. D., Luna, C., Pichardie, D. (2014).
   *System-level non-interference for constant-time cryptography.* DOI:
   [10.1145/2660267.2660283](https://doi.org/10.1145/2660267.2660283).
