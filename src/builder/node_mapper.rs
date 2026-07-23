//! Tree-sitter node type to CpgNodeKind mapping.
//!
//! This module provides language-specific mappers that convert tree-sitter
//! node types into unified CpgNodeKind variants.

use std::sync::Arc;
use smallvec::SmallVec;

use crate::{
    CpgNodeKind, Language, LiteralKind, MethodSignature, ScopeId, TypeInfo, Visibility,
};

/// Maps tree-sitter node types to CpgNodeKind.
///
/// Each language has its own mapping function that understands
/// the specific node types produced by that language's grammar.
pub struct NodeMapper {
    language: Language,
}

impl NodeMapper {
    /// Creates a new node mapper for the given language.
    pub fn new(language: Language) -> Self {
        Self { language }
    }

    /// Returns the language this mapper is configured for.
    pub fn language(&self) -> Language {
        self.language
    }

    /// Maps a tree-sitter node kind string to CpgNodeKind.
    ///
    /// # Arguments
    /// * `ts_kind` - The tree-sitter node kind string
    /// * `node` - The tree-sitter node (for extracting child information)
    /// * `source` - The source code (for extracting text)
    pub fn map_kind(&self, ts_kind: &str, node: &tree_sitter::Node, source: &str) -> CpgNodeKind {
        match self.language {
            Language::Rust => self.map_rust(ts_kind, node, source),
            Language::Python => self.map_python(ts_kind, node, source),
            Language::JavaScript | Language::TypeScript => self.map_javascript(ts_kind, node, source),
            Language::Go => self.map_go(ts_kind, node, source),
            Language::Java => self.map_java(ts_kind, node, source),
            Language::C | Language::Cpp => self.map_c_cpp(ts_kind, node, source),
            Language::Ruby => self.map_ruby(ts_kind, node, source),
            Language::Json => self.map_json(ts_kind, node, source),
            Language::Html => self.map_html(ts_kind, node, source),
            Language::Css => self.map_css(ts_kind, node, source),
            Language::Bash => self.map_bash(ts_kind, node, source),
            Language::Yaml | Language::Toml => self.map_config(ts_kind, node, source),
            Language::Markdown => self.map_markdown(ts_kind, node, source),
            // F1R3FLY.io languages. Gated on the (dependency-free) `rholang` /
            // `metta` cfg toggles: the arms reference only `ts_kind` strings and
            // `tree_sitter::Node` navigation — no grammar crate symbol — so the
            // features pull in nothing (Mode B; the caller hands over an
            // already-parsed tree via `build_from_tree`). See §7 of ADR-041.
            #[cfg(feature = "rholang")]
            Language::Rholang => self.map_rholang(ts_kind, node, source),
            #[cfg(feature = "metta")]
            Language::MeTTa => self.map_metta(ts_kind, node, source),
            _ => self.map_generic(ts_kind, node, source),
        }
    }

    /// Returns true if this node type should be included in the CPG.
    ///
    /// Some tree-sitter nodes are purely syntactic (punctuation, etc.)
    /// and don't contribute to the semantic structure.
    ///
    /// This is the string-keyed inclusion test: punctuation, comments, and the
    /// language-specific *transparent wrapper / grouping-container* rule nodes
    /// whose names never collide with a semantic rule (so keying on the string
    /// alone is safe). Anonymous keyword/operator *tokens* — which in
    /// tree-sitter share their `kind()` string with a same-named rule node (e.g.
    /// the `"contract"` keyword vs. the `contract` rule) — cannot be filtered
    /// here without also dropping the semantic node; that job belongs to the
    /// node-aware [`Self::should_include_node`], which can consult
    /// [`tree_sitter::Node::is_named`].
    pub fn should_include(&self, ts_kind: &str, include_comments: bool) -> bool {
        // Skip pure punctuation
        if matches!(
            ts_kind,
            "(" | ")" | "{" | "}" | "[" | "]" | "," | ";" | ":" | "::" | "." | "->" | "=>" | "<" | ">"
        ) {
            return false;
        }

        // Skip comments unless configured to include them
        if !include_comments && ts_kind.contains("comment") {
            return false;
        }

        // Language-aware flattening of transparent wrappers / grouping
        // containers. Each name below is a *named rule* that carries no CFG/DFG
        // meaning of its own; dropping it reparents its children to the current
        // parent so the head-dispatch (MeTTa) and child-ordering (Rholang)
        // logic in `map_*` sees the real structure directly. None of these
        // names collide with a semantic rule this crate maps, so the string key
        // is sufficient and safe.
        match self.language {
            // MeTTa: `expression` / `atom_expression` wrap *every* node and
            // `prefixed_expression` wraps the `!e`/`?e`/`'e` exec forms
            // (grammar.js:14,43,28). Flatten them so a `list`'s head atom and
            // operands are direct AST children. (The `operator` wrapper is left
            // in place — `map_metta` unwraps it during head-dispatch — because a
            // standalone operator glyph is a meaningful, if inert, leaf.)
            #[cfg(feature = "metta")]
            Language::MeTTa
                if matches!(ts_kind, "expression" | "atom_expression" | "prefixed_expression") =>
            {
                false
            }
            // Rholang: pure comma/semicolon/`&`-list and marker containers with
            // no CFG/DFG meaning. Their concrete members (name_decl, receipt's
            // binds, a send's argument procs, …) reparent to the enclosing
            // construct. `send_single`/`send_multiple`/`var_ref_kind` are
            // arity/kind markers already captured by the `Call`/`Identifier`
            // they annotate.
            #[cfg(feature = "rholang")]
            Language::Rholang
                if matches!(
                    ts_kind,
                    "names"
                        | "name_decls"
                        | "receipts"
                        | "receipt"
                        | "linear_decls"
                        | "conc_decls"
                        | "agent_decls"
                        | "agent_decl"
                        | "inputs"
                        | "messages"
                        | "args"
                        | "procs"
                        | "cases"
                        | "branches"
                        | "send_single"
                        | "send_multiple"
                        | "var_ref_kind"
                ) =>
            {
                false
            }
            _ => true,
        }
    }

    /// Node-aware inclusion test used by the builder walk
    /// ([`crate::builder::tree_sitter`]). It layers a check that *requires* the
    /// raw node on top of the string-keyed [`Self::should_include`]: for
    /// Rholang it drops anonymous keyword/operator/punctuation **tokens**
    /// (`is_named() == false`), which the string test cannot filter because
    /// their `kind()` collides with the same-named semantic rule nodes
    /// (`contract`, `match`, `new`, `let`, `bundle`, `method`, …). Dropping the
    /// tokens — but never the rules — yields clean child ordering for the CFG
    /// builder (e.g. an `ifElse` node's surviving children become exactly
    /// `[condition, consequence, alternative]`, matching `process_if`) and keeps
    /// operator glyphs (`!`, `|`, `++`, `*`) out of the DFG use-set. All CFG/DFG
    /// content in Rholang lives in named rules, so no information is lost.
    ///
    /// MeTTa needs no token filtering: its only anonymous tokens are `(`/`)`
    /// (already dropped as punctuation), and its "keywords" (`if`, `let`,
    /// `import!`) are *named* `identifier` atoms that `map_metta` dispatches on —
    /// they must be retained.
    pub fn should_include_node(&self, node: &tree_sitter::Node, include_comments: bool) -> bool {
        if !self.should_include(node.kind(), include_comments) {
            return false;
        }
        #[cfg(feature = "rholang")]
        if self.language == Language::Rholang && !node.is_named() {
            return false;
        }
        true
    }

    // ========== Language-specific mappers ==========

    fn map_rust(&self, ts_kind: &str, node: &tree_sitter::Node, source: &str) -> CpgNodeKind {
        match ts_kind {
            // Structural
            "source_file" => CpgNodeKind::Root,
            "mod_item" => CpgNodeKind::Module {
                name: self.extract_child_text(node, "name", source),
            },
            "struct_item" => CpgNodeKind::Struct {
                name: self.extract_child_text(node, "name", source),
            },
            "enum_item" => CpgNodeKind::Enum {
                name: self.extract_child_text(node, "name", source),
            },
            "trait_item" => CpgNodeKind::Trait {
                name: self.extract_child_text(node, "name", source),
            },
            "impl_item" => CpgNodeKind::Impl {
                for_type: self.extract_impl_type(node, source),
                trait_name: self.extract_impl_trait(node, source),
            },

            // Functions
            "function_item" | "function_signature_item" => CpgNodeKind::Function {
                signature: self.extract_rust_function_signature(node, source),
            },
            "closure_expression" => CpgNodeKind::Lambda {
                captures: SmallVec::new(),
            },
            "parameter" => CpgNodeKind::Parameter {
                name: self.extract_pattern_name(node, source),
                param_type: self.extract_type_from_node(node, source),
                is_variadic: false,
            },
            "block" => CpgNodeKind::Block {
                scope: ScopeId::GLOBAL,
            },

            // Variables
            "let_declaration" => CpgNodeKind::Variable {
                name: self.extract_pattern_name(node, source),
                var_type: self.extract_type_from_node(node, source),
                scope: ScopeId::GLOBAL,
                is_mutable: self.has_child_kind(node, "mutable_specifier"),
            },
            "const_item" | "static_item" => CpgNodeKind::Variable {
                name: self.extract_child_text(node, "name", source),
                var_type: self.extract_type_from_node(node, source),
                scope: ScopeId::GLOBAL,
                is_mutable: ts_kind == "static_item" && self.has_child_kind(node, "mutable_specifier"),
            },
            "field_declaration" => CpgNodeKind::Field {
                name: self.extract_child_text(node, "name", source),
                field_type: self.extract_type_from_node(node, source),
                visibility: self.extract_rust_visibility(node),
            },

            // Control flow
            "if_expression" => CpgNodeKind::If,
            "else_clause" => CpgNodeKind::Else,
            "while_expression" => CpgNodeKind::While,
            "for_expression" => CpgNodeKind::For,
            "loop_expression" => CpgNodeKind::Loop,
            "match_expression" => CpgNodeKind::Match,
            "match_arm" => CpgNodeKind::MatchArm,
            "return_expression" => CpgNodeKind::Return,
            "break_expression" => CpgNodeKind::Break,
            "continue_expression" => CpgNodeKind::Continue,
            "try_expression" => CpgNodeKind::Try,

            // Expressions
            "binary_expression" => CpgNodeKind::BinaryOp {
                operator: self.extract_operator(node, source),
            },
            "unary_expression" => CpgNodeKind::UnaryOp {
                operator: self.extract_operator(node, source),
            },
            "assignment_expression" | "compound_assignment_expr" => CpgNodeKind::Assignment {
                operator: self.extract_operator(node, source),
            },
            "call_expression" => CpgNodeKind::Call {
                target: None,
                is_method: false,
            },
            "method_call_expression" => CpgNodeKind::Call {
                target: None,
                is_method: true,
            },
            "field_expression" => CpgNodeKind::MemberAccess {
                member: self.extract_child_text(node, "field", source),
            },
            "index_expression" => CpgNodeKind::IndexAccess,
            "identifier" | "type_identifier" | "field_identifier" => CpgNodeKind::Identifier {
                name: Arc::from(self.node_text(node, source)),
                definition: None,
            },
            "await_expression" => CpgNodeKind::Await,

            // Literals
            "integer_literal" => CpgNodeKind::Literal {
                kind: self.parse_integer(node, source),
            },
            "float_literal" => CpgNodeKind::Literal {
                kind: self.parse_float(node, source),
            },
            "string_literal" | "raw_string_literal" => CpgNodeKind::Literal {
                kind: LiteralKind::String(Arc::from(self.node_text(node, source))),
            },
            "char_literal" => CpgNodeKind::Literal {
                kind: self.parse_char(node, source),
            },
            "boolean_literal" => CpgNodeKind::Literal {
                kind: LiteralKind::Bool(self.node_text(node, source) == "true"),
            },
            "array_expression" => CpgNodeKind::Literal {
                kind: LiteralKind::Array,
            },

            // Other
            "use_declaration" => CpgNodeKind::Import {
                path: Arc::from(self.extract_use_path(node, source)),
            },
            "attribute_item" | "inner_attribute_item" => CpgNodeKind::Attribute {
                name: self.extract_attribute_name(node, source),
            },
            "macro_invocation" => CpgNodeKind::Macro {
                name: self.extract_child_text(node, "macro", source),
            },
            "line_comment" | "block_comment" => CpgNodeKind::Comment {
                is_doc: self.node_text(node, source).starts_with("///")
                    || self.node_text(node, source).starts_with("//!"),
            },
            "type_item" => CpgNodeKind::TypeAnnotation {
                type_info: TypeInfo::new(self.extract_child_text(node, "name", source)),
            },
            "generic_type" | "type_parameters" => CpgNodeKind::GenericParam {
                name: Arc::from(self.node_text(node, source)),
            },
            "ERROR" => CpgNodeKind::Error {
                message: Arc::from("Parse error"),
            },

            _ => CpgNodeKind::Unknown {
                kind: Arc::from(ts_kind),
            },
        }
    }

