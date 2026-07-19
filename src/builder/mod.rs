//! CPG Builder infrastructure.
//!
//! This module defines traits and implementations for constructing
//! Code Property Graphs from source code.
//!
//! ## Components
//!
//! - [`TreeSitterCpgBuilder`]: Builds CPGs from source code using tree-sitter parsing
//! - [`CfgExtractor`]: Extracts control flow graph edges from AST
//! - [`DfgExtractor`]: Extracts data flow graph edges using reaching definitions analysis
//! - [`BasicBlockIdentifier`]: Identifies basic blocks in the CFG
//!
//! ## Usage
//!
//! ```ignore
//! use libcpg::builder::{CpgBuilder, TreeSitterCpgBuilder, CfgExtractor, DfgExtractor};
//! use libcpg::Language;
//!
//! // Build CPG from source
//! let builder = TreeSitterCpgBuilder::new();
//! let mut cpg = builder.build(source_code, Language::Rust)?;
//!
//! // Extract CFG edges
//! let cfg_extractor = CfgExtractor::new();
//! cfg_extractor.extract(&mut cpg);
//!
//! // Extract DFG edges
//! let dfg_extractor = DfgExtractor::new();
//! dfg_extractor.extract(&mut cpg);
//! ```

mod cfg;
mod dfg;
mod node_mapper;
mod parser_registry;
mod pdg;
mod tree_sitter;

pub use cfg::{CfgExtractor, CfgExtractorConfig, BasicBlockIdentifier};
pub use dfg::{DfgExtractor, DfgExtractorConfig, DefUseChain, Definition, DefinitionKind, Use, UseKind, build_def_use_chains};
pub use node_mapper::NodeMapper;
pub use parser_registry::ParserRegistry;
pub use pdg::{PdgBuilder, backward_slice, forward_slice};
pub use tree_sitter::TreeSitterCpgBuilder;

use crate::{CodePropertyGraph, Language, Result};

/// Trait for building Code Property Graphs from source code.
///
/// Implementations of this trait parse source code and construct
/// a unified CPG containing AST, CFG, and DFG information.
pub trait CpgBuilder: Send + Sync {
    /// Builds a CPG from source code.
    ///
    /// # Arguments
    /// * `source` - The source code to parse
    /// * `language` - The programming language of the source
    ///
    /// # Returns
    /// A constructed `CodePropertyGraph` or an error
    fn build(&self, source: &str, language: Language) -> Result<CodePropertyGraph>;

    /// Builds a CPG from a file.
    ///
    /// The language is inferred from the file extension.
    fn build_file(&self, path: &std::path::Path) -> Result<CodePropertyGraph> {
        let source = std::fs::read_to_string(path)?;
        let language = path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(Language::from_extension)
            .unwrap_or(Language::Unknown);

        let mut cpg = self.build(&source, language)?;

        // Set source path
        if let Some(path_str) = path.to_str() {
            cpg = cpg.with_source_path(path_str);
        }

        Ok(cpg)
    }

    /// Returns the languages supported by this builder.
    fn supported_languages(&self) -> &[Language];

    /// Returns true if the given language is supported.
    fn supports_language(&self, language: Language) -> bool {
        self.supported_languages().contains(&language)
    }
}

/// Configuration options for CPG construction.
#[derive(Debug, Clone)]
pub struct CpgBuilderConfig {
    /// Whether to retain source code in the CPG.
    pub retain_source: bool,
    /// Whether to build the control flow graph.
    pub build_cfg: bool,
    /// Whether to build the data flow graph.
    pub build_dfg: bool,
    /// Whether to include comments in the AST.
    pub include_comments: bool,
    /// Maximum file size to process (in bytes).
    pub max_file_size: usize,
    /// Whether to resolve cross-file references.
    pub resolve_imports: bool,
}

impl Default for CpgBuilderConfig {
    fn default() -> Self {
        Self {
            retain_source: false,
            build_cfg: true,
            build_dfg: true,
            include_comments: false,
            max_file_size: 10 * 1024 * 1024, // 10MB
            resolve_imports: false,
        }
    }
}

impl CpgBuilderConfig {
    /// Creates a new configuration with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets whether to retain source code.
    pub fn with_source(mut self, retain: bool) -> Self {
        self.retain_source = retain;
        self
    }

    /// Sets whether to build CFG.
    pub fn with_cfg(mut self, build: bool) -> Self {
        self.build_cfg = build;
        self
    }

    /// Sets whether to build DFG.
    pub fn with_dfg(mut self, build: bool) -> Self {
        self.build_dfg = build;
        self
    }

    /// Sets whether to include comments.
    pub fn with_comments(mut self, include: bool) -> Self {
        self.include_comments = include;
        self
    }

    /// Sets the maximum file size.
    pub fn with_max_file_size(mut self, size: usize) -> Self {
        self.max_file_size = size;
        self
    }

    /// Sets whether to resolve imports.
    pub fn with_import_resolution(mut self, resolve: bool) -> Self {
        self.resolve_imports = resolve;
        self
    }
}
