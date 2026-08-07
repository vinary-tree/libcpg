# Strongly-connected-component analysis

`libcpg` provides exact, feature-free strongly-connected-component (SCC)
analysis for two directed projections of a code property graph:

- one function's intraprocedural control-flow graph (CFG); and
- the whole CPG's resolved function-to-function call graph.

SCCs expose loop regions in a CFG and direct or mutual recursion in a call
graph. Unlike the optional algorithm-family detector, this structural analysis
does not infer a likely algorithm from graph shape: it computes the maximal SCC
partition exactly.

## Public API

The functions and result types are available from both `libcpg::analysis` and
the crate root:

```rust
use libcpg::{call_graph_sccs, control_flow_sccs, CodePropertyGraph, NodeId};

fn analyze(cpg: &CodePropertyGraph, function: NodeId) {
    let cfg = control_flow_sccs(cpg, function).expect("function node");
    for component in cfg.cyclic_components() {
        println!("CFG cycle: {:?}", component.nodes);
    }

    let calls = call_graph_sccs(cpg);
    for component in calls.cyclic_components() {
        println!("recursive function cluster: {:?}", component.nodes);
    }
}
```

```rust
pub fn control_flow_sccs(
    cpg: &CodePropertyGraph,
    function: NodeId,
) -> Result<SccDecomposition, SccAnalysisError>;

pub fn call_graph_sccs(cpg: &CodePropertyGraph) -> SccDecomposition;
```

`control_flow_sccs` distinguishes an unknown node from a node that exists but
is not a function. `call_graph_sccs` needs no input node because it includes
every function in the CPG, including isolated functions.

## Result contract

`SccDecomposition` contains:

| Field / method | Meaning |
|---|---|
| `projection` | `ControlFlow { function }` or `CallGraph`. |
| `components` | Every maximal SCC, ordered by its smallest node id. |
| `component_by_node` | Deterministic `BTreeMap<NodeId, usize>` membership lookup. |
| `condensation_edges` | Deduplicated edges between distinct SCC ids. |
| `component_of(node)` | The component containing `node`, if it was projected. |
| `cyclic_components()` | Only components that contain a directed cycle. |
| `is_acyclic()` | Whether the projection contains no directed cycle. |

Members within a component are sorted by node id. A component with two or more
nodes is cyclic; a singleton is cyclic only if it has a self-loop. The
`SccComponent::is_self_cycle` and `is_multi_node_cycle` helpers make that
distinction explicit. Component ids index `components` and are stable for the
same node and edge set, independent of hash-table iteration order.

When the `serde` feature is enabled, the projection, components, membership
map, and condensation edges serialize and deserialize together.

## CFG projection boundaries

The CFG projection starts at the requested function and follows its `AstChild`
subtree. A nested function establishes a separate analysis boundary: neither
the nested function nor its descendants are attributed to the enclosing
function. Only `ControlFlow` edges whose two endpoints are inside the selected
scope participate. Consequently, an interprocedural `CfgEdgeKind::Call` edge
to another function cannot merge caller and callee CFG components.

The selected function is retained as an isolated singleton when it has no CFG
edges. Other AST nodes without incident CFG edges are not CFG vertices; this
keeps the projection faithful to the edges actually constructed by the CFG
extractor.

## Call-graph projection and resolution

Call-graph vertices are all `Function` nodes. For each function, the analysis
examines its own AST scope while stopping before nested functions. It recognizes
all resolved call encodings supported by the CPG model:

- `Call { target: Some(function) }`;
- `CallSite`, `StaticCall`, and `DynamicCall` edges; and
- `ControlFlow(CfgEdgeKind::Call)` edges emitted by the built-in CFG extractor.

Each such call-site-to-callee relation is collapsed to one caller-function to
callee-function edge. Duplicate encodings are deduplicated. Targets that do not
name a function and unresolved calls (`target: None` without a resolved edge)
are omitted rather than guessed. Thus call-graph SCCs describe recursion in the
resolved graph; the result does not claim completeness beyond available call
resolution.

## Algorithm and complexity

The implementation uses Tarjan's SCC algorithm over normalized adjacency lists
and dense internal vertex indices. Discovery indices and low-link values live
in contiguous arrays; an explicit DFS-frame stack simulates recursive entry and
return, while a second stack holds the active component candidates. Long paths
therefore cannot overflow the native call stack. Tarjan visits each normalized
edge once and does not construct the transpose graph required by Kosaraju.

For a normalized projection with $`V`$ vertices and $`E`$ edges, decomposition
and condensation construction take $`O(V + E)`$ time and $`O(V + E)`$ auxiliary
space, including the projected adjacency itself. Normalizing duplicate edges
adds $`O(\sum_v d_v \log d_v)`$ sorting work. Contracting each SCC yields the
returned condensation graph, which is always a directed acyclic graph.

Validation includes hand-built CFG and call-graph cases, malformed input and
nested-scope boundaries, singleton self-loops, a 50,000-node path, a 50,000-node
cycle, sparse node ids, serde round-tripping, and property-based differential
testing against `petgraph`'s independent Tarjan implementation.

## References

- Tarjan, R. E. (1972). *Depth-First Search and Linear Graph Algorithms.* SIAM Journal on Computing 1(2), 146–160. DOI: [10.1137/0201010](https://doi.org/10.1137/0201010).
- Cormen, T. H., Leiserson, C. E., Rivest, R. L., Stein, C. (2009). *Introduction to Algorithms* (3rd ed.). MIT Press. ISBN 978-0262033848. (Strongly connected components and condensation graphs.)
