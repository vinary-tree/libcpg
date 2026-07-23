//! Recompiles the canonical MeTTa grammar C sources (parser.c + the stub
//! external scanner.c) from the MeTTa-Compiler tree-sitter-metta crate, WITHOUT
//! duplicating them, so this shim exposes the exact same grammar as MeTTaTron
//! but through `tree-sitter-language` (version-agnostic) rather than a pinned
//! `tree-sitter` crate. See Cargo.toml for the full rationale.

use std::path::PathBuf;

fn main() {
    // Canonical grammar sources — the single source of truth for the MeTTa
    // grammar. Kept as an absolute path (consistent with how libcpg's
    // dev-dependency previously referenced this crate) so we recompile, never
    // copy, the upstream parser.
    let src_dir = PathBuf::from(
        "/home/dylon/Workspace/f1r3fly.io/MeTTa-Compiler/tree-sitter-metta/src",
    );

    let parser_path = src_dir.join("parser.c");
    let scanner_path = src_dir.join("scanner.c");

    let mut c_config = cc::Build::new();
    c_config.include(&src_dir);
    c_config
        .flag_if_supported("-Wno-unused-parameter")
        .flag_if_supported("-Wno-unused-but-set-variable")
        .flag_if_supported("-Wno-trigraphs");

    c_config.file(&parser_path);
    if scanner_path.exists() {
        c_config.file(&scanner_path);
    }

    c_config.compile("tree-sitter-metta");

    println!("cargo:rerun-if-changed={}", parser_path.display());
    println!("cargo:rerun-if-changed={}", scanner_path.display());
}
