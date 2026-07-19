# Data Flow Architecture

This document describes how data flows through libcpg, from source code input to analysis results.

## Construction Flow

### Stage 1: Parsing

Source code enters through the `CpgBuilder` interface:

```rust
let builder = TreeSitterCpgBuilder::new();
let cpg = builder.build(source, Language::Rust)?;
```

The tree-sitter parser produces a concrete syntax tree (CST), which is then converted to our abstract representation:

```
   Source Code                    Tree-sitter CST                  CpgNodes
        │                               │                              │
        ▼                               ▼                              ▼
   "fn foo() {           ───▶    (function_item              ───▶   Function
     let x = 1;                    (identifier)                        │
   }"                               (block                           Block
                                      (let_declaration                  │
                                        (identifier)                Variable
                                        (integer_literal))))          Literal
```

### Stage 2: AST Edge Creation

AST edges are created during parsing, connecting parent nodes to children:

```
         Function
            │
    ─── AstChild ───
            │
            ▼
         Block
            │
    ─── AstChild ───
            │
            ▼
       LetDecl
         /   \
   AstChild  AstChild
       /         \
      ▼           ▼
  Variable     Literal
```

### Stage 3: CFG Extraction

The `CfgExtractor` walks the AST to identify basic blocks and control flow:

```rust
let cfg_config = CfgExtractorConfig::default();
let cfg_extractor = CfgExtractor::new(cfg_config);
cfg_extractor.extract(&mut cpg)?;
```

**Basic Block Identification**:

```
   fn example(x: i32) {         ┌────────────────────┐
       let y = x + 1;           │ BB0: Entry         │
                         ───▶   │   let y = x + 1    │
       if y > 0 {               └─────────┬──────────┘
           return y;                      │
       }                         ┌────────┴────────┐
       return 0;                 ▼                 ▼
   }                    ┌──────────────┐  ┌──────────────┐
                        │ BB1: if-true │  │ BB2: if-false│
                        │   return y   │  │   return 0   │
                        └──────────────┘  └──────────────┘
```

**CFG Edge Types**:

| Edge Type | Meaning |
|-----------|---------|
| `Sequential` | Normal flow to next statement |
| `BranchTrue` | Condition evaluated to true |
| `BranchFalse` | Condition evaluated to false |
| `LoopBack` | Back edge in a loop |
| `LoopExit` | Exit from loop body |
| `ExceptionThrow` | Exception thrown |
| `ExceptionCatch` | Exception caught |

### Stage 4: DFG Extraction

The `DfgExtractor` builds def-use chains showing how data flows:

```rust
let dfg_config = DfgExtractorConfig::default();
let dfg_extractor = DfgExtractor::new(dfg_config);
dfg_extractor.extract(&mut cpg)?;
```

**Def-Use Chain Construction**:

```
   let x = 5;        ─── Definition of 'x' ───▶  ┌──────────┐
   let y = x + 1;    ◀── Use of 'x' ─────────────│ DefUse   │
   print(y);         ◀── Use of 'y' ─────────────│ Chain    │
                                                  └──────────┘
```

**DFG Edge Types**:

| Edge Type | Meaning |
|-----------|---------|
| `DefUse` | Definition reaches this use |
| `UseUse` | Two uses of same definition |
| `Phi` | Value from multiple paths (SSA) |
| `Call` | Value passed to function |
| `Return` | Value returned from function |
| `Field` | Field access dependency |

## Analysis Flow

Once the CPG is constructed, various analyses can be performed.

### Pattern Detection Flow

```
   ┌────────────────────────────────────────────────────────────────┐
   │                     Pattern Detection Flow                      │
   ├────────────────────────────────────────────────────────────────┤
   │                                                                │
   │   CPG                Pattern                  Matcher          │
   │    │                 Template                   │              │
   │    │                    │                       │              │
   │    ▼                    ▼                       ▼              │
   │  ┌───────┐         ┌──────────┐           ┌──────────┐        │
   │  │ Nodes │  ───▶   │ Required │   ───▶    │  VF2     │        │
   │  │ Edges │         │ Nodes    │           │ Matcher  │        │
   │  └───────┘         │ Edges    │           └──────────┘        │
   │                    └──────────┘                 │              │
   │                                                 ▼              │
   │                                          ┌──────────┐         │
   │                                          │ Matches  │         │
   │                                          │ + Score  │         │
   │                                          └──────────┘         │
   │                                                                │
   └────────────────────────────────────────────────────────────────┘
```

