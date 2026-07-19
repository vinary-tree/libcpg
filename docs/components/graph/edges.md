# CPG Edges

Edges in a Code Property Graph connect nodes to represent three types of relationships: syntactic structure (AST), control flow (CFG), and data flow (DFG).

## Edge Structure

```rust
pub struct CpgEdge {
    /// Unique identifier
    id: EdgeId,
    /// Source node
    source: NodeId,
    /// Target node
    target: NodeId,
    /// Edge type
    kind: CpgEdgeKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EdgeId(u32);
```

## Edge Kinds

```rust
pub enum CpgEdgeKind {
    /// AST parent-child relationship
    AstChild,
    /// AST sibling relationship
    AstNextSibling,
    /// Control flow edge
    CfgEdge(CfgEdgeKind),
    /// Data flow edge
    DfgEdge(DfgEdgeKind),
}
```

## AST Edges

AST edges represent the syntactic structure of the code:

### AstChild

Connects a parent node to its child in the syntax tree.

```
       Function
          │
    ── AstChild ──
          │
          ▼
        Block
       /     \
  AstChild   AstChild
     /           \
    ▼             ▼
 Statement    Statement
```

**Example: Walking AST Children**

```rust
// Get immediate AST children
fn ast_children(cpg: &CodePropertyGraph, node_id: NodeId) -> Vec<&CpgNode> {
    cpg.outgoing_edges(node_id)
        .filter_map(|edge_id| {
            let edge = cpg.edge(edge_id).ok()?;
            if edge.kind() == CpgEdgeKind::AstChild {
                cpg.node(edge.target()).ok()
            } else {
                None
            }
        })
        .collect()
}
```

### AstNextSibling

Connects sibling nodes at the same level:

```
    Statement ─── AstNextSibling ──▶ Statement ─── AstNextSibling ──▶ Statement
```

**Example: Iterating Siblings**

```rust
// Get all following siblings
fn following_siblings(cpg: &CodePropertyGraph, node_id: NodeId) -> Vec<&CpgNode> {
    let mut siblings = Vec::new();
    let mut current = node_id;

    while let Some(next) = cpg.outgoing_edges(current)
        .filter_map(|edge_id| {
            let edge = cpg.edge(edge_id).ok()?;
            if edge.kind() == CpgEdgeKind::AstNextSibling {
                Some(edge.target())
            } else {
                None
            }
        })
        .next()
    {
        if let Ok(node) = cpg.node(next) {
            siblings.push(node);
            current = next;
        } else {
            break;
        }
    }

    siblings
}
```

## CFG Edges

Control flow edges represent execution paths through the code:

```rust
pub enum CfgEdgeKind {
    /// Sequential execution to next statement
    Sequential,
    /// Branch taken when condition is true
    BranchTrue,
    /// Branch taken when condition is false
    BranchFalse,
    /// Back edge to loop header
    LoopBack,
    /// Exit from loop body
    LoopExit,
    /// Exception is thrown
    ExceptionThrow,
    /// Exception is caught
    ExceptionCatch,
    /// Default case in switch/match
    Default,
    /// Case in switch/match
    Case,
}
```

### Sequential

Normal flow to the next statement:

```
   let x = 1;
       │
   Sequential
       │
       ▼
   let y = 2;
       │
   Sequential
       │
       ▼
   print(y);
```

### BranchTrue / BranchFalse

Conditional branching:

```
       if condition
         /      \
   BranchTrue  BranchFalse
       /            \
      ▼              ▼
   then_block    else_block
```

**Example: Finding Branches**

```rust
// Find all branch points
for node in cpg.nodes_of_kind(CpgNodeKind::If) {
    let mut has_true = false;
    let mut has_false = false;

    for edge_id in cpg.outgoing_edges(node.id()) {
        if let Ok(edge) = cpg.edge(edge_id) {
            match edge.kind() {
                CpgEdgeKind::CfgEdge(CfgEdgeKind::BranchTrue) => has_true = true,
                CpgEdgeKind::CfgEdge(CfgEdgeKind::BranchFalse) => has_false = true,
                _ => {}
            }
        }
    }

    println!("If at line {}: true={}, false={}",
             node.source_range().start_line,
             has_true, has_false);
}
```

### LoopBack / LoopExit

Loop control flow:

```
          ┌─────────────────────┐
          │                     │
          ▼                     │
   ┌─────────────┐              │
   │ Loop Header │              │
   └─────────────┘          LoopBack
          │                     │
      Sequential                │
          │                     │
          ▼                     │
   ┌─────────────┐              │
   │  Loop Body  │──────────────┘
   └─────────────┘
          │
       LoopExit
          │
          ▼
   ┌─────────────┐
   │ After Loop  │
   └─────────────┘
```

