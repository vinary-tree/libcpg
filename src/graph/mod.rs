//! Core graph types for Code Property Graphs.
//!
//! This module provides the fundamental data structures for representing
//! code as a unified graph combining AST, CFG, and DFG information.

mod cpg;
mod edge;
mod language;
mod node;
mod property_value;

#[cfg(feature = "serde")]
pub(crate) mod serde_util;

pub use cpg::{CodePropertyGraph, CpgStats};
pub use edge::{CfgEdgeKind, CpgEdge, CpgEdgeKind, DfgEdgeKind, EdgeId};
pub use language::{Language, Paradigm};
pub use node::{
    CpgNode, CpgNodeKind, LiteralKind, LockKind, LockMode, MethodSignature, MsgKind, NodeId,
    PropertyKey, ScopeId, SourceRange, TypeInfo, Visibility,
};
pub use property_value::{
    PropertyList, PropertyListIter, PropertyValue, PropertyValueError, PropertyValueKind,
    PropertyValueLimit, PropertyValueLimits, PropertyValueMetrics, PropertyValueRef,
};
