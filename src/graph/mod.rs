//! Core graph types for Code Property Graphs.
//!
//! This module provides the fundamental data structures for representing
//! code as a unified graph combining AST, CFG, and DFG information.

mod cpg;
mod node;
mod edge;
mod language;

#[cfg(feature = "serde")]
pub(crate) mod serde_util;

pub use cpg::{CodePropertyGraph, CpgStats};
pub use node::{
    CpgNode, CpgNodeKind, NodeId, SourceRange, PropertyKey, PropertyValue,
    TypeInfo, MethodSignature, Visibility, ScopeId, LiteralKind,
};
pub use edge::{CpgEdge, CpgEdgeKind, EdgeId, CfgEdgeKind, DfgEdgeKind};
pub use language::{Language, Paradigm};