    fn map_python(&self, ts_kind: &str, node: &tree_sitter::Node, source: &str) -> CpgNodeKind {
        match ts_kind {
            // Structural
            "module" => CpgNodeKind::Root,
            "class_definition" => CpgNodeKind::Class {
                name: self.extract_child_text(node, "name", source),
                is_abstract: false,
            },

            // Functions
            "function_definition" => CpgNodeKind::Function {
                signature: self.extract_python_function_signature(node, source),
            },
            "lambda" => CpgNodeKind::Lambda {
                captures: SmallVec::new(),
            },
            "parameters" | "default_parameter" | "typed_parameter" => CpgNodeKind::Parameter {
                name: self.extract_child_text(node, "name", source),
                param_type: None,
                is_variadic: false,
            },
            "block" => CpgNodeKind::Block {
                scope: ScopeId::GLOBAL,
            },

            // Variables
            "assignment" => {
                // In Python, assignments can be variable declarations
                CpgNodeKind::Assignment {
                    operator: Arc::from("="),
                }
            }
            "augmented_assignment" => CpgNodeKind::Assignment {
                operator: self.extract_operator(node, source),
            },

            // Control flow
            "if_statement" => CpgNodeKind::If,
            "elif_clause" | "else_clause" => CpgNodeKind::Else,
            "while_statement" => CpgNodeKind::While,
            "for_statement" => CpgNodeKind::For,
            "match_statement" => CpgNodeKind::Match,
            "case_clause" => CpgNodeKind::MatchArm,
            "return_statement" => CpgNodeKind::Return,
            "break_statement" => CpgNodeKind::Break,
            "continue_statement" => CpgNodeKind::Continue,
            "try_statement" => CpgNodeKind::Try,
            "except_clause" => CpgNodeKind::Catch,
            "finally_clause" => CpgNodeKind::Finally,
            "raise_statement" => CpgNodeKind::Throw,

            // Expressions
            "binary_operator" | "comparison_operator" | "boolean_operator" => CpgNodeKind::BinaryOp {
                operator: self.extract_operator(node, source),
            },
            "unary_operator" | "not_operator" => CpgNodeKind::UnaryOp {
                operator: self.extract_operator(node, source),
            },
            "call" => CpgNodeKind::Call {
                target: None,
                is_method: false,
            },
            "attribute" => CpgNodeKind::MemberAccess {
                member: self.extract_child_text(node, "attribute", source),
            },
            "subscript" => CpgNodeKind::IndexAccess,
            "identifier" => CpgNodeKind::Identifier {
                name: Arc::from(self.node_text(node, source)),
                definition: None,
            },
            "await" => CpgNodeKind::Await,
            "yield" => CpgNodeKind::Yield,

            // Literals
            "integer" => CpgNodeKind::Literal {
                kind: self.parse_integer(node, source),
            },
            "float" => CpgNodeKind::Literal {
                kind: self.parse_float(node, source),
            },
            "string" | "concatenated_string" => CpgNodeKind::Literal {
                kind: LiteralKind::String(Arc::from(self.node_text(node, source))),
            },
            "true" | "false" => CpgNodeKind::Literal {
                kind: LiteralKind::Bool(ts_kind == "true"),
            },
            "none" => CpgNodeKind::Literal {
                kind: LiteralKind::Null,
            },
            "list" => CpgNodeKind::Literal {
                kind: LiteralKind::Array,
            },
            "dictionary" => CpgNodeKind::Literal {
                kind: LiteralKind::Object,
            },

            // Other
            "import_statement" | "import_from_statement" => CpgNodeKind::Import {
                path: Arc::from(self.node_text(node, source)),
            },
            "decorator" => CpgNodeKind::Attribute {
                name: self.extract_decorator_name(node, source),
            },
            "comment" => CpgNodeKind::Comment {
                is_doc: false,
            },
            "expression_statement" if self.is_docstring(node, source) => CpgNodeKind::Comment {
                is_doc: true,
            },
            "ERROR" => CpgNodeKind::Error {
                message: Arc::from("Parse error"),
            },

            _ => CpgNodeKind::Unknown {
                kind: Arc::from(ts_kind),
            },
        }
    }

    fn map_javascript(&self, ts_kind: &str, node: &tree_sitter::Node, source: &str) -> CpgNodeKind {
        match ts_kind {
            // Structural
            "program" => CpgNodeKind::Root,
            "class_declaration" | "class" => CpgNodeKind::Class {
                name: self.extract_child_text(node, "name", source),
                is_abstract: false,
            },

            // Functions
            "function_declaration" | "function" | "generator_function_declaration" => {
                CpgNodeKind::Function {
                    signature: self.extract_js_function_signature(node, source),
                }
            }
            "method_definition" => CpgNodeKind::Function {
                signature: self.extract_js_function_signature(node, source),
            },
            "arrow_function" => CpgNodeKind::Lambda {
                captures: SmallVec::new(),
            },
            "formal_parameters" | "required_parameter" | "optional_parameter" => {
                CpgNodeKind::Parameter {
                    name: self.extract_child_text(node, "pattern", source),
                    param_type: None,
                    is_variadic: false,
                }
            }
            "statement_block" => CpgNodeKind::Block {
                scope: ScopeId::GLOBAL,
            },

            // Variables
            "variable_declaration" | "lexical_declaration" => CpgNodeKind::Variable {
                name: Arc::from(""),
                var_type: None,
                scope: ScopeId::GLOBAL,
                is_mutable: !self.node_text(node, source).starts_with("const"),
            },
            "variable_declarator" => CpgNodeKind::Variable {
                name: self.extract_child_text(node, "name", source),
                var_type: None,
                scope: ScopeId::GLOBAL,
                is_mutable: true,
            },
            "public_field_definition" | "field_definition" => CpgNodeKind::Field {
                name: self.extract_child_text(node, "property", source),
                field_type: None,
                visibility: Visibility::Public,
            },

            // Control flow
            "if_statement" => CpgNodeKind::If,
            "else_clause" => CpgNodeKind::Else,
            "while_statement" => CpgNodeKind::While,
            "for_statement" | "for_in_statement" | "for_of_statement" => CpgNodeKind::For,
            "do_statement" => CpgNodeKind::Loop,
            "switch_statement" => CpgNodeKind::Match,
            "switch_case" | "switch_default" => CpgNodeKind::MatchArm,
            "return_statement" => CpgNodeKind::Return,
            "break_statement" => CpgNodeKind::Break,
            "continue_statement" => CpgNodeKind::Continue,
            "try_statement" => CpgNodeKind::Try,
            "catch_clause" => CpgNodeKind::Catch,
            "finally_clause" => CpgNodeKind::Finally,
            "throw_statement" => CpgNodeKind::Throw,

            // Expressions
            "binary_expression" => CpgNodeKind::BinaryOp {
                operator: self.extract_operator(node, source),
            },
            "unary_expression" | "update_expression" => CpgNodeKind::UnaryOp {
                operator: self.extract_operator(node, source),
            },
            "assignment_expression" | "augmented_assignment_expression" => CpgNodeKind::Assignment {
                operator: self.extract_operator(node, source),
            },
            "call_expression" | "new_expression" => CpgNodeKind::Call {
                target: None,
                is_method: false,
            },
            "member_expression" => CpgNodeKind::MemberAccess {
                member: self.extract_child_text(node, "property", source),
            },
            "subscript_expression" => CpgNodeKind::IndexAccess,
            "identifier" | "property_identifier" | "shorthand_property_identifier" => {
                CpgNodeKind::Identifier {
                    name: Arc::from(self.node_text(node, source)),
                    definition: None,
                }
            }
            "await_expression" => CpgNodeKind::Await,
            "yield_expression" => CpgNodeKind::Yield,

            // Literals
            "number" => CpgNodeKind::Literal {
                kind: if self.node_text(node, source).contains('.') {
                    self.parse_float(node, source)
                } else {
                    self.parse_integer(node, source)
                },
            },
            "string" | "template_string" => CpgNodeKind::Literal {
                kind: LiteralKind::String(Arc::from(self.node_text(node, source))),
            },
            "true" | "false" => CpgNodeKind::Literal {
                kind: LiteralKind::Bool(ts_kind == "true"),
            },
            "null" | "undefined" => CpgNodeKind::Literal {
                kind: LiteralKind::Null,
            },
            "array" => CpgNodeKind::Literal {
                kind: LiteralKind::Array,
            },
            "object" => CpgNodeKind::Literal {
                kind: LiteralKind::Object,
            },
            "regex" => CpgNodeKind::Literal {
                kind: LiteralKind::Regex(Arc::from(self.node_text(node, source))),
            },

            // Other
            "import_statement" | "import" => CpgNodeKind::Import {
                path: Arc::from(self.node_text(node, source)),
            },
            "export_statement" => CpgNodeKind::Import {
                path: Arc::from("export"),
            },
            "comment" => CpgNodeKind::Comment {
                is_doc: self.node_text(node, source).starts_with("/**"),
            },
            "ERROR" => CpgNodeKind::Error {
                message: Arc::from("Parse error"),
            },

            _ => CpgNodeKind::Unknown {
                kind: Arc::from(ts_kind),
            },
        }
    }

