//! # libcpg - Code Property Graph Library
//!
//! A Rust library for constructing and analyzing Code Property Graphs (CPGs),
//! which combine Abstract Syntax Trees (AST), Control Flow Graphs (CFG), and
//! Data Flow Graphs (DFG) into a unified representation for program analysis.
//!
//! ## Features
//!
//! - **Graph Construction**: Build CPGs from source code via tree-sitter ASTs
//! - **Pattern Detection**: Detect design patterns using subgraph isomorphism (VF2/VF3)
//! - **Algorithm Analysis**: Extract algorithm signatures and estimate complexity
//! - **GNN Support**: Graph neural network message passing for semantic embeddings
//! - **Parallel Processing** (planned): `rayon` is a declared dependency
//!   reserved for future multi-threaded analysis; the current extractors and
//!   detectors run sequentially (no `par_iter` is wired yet)
//!
//! ## Example
//!
//! ```rust,ignore
//! use libcpg::{CodePropertyGraph, CpgBuilder, Language};
//!
//! // Build a CPG from source code
//! let builder = TreeSitterCpgBuilder::new();
//! let cpg = builder.build(source_code, Language::Rust)?;
//!
//! // Analyze control flow
//! for node in cpg.cfg_nodes() {
//!     println!("CFG node: {:?}", node);
//! }
//!
//! // Detect design patterns
//! let detector = GofPatternDetector::new();
//! let patterns = detector.detect(&cpg);
//! ```

#![warn(missing_docs)]
#![warn(clippy::all)]
#![allow(clippy::module_inception)]

pub mod graph;
pub mod builder;

#[cfg(feature = "gnn")]
pub mod gnn;

pub mod pattern;

#[cfg(feature = "algorithm-detection")]
pub mod algorithms;

#[cfg(feature = "design-patterns")]
pub mod patterns;

/// Shared proptest strategies and helpers for the crate's own tests.
#[cfg(test)]
pub(crate) mod testutil;

// Re-export commonly used types
pub use graph::{
    CodePropertyGraph,
    CpgStats,
    CpgNode,
    CpgNodeKind,
    CpgEdge,
    CpgEdgeKind,
    CfgEdgeKind,
    DfgEdgeKind,
    NodeId,
    EdgeId,
    SourceRange,
    Language,
    Paradigm,
    TypeInfo,
    MethodSignature,
    Visibility,
    ScopeId,
    PropertyKey,
    PropertyValue,
    LiteralKind,
};

pub use builder::{
    CpgBuilder,
    CpgBuilderConfig,
    TreeSitterCpgBuilder,
    CfgExtractor,
    CfgExtractorConfig,
    BasicBlockIdentifier,
    DfgExtractor,
    DfgExtractorConfig,
    DefUseChain,
    Definition,
    DefinitionKind,
    Use,
    UseKind,
    build_def_use_chains,
    PdgBuilder,
    backward_slice,
    forward_slice,
};

#[cfg(feature = "gnn")]
pub use gnn::GraphNeuralNetwork;

pub use pattern::{
    PatternMatch,
    SubgraphMatcher,
};

/// Result type alias for libcpg operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Error types for libcpg operations.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Error during graph construction.
    #[error("Graph construction error: {0}")]
    Construction(String),

    /// Error during pattern matching.
    #[error("Pattern matching error: {0}")]
    PatternMatch(String),

    /// Error during GNN operations.
    #[error("GNN error: {0}")]
    #[cfg(feature = "gnn")]
    Gnn(String),

    /// Invalid node ID.
    #[error("Invalid node ID: {0:?}")]
    InvalidNodeId(NodeId),

    /// Invalid edge ID.
    #[error("Invalid edge ID: {0:?}")]
    InvalidEdgeId(EdgeId),

    /// Unsupported language.
    #[error("Unsupported language: {0}")]
    UnsupportedLanguage(String),

    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Serialization error.
    #[cfg(feature = "serde")]
    #[error("Serialization error: {0}")]
    Serialization(String),
}
