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
            _ => self.map_generic(ts_kind, node, source),
        }
    }

    /// Returns true if this node type should be included in the CPG.
    ///
    /// Some tree-sitter nodes are purely syntactic (punctuation, etc.)
    /// and don't contribute to the semantic structure.
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
}
