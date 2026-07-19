# Graph Traversal

libcpg provides multiple ways to navigate the Code Property Graph. This document covers traversal patterns for common analysis tasks.

## Basic Navigation

### Outgoing and Incoming Edges

The fundamental navigation primitives:

```rust
// Get all outgoing edges from a node
for edge_id in cpg.outgoing_edges(node_id) {
    let edge = cpg.edge(edge_id)?;
    let target = cpg.node(edge.target())?;
    println!("{:?} -> {:?}", edge.kind(), target.kind());
}

// Get all incoming edges to a node
for edge_id in cpg.incoming_edges(node_id) {
    let edge = cpg.edge(edge_id)?;
    let source = cpg.node(edge.source())?;
    println!("{:?} <- {:?}", source.kind(), edge.kind());
}
```

### Neighbors

Get connected nodes regardless of direction:

```rust
// All nodes connected by outgoing edges
let successors: Vec<NodeId> = cpg.successors(node_id).collect();

// All nodes connected by incoming edges
let predecessors: Vec<NodeId> = cpg.predecessors(node_id).collect();

// All connected nodes (both directions)
let neighbors: Vec<NodeId> = cpg.neighbors(node_id).collect();
```

## AST Traversal

### Children and Parent

Navigate the syntax tree:

```rust
// Get AST children
for child in cpg.ast_children(node_id) {
    println!("Child: {:?}", child.kind());
}

// Get AST parent
if let Some(parent) = cpg.ast_parent(node_id) {
    println!("Parent: {:?}", parent.kind());
}
```

### Descendants and Ancestors

Traverse the full subtree:

```rust
// All descendants (pre-order)
for descendant in cpg.ast_descendants(node_id) {
    println!("  {:?}", descendant.kind());
}

// All ancestors (up to root)
for ancestor in cpg.ast_ancestors(node_id) {
    println!("  {:?}", ancestor.kind());
}
```

**Example: Finding Nested Loops**

```rust
fn find_nested_loops(cpg: &CodePropertyGraph) -> Vec<(NodeId, usize)> {
    let mut results = Vec::new();

    for node in cpg.nodes_of_kind(CpgNodeKind::Loop) {
        let depth = cpg.ast_ancestors(node.id())
            .filter(|n| n.kind() == CpgNodeKind::Loop)
            .count();

        if depth > 0 {
            results.push((node.id(), depth));
        }
    }

    results
}
```

### Siblings

Navigate between siblings:

```rust
// Next sibling
if let Some(next) = cpg.ast_next_sibling(node_id) {
    println!("Next: {:?}", next.kind());
}

// Previous sibling
if let Some(prev) = cpg.ast_prev_sibling(node_id) {
    println!("Prev: {:?}", prev.kind());
}

// All following siblings
for sibling in cpg.ast_following_siblings(node_id) {
    println!("Following: {:?}", sibling.kind());
}
```

## CFG Traversal

### Control Flow Successors

Navigate execution paths:

```rust
// Immediate CFG successors
for (successor, edge_kind) in cpg.cfg_successors(node_id) {
    match edge_kind {
        CfgEdgeKind::Sequential => println!("Next: {:?}", successor.kind()),
        CfgEdgeKind::BranchTrue => println!("If true: {:?}", successor.kind()),
        CfgEdgeKind::BranchFalse => println!("If false: {:?}", successor.kind()),
        CfgEdgeKind::LoopBack => println!("Loop back to: {:?}", successor.kind()),
        _ => {}
    }
}

// CFG predecessors
for (predecessor, edge_kind) in cpg.cfg_predecessors(node_id) {
    println!("From {:?} via {:?}", predecessor.kind(), edge_kind);
}
```

### Reachability

Check if one node can reach another via CFG:

```rust
// BFS reachability check
fn cfg_reachable(cpg: &CodePropertyGraph, from: NodeId, to: NodeId) -> bool {
    use std::collections::{HashSet, VecDeque};

    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    queue.push_back(from);

    while let Some(current) = queue.pop_front() {
        if current == to {
            return true;
        }

        if !visited.insert(current) {
            continue;
        }

        for (successor, _) in cpg.cfg_successors(current) {
            queue.push_back(successor.id());
        }
    }

    false
}
```

### Finding Paths

Find all CFG paths between two nodes:

```rust
fn find_cfg_paths(
    cpg: &CodePropertyGraph,
    from: NodeId,
    to: NodeId,
    max_length: usize,
) -> Vec<Vec<NodeId>> {
    let mut paths = Vec::new();
    let mut current_path = vec![from];

    fn dfs(
        cpg: &CodePropertyGraph,
        current: NodeId,
        target: NodeId,
        path: &mut Vec<NodeId>,
        paths: &mut Vec<Vec<NodeId>>,
        max_length: usize,
    ) {
        if path.len() > max_length {
            return;
        }

        if current == target {
            paths.push(path.clone());
            return;
        }

        for (successor, _) in cpg.cfg_successors(current) {
            let succ_id = successor.id();
            if !path.contains(&succ_id) {
                path.push(succ_id);
                dfs(cpg, succ_id, target, path, paths, max_length);
                path.pop();
            }
        }
    }

    dfs(cpg, from, to, &mut current_path, &mut paths, max_length);
    paths
}
```

## DFG Traversal

### Def-Use Chains

Follow data dependencies:

