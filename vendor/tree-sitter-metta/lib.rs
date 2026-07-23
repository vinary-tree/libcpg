//! Version-agnostic, dev-only shim of the MeTTa tree-sitter grammar.
//!
//! Exposes the canonical MeTTa grammar (whose C sources live in
//! `MeTTa-Compiler/tree-sitter-metta/src`, recompiled by this crate's
//! `build.rs`) as a [`tree_sitter_language::LanguageFn`], mirroring how
//! `rholang-tree-sitter` publishes its grammar. Using `LanguageFn` decouples
//! the grammar from any specific `tree-sitter` crate version, so a consumer can
//! obtain a `tree_sitter::Language` via `LANGUAGE.into()` regardless of whether
//! it links tree-sitter 0.25, 0.26, or later.
//!
//! See `Cargo.toml` in this directory for the full rationale and the condition
//! under which this shim should be dropped.

use tree_sitter_language::LanguageFn;

extern "C" {
    fn tree_sitter_metta() -> *const ();
}

/// The MeTTa tree-sitter grammar as a version-agnostic language function.
///
/// Convert to a `tree_sitter::Language` at the call site with `LANGUAGE.into()`.
pub const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_metta) };