    fn map_go(&self, ts_kind: &str, node: &tree_sitter::Node, source: &str) -> CpgNodeKind {
        match ts_kind {
            "source_file" => CpgNodeKind::Root,
            "package_clause" => CpgNodeKind::Module {
                name: self.extract_child_text(node, "name", source),
            },
            "type_declaration" | "type_spec" => CpgNodeKind::Struct {
                name: self.extract_child_text(node, "name", source),
            },
            "interface_type" => CpgNodeKind::Trait {
                name: Arc::from(self.node_text(node, source)),
            },
            "function_declaration" | "method_declaration" => CpgNodeKind::Function {
                signature: self.extract_go_function_signature(node, source),
            },
            "func_literal" => CpgNodeKind::Lambda {
                captures: SmallVec::new(),
            },
            "parameter_declaration" => CpgNodeKind::Parameter {
                name: Arc::from(self.node_text(node, source)),
                param_type: None,
                is_variadic: self.has_child_kind(node, "variadic_parameter_declaration"),
            },
            "block" => CpgNodeKind::Block {
                scope: ScopeId::GLOBAL,
            },
            "short_var_declaration" | "var_declaration" => CpgNodeKind::Variable {
                name: Arc::from(""),
                var_type: None,
                scope: ScopeId::GLOBAL,
                is_mutable: true,
            },
            "const_declaration" => CpgNodeKind::Variable {
                name: Arc::from(""),
                var_type: None,
                scope: ScopeId::GLOBAL,
                is_mutable: false,
            },
            "if_statement" => CpgNodeKind::If,
            "for_statement" => CpgNodeKind::For,
            "switch_statement" | "type_switch_statement" => CpgNodeKind::Match,
            "expression_case" | "type_case" | "default_case" => CpgNodeKind::MatchArm,
            "return_statement" => CpgNodeKind::Return,
            "break_statement" => CpgNodeKind::Break,
            "continue_statement" => CpgNodeKind::Continue,
            "go_statement" => CpgNodeKind::Await, // Goroutine
            "defer_statement" => CpgNodeKind::Finally,
            "binary_expression" => CpgNodeKind::BinaryOp {
                operator: self.extract_operator(node, source),
            },
            "unary_expression" => CpgNodeKind::UnaryOp {
                operator: self.extract_operator(node, source),
            },
            "assignment_statement" => CpgNodeKind::Assignment {
                operator: self.extract_operator(node, source),
            },
            "call_expression" => CpgNodeKind::Call {
                target: None,
                is_method: false,
            },
            "selector_expression" => CpgNodeKind::MemberAccess {
                member: self.extract_child_text(node, "field", source),
            },
            "index_expression" => CpgNodeKind::IndexAccess,
            "identifier" | "field_identifier" | "package_identifier" | "type_identifier" => {
                CpgNodeKind::Identifier {
                    name: Arc::from(self.node_text(node, source)),
                    definition: None,
                }
            }
            "int_literal" => CpgNodeKind::Literal {
                kind: self.parse_integer(node, source),
            },
            "float_literal" => CpgNodeKind::Literal {
                kind: self.parse_float(node, source),
            },
            "interpreted_string_literal" | "raw_string_literal" => CpgNodeKind::Literal {
                kind: LiteralKind::String(Arc::from(self.node_text(node, source))),
            },
            "true" | "false" => CpgNodeKind::Literal {
                kind: LiteralKind::Bool(ts_kind == "true"),
            },
            "nil" => CpgNodeKind::Literal {
                kind: LiteralKind::Null,
            },
            "import_declaration" => CpgNodeKind::Import {
                path: Arc::from(self.node_text(node, source)),
            },
            "comment" => CpgNodeKind::Comment {
                is_doc: self.node_text(node, source).starts_with("//"),
            },
            "ERROR" => CpgNodeKind::Error {
                message: Arc::from("Parse error"),
            },
            _ => CpgNodeKind::Unknown {
                kind: Arc::from(ts_kind),
            },
        }
    }

    fn map_java(&self, ts_kind: &str, node: &tree_sitter::Node, source: &str) -> CpgNodeKind {
        match ts_kind {
            "program" => CpgNodeKind::Root,
            "package_declaration" => CpgNodeKind::Module {
                name: Arc::from(self.node_text(node, source)),
            },
            "class_declaration" => CpgNodeKind::Class {
                name: self.extract_child_text(node, "name", source),
                is_abstract: self.has_modifier(node, "abstract"),
            },
            "interface_declaration" => CpgNodeKind::Trait {
                name: self.extract_child_text(node, "name", source),
            },
            "enum_declaration" => CpgNodeKind::Enum {
                name: self.extract_child_text(node, "name", source),
            },
            "method_declaration" | "constructor_declaration" => CpgNodeKind::Function {
                signature: self.extract_java_function_signature(node, source),
            },
            "lambda_expression" => CpgNodeKind::Lambda {
                captures: SmallVec::new(),
            },
            "formal_parameter" | "spread_parameter" => CpgNodeKind::Parameter {
                name: self.extract_child_text(node, "name", source),
                param_type: None,
                is_variadic: ts_kind == "spread_parameter",
            },
            "block" => CpgNodeKind::Block {
                scope: ScopeId::GLOBAL,
            },
            "local_variable_declaration" | "variable_declarator" => CpgNodeKind::Variable {
                name: self.extract_child_text(node, "name", source),
                var_type: None,
                scope: ScopeId::GLOBAL,
                is_mutable: !self.has_modifier(node, "final"),
            },
            "field_declaration" => CpgNodeKind::Field {
                name: Arc::from(""),
                field_type: None,
                visibility: self.extract_java_visibility(node),
            },
            "if_statement" => CpgNodeKind::If,
            "while_statement" => CpgNodeKind::While,
            "for_statement" | "enhanced_for_statement" => CpgNodeKind::For,
            "do_statement" => CpgNodeKind::Loop,
            "switch_expression" => CpgNodeKind::Match,
            "switch_block_statement_group" | "switch_rule" => CpgNodeKind::MatchArm,
            "return_statement" => CpgNodeKind::Return,
            "break_statement" => CpgNodeKind::Break,
            "continue_statement" => CpgNodeKind::Continue,
            "try_statement" | "try_with_resources_statement" => CpgNodeKind::Try,
            "catch_clause" => CpgNodeKind::Catch,
            "finally_clause" => CpgNodeKind::Finally,
            "throw_statement" => CpgNodeKind::Throw,
            "binary_expression" => CpgNodeKind::BinaryOp {
                operator: self.extract_operator(node, source),
            },
            "unary_expression" | "update_expression" => CpgNodeKind::UnaryOp {
                operator: self.extract_operator(node, source),
            },
            "assignment_expression" => CpgNodeKind::Assignment {
                operator: self.extract_operator(node, source),
            },
            "method_invocation" | "object_creation_expression" => CpgNodeKind::Call {
                target: None,
                is_method: true,
            },
            "field_access" => CpgNodeKind::MemberAccess {
                member: self.extract_child_text(node, "field", source),
            },
            "array_access" => CpgNodeKind::IndexAccess,
            "identifier" | "type_identifier" => CpgNodeKind::Identifier {
                name: Arc::from(self.node_text(node, source)),
                definition: None,
            },
            "decimal_integer_literal" | "hex_integer_literal" | "octal_integer_literal" | "binary_integer_literal" => {
                CpgNodeKind::Literal {
                    kind: self.parse_integer(node, source),
                }
            }
            "decimal_floating_point_literal" | "hex_floating_point_literal" => CpgNodeKind::Literal {
                kind: self.parse_float(node, source),
            },
            "string_literal" => CpgNodeKind::Literal {
                kind: LiteralKind::String(Arc::from(self.node_text(node, source))),
            },
            "character_literal" => CpgNodeKind::Literal {
                kind: self.parse_char(node, source),
            },
            "true" | "false" => CpgNodeKind::Literal {
                kind: LiteralKind::Bool(ts_kind == "true"),
            },
            "null_literal" => CpgNodeKind::Literal {
                kind: LiteralKind::Null,
            },
            "array_initializer" => CpgNodeKind::Literal {
                kind: LiteralKind::Array,
            },
            "import_declaration" => CpgNodeKind::Import {
                path: Arc::from(self.node_text(node, source)),
            },
            "marker_annotation" | "annotation" => CpgNodeKind::Attribute {
                name: self.extract_child_text(node, "name", source),
            },
            "line_comment" | "block_comment" => CpgNodeKind::Comment {
                is_doc: self.node_text(node, source).starts_with("/**"),
            },
            "ERROR" => CpgNodeKind::Error {
                message: Arc::from("Parse error"),
            },
            _ => CpgNodeKind::Unknown {
                kind: Arc::from(ts_kind),
            },
        }
    }

    fn map_c_cpp(&self, ts_kind: &str, node: &tree_sitter::Node, source: &str) -> CpgNodeKind {
        match ts_kind {
            "translation_unit" => CpgNodeKind::Root,
            "namespace_definition" => CpgNodeKind::Module {
                name: self.extract_child_text(node, "name", source),
            },
            "class_specifier" => CpgNodeKind::Class {
                name: self.extract_child_text(node, "name", source),
                is_abstract: false,
            },
            "struct_specifier" => CpgNodeKind::Struct {
                name: self.extract_child_text(node, "name", source),
            },
            "enum_specifier" => CpgNodeKind::Enum {
                name: self.extract_child_text(node, "name", source),
            },
            "function_definition" | "function_declarator" => CpgNodeKind::Function {
                signature: self.extract_c_function_signature(node, source),
            },
            "lambda_expression" => CpgNodeKind::Lambda {
                captures: SmallVec::new(),
            },
            "parameter_declaration" => CpgNodeKind::Parameter {
                name: Arc::from(self.node_text(node, source)),
                param_type: None,
                is_variadic: false,
            },
            "compound_statement" => CpgNodeKind::Block {
                scope: ScopeId::GLOBAL,
            },
            "declaration" | "init_declarator" => CpgNodeKind::Variable {
                name: Arc::from(""),
                var_type: None,
                scope: ScopeId::GLOBAL,
                is_mutable: true,
            },
            "field_declaration" => CpgNodeKind::Field {
                name: Arc::from(""),
                field_type: None,
                visibility: Visibility::Public,
            },
            "if_statement" => CpgNodeKind::If,
            "else_clause" => CpgNodeKind::Else,
            "while_statement" => CpgNodeKind::While,
            "for_statement" | "for_range_loop" => CpgNodeKind::For,
            "do_statement" => CpgNodeKind::Loop,
            "switch_statement" => CpgNodeKind::Match,
            "case_statement" => CpgNodeKind::MatchArm,
            "return_statement" => CpgNodeKind::Return,
            "break_statement" => CpgNodeKind::Break,
            "continue_statement" => CpgNodeKind::Continue,
            "try_statement" => CpgNodeKind::Try,
            "catch_clause" => CpgNodeKind::Catch,
            "throw_statement" => CpgNodeKind::Throw,
            "binary_expression" => CpgNodeKind::BinaryOp {
                operator: self.extract_operator(node, source),
            },
            "unary_expression" | "update_expression" => CpgNodeKind::UnaryOp {
                operator: self.extract_operator(node, source),
            },
            "assignment_expression" => CpgNodeKind::Assignment {
                operator: self.extract_operator(node, source),
            },
            "call_expression" => CpgNodeKind::Call {
                target: None,
                is_method: false,
            },
            "field_expression" => CpgNodeKind::MemberAccess {
                member: self.extract_child_text(node, "field", source),
            },
            "subscript_expression" => CpgNodeKind::IndexAccess,
            "identifier" | "field_identifier" | "type_identifier" | "namespace_identifier" => {
                CpgNodeKind::Identifier {
                    name: Arc::from(self.node_text(node, source)),
                    definition: None,
                }
            }
            "number_literal" => CpgNodeKind::Literal {
                kind: if self.node_text(node, source).contains('.') {
                    self.parse_float(node, source)
                } else {
                    self.parse_integer(node, source)
                },
            },
            "string_literal" | "raw_string_literal" => CpgNodeKind::Literal {
                kind: LiteralKind::String(Arc::from(self.node_text(node, source))),
            },
            "char_literal" => CpgNodeKind::Literal {
                kind: self.parse_char(node, source),
            },
            "true" | "false" => CpgNodeKind::Literal {
                kind: LiteralKind::Bool(ts_kind == "true"),
            },
            "null" | "nullptr" => CpgNodeKind::Literal {
                kind: LiteralKind::Null,
            },
            "initializer_list" => CpgNodeKind::Literal {
                kind: LiteralKind::Array,
            },
            "preproc_include" => CpgNodeKind::Import {
                path: Arc::from(self.node_text(node, source)),
            },
            "comment" => CpgNodeKind::Comment {
                is_doc: self.node_text(node, source).starts_with("/**")
                    || self.node_text(node, source).starts_with("///"),
            },
            "ERROR" => CpgNodeKind::Error {
                message: Arc::from("Parse error"),
            },
            _ => CpgNodeKind::Unknown {
                kind: Arc::from(ts_kind),
            },
        }
    }