The pattern detector:
1. Loads pattern templates (e.g., GoF patterns)
2. For each template, runs VF2 subgraph isomorphism
3. Collects all matches with confidence scores
4. Filters by minimum confidence threshold

### GNN Embedding Flow

```
   ┌────────────────────────────────────────────────────────────────┐
   │                      GNN Embedding Flow                         │
   ├────────────────────────────────────────────────────────────────┤
   │                                                                │
   │   CPG                Initial              Message Passing      │
   │    │                Features                   │               │
   │    │                   │                       │               │
   │    ▼                   ▼                       ▼               │
   │  ┌───────┐        ┌──────────┐          ┌──────────────┐      │
   │  │ Node  │  ───▶  │ Node     │   ───▶   │ Propagate    │      │
   │  │ Props │        │ Vectors  │          │ K iterations │      │
   │  └───────┘        └──────────┘          └──────────────┘      │
   │                                                │               │
   │                                                ▼               │
   │                                         ┌──────────────┐      │
   │                                         │ Final Node   │      │
   │                                         │ Embeddings   │      │
   │                                         └──────────────┘      │
   │                                                │               │
   │                         ┌──────────────────────┴───────┐      │
   │                         ▼                              ▼      │
   │                  ┌──────────────┐              ┌────────────┐ │
   │                  │ Subgraph     │              │ Similarity │ │
   │                  │ Aggregation  │              │ Search     │ │
   │                  └──────────────┘              └────────────┘ │
   │                                                                │
   └────────────────────────────────────────────────────────────────┘
```

### Algorithm Detection Flow

```
   ┌────────────────────────────────────────────────────────────────┐
   │                   Algorithm Detection Flow                      │
   ├────────────────────────────────────────────────────────────────┤
   │                                                                │
   │   CPG            Feature              Classifier              │
   │    │            Extraction               │                     │
   │    │                │                    │                     │
   │    ▼                ▼                    ▼                     │
   │  ┌───────┐     ┌──────────┐       ┌──────────────┐            │
   │  │ CFG   │ ──▶ │ Loop     │  ──▶  │ Algorithm    │            │
   │  │ Edges │     │ Patterns │       │ Family ID    │            │
   │  └───────┘     │ Recursion│       └──────────────┘            │
   │                │ Depth    │              │                     │
   │                └──────────┘              ▼                     │
   │                                    ┌──────────────┐           │
   │                                    │ Complexity   │           │
   │                                    │ Estimation   │           │
   │                                    └──────────────┘           │
   │                                                                │
   └────────────────────────────────────────────────────────────────┘
```

## Caching Strategy

libcpg employs caching at multiple levels:

### Parse Cache

Tree-sitter parse results can be cached for incremental updates:

```rust
let mut builder = TreeSitterCpgBuilder::new();
builder.enable_cache(true);

// First parse - builds full CPG
let cpg1 = builder.build(source1, Language::Rust)?;

// Second parse with minor changes - reuses parts
let cpg2 = builder.build(source2, Language::Rust)?;
```

### Embedding Cache

GNN embeddings are cached by node ID:

```rust
let gnn = CpgGnn::new(config);
gnn.propagate(&cpg, iterations);

// Embeddings are cached after propagation
let emb1 = gnn.node_embedding(node_id);  // Computed
let emb2 = gnn.node_embedding(node_id);  // Cache hit
```

## Serialization

CPGs can be serialized for persistence:

```rust
#[cfg(feature = "serde")]
{
    // Save to disk
    let bytes = bincode::serialize(&cpg)?;
    std::fs::write("graph.cpg", &bytes)?;

    // Load from disk
    let bytes = std::fs::read("graph.cpg")?;
    let cpg: CodePropertyGraph = bincode::deserialize(&bytes)?;
}
```

## Error Propagation

Errors flow upward through the call stack:

```
   build()
      │
      ├── parse() ─────── Error::UnsupportedLanguage
      │      │
      │      └── tree_sitter::parse() ─── Error::Construction
      │
      ├── extract_cfg() ── Error::Construction
      │
      └── extract_dfg() ── Error::Construction
```

All errors are wrapped in `libcpg::Error` for consistent handling.

## Next Steps

- [Graph Overview](../components/graph/overview.md) - CPG structure details
- [Pattern Detection](../components/patterns/overview.md) - How patterns are matched
- [GNN Operations](../components/gnn/overview.md) - Embedding computation