```rust
// Get all uses of a definition
fn get_uses(cpg: &CodePropertyGraph, def_id: NodeId) -> Vec<NodeId> {
    cpg.outgoing_edges(def_id)
        .filter_map(|edge_id| {
            let edge = cpg.edge(edge_id).ok()?;
            match edge.kind() {
                CpgEdgeKind::DfgEdge(DfgEdgeKind::DefUse) => Some(edge.target()),
                _ => None,
            }
        })
        .collect()
}

// Get all definitions reaching a use
fn get_reaching_defs(cpg: &CodePropertyGraph, use_id: NodeId) -> Vec<NodeId> {
    cpg.incoming_edges(use_id)
        .filter_map(|edge_id| {
            let edge = cpg.edge(edge_id).ok()?;
            match edge.kind() {
                CpgEdgeKind::DfgEdge(DfgEdgeKind::DefUse) => Some(edge.source()),
                _ => None,
            }
        })
        .collect()
}
```

### Taint Analysis

Track data flow from sources to sinks:

```rust
/// Find all nodes reachable from a source via data flow
fn taint_analysis(cpg: &CodePropertyGraph, source: NodeId) -> HashSet<NodeId> {
    use std::collections::{HashSet, VecDeque};

    let mut tainted = HashSet::new();
    let mut queue = VecDeque::new();
    queue.push_back(source);

    while let Some(current) = queue.pop_front() {
        if !tainted.insert(current) {
            continue;
        }

        // Follow all DFG edges
        for edge_id in cpg.outgoing_edges(current) {
            if let Ok(edge) = cpg.edge(edge_id) {
                if matches!(edge.kind(), CpgEdgeKind::DfgEdge(_)) {
                    queue.push_back(edge.target());
                }
            }
        }
    }

    tainted
}
```

## Combined Traversals

### Finding Variables in Scope

Combine AST and DFG traversal:

```rust
fn variables_in_scope(cpg: &CodePropertyGraph, node_id: NodeId) -> Vec<NodeId> {
    let mut variables = Vec::new();

    // Walk up the AST to find enclosing scopes
    for ancestor in cpg.ast_ancestors(node_id) {
        // Check for variable declarations
        for child in cpg.ast_children(ancestor.id()) {
            if child.kind() == CpgNodeKind::Variable {
                variables.push(child.id());
            }
        }

        // Stop at function boundary
        if ancestor.kind() == CpgNodeKind::Function {
            // Add parameters
            for child in cpg.ast_children(ancestor.id()) {
                if child.kind() == CpgNodeKind::Parameter {
                    variables.push(child.id());
                }
            }
            break;
        }
    }

    variables
}
```

### Dead Code Detection

Combine CFG reachability with AST:

```rust
fn find_dead_code(cpg: &CodePropertyGraph) -> Vec<NodeId> {
    use std::collections::HashSet;

    let mut dead = Vec::new();

    // Find all function entry points
    let functions: Vec<_> = cpg.nodes_of_kind(CpgNodeKind::Function).collect();

    for func in functions {
        // Find all reachable nodes from function entry
        let mut reachable = HashSet::new();
        let mut stack = vec![func.id()];

        while let Some(current) = stack.pop() {
            if !reachable.insert(current) {
                continue;
            }

            for (successor, _) in cpg.cfg_successors(current) {
                stack.push(successor.id());
            }
        }

        // Find unreachable statements in this function
        for descendant in cpg.ast_descendants(func.id()) {
            if matches!(
                descendant.kind(),
                CpgNodeKind::Return | CpgNodeKind::Variable | CpgNodeKind::Call
            ) && !reachable.contains(&descendant.id()) {
                dead.push(descendant.id());
            }
        }
    }

    dead
}
```

## Visitor Pattern

For complex traversals, use the visitor pattern:

```rust
trait CpgVisitor {
    fn visit_node(&mut self, node: &CpgNode, cpg: &CodePropertyGraph);
    fn visit_edge(&mut self, edge: &CpgEdge, cpg: &CodePropertyGraph);
}

fn walk_cpg<V: CpgVisitor>(cpg: &CodePropertyGraph, visitor: &mut V) {
    // Visit all nodes
    for node in cpg.nodes() {
        visitor.visit_node(node, cpg);
    }

    // Visit all edges
    for edge in cpg.edges() {
        visitor.visit_edge(edge, cpg);
    }
}

// Example: Counting node types
struct NodeCounter {
    counts: HashMap<CpgNodeKind, usize>,
}

impl CpgVisitor for NodeCounter {
    fn visit_node(&mut self, node: &CpgNode, _cpg: &CodePropertyGraph) {
        *self.counts.entry(node.kind()).or_insert(0) += 1;
    }

    fn visit_edge(&mut self, _edge: &CpgEdge, _cpg: &CodePropertyGraph) {}
}
```

## Performance Tips

1. **Use iterators lazily**: Don't collect unless necessary
2. **Avoid repeated lookups**: Cache node references
3. **Filter early**: Apply filters before expensive operations
4. **Use parallel iteration** with rayon for large graphs:

```rust
use rayon::prelude::*;

// Parallel node processing
let results: Vec<_> = cpg.nodes()
    .par_bridge()
    .filter(|n| n.kind() == CpgNodeKind::Function)
    .map(|n| analyze_function(cpg, n))
    .collect();
```

## Next Steps

- [Nodes](nodes.md) - Node type details
- [Edges](edges.md) - Edge type details
- [Overview](overview.md) - Back to overview