**Example: Detecting Loops**

```rust
// Find all loops and their complexity
for node in cpg.nodes_of_kind(CpgNodeKind::Loop) {
    let has_back_edge = cpg.outgoing_edges(node.id())
        .filter_map(|e| cpg.edge(e).ok())
        .any(|e| matches!(e.kind(), CpgEdgeKind::CfgEdge(CfgEdgeKind::LoopBack)));

    // Count nested loops (loops with loop descendants)
    let nested_count = cpg.ast_descendants(node.id())
        .filter(|n| n.kind() == CpgNodeKind::Loop)
        .count();

    println!("Loop at line {}: nested={}",
             node.source_range().start_line,
             nested_count);
}
```

### ExceptionThrow / ExceptionCatch

Exception handling flow:

```
   ┌──────────────┐
   │   try {      │
   │     may_fail │─── ExceptionThrow ───┐
   │   }          │                      │
   └──────────────┘                      │
          │                              │
      Sequential                         │
          │                              │
          ▼                              ▼
   ┌──────────────┐            ┌──────────────┐
   │  after try   │            │ catch block  │
   └──────────────┘            └──────────────┘
                                      │
                               ExceptionCatch
                                      │
                                      ▼
                               (merged flow)
```

## DFG Edges

Data flow edges track how data moves through the program:

```rust
pub enum DfgEdgeKind {
    /// Definition reaches this use
    DefUse,
    /// Two uses of the same definition
    UseUse,
    /// SSA phi node (value from multiple paths)
    Phi,
    /// Argument passed to function
    Call,
    /// Value returned from function
    Return,
    /// Field access dependency
    Field,
    /// Array/index dependency
    Index,
}
```

### DefUse

The fundamental data flow relationship:

```
   let x = 5;      ◄── Definition of 'x'
       │
     DefUse
       │
       ▼
   let y = x + 1;  ◄── Use of 'x'
       │
     DefUse
       │
       ▼
   print(y);       ◄── Use of 'y'
```

**Example: Finding All Uses of a Definition**

```rust
// Find all uses of a variable definition
fn find_uses(cpg: &CodePropertyGraph, def_id: NodeId) -> Vec<NodeId> {
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
```

### Phi

For SSA-form when a variable can have different values from different paths:

```
   if condition {
       x = 1;        ◄── def1
   } else {
       x = 2;        ◄── def2
   }
                     ◄── phi(def1, def2)
   print(x);         ◄── use of phi
```

**Example: Finding Phi Nodes**

```rust
// Find variables with multiple reaching definitions
for node in cpg.nodes_of_kind(CpgNodeKind::Variable) {
    let phi_count = cpg.incoming_edges(node.id())
        .filter_map(|e| cpg.edge(e).ok())
        .filter(|e| matches!(e.kind(), CpgEdgeKind::DfgEdge(DfgEdgeKind::Phi)))
        .count();

    if phi_count > 1 {
        println!("Variable at line {} has {} reaching definitions",
                 node.source_range().start_line,
                 phi_count);
    }
}
```

### Call / Return

Function call data flow:

```
   fn add(a, b) {
       return a + b;
   }

   let x = 5;
   let y = 3;
   let z = add(x, y);

   x ─── Call ───▶ a (parameter)
   y ─── Call ───▶ b (parameter)
   (a + b) ─── Return ───▶ z
```

### Field

Field access dependencies:

```
   struct Point { x: i32, y: i32 }

   let p = Point { x: 1, y: 2 };
       │
     Field
       │
       ▼
   let sum = p.x + p.y;
```

## Filtering Edges

Common patterns for edge filtering:

```rust
// Get only CFG edges
let cfg_edges: Vec<_> = cpg.edges()
    .filter(|e| matches!(e.kind(), CpgEdgeKind::CfgEdge(_)))
    .collect();

// Get only DFG edges
let dfg_edges: Vec<_> = cpg.edges()
    .filter(|e| matches!(e.kind(), CpgEdgeKind::DfgEdge(_)))
    .collect();

// Get specific CFG edge type
let branches: Vec<_> = cpg.edges()
    .filter(|e| matches!(
        e.kind(),
        CpgEdgeKind::CfgEdge(CfgEdgeKind::BranchTrue | CfgEdgeKind::BranchFalse)
    ))
    .collect();
```

## Edge Statistics

```rust
let stats = cpg.stats();

println!("Total edges: {}", stats.edge_count);
println!("AST edges: {}", stats.ast_edge_count);
println!("CFG edges: {}", stats.cfg_edge_count);
println!("DFG edges: {}", stats.dfg_edge_count);
```

## Next Steps

- [Traversal](traversal.md) - Navigation patterns
- [Nodes](nodes.md) - Node types
- [Overview](overview.md) - Back to overview