    fn map_ruby(&self, ts_kind: &str, node: &tree_sitter::Node, source: &str) -> CpgNodeKind {
        match ts_kind {
            "program" => CpgNodeKind::Root,
            "module" => CpgNodeKind::Module {
                name: self.extract_child_text(node, "name", source),
            },
            "class" => CpgNodeKind::Class {
                name: self.extract_child_text(node, "name", source),
                is_abstract: false,
            },
            "method" | "singleton_method" => CpgNodeKind::Function {
                signature: self.extract_ruby_function_signature(node, source),
            },
            "lambda" | "block" | "do_block" => CpgNodeKind::Lambda {
                captures: SmallVec::new(),
            },
            "method_parameters" | "block_parameters" => CpgNodeKind::Parameter {
                name: Arc::from(self.node_text(node, source)),
                param_type: None,
                is_variadic: false,
            },
            "body_statement" => CpgNodeKind::Block {
                scope: ScopeId::GLOBAL,
            },
            "assignment" => CpgNodeKind::Assignment {
                operator: Arc::from("="),
            },
            "if" | "unless" => CpgNodeKind::If,
            "elsif" | "else" => CpgNodeKind::Else,
            "while" | "until" => CpgNodeKind::While,
            "for" => CpgNodeKind::For,
            "case" => CpgNodeKind::Match,
            "when" => CpgNodeKind::MatchArm,
            "return" => CpgNodeKind::Return,
            "break" => CpgNodeKind::Break,
            "next" => CpgNodeKind::Continue,
            "begin" => CpgNodeKind::Try,
            "rescue" => CpgNodeKind::Catch,
            "ensure" => CpgNodeKind::Finally,
            "raise" => CpgNodeKind::Throw,
            "yield" => CpgNodeKind::Yield,
            "binary" => CpgNodeKind::BinaryOp {
                operator: self.extract_operator(node, source),
            },
            "unary" => CpgNodeKind::UnaryOp {
                operator: self.extract_operator(node, source),
            },
            "call" | "method_call" => CpgNodeKind::Call {
                target: None,
                is_method: true,
            },
            "element_reference" => CpgNodeKind::IndexAccess,
            "identifier" | "constant" | "instance_variable" | "class_variable" | "global_variable" => {
                CpgNodeKind::Identifier {
                    name: Arc::from(self.node_text(node, source)),
                    definition: None,
                }
            }
            "integer" => CpgNodeKind::Literal {
                kind: self.parse_integer(node, source),
            },
            "float" => CpgNodeKind::Literal {
                kind: self.parse_float(node, source),
            },
            "string" | "symbol" => CpgNodeKind::Literal {
                kind: LiteralKind::String(Arc::from(self.node_text(node, source))),
            },
            "true" | "false" => CpgNodeKind::Literal {
                kind: LiteralKind::Bool(ts_kind == "true"),
            },
            "nil" => CpgNodeKind::Literal {
                kind: LiteralKind::Null,
            },
            "array" => CpgNodeKind::Literal {
                kind: LiteralKind::Array,
            },
            "hash" => CpgNodeKind::Literal {
                kind: LiteralKind::Object,
            },
            "regex" => CpgNodeKind::Literal {
                kind: LiteralKind::Regex(Arc::from(self.node_text(node, source))),
            },
            "comment" => CpgNodeKind::Comment {
                is_doc: false,
            },
            "ERROR" => CpgNodeKind::Error {
                message: Arc::from("Parse error"),
            },
            _ => CpgNodeKind::Unknown {
                kind: Arc::from(ts_kind),
            },
        }
    }

    fn map_json(&self, ts_kind: &str, node: &tree_sitter::Node, source: &str) -> CpgNodeKind {
        match ts_kind {
            "document" => CpgNodeKind::Root,
            "object" => CpgNodeKind::Literal {
                kind: LiteralKind::Object,
            },
            "array" => CpgNodeKind::Literal {
                kind: LiteralKind::Array,
            },
            "pair" => CpgNodeKind::Field {
                name: Arc::from(""),
                field_type: None,
                visibility: Visibility::Public,
            },
            "string" => CpgNodeKind::Literal {
                kind: LiteralKind::String(Arc::from(self.node_text(node, source))),
            },
            "number" => CpgNodeKind::Literal {
                kind: if self.node_text(node, source).contains('.') {
                    self.parse_float(node, source)
                } else {
                    self.parse_integer(node, source)
                },
            },
            "true" | "false" => CpgNodeKind::Literal {
                kind: LiteralKind::Bool(ts_kind == "true"),
            },
            "null" => CpgNodeKind::Literal {
                kind: LiteralKind::Null,
            },
            "ERROR" => CpgNodeKind::Error {
                message: Arc::from("Parse error"),
            },
            _ => CpgNodeKind::Unknown {
                kind: Arc::from(ts_kind),
            },
        }
    }

    fn map_html(&self, ts_kind: &str, node: &tree_sitter::Node, source: &str) -> CpgNodeKind {
        match ts_kind {
            "document" | "fragment" => CpgNodeKind::Root,
            "element" | "self_closing_tag" => CpgNodeKind::Unknown {
                kind: Arc::from(self.extract_tag_name(node, source)),
            },
            "attribute" => CpgNodeKind::Attribute {
                name: self.extract_child_text(node, "name", source),
            },
            "text" => CpgNodeKind::Literal {
                kind: LiteralKind::String(Arc::from(self.node_text(node, source))),
            },
            "comment" => CpgNodeKind::Comment { is_doc: false },
            "ERROR" => CpgNodeKind::Error {
                message: Arc::from("Parse error"),
            },
            _ => CpgNodeKind::Unknown {
                kind: Arc::from(ts_kind),
            },
        }
    }

    fn map_css(&self, ts_kind: &str, node: &tree_sitter::Node, source: &str) -> CpgNodeKind {
        match ts_kind {
            "stylesheet" => CpgNodeKind::Root,
            "rule_set" => CpgNodeKind::Block {
                scope: ScopeId::GLOBAL,
            },
            "declaration" => CpgNodeKind::Variable {
                name: Arc::from(""),
                var_type: None,
                scope: ScopeId::GLOBAL,
                is_mutable: true,
            },
            "property_name" | "class_name" | "id_name" | "tag_name" => CpgNodeKind::Identifier {
                name: Arc::from(self.node_text(node, source)),
                definition: None,
            },
            "integer_value" | "float_value" => CpgNodeKind::Literal {
                kind: self.parse_float(node, source),
            },
            "string_value" => CpgNodeKind::Literal {
                kind: LiteralKind::String(Arc::from(self.node_text(node, source))),
            },
            "color_value" => CpgNodeKind::Literal {
                kind: LiteralKind::String(Arc::from(self.node_text(node, source))),
            },
            "import_statement" => CpgNodeKind::Import {
                path: Arc::from(self.node_text(node, source)),
            },
            "comment" => CpgNodeKind::Comment { is_doc: false },
            "ERROR" => CpgNodeKind::Error {
                message: Arc::from("Parse error"),
            },
            _ => CpgNodeKind::Unknown {
                kind: Arc::from(ts_kind),
            },
        }
    }

    fn map_bash(&self, ts_kind: &str, node: &tree_sitter::Node, source: &str) -> CpgNodeKind {
        match ts_kind {
            "program" => CpgNodeKind::Root,
            "function_definition" => CpgNodeKind::Function {
                signature: self.extract_bash_function_signature(node, source),
            },
            "compound_statement" => CpgNodeKind::Block {
                scope: ScopeId::GLOBAL,
            },
            "variable_assignment" => CpgNodeKind::Variable {
                name: Arc::from(""),
                var_type: None,
                scope: ScopeId::GLOBAL,
                is_mutable: true,
            },
            "if_statement" => CpgNodeKind::If,
            "elif_clause" | "else_clause" => CpgNodeKind::Else,
            "while_statement" | "until_statement" => CpgNodeKind::While,
            "for_statement" | "c_style_for_statement" => CpgNodeKind::For,
            "case_statement" => CpgNodeKind::Match,
            "case_item" => CpgNodeKind::MatchArm,
            "command" => CpgNodeKind::Call {
                target: None,
                is_method: false,
            },
            "command_name" | "variable_name" | "word" => CpgNodeKind::Identifier {
                name: Arc::from(self.node_text(node, source)),
                definition: None,
            },
            "raw_string" | "string" | "concatenation" => CpgNodeKind::Literal {
                kind: LiteralKind::String(Arc::from(self.node_text(node, source))),
            },
            "number" => CpgNodeKind::Literal {
                kind: self.parse_integer(node, source),
            },
            "comment" => CpgNodeKind::Comment { is_doc: false },
            "ERROR" => CpgNodeKind::Error {
                message: Arc::from("Parse error"),
            },
            _ => CpgNodeKind::Unknown {
                kind: Arc::from(ts_kind),
            },
        }
    }

    fn map_config(&self, ts_kind: &str, node: &tree_sitter::Node, source: &str) -> CpgNodeKind {
        match ts_kind {
            "document" | "stream" => CpgNodeKind::Root,
            "block_mapping" | "flow_mapping" | "table" | "inline_table" => CpgNodeKind::Literal {
                kind: LiteralKind::Object,
            },
            "block_sequence" | "flow_sequence" | "array" => CpgNodeKind::Literal {
                kind: LiteralKind::Array,
            },
            "block_mapping_pair" | "pair" => CpgNodeKind::Field {
                name: Arc::from(""),
                field_type: None,
                visibility: Visibility::Public,
            },
            "string_scalar" | "single_quote_scalar" | "double_quote_scalar" | "string" | "basic_string" | "literal_string" => {
                CpgNodeKind::Literal {
                    kind: LiteralKind::String(Arc::from(self.node_text(node, source))),
                }
            }
            "integer_scalar" | "integer" => CpgNodeKind::Literal {
                kind: self.parse_integer(node, source),
            },
            "float_scalar" | "float" => CpgNodeKind::Literal {
                kind: self.parse_float(node, source),
            },
            "boolean_scalar" | "boolean" => CpgNodeKind::Literal {
                kind: LiteralKind::Bool(
                    self.node_text(node, source).to_lowercase() == "true"
                        || self.node_text(node, source).to_lowercase() == "yes",
                ),
            },
            "null_scalar" => CpgNodeKind::Literal {
                kind: LiteralKind::Null,
            },
            "comment" => CpgNodeKind::Comment { is_doc: false },
            "ERROR" => CpgNodeKind::Error {
                message: Arc::from("Parse error"),
            },
            _ => CpgNodeKind::Unknown {
                kind: Arc::from(ts_kind),
            },
        }
    }

    fn map_markdown(&self, ts_kind: &str, node: &tree_sitter::Node, source: &str) -> CpgNodeKind {
        match ts_kind {
            "document" => CpgNodeKind::Root,
            "section" | "paragraph" | "heading" => CpgNodeKind::Block {
                scope: ScopeId::GLOBAL,
            },
            "code_span" | "fenced_code_block" | "indented_code_block" => CpgNodeKind::Literal {
                kind: LiteralKind::String(Arc::from(self.node_text(node, source))),
            },
            "link" | "image" => CpgNodeKind::Import {
                path: Arc::from(self.node_text(node, source)),
            },
            "html_block" => CpgNodeKind::Comment { is_doc: false },
            "ERROR" => CpgNodeKind::Error {
                message: Arc::from("Parse error"),
            },
            _ => CpgNodeKind::Unknown {
                kind: Arc::from(ts_kind),
            },
        }
    }

