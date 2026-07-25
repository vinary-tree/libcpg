//! Version-agnostic shim of the MeTTa tree-sitter grammar.
//!
//! Exposes the canonical MeTTa grammar (whose C sources live in
//! `MeTTa-Compiler/tree-sitter-metta/src`, recompiled by this crate's
//! `build.rs`) as a [`tree_sitter_language::LanguageFn`], mirroring how
//! `rholang-tree-sitter` publishes its grammar. Using `LanguageFn` decouples
//! the grammar from any specific `tree-sitter` crate version, so a consumer can
//! obtain a `tree_sitter::Language` via `LANGUAGE.into()` regardless of whether
//! it links tree-sitter 0.25, 0.26, or later.
//!
//! Consumers (all by path; see `Cargo.toml` for the full rationale and the
//! condition under which this shim should be dropped):
//!
//! | Crate | Kind | Uses |
//! |---|---|---|
//! | `libcpg` | dev-dependency | [`LANGUAGE`] |
//! | `pgmcp` | dependency | [`LANGUAGE`] |
//! | `libgrammstein` | dependency | [`LANGUAGE`] |
//! | `libgeiststein` | dependency | [`LANGUAGE`], [`node_types`] |

use tree_sitter_language::LanguageFn;

extern "C" {
    fn tree_sitter_metta() -> *const ();
}

/// The MeTTa tree-sitter grammar as a version-agnostic language function.
///
/// Convert to a `tree_sitter::Language` at the call site with `LANGUAGE.into()`.
pub const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_metta) };

/// Node type names exposed by the grammar.
///
/// Mirrors the canonical crate's `node_types` module verbatim so a consumer can
/// switch to this shim by changing only its `language()` call. Two constants —
/// [`node_types::BRACE_LIST`] and [`node_types::BLOCK_COMMENT`] — name nodes the
/// current grammar does **not** produce (MeTTa has no brace-list form, and its
/// only comment form is the `;` line comment); they are retained for API parity
/// with the canonical crate, and `node_types_match_grammar` below pins that
/// asymmetry so it cannot drift silently in either direction.
pub mod node_types {
    pub const EXPRESSION: &str = "expression";
    pub const LIST: &str = "list";
    pub const BRACE_LIST: &str = "brace_list";
    pub const PREFIXED_EXPRESSION: &str = "prefixed_expression";
    pub const ATOM_EXPRESSION: &str = "atom_expression";

    // Semantic atom types
    pub const VARIABLE: &str = "variable";
    pub const WILDCARD: &str = "wildcard";
    pub const IDENTIFIER: &str = "identifier";
    pub const STRING_LITERAL: &str = "string_literal";
    pub const FLOAT_LITERAL: &str = "float_literal";
    pub const INTEGER_LITERAL: &str = "integer_literal";
    pub const BOOLEAN_LITERAL: &str = "boolean_literal";

    // Operator types
    pub const OPERATOR: &str = "operator";
    pub const ARROW_OPERATOR: &str = "arrow_operator";
    pub const COMPARISON_OPERATOR: &str = "comparison_operator";
    pub const ASSIGNMENT_OPERATOR: &str = "assignment_operator";
    pub const TYPE_ANNOTATION_OPERATOR: &str = "type_annotation_operator";
    pub const RULE_DEFINITION_OPERATOR: &str = "rule_definition_operator";
    pub const PUNCTUATION_OPERATOR: &str = "punctuation_operator";
    pub const ARITHMETIC_OPERATOR: &str = "arithmetic_operator";
    pub const LOGIC_OPERATOR: &str = "logic_operator";

    // Prefix types
    pub const EXCLAIM_PREFIX: &str = "exclaim_prefix";
    pub const QUESTION_PREFIX: &str = "question_prefix";
    pub const QUOTE_PREFIX: &str = "quote_prefix";

    // Comments
    pub const LINE_COMMENT: &str = "line_comment";
    pub const BLOCK_COMMENT: &str = "block_comment";
}

#[cfg(test)]
mod tests {
    use super::node_types as nt;

    /// Node kinds the canonical grammar is expected to emit.
    const PRESENT: &[&str] = &[
        nt::EXPRESSION,
        nt::LIST,
        nt::PREFIXED_EXPRESSION,
        nt::ATOM_EXPRESSION,
        nt::VARIABLE,
        nt::WILDCARD,
        nt::IDENTIFIER,
        nt::STRING_LITERAL,
        nt::FLOAT_LITERAL,
        nt::INTEGER_LITERAL,
        nt::BOOLEAN_LITERAL,
        nt::OPERATOR,
        nt::ARROW_OPERATOR,
        nt::COMPARISON_OPERATOR,
        nt::ASSIGNMENT_OPERATOR,
        nt::TYPE_ANNOTATION_OPERATOR,
        nt::RULE_DEFINITION_OPERATOR,
        nt::PUNCTUATION_OPERATOR,
        nt::ARITHMETIC_OPERATOR,
        nt::LOGIC_OPERATOR,
        nt::EXCLAIM_PREFIX,
        nt::QUESTION_PREFIX,
        nt::QUOTE_PREFIX,
        nt::LINE_COMMENT,
    ];

    /// Constants kept for API parity with the canonical crate that the current
    /// grammar does not define. If upstream adds either node, this test fails
    /// and the module docs must be updated to move it into `PRESENT`.
    const ABSENT: &[&str] = &[nt::BRACE_LIST, nt::BLOCK_COMMENT];

    fn kind_exists(language: &tree_sitter::Language, kind: &str) -> bool {
        language.id_for_node_kind(kind, true) != 0 || language.id_for_node_kind(kind, false) != 0
    }

    #[test]
    fn language_loads() {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&super::LANGUAGE.into())
            .expect("load MeTTa grammar");
        let tree = parser.parse("(= (double $x) (* 2 $x))\n", None).expect("parse");
        assert!(!tree.root_node().has_error());
    }

    #[test]
    fn node_types_match_grammar() {
        let language: tree_sitter::Language = super::LANGUAGE.into();
        let missing: Vec<&str> = PRESENT
            .iter()
            .copied()
            .filter(|kind| !kind_exists(&language, kind))
            .collect();
        assert!(
            missing.is_empty(),
            "node_types constants absent from the canonical grammar: {missing:?}"
        );
        let unexpected: Vec<&str> = ABSENT
            .iter()
            .copied()
            .filter(|kind| kind_exists(&language, kind))
            .collect();
        assert!(
            unexpected.is_empty(),
            "grammar gained nodes documented as absent; update the docs: {unexpected:?}"
        );
    }
}
