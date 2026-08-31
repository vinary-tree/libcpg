//! Compiles the repository-contained snapshot of the canonical MeTTa grammar C
//! sources through `tree-sitter-language`. See `SOURCE.md` for provenance and
//! reproducible integrity checks.

use std::path::PathBuf;

fn main() {
    let src_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");

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