    fn map_generic(&self, ts_kind: &str, _node: &tree_sitter::Node, _source: &str) -> CpgNodeKind {
        CpgNodeKind::Unknown {
            kind: Arc::from(ts_kind),
        }
    }

    // ========== Rholang mapper (ρ-calculus → CPG) ==========
    //
    // Maps the `rholang-tree-sitter` grammar
    // (`rholang-rs/rholang-tree-sitter/grammar.js`) onto the imperative CPG
    // vocabulary. Process-calculus constructs have no dedicated CPG node kind;
    // each is normalized onto the nearest anchor that keeps the CFG/DFG *sound*
    // (never an edge the semantics forbid) even where it is *incomplete* (it may
    // omit an edge the semantics allow) — the standard multi-language-CPG
    // posture (Yamaguchi et al. 2014, "Modeling and Discovering Vulnerabilities
    // with Code Property Graphs", DOI:10.1109/SP.2014.44).
    //
    // Key soundness invariants this mapping upholds:
    //   * a named process abstraction (`contract`/agent decl) → `Function`, so
    //     the CFG builder (`cfg.rs`) seeds an entry and treats its trailing
    //     `block` child as the body;
    //   * a *bound* name (new-restriction, receive-bound, contract/agent formal,
    //     `let` binder) → `Variable`/`Parameter` (a DFG definition), while a
    //     *referenced* name → `Identifier` (a DFG use) — so def-use edges form
    //     (see `classify_rholang_var`);
    //   * a send / receive / method-call → `Call`, so the argument→parameter
    //     machinery can fire.
    #[cfg(feature = "rholang")]
    fn map_rholang(&self, ts_kind: &str, node: &tree_sitter::Node, source: &str) -> CpgNodeKind {
        match ts_kind {
            // --- Structural ---
            "source_file" => CpgNodeKind::Root,

            // Named process abstractions → Function (CFG entries). Their body
            // `block` is the LAST AST child, matching the CFG builder's
            // body = last-child rule (`cfg.rs:76-77`).
            "contract" => CpgNodeKind::Function {
                signature: self.rholang_signature(node, source, "contract"),
            },
            "constructor_decl" => CpgNodeKind::Function {
                signature: self.rholang_signature(node, source, "constructor"),
            },
            "method_decl" => CpgNodeKind::Function {
                signature: self.rholang_signature(node, source, "method"),
            },
            "default_decl" => CpgNodeKind::Function {
                signature: self.rholang_signature(node, source, "default"),
            },
            // Agent = object-like grouping of a constructor + methods.
            "agent_block" => CpgNodeKind::Class {
                name: self.extract_child_text(node, "name", source),
                is_abstract: false,
            },

            // --- Scopes / process blocks ---
            // `new … in {…}` (ν-restriction), `let … in {…}`, `for(…){…}`
            // (receive), explicit `{…}`, `|` parallel composition, `bundle`,
            // and send-continuations all introduce a process region → `Block`.
            // `par` keeps both operands as concurrent sibling sub-blocks: the
            // CFG is sound (both run) though it does not model interleaving.
            "new" | "let" | "input" | "block" | "par" | "bundle" | "sync_send_cont"
            | "non_empty_cont" | "empty_cont" => CpgNodeKind::Block {
                scope: ScopeId::GLOBAL,
            },

            // --- Control flow ---
            "ifElse" => CpgNodeKind::If,
            // `match` scrutinee-dispatch and `select` (non-deterministic choice)
            // both behave like a switch.
            "match" | "choice" => CpgNodeKind::Match,
            "case" | "branch" => CpgNodeKind::MatchArm,

            // --- Message passing → Call ---
            // `x!(v)` / `x!?(v)` send at the send site (the load-bearing Call).
            "send" | "send_sync" => CpgNodeKind::Call {
                target: None,
                is_method: false,
            },
            // `x!m(v)` method-style send / `recv.m(args)` method call.
            "send_method" | "method" | "send_method_source" => CpgNodeKind::Call {
                target: None,
                is_method: true,
            },
            // `names <- src` / `<= src` / `<<- src` receive: consumes on `src`.
            "linear_bind" | "repeated_bind" | "peek_bind" | "receive_send_source"
            | "send_receive_source" => CpgNodeKind::Call {
                target: None,
                is_method: false,
            },
            // Bare channel name in for-source position = use.
            "simple_source" => CpgNodeKind::Identifier {
                name: Arc::from(self.node_text(node, source)),
                definition: None,
            },

            // --- Declarations: the polyglot import anchor + binding scopes ---
            // A URI-bearing `new` decl (`new stdout(`rho:io:stdout`)`) is a
            // system-process import (the polyglot anchor); the bound `var` child
            // still becomes the `Variable` def (`classify_rholang_var`). A plain
            // decl is just a scope wrapper around that `Variable`.
            "name_decl" => match self.rholang_uri_child(node, source) {
                Some(uri) => CpgNodeKind::Import { path: uri },
                None => CpgNodeKind::Block {
                    scope: ScopeId::GLOBAL,
                },
            },
            "let_var_decl" | "decl" => CpgNodeKind::Block {
                scope: ScopeId::GLOBAL,
            },

            // --- Identifiers — the DFG-soundness core (def vs. use by position) ---
            "var" => self.classify_rholang_var(node, source),
            "wildcard" => CpgNodeKind::Identifier {
                name: Arc::from("_"),
                definition: None,
            },
            // `=x` / `=*x` pattern-variable reference = use.
            "var_ref" => CpgNodeKind::Identifier {
                name: Arc::from(self.node_text(node, source)),
                definition: None,
            },

            // --- Dereference / quote (unary) ---
            // `*x` (unquote): child `name` stays a DFG *use* (strictly more
            // informative for slicing than treating the whole thing as a Call).
            "eval" => CpgNodeKind::UnaryOp {
                operator: Arc::from("*"),
            },
            // `@P` reify process as a name.
            "quote" => CpgNodeKind::UnaryOp {
                operator: Arc::from("@"),
            },

            // --- Binary / unary operators ---
            "add" | "sub" | "mult" | "div" | "mod" | "concat" | "diff" | "interpolation" | "eq"
            | "neq" | "lt" | "lte" | "gt" | "gte" | "and" | "or" | "matches" | "disjunction"
            | "conjunction" => CpgNodeKind::BinaryOp {
                operator: Arc::from(self.rholang_operator_text(ts_kind)),
            },
            "not" | "neg" | "negation" => CpgNodeKind::UnaryOp {
                operator: Arc::from(self.rholang_operator_text(ts_kind)),
            },

            // --- Types ---
            "simple_type" => CpgNodeKind::TypeAnnotation {
                type_info: TypeInfo::new(self.node_text(node, source)),
            },

            // --- Literals ---
            "bool_literal" => CpgNodeKind::Literal {
                kind: LiteralKind::Bool(self.node_text(node, source) == "true"),
            },
            "signed_int_literal" | "unsigned_int_literal" | "bigint_literal" | "long_literal" => {
                CpgNodeKind::Literal {
                    kind: self.rholang_int_literal(node, source),
                }
            }
            "bigrat_literal" | "float_literal" | "fixed_point_literal" => CpgNodeKind::Literal {
                kind: self.rholang_float_literal(node, source),
            },
            "string_literal" => CpgNodeKind::Literal {
                kind: LiteralKind::String(Arc::from(self.node_text(node, source))),
            },
            // `` `rho:…` `` system URI — a polyglot anchor lifted by pgmcp's
            // binding classifier; here it is simply its string value.
            "uri_literal" => CpgNodeKind::Literal {
                kind: LiteralKind::String(Arc::from(self.node_text(node, source))),
            },
            // Inert process `Nil` and empty value `()`.
            "nil" | "unit" => CpgNodeKind::Literal {
                kind: LiteralKind::Null,
            },
            "list" | "set" | "tuple" | "collection" => CpgNodeKind::Literal {
                kind: LiteralKind::Array,
            },
            "map" | "pathmap" => CpgNodeKind::Literal {
                kind: LiteralKind::Object,
            },
            "key_value_pair" => CpgNodeKind::Field {
                name: Arc::from(""),
                field_type: None,
                visibility: Visibility::Public,
            },

            // --- Bundle capability markers ---
            "bundle_read" | "bundle_write" | "bundle_equiv" | "bundle_read_write" => {
                CpgNodeKind::Attribute {
                    name: Arc::from(ts_kind),
                }
            }

            // --- Comments / errors ---
            "line_comment" | "block_comment" => CpgNodeKind::Comment { is_doc: false },
            "ERROR" => CpgNodeKind::Error {
                message: Arc::from("Parse error"),
            },

            _ => CpgNodeKind::Unknown {
                kind: Arc::from(ts_kind),
            },
        }
    }

    /// Classifies a Rholang `var` (the sole lexical identifier token) as a DFG
    /// **definition** (`Variable`/`Parameter`) or a **use** (`Identifier`) by its
    /// syntactic position, walking `node.parent()` (parent pointers are set by
    /// the builder). This is what makes the Rholang DFG sound: a name bound in a
    /// `new`, a `let`, a contract/agent formal list, or a `for`-receipt is a
    /// definition; a name mentioned as a send channel, an argument, an operand,
    /// or an `*x` dereference target is a use.
    ///
    /// `name` is `inline` in the grammar (grammar.js:19), so in binder positions
    /// a `var`'s parent is the `names`/`name_decl`/`let_var_decl` node directly
    /// (no intervening `name`); a `@`-quoted pattern (`@msg`) interposes a
    /// `quote`, which this peeks through.
    #[cfg(feature = "rholang")]
    fn classify_rholang_var(&self, node: &tree_sitter::Node, source: &str) -> CpgNodeKind {
        let name: Arc<str> = Arc::from(self.node_text(node, source));
        let def_var = |name: Arc<str>| CpgNodeKind::Variable {
            name,
            var_type: None,
            scope: ScopeId::GLOBAL,
            is_mutable: false,
        };
        let use_ident = |name: Arc<str>| CpgNodeKind::Identifier {
            name,
            definition: None,
        };

        let Some(parent) = node.parent() else {
            return use_ident(name);
        };

        match parent.kind() {
            // `new x(…)` / `new x`: the new-restricted channel name is a def.
            "name_decl" => return def_var(name),
            // `let x = P`: the FIRST child of `let_var_decl` is the bound name.
            "let_var_decl" if parent.named_child(0).map(|c| c.id()) == Some(node.id()) => {
                return def_var(name)
            }
            _ => {}
        }

        // Formal parameter / receive-bound name: `var` sits directly under a
        // `names` list — possibly through a `@`-quote pattern — whose parent is a
        // binding construct.
        let container = if parent.kind() == "quote" {
            parent.parent()
        } else {
            Some(parent)
        };
        if let Some(c) = container {
            if c.kind() == "names" {
                if let Some(gp) = c.parent() {
                    if matches!(
                        gp.kind(),
                        "contract"
                            | "constructor_decl"
                            | "method_decl"
                            | "default_decl"
                            | "linear_bind"
                            | "repeated_bind"
                            | "peek_bind"
                            | "decl"
                    ) {
                        return CpgNodeKind::Parameter {
                            name,
                            param_type: None,
                            is_variadic: false,
                        };
                    }
                }
            }
        }

        use_ident(name)
    }

