//! CPG edge types for AST, CFG, and DFG relationships.

use super::NodeId;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Unique identifier for an edge in the CPG.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct EdgeId(pub u32);

impl EdgeId {
    /// Creates a new edge ID.
    #[inline]
    pub fn new(id: u32) -> Self {
        Self(id)
    }

    /// Returns the raw ID value.
    #[inline]
    pub fn as_u32(self) -> u32 {
        self.0
    }
}

impl From<u32> for EdgeId {
    fn from(id: u32) -> Self {
        Self(id)
    }
}

/// Control flow edge kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum CfgEdgeKind {
    /// Sequential control flow (fallthrough).
    Sequential,
    /// Conditional true branch.
    ConditionalTrue,
    /// Conditional false branch.
    ConditionalFalse,
    /// Loop back edge.
    LoopBack,
    /// Loop exit edge.
    LoopExit,
    /// Break edge (exits loop).
    Break,
    /// Continue edge (jumps to loop head).
    Continue,
    /// Return edge (to function exit).
    Return,
    /// Exception throw edge.
    Throw,
    /// Exception catch edge.
    Catch,
    /// Call edge (into a function).
    Call,
    /// Return from call edge.
    CallReturn,
    /// Match/switch case edge.
    Case,
    /// Default case edge.
    DefaultCase,
}

impl CfgEdgeKind {
    /// Returns true if this is a conditional edge.
    pub fn is_conditional(&self) -> bool {
        matches!(
            self,
            Self::ConditionalTrue | Self::ConditionalFalse | Self::Case | Self::DefaultCase
        )
    }

    /// Returns true if this is a loop-related edge.
    pub fn is_loop(&self) -> bool {
        matches!(self, Self::LoopBack | Self::LoopExit | Self::Break | Self::Continue)
    }

    /// Returns true if this is an exception-related edge.
    pub fn is_exception(&self) -> bool {
        matches!(self, Self::Throw | Self::Catch)
    }
}

/// Data flow edge kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum DfgEdgeKind {
    /// Definition to use (def-use chain).
    DefUse,
    /// Use to definition (use-def chain).
    UseDef,
    /// Reaching definition.
    ReachingDef,
    /// Data dependency.
    DataDependency,
    /// Parameter passing.
    Parameter,
    /// Return value.
    ReturnValue,
    /// Field read.
    FieldRead,
    /// Field write.
    FieldWrite,
    /// Array/index read.
    IndexRead,
    /// Array/index write.
    IndexWrite,
    /// Alias relationship.
    Alias,
    /// Pointer dereference.
    Dereference,
    /// Address-of operation.
    AddressOf,
}

impl DfgEdgeKind {
    /// Returns true if this is a read operation.
    pub fn is_read(&self) -> bool {
        matches!(self, Self::DefUse | Self::FieldRead | Self::IndexRead | Self::Dereference)
    }

    /// Returns true if this is a write operation.
    pub fn is_write(&self) -> bool {
        matches!(self, Self::UseDef | Self::FieldWrite | Self::IndexWrite)
    }
}

/// Kind of CPG edge.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum CpgEdgeKind {
    // === AST edges ===
    /// Parent to child in AST.
    AstChild,
    /// Child to parent in AST.
    AstParent,
    /// Sibling in AST (same parent, to next).
    AstNextSibling,
    /// Sibling in AST (same parent, to previous).
    AstPrevSibling,

    // === CFG edges ===
    /// Control flow edge.
    ControlFlow(CfgEdgeKind),

    // === DFG edges ===
    /// Data flow edge.
    DataFlow(DfgEdgeKind),

    // === Program Dependence Graph (PDG) edges ===
    /// Control dependence (CFG-derived).
    ControlDependence,
    /// Data dependence (DFG-derived).
    DataDependence,

    // === Call graph edges ===
    /// Static call (target known at compile time).
    StaticCall,
    /// Dynamic call (virtual dispatch).
    DynamicCall,
    /// Call site to callee.
    CallSite,

    // === Type edges ===
    /// Type annotation.
    TypeOf,
    /// Inheritance/extends.
    Inherits,
    /// Implements trait/interface.
    Implements,
    /// Generic instantiation.
    GenericInstance,

    // === Reference edges ===
    /// Variable reference (use to def).
    Reference,
    /// Definition site.
    Definition,
    /// Declaration (forward declaration).
    Declaration,

    // === Scope edges ===
    /// Enclosing scope.
    EnclosingScope,
    /// Contained in scope.
    ContainedIn,

    // === Import/dependency edges ===
    /// Import dependency.
    Imports,
    /// Export.
    Exports,
}

