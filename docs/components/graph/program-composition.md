# Multi-file program composition and import linkage

A parser-built [`CodePropertyGraph`](../../GLOSSARY.md#code-property-graph-cpg)
(CPG) normally represents one source file. The feature-free program API combines
any number of those graphs into one append-only `ProgramCpg`, records which
translated nodes came from each file, and materializes only uniquely supported
`Imports` and `Exports` edges. This gives interprocedural analyses one shared
identifier space without discarding per-file identity.

![Per-file graphs pass through deterministic identifier translation, then evidence-bounded import linkage feeds cross-file call resolution.](../../diagrams/program-composition.svg)

*Figure — deterministic multi-file composition and its evidence boundary.
Source: [`diagrams/program-composition.puml`](../../diagrams/program-composition.puml).*

## Public API

The graph method and program functions are available with `default = []`:

```rust
use rustc_hash::FxHashMap;
use libcpg::{
    build_import_graph, merge_programs, CodePropertyGraph, ImportGraph, NodeId,
    ProgramCpg,
};

fn append_file(
    destination: &mut CodePropertyGraph,
    source: &CodePropertyGraph,
) -> FxHashMap<NodeId, NodeId> {
    destination.merge_from(source)
}

fn compose(files: Vec<CodePropertyGraph>) -> (ProgramCpg, ImportGraph) {
    let mut program = merge_programs(files);
    let imports = build_import_graph(&mut program);
    (program, imports)
}
```

The exact signatures are:

```rust
impl CodePropertyGraph {
    pub fn merge_from(
        &mut self,
        other: &CodePropertyGraph,
    ) -> FxHashMap<NodeId, NodeId>;
}

pub fn merge_programs(files: Vec<CodePropertyGraph>) -> ProgramCpg;
pub fn build_import_graph(program: &mut ProgramCpg) -> ImportGraph;
```

`ProgramCpg::cpg` is the unified graph. `ProgramCpg::files` retains input order;
each `FileScope` contains the original path, source text, language, translated
root, sorted translated node set, and an ordered source-to-program `node_map`.
`FileScope::translated` performs a lookup and `FileScope::contains` tests
membership by binary search.

## Complete identifier translation

Every input file begins with an independent `NodeId` and `EdgeId` namespace.
Copying those numbers directly would collide. `merge_from` therefore applies a
total source-node map $`\mu : V_s \rightarrow V_d`$, where $`V_s`$ is the source
node set and $`V_d`$ is a newly allocated, disjoint destination range.

```text
merge_from(destination, source):
    source_nodes = sort source nodes by NodeId
    for node in source_nodes:
        map[node.id] = destination.add_node(clone(node))

    for source id in sorted map keys:
        rewrite parent, children, Call.target,
                Identifier.definition, Lambda.captures through map
        discard any embedded reference outside source

    for edge in ascending EdgeId order:
        append an edge between mapped endpoints using a fresh EdgeId

    map and append valid CFG entries/exits
    preserve every pre-existing destination node and edge
    return map
```

The complete typed reference inventory is intentionally explicit:

| Location | Shape | Translation |
|---|---|---|
| `CpgNode::parent` | `Option<NodeId>` | map or clear |
| `CpgNode::children` | `SmallVec<NodeId>` | map each resolvable member |
| `CpgNodeKind::Call::target` | `Option<NodeId>` | map or clear |
| `CpgNodeKind::Identifier::definition` | `Option<NodeId>` | map or clear |
| `CpgNodeKind::Lambda::captures` | `SmallVec<NodeId>` | map each resolvable member |
| `CpgEdge::{source,target}` | two `NodeId` values | map both; omit malformed edge otherwise |
| CFG entry/exit lists | `Vec<NodeId>` | map valid members, sort, append uniquely |

Clearing an out-of-source reference is important. Retaining its numeric value
could accidentally bind it to an unrelated destination node with the same
number—a collision that looks valid to `contains_node` but is semantically
false. The merge is append-only: source defects cannot remove or rewrite
pre-existing destination content.

When the destination is empty, its primary `root()` becomes the translated
source root rather than merely the smallest source identifier. Later file roots
live in their `FileScope`; one scalar graph root cannot represent multiple
compilation units. For one input file, the program graph also retains that
file's `source_path` and `source_code`. For multiple files, the per-file scopes
are authoritative because the source-coordinate spaces differ.

## Program language and ordering

`merge_programs` processes files in caller-supplied order. File $`i`$ receives
a contiguous fresh identifier range before file $`i+1`$. Within a file, source
identifiers are sorted, so hash-table order and petgraph insertion order do not
affect the mapping.

The program graph records the common language if every input file agrees. An
empty or heterogeneous program records `Language::Unknown`; each `FileScope`
still retains its exact input language. The API does not sort files by path,
because caller order may carry build-system or compilation-unit meaning.

## Import aliases and evidence tiers

`build_import_graph` derives normalized identifier-token aliases from:

- every extensionless source-path suffix (`src/math.rs` gives `src/math` and
  `math`); and
- every represented `Module { name }` node and its suffixes.

Conventional module filenames `mod`, `lib`, `index`, and `__init__` identify
their parent directory. Separators are language-neutral: punctuation, path
slashes, namespace colons, dots, quotes, and whitespace delimit tokens.
Consequently common Rust, Python, JavaScript, Java, Go, and C/C++ import text
uses the same matcher without a language-specific parser.

For one import, every non-importing file is scored by its longest contiguous
alias found in the import tokens. Only the strongest length survives:

| Result | Condition | Dependency topology |
|---|---|---|
| `Exact` | exactly one file has the strongest alias | emitted |
| `Ambiguous` | at least two files tie for strongest alias | none |
| `External` | no represented file has an alias | none |

The full decision is retained as `ImportResolution`, including the import node,
importing file index, original path, matched alias, candidate files, and selected
declarations. `ImportGraph::resolutions` is ordered by import `NodeId`, so an
unresolved result is observable rather than silently absent.

## Export and import edge semantics

An export candidate is a named declaration at the file root or beneath only
`Module` ancestors. Declarations nested inside a function, class, struct,
trait, implementation, or block are not file-namespace exports.

For every file root $`r_f`$ and export declaration $`d`$, the pass emits:

```text
Exports(r_f, d)
```

For an exact import from file $`a`$ to file $`b`$, it also emits:

```text
Imports(root_a, root_b)       // module/file dependency
Imports(import_node, d)[name] // declaration evidence + local binding label
```

If tokens after the matched module alias name represented exports, only those
declarations are connected. For brace-delimited JavaScript/TypeScript imports,
the named declarations before `from` are selected instead. Tokens immediately
following `as` are local aliases, never additional exported declarations.
Otherwise the import is treated as a module, wildcard, or alias import and
connects all file-namespace declarations. The
call resolver groups declaration-level imported functions by lexical scope and
local binding. For example, `from math import add as sum` labels the declaration
edge `sum`, while an unrenamed import labels it `add`. This lets
`ExactViaImport` choose `math::add` for a call to `sum`, even when another file
defines either `add` or `sum`. Repeated import statements pointing to the same
binding and declaration remain one candidate.

Unlabelled file/export topology uses endpoint-and-kind uniqueness;
declaration imports use endpoint, kind, and local-binding-label uniqueness. A
second `build_import_graph` invocation produces the same evidence and reports
`emitted_edges == 0`.

The insertion helpers check that identity through petgraph's native directed
adjacency at the source node. They do not rebuild the whole-graph read
projection between insertions. The kind-only `Exports` rule deliberately
rejects an additional unlabelled edge even when the existing `Exports` edge
has a label; the label-sensitive `Imports` rule does not. The [finite-list
refinement proof](../../../formal/rocq/ImportOverlayEdgeIdentity.v) establishes
both duplicate decisions and insertion idempotence. Generated differential
tests compare native adjacency with the earlier projected query, including
pre-existing labelled exports and arbitrary insertion orders. These checks
support the edge-identity claim; they are not a proof of all petgraph internals.

## Complexity and boundaries

For $`N`$ source nodes, $`E`$ source edges, and $`R`$ embedded references,
`merge_from` uses $`O(N \log N + E \log E + R)`$ time and $`O(N + E)`$
temporary space because it clones and sorts the finite input inventory.

For $`F`$ files, $`I`$ import nodes, at most $`T`$ import tokens, $`A`$ aliases,
and $`D`$ declarations, import construction is bounded by the represented
program. Its conservative worst case is $`O(DN + ITA)`$ before edge insertion;
ordinary AST depth and alias buckets make the practical work much smaller.
For $`K`$ attempted overlay insertions, let $`d_i`$ be the source node's
outgoing degree at attempt $`i`$. The pinned petgraph `edges_connecting`
iterator scans that local outgoing row, so duplicate probing adds at most
$`O(\sum_{i=1}^{K} d_i)`$ time and constant iterator storage per attempt.
This removes the repeated global projection rebuild, but does **not** promise
constant-time insertion: a high-degree source can still yield a quadratic
worst case in $`K`$. The bounded full-corpus replay must therefore measure
the remaining cost before a release claim is made.
There is no filesystem access, package-manager lookup, network resolution, or
language parser in this pass. `External` means “not represented by this
`ProgramCpg`,” not “the package does not exist.”

The matcher is intentionally conservative at file identity: a strongest-alias
tie never creates dependency topology. Declaration selection may include all
namespace declarations when syntax is module-shaped or cannot name one exact
export. The downstream call resolver still requires one same-named imported
function before creating call topology.

## See also

- [Theory 24](../../theory/24-program-composition.md) formalizes the disjoint
  identifier injection and evidence relation.
- [ADR-0031](../../design/0031-deterministic-program-composition.md) records the
  accepted design and rejected alternatives.
- [Scientific validation 26](../../scientific/26-program-composition-validation.md)
  maps every invariant to an executable oracle.
- [Usage 07](../../usage/07-multi-file-programs.md) gives an end-to-end example.
- [Evidence-bounded call resolution](call-resolution.md) defines the topology
  boundary that consumes declaration-level imports.

## References

This component description makes no literature-dependent algorithmic claim.
Its normative sources are the public API, [ADR-0031](../../design/0031-deterministic-program-composition.md),
and the executable oracle ledger in [Scientific validation 26](../../scientific/26-program-composition-validation.md).
