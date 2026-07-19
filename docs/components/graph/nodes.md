# CPG Nodes

Nodes in a Code Property Graph represent syntactic elements from the source code. Each node has a kind, source location, and optional properties.

## Node Structure

```rust
pub struct CpgNode {
    /// Unique identifier within the graph
    id: NodeId,
    /// The type of syntactic element
    kind: CpgNodeKind,
    /// Location in source code
    source_range: SourceRange,
    /// Original source text (optional)
    text: Option<Arc<str>>,
    /// Additional properties
    properties: HashMap<PropertyKey, PropertyValue>,
}
```

## Node Kinds

### Control Flow Nodes

These nodes represent control flow structures:

| Kind | Description | Example |
|------|-------------|---------|
| `Function` | Function definition | `fn foo() {}` |
| `Method` | Method definition | `impl T { fn bar() {} }` |
| `Constructor` | Constructor | `new()`, `__init__` |
| `Block` | Statement block | `{ stmt1; stmt2; }` |
| `If` | Conditional | `if cond { }` |
| `Loop` | Loop construct | `for`, `while`, `loop` |
| `Match` | Pattern match | `match x { }` |
| `Return` | Return statement | `return value` |
| `Break` | Break statement | `break` |
| `Continue` | Continue statement | `continue` |

**Example: Extracting Functions**

```rust
// Find all functions in the CPG
let functions: Vec<&CpgNode> = cpg
    .nodes_of_kind(CpgNodeKind::Function)
    .collect();

for func in functions {
    let name = func.property(PropertyKey::Name)
        .and_then(|v| v.as_string())
        .unwrap_or("anonymous");

    println!("Function: {} at line {}",
             name,
             func.source_range().start_line);
}
```

### Expression Nodes

Nodes representing expressions:

| Kind | Description | Example |
|------|-------------|---------|
| `BinaryOp` | Binary operation | `a + b`, `x && y` |
| `UnaryOp` | Unary operation | `-x`, `!flag` |
| `Call` | Function call | `foo(arg)` |
| `Index` | Index access | `arr[i]` |
| `FieldAccess` | Field access | `obj.field` |
| `Cast` | Type cast | `x as i32` |
| `Reference` | Reference | `&x`, `&mut x` |
| `Dereference` | Dereference | `*ptr` |

**Example: Finding All Function Calls**

```rust
// Find all function calls
for call in cpg.nodes_of_kind(CpgNodeKind::Call) {
    // Get the callee name from the first AST child
    let callee = cpg.ast_children(call.id())
        .next()
        .and_then(|child| child.text().map(|s| s.to_string()))
        .unwrap_or_else(|| "unknown".to_string());

    println!("Call to: {} at {:?}", callee, call.source_range());
}
```

### Declaration Nodes

Nodes for variable and constant declarations:

| Kind | Description | Example |
|------|-------------|---------|
| `Variable` | Variable declaration | `let x = 5` |
| `Parameter` | Function parameter | `fn f(x: i32)` |
| `Constant` | Constant declaration | `const X: i32 = 5` |
| `Field` | Struct/class field | `struct S { x: i32 }` |

**Example: Variable Declarations**

```rust
// Find all variable declarations with their types
for var in cpg.nodes_of_kind(CpgNodeKind::Variable) {
    let name = var.property(PropertyKey::Name)
        .and_then(|v| v.as_string());

    let type_info = var.type_info();

    if let (Some(name), Some(ti)) = (name, type_info) {
        println!("Variable: {}: {:?}", name, ti);
    }
}
```

### Type Definition Nodes

Nodes for type definitions:

| Kind | Description | Example |
|------|-------------|---------|
| `Class` | Class definition | `class Foo { }` |
| `Struct` | Struct definition | `struct Bar { }` |
| `Enum` | Enum definition | `enum Baz { }` |
| `Interface` | Interface/protocol | `interface I { }` |
| `Trait` | Trait definition | `trait T { }` |
| `TypeAlias` | Type alias | `type X = Y` |

**Example: Finding Classes with Methods**

```rust
// Find all classes and their methods
for class in cpg.nodes_of_kind(CpgNodeKind::Class) {
    let class_name = class.property(PropertyKey::Name)
        .and_then(|v| v.as_string())
        .unwrap_or("anonymous");

    println!("Class: {}", class_name);

    // Find methods (traverse AST children)
    for child in cpg.ast_descendants(class.id()) {
        if child.kind() == CpgNodeKind::Method {
            let method_name = child.property(PropertyKey::Name)
                .and_then(|v| v.as_string())
                .unwrap_or("anonymous");
            println!("  Method: {}", method_name);
        }
    }
}
```

### Literal Nodes

Literal value nodes:

```rust
pub enum LiteralKind {
    Integer,
    Float,
    String,
    Char,
    Boolean,
    Null,
    Array,
    Object,
}
```