    /// Builds a `MethodSignature` for a Rholang process abstraction. The name is
    /// the `name` field's text (a `contract`'s name / a `method`'s name) or the
    /// supplied default (`constructor`/`default`, which have no name field).
    #[cfg(feature = "rholang")]
    fn rholang_signature(
        &self,
        node: &tree_sitter::Node,
        source: &str,
        default_name: &str,
    ) -> MethodSignature {
        let name = node
            .child_by_field_name("name")
            .map(|n| Arc::from(self.node_text(&n, source)))
            .unwrap_or_else(|| Arc::from(default_name));
        MethodSignature {
            name,
            params: SmallVec::new(),
            return_type: None,
            is_static: false,
            is_async: false,
            visibility: Visibility::Public,
        }
    }

    /// The URI string of a `name_decl`'s `uri_literal` child, backticks
    /// stripped (`` `rho:io:stdout` `` → `rho:io:stdout`), or `None` for a
    /// plain (non-URI) declaration.
    #[cfg(feature = "rholang")]
    fn rholang_uri_child(&self, node: &tree_sitter::Node, source: &str) -> Option<Arc<str>> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "uri_literal" {
                return Some(Arc::from(self.node_text(&child, source).trim_matches('`')));
            }
        }
        None
    }

    /// The glyph for a Rholang operator rule kind.
    #[cfg(feature = "rholang")]
    fn rholang_operator_text(&self, kind: &str) -> &'static str {
        match kind {
            "add" => "+",
            "sub" | "neg" => "-",
            "mult" => "*",
            "div" => "/",
            "mod" => "%",
            "concat" => "++",
            "diff" => "--",
            "interpolation" => "%%",
            "eq" => "==",
            "neq" => "!=",
            "lt" => "<",
            "lte" => "<=",
            "gt" => ">",
            "gte" => ">=",
            "and" => "and",
            "or" => "or",
            "matches" => "matches",
            "disjunction" => "\\/",
            "conjunction" => "/\\",
            "not" => "not",
            "negation" => "~",
            _ => "?",
        }
    }

    /// Parses a Rholang integer literal, tolerating the grammar's type suffixes
    /// (`5i32`, `7u8`, `9n`) by keeping the leading `-?\d+` — the literal's
    /// *value* is irrelevant to CFG/DFG structure, only its `Integer` kind is.
    #[cfg(feature = "rholang")]
    fn rholang_int_literal(&self, node: &tree_sitter::Node, source: &str) -> LiteralKind {
        let text = self.node_text(node, source);
        let mut digits = String::new();
        for (i, c) in text.chars().enumerate() {
            if c.is_ascii_digit() || (i == 0 && c == '-') {
                digits.push(c);
            } else {
                break;
            }
        }
        LiteralKind::Integer(digits.parse().unwrap_or(0))
    }

    /// Parses a Rholang float/bigrat/fixed-point literal, tolerating the
    /// grammar's type suffixes (`1.5f64`, `3r`, `2.0p10`) by keeping the leading
    /// numeric run.
    #[cfg(feature = "rholang")]
    fn rholang_float_literal(&self, node: &tree_sitter::Node, source: &str) -> LiteralKind {
        let text = self.node_text(node, source);
        let mut num = String::new();
        for (i, c) in text.chars().enumerate() {
            if c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+') || (i == 0 && c == '-') {
                num.push(c);
            } else {
                break;
            }
        }
        LiteralKind::Float(num.parse().unwrap_or(0.0))
    }

    // ========== MeTTa mapper (S-expression head-atom dispatch → CPG) ==========
    //
    // MeTTa (`MeTTa-Compiler/tree-sitter-metta/grammar.js`) is a minimal
    // S-expression grammar: `expression` and `atom_expression` wrap *every*
    // node, and a rule `(= (f $x) body)` is not a distinct node but a `list`
    // whose head atom (an `operator`) carries the semantics. The wrappers are
    // flattened by `should_include`; `map_metta` navigates the *raw* tree (which
    // still contains them) via `metta_unwrap` to reach each head/operand.
    #[cfg(feature = "metta")]
    fn map_metta(&self, ts_kind: &str, node: &tree_sitter::Node, source: &str) -> CpgNodeKind {
        match ts_kind {
            "source_file" => CpgNodeKind::Root,
            // The semantic core: a compound form dispatches on its head atom.
            "list" => self.map_metta_list(node, source),

            // --- Atoms ---
            "identifier" => CpgNodeKind::Identifier {
                name: Arc::from(self.node_text(node, source)),
                definition: None,
            },
            // `$x`: a use by default, promoted to `Parameter` in a rule-LHS
            // binder position (DFG soundness; see `classify_metta_var`).
            "variable" => self.classify_metta_var(node, source),
            "wildcard" => CpgNodeKind::Identifier {
                name: Arc::from("_"),
                definition: None,
            },
            // `&self` atom-space handle / `%Undefined%` type marker = use atoms.
            "space_reference" | "special_type_symbol" => CpgNodeKind::Identifier {
                name: Arc::from(self.node_text(node, source)),
                definition: None,
            },

            "boolean_literal" => CpgNodeKind::Literal {
                kind: LiteralKind::Bool(self.node_text(node, source) == "True"),
            },
            "integer_literal" => CpgNodeKind::Literal {
                kind: self.parse_integer(node, source),
            },
            "float_literal" => CpgNodeKind::Literal {
                kind: self.parse_float(node, source),
            },
            "string_literal" => CpgNodeKind::Literal {
                kind: LiteralKind::String(Arc::from(self.node_text(node, source))),
            },

            // Operator glyphs as *standalone* leaves (a list head is consumed by
            // `map_metta_list` and never reaches here). Kept out of the DFG
            // use-set as `Unknown`.
            "operator" | "arrow_operator" | "comparison_operator" | "assignment_operator"
            | "type_annotation_operator" | "rule_definition_operator" | "punctuation_operator"
            | "arithmetic_operator" | "logic_operator" | "exclaim_prefix" | "question_prefix"
            | "quote_prefix" => CpgNodeKind::Unknown {
                kind: Arc::from(ts_kind),
            },

            "line_comment" => CpgNodeKind::Comment { is_doc: false },
            "ERROR" => CpgNodeKind::Error {
                message: Arc::from("Parse error"),
            },

            _ => CpgNodeKind::Unknown {
                kind: Arc::from(ts_kind),
            },
        }
    }

    /// Head-dispatch for a MeTTa `list` — the semantic core of the mapper. The
    /// head is the first named child, unwrapped through the
    /// `expression → atom_expression → operator` layers.
    #[cfg(feature = "metta")]
    fn map_metta_list(&self, node: &tree_sitter::Node, source: &str) -> CpgNodeKind {
        let Some(head) = node.named_child(0).map(|h| self.metta_unwrap(h)) else {
            // Empty list `()` = unit.
            return CpgNodeKind::Literal {
                kind: LiteralKind::Null,
            };
        };
        match head.kind() {
            // `(= LHS RHS)` / `(:= LHS RHS)` rule definition → Function (CFG
            // entry); body = RHS = last AST child.
            "assignment_operator" | "rule_definition_operator" => CpgNodeKind::Function {
                signature: MethodSignature {
                    name: self.metta_rule_name(node, source),
                    params: SmallVec::new(),
                    return_type: None,
                    is_static: false,
                    is_async: false,
                    visibility: Visibility::Public,
                },
            },
            // `(: name Type)` annotation / `(-> A B R)` function-type.
            "type_annotation_operator" | "arrow_operator" => CpgNodeKind::TypeAnnotation {
                type_info: TypeInfo::new(self.node_text(node, source)),
            },
            // A user/built-in identifier head: a handful of built-ins have
            // control/structural meaning; everything else is a function
            // application / grounded call.
            "identifier" => match self.node_text(&head, source) {
                "if" => CpgNodeKind::If,
                "case" | "match" => CpgNodeKind::Match,
                "let" | "let*" => CpgNodeKind::Block {
                    scope: ScopeId::GLOBAL,
                },
                "import!" => CpgNodeKind::Import {
                    path: self.metta_import_path(node, source),
                },
                _ => CpgNodeKind::Call {
                    target: None,
                    is_method: false,
                },
            },
            // Grounded-operator application (`(+ $a $b)`, `(== $x 1)`),
            // application against a space (`(&self …)`), higher-order/computed
            // head (`($f …)` / nested `((…) …)`) → Call.
            _ => CpgNodeKind::Call {
                target: None,
                is_method: false,
            },
        }
    }

    /// Descends through the MeTTa wrapper nodes (`expression`, `atom_expression`,
    /// `operator`) that the grammar interposes on every atom, returning the
    /// innermost meaningful node (an `identifier`, `variable`, `list`, a specific
    /// operator kind, a literal, …).
    #[cfg(feature = "metta")]
    fn metta_unwrap<'tree>(&self, node: tree_sitter::Node<'tree>) -> tree_sitter::Node<'tree> {
        let mut n = node;
        while matches!(n.kind(), "expression" | "atom_expression" | "operator") {
            match n.named_child(0) {
                Some(child) => n = child,
                None => break,
            }
        }
        n
    }

    /// The name of a MeTTa rule `list` `(= LHS RHS)`: if `LHS` is itself a
    /// `list` (`(= (foo $x) …)`) its head `identifier` is the name; if `LHS` is a
    /// bare atom (`(:= bar …)`) that atom's text is the name. Best-effort — an
    /// empty name is harmless because a CPG node anchors by source range.
    #[cfg(feature = "metta")]
    fn metta_rule_name(&self, list_node: &tree_sitter::Node, source: &str) -> Arc<str> {
        let Some(lhs) = list_node.named_child(1).map(|l| self.metta_unwrap(l)) else {
            return Arc::from("");
        };
        if lhs.kind() == "list" {
            match lhs.named_child(0).map(|h| self.metta_unwrap(h)) {
                Some(head) => Arc::from(self.node_text(&head, source)),
                None => Arc::from(""),
            }
        } else {
            Arc::from(self.node_text(&lhs, source))
        }
    }

    /// The imported module/file of a MeTTa `(import! &space module)` — the last
    /// named child, unwrapped.
    #[cfg(feature = "metta")]
    fn metta_import_path(&self, list_node: &tree_sitter::Node, source: &str) -> Arc<str> {
        let count = list_node.named_child_count();
        if count == 0 {
            return Arc::from("");
        }
        // tree-sitter 0.26 changed `named_child`'s index parameter from `usize`
        // to `u32` (while `named_child_count` still returns `usize`); `count >= 1`
        // is guaranteed by the guard above, so `count - 1` cannot underflow.
        match list_node.named_child((count - 1) as u32).map(|c| self.metta_unwrap(c)) {
            Some(module) => Arc::from(self.node_text(&module, source)),
            None => Arc::from(""),
        }
    }

    /// Classifies a MeTTa `$variable` as a DFG **definition** (`Parameter`) when
    /// it is a rule-LHS binder — `$x` in `(= (foo $x) …)` — or a **use**
    /// (`Identifier`) everywhere else (RHS occurrences, non-rule contexts). The
    /// LHS shape is `outer_list(=|:=) → … → lhs_list → … → variable`; the check
    /// walks the raw parent chain (`variable → atom_expression → expression →
    /// lhs_list → expression → outer_list`) and confirms (a) the outer head is
    /// `=`/`:=` and (b) the enclosing list is the LHS (second) operand, not the
    /// RHS. With it, `(= (double $x) (* $x 2))` gets a `$x` def→use edge.
    #[cfg(feature = "metta")]
    fn classify_metta_var(&self, node: &tree_sitter::Node, source: &str) -> CpgNodeKind {
        let name: Arc<str> = Arc::from(self.node_text(node, source));
        let is_lhs_binder = (|| {
            let lhs = node.parent()?.parent()?.parent()?; // atom_expression → expression → lhs_list
            if lhs.kind() != "list" {
                return Some(false);
            }
            let outer = lhs.parent()?.parent()?; // expression → outer_list
            if outer.kind() != "list" {
                return Some(false);
            }
            let head = self.metta_unwrap(outer.named_child(0)?);
            let head_is_rule =
                matches!(head.kind(), "assignment_operator" | "rule_definition_operator");
            let lhs_is_second = outer
                .named_child(1)
                .map(|c| self.metta_unwrap(c).id())
                == Some(lhs.id());
            Some(head_is_rule && lhs_is_second)
        })()
        .unwrap_or(false);

        if is_lhs_binder {
            CpgNodeKind::Parameter {
                name,
                param_type: None,
                is_variadic: false,
            }
        } else {
            CpgNodeKind::Identifier {
                name,
                definition: None,
            }
        }
    }

    // ========== Helper methods ==========

    fn node_text<'a>(&self, node: &tree_sitter::Node, source: &'a str) -> &'a str {
        &source[node.start_byte()..node.end_byte()]
    }

    fn extract_child_text(&self, node: &tree_sitter::Node, field: &str, source: &str) -> Arc<str> {
        node.child_by_field_name(field)
            .map(|n| Arc::from(self.node_text(&n, source)))
            .unwrap_or_else(|| Arc::from(""))
    }

    fn has_child_kind(&self, node: &tree_sitter::Node, kind: &str) -> bool {
        let mut cursor = node.walk();
        let result = node.children(&mut cursor).any(|c| c.kind() == kind);
        result
    }

    fn has_modifier(&self, node: &tree_sitter::Node, modifier: &str) -> bool {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "modifiers" {
                let mut inner_cursor = child.walk();
                for m in child.children(&mut inner_cursor) {
                    let text = &m.kind();
                    if *text == modifier {
                        return true;
                    }
                }
            }
        }
        false
    }

    fn extract_operator(&self, node: &tree_sitter::Node, source: &str) -> Arc<str> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            let kind = child.kind();
            if kind.len() <= 3
                && (kind.contains('+')
                    || kind.contains('-')
                    || kind.contains('*')
                    || kind.contains('/')
                    || kind.contains('%')
                    || kind.contains('=')
                    || kind.contains('<')
                    || kind.contains('>')
                    || kind.contains('&')
                    || kind.contains('|')
                    || kind.contains('^')
                    || kind.contains('!')
                    || kind == "and"
                    || kind == "or"
                    || kind == "not")
            {
                return Arc::from(kind);
            }
            if kind == "operator" {
                return Arc::from(self.node_text(&child, source));
            }
        }
        Arc::from("?")
    }

    fn extract_pattern_name(&self, node: &tree_sitter::Node, source: &str) -> Arc<str> {
        // For let declarations, the pattern might be an identifier or a destructuring pattern
        if let Some(pattern) = node.child_by_field_name("pattern") {
            if pattern.kind() == "identifier" {
                return Arc::from(self.node_text(&pattern, source));
            }
        }
        if let Some(name) = node.child_by_field_name("name") {
            return Arc::from(self.node_text(&name, source));
        }
        Arc::from("")
    }

    fn extract_type_from_node(&self, node: &tree_sitter::Node, source: &str) -> Option<TypeInfo> {
        node.child_by_field_name("type")
            .map(|t| TypeInfo::new(self.node_text(&t, source)))
    }

    fn parse_integer(&self, node: &tree_sitter::Node, source: &str) -> LiteralKind {
        let text = self.node_text(node, source);
        let cleaned = text.replace('_', "");
        let value = if cleaned.starts_with("0x") || cleaned.starts_with("0X") {
            i64::from_str_radix(&cleaned[2..], 16).unwrap_or(0)
        } else if cleaned.starts_with("0b") || cleaned.starts_with("0B") {
            i64::from_str_radix(&cleaned[2..], 2).unwrap_or(0)
        } else if cleaned.starts_with("0o") || cleaned.starts_with("0O") {
            i64::from_str_radix(&cleaned[2..], 8).unwrap_or(0)
        } else {
            cleaned.parse().unwrap_or(0)
        };
        LiteralKind::Integer(value)
    }

    fn parse_float(&self, node: &tree_sitter::Node, source: &str) -> LiteralKind {
        let text = self.node_text(node, source).replace('_', "");
        LiteralKind::Float(text.parse().unwrap_or(0.0))
    }

    fn parse_char(&self, node: &tree_sitter::Node, source: &str) -> LiteralKind {
        let text = self.node_text(node, source);
        let ch = text
            .trim_matches('\'')
            .chars()
            .next()
            .unwrap_or('\0');
        LiteralKind::Char(ch)
    }

    fn extract_rust_visibility(&self, node: &tree_sitter::Node) -> Visibility {
        if self.has_child_kind(node, "visibility_modifier") {
            Visibility::Public
        } else {
            Visibility::Private
        }
    }

    fn extract_java_visibility(&self, node: &tree_sitter::Node) -> Visibility {
        if self.has_modifier(node, "public") {
            Visibility::Public
        } else if self.has_modifier(node, "protected") {
            Visibility::Protected
        } else if self.has_modifier(node, "private") {
            Visibility::Private
        } else {
            Visibility::Package
        }
    }

    fn extract_impl_type(&self, node: &tree_sitter::Node, source: &str) -> Option<Arc<str>> {
        node.child_by_field_name("type")
            .map(|t| Arc::from(self.node_text(&t, source)))
    }

    fn extract_impl_trait(&self, node: &tree_sitter::Node, source: &str) -> Option<Arc<str>> {
        node.child_by_field_name("trait")
            .map(|t| Arc::from(self.node_text(&t, source)))
    }

    fn extract_use_path(&self, node: &tree_sitter::Node, source: &str) -> String {
        // Collect the use path from tree-sitter node
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "use_tree" || child.kind() == "scoped_identifier" {
                return self.node_text(&child, source).to_string();
            }
        }
        self.node_text(node, source).to_string()
    }

    fn extract_attribute_name(&self, node: &tree_sitter::Node, source: &str) -> Arc<str> {
        // Try to find the attribute path/name
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "meta_item" || child.kind() == "attribute" {
                if let Some(path) = child.child_by_field_name("path") {
                    return Arc::from(self.node_text(&path, source));
                }
                return Arc::from(self.node_text(&child, source));
            }
        }
        Arc::from(self.node_text(node, source))
    }

    fn extract_decorator_name(&self, node: &tree_sitter::Node, source: &str) -> Arc<str> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "identifier" || child.kind() == "attribute" {
                return Arc::from(self.node_text(&child, source));
            }
        }
        Arc::from(self.node_text(node, source))
    }

    fn extract_tag_name(&self, node: &tree_sitter::Node, source: &str) -> String {
        if let Some(start_tag) = node.child_by_field_name("start_tag") {
            if let Some(tag_name) = start_tag.child(0) {
                return format!("html:{}", self.node_text(&tag_name, source));
            }
        }
        format!("html:{}", self.node_text(node, source))
    }

    fn is_docstring(&self, node: &tree_sitter::Node, source: &str) -> bool {
        if let Some(child) = node.child(0) {
            let text = self.node_text(&child, source);
            return text.starts_with("\"\"\"") || text.starts_with("'''");
        }
        false
    }

    fn extract_rust_function_signature(&self, node: &tree_sitter::Node, source: &str) -> MethodSignature {
        let name = self.extract_child_text(node, "name", source);
        let is_async = self.has_child_kind(node, "async");
        let visibility = self.extract_rust_visibility(node);

        MethodSignature {
            name,
            params: SmallVec::new(),
            return_type: node.child_by_field_name("return_type")
                .map(|rt| TypeInfo::new(self.node_text(&rt, source))),
            is_static: false,
            is_async,
            visibility,
        }
    }

    fn extract_python_function_signature(&self, node: &tree_sitter::Node, source: &str) -> MethodSignature {
        let name = self.extract_child_text(node, "name", source);
        let is_async = self.has_child_kind(node, "async");

        MethodSignature {
            name,
            params: SmallVec::new(),
            return_type: None,
            is_static: false,
            is_async,
            visibility: Visibility::Public,
        }
    }

    fn extract_js_function_signature(&self, node: &tree_sitter::Node, source: &str) -> MethodSignature {
        let name = self.extract_child_text(node, "name", source);
        let is_async = self.has_child_kind(node, "async");
        let is_static = self.has_child_kind(node, "static");

        MethodSignature {
            name,
            params: SmallVec::new(),
            return_type: None,
            is_static,
            is_async,
            visibility: Visibility::Public,
        }
    }

    fn extract_go_function_signature(&self, node: &tree_sitter::Node, source: &str) -> MethodSignature {
        let name = self.extract_child_text(node, "name", source);

        MethodSignature {
            name,
            params: SmallVec::new(),
            return_type: None,
            is_static: false,
            is_async: false,
            visibility: Visibility::Public,
        }
    }

    fn extract_java_function_signature(&self, node: &tree_sitter::Node, source: &str) -> MethodSignature {
        let name = self.extract_child_text(node, "name", source);
        let is_static = self.has_modifier(node, "static");
        let visibility = self.extract_java_visibility(node);

        MethodSignature {
            name,
            params: SmallVec::new(),
            return_type: node.child_by_field_name("type")
                .map(|t| TypeInfo::new(self.node_text(&t, source))),
            is_static,
            is_async: false,
            visibility,
        }
    }

    fn extract_c_function_signature(&self, node: &tree_sitter::Node, source: &str) -> MethodSignature {
        let name = node.child_by_field_name("declarator")
            .and_then(|d| d.child_by_field_name("declarator"))
            .map(|n| Arc::from(self.node_text(&n, source)))
            .unwrap_or_else(|| Arc::from(""));

        MethodSignature {
            name,
            params: SmallVec::new(),
            return_type: node.child_by_field_name("type")
                .map(|t| TypeInfo::new(self.node_text(&t, source))),
            is_static: self.has_child_kind(node, "storage_class_specifier"),
            is_async: false,
            visibility: Visibility::Public,
        }
    }

    fn extract_ruby_function_signature(&self, node: &tree_sitter::Node, source: &str) -> MethodSignature {
        let name = self.extract_child_text(node, "name", source);

        MethodSignature {
            name,
            params: SmallVec::new(),
            return_type: None,
            is_static: false,
            is_async: false,
            visibility: Visibility::Public,
        }
    }

    fn extract_bash_function_signature(&self, node: &tree_sitter::Node, source: &str) -> MethodSignature {
        let name = self.extract_child_text(node, "name", source);

        MethodSignature {
            name,
            params: SmallVec::new(),
            return_type: None,
            is_static: false,
            is_async: false,
            visibility: Visibility::Public,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mapper_creation() {
        let mapper = NodeMapper::new(Language::Rust);
        assert_eq!(mapper.language(), Language::Rust);
    }

    #[test]
    fn test_should_include() {
        let mapper = NodeMapper::new(Language::Rust);

        // Punctuation should be excluded
        assert!(!mapper.should_include("(", false));
        assert!(!mapper.should_include(")", false));
        assert!(!mapper.should_include("{", false));
        assert!(!mapper.should_include(",", false));

        // Comments excluded by default
        assert!(!mapper.should_include("line_comment", false));
        assert!(!mapper.should_include("block_comment", false));

        // Comments included when enabled
        assert!(mapper.should_include("line_comment", true));
        assert!(mapper.should_include("block_comment", true));

        // Regular nodes should be included
        assert!(mapper.should_include("function_item", false));
        assert!(mapper.should_include("identifier", false));
    }

    // ========================================================================
    // Rholang / MeTTa (Mode B): parse a snippet with the real grammar (a
    // TEST-ONLY path dev-dependency, never propagated to downstream crates) and
    // drive `build_from_tree` end-to-end, asserting the resulting CPG. These
    // are the behavioral tests the no-vendoring feature design (§7/§8) could
    // not place in libcpg until it was recognized that dev-dependencies solve
    // the duplicate-C-symbol hazard without leaking to consumers.
    // ========================================================================

    #[cfg(any(feature = "rholang", feature = "metta"))]
    fn kinds(cpg: &crate::CodePropertyGraph) -> Vec<crate::CpgNodeKind> {
        cpg.nodes().map(|n| n.kind.clone()).collect()
    }

    /// True iff the DFG contains a `DefUse` edge whose def side and use side are
    /// both a binder/reference of `name` — i.e. the reaching-defs pass linked a
    /// `Variable`/`Parameter` definition of `name` to an `Identifier` use of it.
    #[cfg(any(feature = "rholang", feature = "metta"))]
    fn defuse_by_name(cpg: &crate::CodePropertyGraph, name: &str) -> bool {
        use crate::{CpgEdgeKind, DfgEdgeKind};
        let name_of = |id: crate::NodeId| {
            cpg.node(id).and_then(|n| match &n.kind {
                CpgNodeKind::Variable { name, .. }
                | CpgNodeKind::Parameter { name, .. }
                | CpgNodeKind::Identifier { name, .. } => Some(name.clone()),
                _ => None,
            })
        };
        cpg.edges().any(|e| {
            matches!(e.kind, CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse))
                && name_of(e.source).as_deref() == Some(name)
                && name_of(e.target).as_deref() == Some(name)
        })
    }

    #[cfg(feature = "rholang")]
    fn build_rholang(source: &str) -> crate::CodePropertyGraph {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_rholang::LANGUAGE.into())
            .expect("set rholang grammar");
        let tree = parser.parse(source, None).expect("parse rholang");
        crate::TreeSitterCpgBuilder::new()
            .build_from_tree(&tree, source, Language::Rholang)
            .expect("build_from_tree rholang")
    }

    /// The load-bearing gate: a `.rho` `contract` becomes a `Function` (named
    /// from the contract) that seeds a CFG entry, and the `x!(…)` send becomes a
    /// `Call` at the send site. Without the `Function`, a `.rho` CPG has no CFG.
    #[test]
    #[cfg(feature = "rholang")]
    fn rholang_contract_is_function_with_cfg_and_send_call() {
        let src = "new stdout(`rho:io:stdout`) in {\n  \
                   contract @\"greet\"(@name) = {\n    \
                   stdout!(\"hello, \" ++ *name)\n  }\n}\n";
        let cpg = build_rholang(src);

        let funcs: Vec<_> = cpg.functions().collect();
        assert_eq!(funcs.len(), 1, "the contract must map to exactly one Function");
        match &funcs[0].kind {
            CpgNodeKind::Function { signature } => {
                assert!(
                    signature.name.contains("greet"),
                    "signature name should carry the contract name, got {:?}",
                    signature.name
                );
            }
            other => panic!("expected Function, got {other:?}"),
        }
        assert!(
            !cpg.cfg_entries().is_empty(),
            "a contract-as-Function must seed a CFG entry (Mode B → CFG for .rho)"
        );
        let send_calls = kinds(&cpg)
            .into_iter()
            .filter(|k| matches!(k, CpgNodeKind::Call { is_method: false, .. }))
            .count();
        assert!(send_calls >= 1, "the `stdout!(…)` send must be a Call");
    }

    /// A URI-bearing `new` declaration yields the `Import` polyglot anchor (URI
    /// backticks stripped) and the bound channel becomes a `Variable` def.
    #[test]
    #[cfg(feature = "rholang")]
    fn rholang_new_decl_yields_uri_import_and_channel_variable() {
        let src = "new stdout(`rho:io:stdout`) in { stdout!(42) }\n";
        let cpg = build_rholang(src);

        let import = cpg.nodes().find_map(|n| match &n.kind {
            CpgNodeKind::Import { path } => Some(path.clone()),
            _ => None,
        });
        assert_eq!(
            import.as_deref(),
            Some("rho:io:stdout"),
            "the `rho:` URI decl must be an Import anchor with the stripped URI"
        );
        assert!(
            cpg.nodes().any(|n| matches!(&n.kind,
                CpgNodeKind::Variable { name, .. } if &**name == "stdout")),
            "the new-bound channel `stdout` must be a Variable def"
        );
    }

    /// A `for(@msg <- c){…}` receive: the received name is a `Parameter` def, the
    /// receive is a `Call`, and the new-bound channel `c` is a `Variable`.
    #[test]
    #[cfg(feature = "rholang")]
    fn rholang_for_receive_binds_parameter() {
        let src = "new c in { for (@msg <- c) { c!(*msg) } }\n";
        let cpg = build_rholang(src);

        assert!(
            cpg.nodes().any(|n| matches!(&n.kind,
                CpgNodeKind::Parameter { name, .. } if &**name == "msg")),
            "the received `@msg` must be a Parameter def"
        );
        assert!(
            cpg.nodes().any(|n| matches!(&n.kind, CpgNodeKind::Call { .. })),
            "the receive must be a Call"
        );
        assert!(
            cpg.nodes().any(|n| matches!(&n.kind,
                CpgNodeKind::Variable { name, .. } if &**name == "c")),
            "the new-bound channel `c` must be a Variable def"
        );
    }

    /// DFG soundness: inside a contract (a Function, so the DFG runs), the
    /// new-bound channel `c` (a `Variable` def) links to its use as the send
    /// channel (`Identifier`) via a `DefUse` edge.
    #[test]
    #[cfg(feature = "rholang")]
    fn rholang_channel_def_links_to_send_use() {
        let src = "contract @\"main\"() = { new c in { c!(1) } }\n";
        let cpg = build_rholang(src);
        assert!(
            defuse_by_name(&cpg, "c"),
            "channel `c` def (new) must reach its use (send channel) in the DFG"
        );
    }

    /// The `should_include` collision fix: a Rholang *rule* node named
    /// `"contract"`/`"match"` is kept, while a same-named anonymous *keyword
    /// token* is dropped by the node-aware `should_include_node`. Grouping
    /// containers (`names`) are dropped; semantic sends (`send`) are kept.
    #[test]
    #[cfg(feature = "rholang")]
    fn rholang_should_include_keyword_vs_rule_collision() {
        let mapper = NodeMapper::new(Language::Rholang);
        // String-keyed view: rule names kept, containers/markers dropped.
        assert!(mapper.should_include("contract", false), "contract RULE kept");
        assert!(mapper.should_include("match", false), "match RULE kept");
        assert!(mapper.should_include("send", false), "send kept");
        assert!(!mapper.should_include("names", false), "names container dropped");
        assert!(!mapper.should_include("send_single", false), "arity marker dropped");

        // Node-aware view against a real tree: the anonymous `contract` keyword
        // token (kind() == "contract", is_named() == false) is dropped, but the
        // `contract` rule node (is_named() == true) is kept — the exact
        // collision the string-only test cannot resolve.
        let src = "contract @\"m\"() = { Nil }\n";
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_rholang::LANGUAGE.into())
            .expect("set rholang grammar");
        let tree = parser.parse(src, None).expect("parse rholang");
        let mut cursor = tree.root_node().walk();
        let contract_rule = tree
            .root_node()
            .children(&mut cursor)
            .find(|n| n.kind() == "contract" && n.is_named())
            .expect("contract rule node");
        assert!(
            mapper.should_include_node(&contract_rule, false),
            "the contract RULE node must be kept"
        );
        let mut inner = contract_rule.walk();
        let contract_kw = contract_rule
            .children(&mut inner)
            .find(|n| n.kind() == "contract" && !n.is_named())
            .expect("contract keyword token");
        assert!(
            !mapper.should_include_node(&contract_kw, false),
            "the anonymous contract KEYWORD token must be dropped"
        );
    }

    #[cfg(feature = "metta")]
    fn build_metta(source: &str) -> crate::CodePropertyGraph {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_metta::LANGUAGE.into())
            .expect("set metta grammar");
        let tree = parser.parse(source, None).expect("parse metta");
        crate::TreeSitterCpgBuilder::new()
            .build_from_tree(&tree, source, Language::MeTTa)
            .expect("build_from_tree metta")
    }

    /// The load-bearing gate: a MeTTa rule `(= (double $x) (* $x 2))` becomes a
    /// `Function` named from the rule-head atom, seeding a CFG entry; and the
    /// LHS `$x` (a `Parameter` def) links to the RHS `$x` use via a `DefUse`
    /// edge (the recommended rule-LHS binder refinement).
    #[test]
    #[cfg(feature = "metta")]
    fn metta_rule_is_function_with_cfg_and_param_defuse() {
        let cpg = build_metta("(= (double $x) (* $x 2))\n");

        let funcs: Vec<_> = cpg.functions().collect();
        assert_eq!(funcs.len(), 1, "the `(= …)` rule must map to exactly one Function");
        match &funcs[0].kind {
            CpgNodeKind::Function { signature } => {
                assert_eq!(&*signature.name, "double", "rule-head atom is the name");
            }
            other => panic!("expected Function, got {other:?}"),
        }
        assert!(
            !cpg.cfg_entries().is_empty(),
            "a rule-as-Function must seed a CFG entry (Mode B → CFG for .metta)"
        );
        assert!(
            cpg.nodes().any(|n| matches!(&n.kind,
                CpgNodeKind::Parameter { name, .. } if &**name == "$x")),
            "the rule-LHS `$x` must be a Parameter def"
        );
        assert!(
            defuse_by_name(&cpg, "$x"),
            "the LHS `$x` def must reach the RHS `$x` use in the DFG"
        );
    }

    /// Head-dispatch of the non-rule forms: `(:` → `TypeAnnotation`,
    /// `(import! …)` → `Import`, a grounded `(+ …)` → `Call`.
    #[test]
    #[cfg(feature = "metta")]
    fn metta_head_dispatch_type_import_call() {
        let ta = build_metta("(: double (-> Number Number))\n");
        assert!(
            ta.nodes().any(|n| matches!(&n.kind, CpgNodeKind::TypeAnnotation { .. })),
            "`(: name Type)` must map to a TypeAnnotation"
        );

        let imp = build_metta("(import! &self math)\n");
        let path = imp.nodes().find_map(|n| match &n.kind {
            CpgNodeKind::Import { path } => Some(path.clone()),
            _ => None,
        });
        assert_eq!(path.as_deref(), Some("math"), "`import!` must map to an Import");

        let call = build_metta("(+ 1 2)\n");
        assert!(
            call.nodes().any(|n| matches!(&n.kind, CpgNodeKind::Call { .. })),
            "a grounded `(+ …)` operation must map to a Call"
        );
        // The grounded-op operands and a bare atom are still in the graph.
        assert!(
            kinds(&call)
                .iter()
                .any(|k| matches!(k, CpgNodeKind::Literal { .. })),
            "operands survive as Literals"
        );
    }

    /// `should_include` flattens the MeTTa transparent wrappers so a `list`'s
    /// head/operands are direct AST children, but keeps `list` and the atoms.
    #[test]
    #[cfg(feature = "metta")]
    fn metta_should_include_flattens_wrappers() {
        let mapper = NodeMapper::new(Language::MeTTa);
        assert!(!mapper.should_include("expression", false));
        assert!(!mapper.should_include("atom_expression", false));
        assert!(!mapper.should_include("prefixed_expression", false));
        assert!(mapper.should_include("list", false));
        assert!(mapper.should_include("identifier", false));
        assert!(mapper.should_include("variable", false));
    }
}