impl CpgEdgeKind {
    /// Returns true if this is an AST edge.
    pub fn is_ast(&self) -> bool {
        matches!(
            self,
            Self::AstChild | Self::AstParent | Self::AstNextSibling | Self::AstPrevSibling
        )
    }

    /// Returns true if this is a CFG edge.
    pub fn is_cfg(&self) -> bool {
        matches!(self, Self::ControlFlow(_))
    }

    /// Returns true if this is a DFG edge.
    pub fn is_dfg(&self) -> bool {
        matches!(self, Self::DataFlow(_))
    }

    /// Returns true if this is a PDG edge.
    pub fn is_pdg(&self) -> bool {
        matches!(self, Self::ControlDependence | Self::DataDependence)
    }

    /// Returns true if this is a call graph edge.
    pub fn is_call(&self) -> bool {
        matches!(self, Self::StaticCall | Self::DynamicCall | Self::CallSite)
    }

    /// Returns true if this is a type edge.
    pub fn is_type(&self) -> bool {
        matches!(
            self,
            Self::TypeOf | Self::Inherits | Self::Implements | Self::GenericInstance
        )
    }
}

/// An edge in the Code Property Graph.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CpgEdge {
    /// Unique edge identifier.
    pub id: EdgeId,
    /// Source node.
    pub source: NodeId,
    /// Target node.
    pub target: NodeId,
    /// Edge kind.
    pub kind: CpgEdgeKind,
    /// Optional label for the edge.
    pub label: Option<String>,
}

impl CpgEdge {
    /// Creates a new CPG edge.
    pub fn new(id: EdgeId, source: NodeId, target: NodeId, kind: CpgEdgeKind) -> Self {
        Self {
            id,
            source,
            target,
            kind,
            label: None,
        }
    }

    /// Sets the edge label.
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Creates an AST child edge.
    pub fn ast_child(id: EdgeId, parent: NodeId, child: NodeId) -> Self {
        Self::new(id, parent, child, CpgEdgeKind::AstChild)
    }

    /// Creates a control flow edge.
    pub fn control_flow(id: EdgeId, from: NodeId, to: NodeId, kind: CfgEdgeKind) -> Self {
        Self::new(id, from, to, CpgEdgeKind::ControlFlow(kind))
    }

    /// Creates a data flow edge.
    pub fn data_flow(id: EdgeId, from: NodeId, to: NodeId, kind: DfgEdgeKind) -> Self {
        Self::new(id, from, to, CpgEdgeKind::DataFlow(kind))
    }

    /// Creates a def-use edge.
    pub fn def_use(id: EdgeId, def: NodeId, use_site: NodeId) -> Self {
        Self::data_flow(id, def, use_site, DfgEdgeKind::DefUse)
    }

    /// Creates a reference edge.
    pub fn reference(id: EdgeId, use_site: NodeId, def: NodeId) -> Self {
        Self::new(id, use_site, def, CpgEdgeKind::Reference)
    }

    /// Creates a call site edge.
    pub fn call_site(id: EdgeId, call: NodeId, callee: NodeId) -> Self {
        Self::new(id, call, callee, CpgEdgeKind::CallSite)
    }

    /// Returns true if the edge goes from source to target in forward direction.
    pub fn is_forward(&self) -> bool {
        !matches!(
            self.kind,
            CpgEdgeKind::AstParent
                | CpgEdgeKind::AstPrevSibling
                | CpgEdgeKind::DataFlow(DfgEdgeKind::UseDef)
        )
    }
}