| Kind | Description | Example |
|------|-------------|---------|
| `Literal(Integer)` | Integer literal | `42` |
| `Literal(Float)` | Float literal | `3.14` |
| `Literal(String)` | String literal | `"hello"` |
| `Literal(Char)` | Character literal | `'a'` |
| `Literal(Boolean)` | Boolean literal | `true`, `false` |
| `Literal(Null)` | Null/None/nil | `null`, `None` |
| `Literal(Array)` | Array literal | `[1, 2, 3]` |
| `Literal(Object)` | Object literal | `{ key: value }` |

**Example: Finding String Literals**

```rust
// Find all string literals (useful for security analysis)
for node in cpg.nodes() {
    if let CpgNodeKind::Literal(LiteralKind::String) = node.kind() {
        if let Some(text) = node.text() {
            println!("String literal: {} at line {}",
                     text,
                     node.source_range().start_line);
        }
    }
}
```

## Node Properties

### Standard Properties

| Key | Value Type | Description |
|-----|------------|-------------|
| `Name` | String | Identifier name |
| `Type` | String | Type annotation |
| `Visibility` | String | Access modifier |
| `Modifiers` | List | static, const, async, etc. |
| `Documentation` | String | Doc comment |
| `Operator` | String | For BinaryOp/UnaryOp |

**Example: Accessing Properties**

```rust
let node = cpg.node(node_id)?;

// Get name
if let Some(PropertyValue::String(name)) = node.property(PropertyKey::Name) {
    println!("Name: {}", name);
}

// Get visibility
if let Some(vis) = node.visibility() {
    match vis {
        Visibility::Public => println!("Public"),
        Visibility::Private => println!("Private"),
        Visibility::Protected => println!("Protected"),
        Visibility::Internal => println!("Internal"),
    }
}

// Get modifiers
if let Some(PropertyValue::List(mods)) = node.property(PropertyKey::Modifiers) {
    for m in mods {
        if let PropertyValue::String(s) = m {
            println!("Modifier: {}", s);
        }
    }
}
```

### Type Information

Nodes can have associated type information:

```rust
pub struct TypeInfo {
    /// The type name
    pub name: Arc<str>,
    /// Generic parameters
    pub generics: Vec<TypeInfo>,
    /// Whether it's a reference
    pub is_reference: bool,
    /// Whether it's mutable
    pub is_mutable: bool,
    /// Whether it's optional/nullable
    pub is_optional: bool,
}
```

**Example**:

```rust
if let Some(type_info) = node.type_info() {
    println!("Type: {}", type_info.name);
    if !type_info.generics.is_empty() {
        print!("<");
        for (i, g) in type_info.generics.iter().enumerate() {
            if i > 0 { print!(", "); }
            print!("{}", g.name);
        }
        println!(">");
    }
}
```

### Method Signatures

Function and method nodes have signature information:

```rust
pub struct MethodSignature {
    /// Parameter names and types
    pub parameters: Vec<(Arc<str>, Option<TypeInfo>)>,
    /// Return type
    pub return_type: Option<TypeInfo>,
    /// Whether it's async
    pub is_async: bool,
    /// Whether it's a generator
    pub is_generator: bool,
}
```

**Example**:

```rust
for func in cpg.nodes_of_kind(CpgNodeKind::Function) {
    if let Some(sig) = func.method_signature() {
        let name = func.property(PropertyKey::Name)
            .and_then(|v| v.as_string())
            .unwrap_or("anonymous");

        print!("fn {}(", name);
        for (i, (param_name, param_type)) in sig.parameters.iter().enumerate() {
            if i > 0 { print!(", "); }
            print!("{}", param_name);
            if let Some(t) = param_type {
                print!(": {}", t.name);
            }
        }
        print!(")");

        if let Some(ret) = &sig.return_type {
            print!(" -> {}", ret.name);
        }
        println!();
    }
}
```

## Node ID

`NodeId` is a lightweight handle for referencing nodes:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NodeId(u32);

impl NodeId {
    /// Get the underlying index
    pub fn index(self) -> usize {
        self.0 as usize
    }
}
```

Node IDs are:
- **Copy**: No ownership issues
- **32-bit**: Compact memory usage
- **Hashable**: Can be used as map keys

## Source Ranges

Every node tracks its location in the source:

```rust
pub struct SourceRange {
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_line: u32,
    pub start_column: u32,
    pub end_line: u32,
    pub end_column: u32,
}
```

**Example: Getting Original Text**

```rust
fn get_source_text<'a>(cpg: &'a CodePropertyGraph, node: &CpgNode, source: &'a str) -> &'a str {
    let range = node.source_range();
    &source[range.start_byte..range.end_byte]
}
```

## Next Steps

- [Edges](edges.md) - Edge types and relationships
- [Traversal](traversal.md) - Navigating the graph
- [Graph Overview](overview.md) - Back to overview
