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

impl From<EdgeId> for u32 {
    fn from(id: EdgeId) -> Self {
        id.0
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
        matches!(
            self,
            Self::LoopBack | Self::LoopExit | Self::Break | Self::Continue
        )
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
        matches!(
            self,
            Self::DefUse | Self::FieldRead | Self::IndexRead | Self::Dereference
        )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edge_id_round_trips() {
        assert_eq!(EdgeId::new(7).as_u32(), 7);
        assert_eq!(EdgeId::from(9u32).as_u32(), 9);
        // `From<EdgeId> for u32` mirrors `NodeId` so the id round-trips both ways.
        assert_eq!(u32::from(EdgeId::new(11)), 11);
        assert_eq!(EdgeId::new(3), EdgeId(3));
    }

    #[test]
    fn cfg_edge_kind_classes() {
        for k in [
            CfgEdgeKind::ConditionalTrue,
            CfgEdgeKind::ConditionalFalse,
            CfgEdgeKind::Case,
            CfgEdgeKind::DefaultCase,
        ] {
            assert!(k.is_conditional(), "{k:?} should be conditional");
            assert!(!k.is_loop());
            assert!(!k.is_exception());
        }
        for k in [
            CfgEdgeKind::LoopBack,
            CfgEdgeKind::LoopExit,
            CfgEdgeKind::Break,
            CfgEdgeKind::Continue,
        ] {
            assert!(k.is_loop(), "{k:?} should be a loop edge");
            assert!(!k.is_conditional());
            assert!(!k.is_exception());
        }
        for k in [CfgEdgeKind::Throw, CfgEdgeKind::Catch] {
            assert!(k.is_exception(), "{k:?} should be an exception edge");
            assert!(!k.is_conditional());
            assert!(!k.is_loop());
        }
        // Sequential / Return / Call belong to none of the three classes.
        assert!(!CfgEdgeKind::Sequential.is_conditional());
        assert!(!CfgEdgeKind::Sequential.is_loop());
        assert!(!CfgEdgeKind::Sequential.is_exception());
    }

    #[test]
    fn dfg_edge_kind_read_write() {
        for k in [
            DfgEdgeKind::DefUse,
            DfgEdgeKind::FieldRead,
            DfgEdgeKind::IndexRead,
            DfgEdgeKind::Dereference,
        ] {
            assert!(k.is_read(), "{k:?} should be a read");
            assert!(!k.is_write(), "{k:?} should not be a write");
        }
        for k in [
            DfgEdgeKind::UseDef,
            DfgEdgeKind::FieldWrite,
            DfgEdgeKind::IndexWrite,
        ] {
            assert!(k.is_write(), "{k:?} should be a write");
            assert!(!k.is_read(), "{k:?} should not be a read");
        }
        // Alias is neither a read nor a write.
        assert!(!DfgEdgeKind::Alias.is_read());
        assert!(!DfgEdgeKind::Alias.is_write());
    }

    #[test]
    fn cpg_edge_kind_classes() {
        assert!(CpgEdgeKind::AstChild.is_ast());
        assert!(CpgEdgeKind::AstParent.is_ast());
        assert!(CpgEdgeKind::AstNextSibling.is_ast());
        assert!(CpgEdgeKind::AstPrevSibling.is_ast());
        assert!(CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential).is_cfg());
        assert!(CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse).is_dfg());
        assert!(CpgEdgeKind::ControlDependence.is_pdg());
        assert!(CpgEdgeKind::DataDependence.is_pdg());
        assert!(CpgEdgeKind::StaticCall.is_call());
        assert!(CpgEdgeKind::DynamicCall.is_call());
        assert!(CpgEdgeKind::CallSite.is_call());
        assert!(CpgEdgeKind::TypeOf.is_type());
        assert!(CpgEdgeKind::Inherits.is_type());
        assert!(CpgEdgeKind::Implements.is_type());
        assert!(CpgEdgeKind::GenericInstance.is_type());

        // Cross-class negatives.
        assert!(!CpgEdgeKind::AstChild.is_cfg());
        assert!(!CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential).is_ast());
        assert!(!CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse).is_cfg());
        // `Reference` is in none of the six classes.
        let reference = CpgEdgeKind::Reference;
        assert!(!reference.is_ast());
        assert!(!reference.is_cfg());
        assert!(!reference.is_dfg());
        assert!(!reference.is_pdg());
        assert!(!reference.is_call());
        assert!(!reference.is_type());
    }

    #[test]
    fn cpg_edge_constructors() {
        let s = NodeId::new(1);
        let t = NodeId::new(2);

        let e = CpgEdge::new(EdgeId::new(0), s, t, CpgEdgeKind::AstChild);
        assert_eq!(e.source, s);
        assert_eq!(e.target, t);
        assert_eq!(e.kind, CpgEdgeKind::AstChild);
        assert!(e.label.is_none());

        let e = e.with_label("child");
        assert_eq!(e.label.as_deref(), Some("child"));

        let ac = CpgEdge::ast_child(EdgeId::new(1), s, t);
        assert_eq!(ac.kind, CpgEdgeKind::AstChild);
        assert_eq!(ac.source, s);
        assert_eq!(ac.target, t);

        let cf = CpgEdge::control_flow(EdgeId::new(2), s, t, CfgEdgeKind::LoopBack);
        assert_eq!(cf.kind, CpgEdgeKind::ControlFlow(CfgEdgeKind::LoopBack));

        let df = CpgEdge::data_flow(EdgeId::new(3), s, t, DfgEdgeKind::ReachingDef);
        assert_eq!(df.kind, CpgEdgeKind::DataFlow(DfgEdgeKind::ReachingDef));

        let du = CpgEdge::def_use(EdgeId::new(4), s, t);
        assert_eq!(du.kind, CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse));
        assert_eq!(du.source, s);
        assert_eq!(du.target, t);

        // `reference` points use-site -> def.
        let rf = CpgEdge::reference(EdgeId::new(5), s, t);
        assert_eq!(rf.kind, CpgEdgeKind::Reference);
        assert_eq!(rf.source, s);
        assert_eq!(rf.target, t);

        let cs = CpgEdge::call_site(EdgeId::new(6), s, t);
        assert_eq!(cs.kind, CpgEdgeKind::CallSite);
        assert_eq!(cs.source, s);
        assert_eq!(cs.target, t);
    }

    #[test]
    fn is_forward_flags() {
        let s = NodeId::new(1);
        let t = NodeId::new(2);
        // Forward kinds.
        assert!(CpgEdge::new(EdgeId::new(0), s, t, CpgEdgeKind::AstChild).is_forward());
        assert!(CpgEdge::ast_child(EdgeId::new(0), s, t).is_forward());
        assert!(CpgEdge::def_use(EdgeId::new(0), s, t).is_forward());
        assert!(CpgEdge::new(EdgeId::new(0), s, t, CpgEdgeKind::AstNextSibling).is_forward());
        // Backward kinds.
        assert!(!CpgEdge::new(EdgeId::new(0), s, t, CpgEdgeKind::AstParent).is_forward());
        assert!(!CpgEdge::new(EdgeId::new(0), s, t, CpgEdgeKind::AstPrevSibling).is_forward());
        assert!(!CpgEdge::new(
            EdgeId::new(0),
            s,
            t,
            CpgEdgeKind::DataFlow(DfgEdgeKind::UseDef)
        )
        .is_forward());
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use crate::testutil::*;
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn prop_edge_id_u32_round_trip(x in any::<u32>()) {
            prop_assert_eq!(EdgeId::from(x).as_u32(), x);
            prop_assert_eq!(u32::from(EdgeId::from(x)), x);
            prop_assert_eq!(EdgeId::new(x).as_u32(), x);
        }

        #[test]
        fn prop_edge_kind_partition(k in arb_edge_kind()) {
            // At most one of the six top-level classes holds.
            let classes = [
                k.is_ast(),
                k.is_cfg(),
                k.is_dfg(),
                k.is_pdg(),
                k.is_call(),
                k.is_type(),
            ];
            let held = classes.iter().filter(|b| **b).count();
            prop_assert!(held <= 1, "kind {:?} is in {} classes", k, held);

            prop_assert_eq!(k.is_cfg(), matches!(k, CpgEdgeKind::ControlFlow(_)));
            prop_assert_eq!(k.is_dfg(), matches!(k, CpgEdgeKind::DataFlow(_)));
            prop_assert_eq!(
                k.is_ast(),
                matches!(
                    k,
                    CpgEdgeKind::AstChild
                        | CpgEdgeKind::AstParent
                        | CpgEdgeKind::AstNextSibling
                        | CpgEdgeKind::AstPrevSibling
                )
            );
        }

        #[test]
        fn prop_cfg_kind_classes_exclusive(k in arb_cfg_edge_kind()) {
            let held = [k.is_conditional(), k.is_loop(), k.is_exception()]
                .iter()
                .filter(|b| **b)
                .count();
            prop_assert!(held <= 1, "cfg kind {:?} in {} classes", k, held);
        }

        #[test]
        fn prop_dfg_read_write_exclusive(k in arb_dfg_edge_kind()) {
            prop_assert!(!(k.is_read() && k.is_write()), "dfg kind {:?} both read and write", k);
        }
    }
}

#[cfg(all(test, feature = "serde"))]
mod serde_tests {
    use super::*;
    use crate::testutil::*;
    use proptest::prelude::*;

    #[test]
    fn serde_edge_round_trip() {
        let e = CpgEdge::new(
            EdgeId::new(3),
            NodeId::new(1),
            NodeId::new(2),
            CpgEdgeKind::ControlFlow(CfgEdgeKind::LoopBack),
        )
        .with_label("back");
        let s = serde_json::to_string(&e).expect("serialize edge");
        let back: CpgEdge = serde_json::from_str(&s).expect("deserialize edge");
        assert_eq!(e, back);

        let eid = EdgeId::new(99);
        let s = serde_json::to_string(&eid).expect("serialize EdgeId");
        assert_eq!(
            serde_json::from_str::<EdgeId>(&s).expect("deserialize EdgeId"),
            eid
        );
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn prop_edge_kind_serde(k in arb_edge_kind()) {
            let s = serde_json::to_string(&k).expect("ser");
            let back: CpgEdgeKind = serde_json::from_str(&s).expect("deser");
            prop_assert_eq!(k, back);
        }

        #[test]
        fn prop_cfg_kind_serde(k in arb_cfg_edge_kind()) {
            let s = serde_json::to_string(&k).expect("ser");
            let back: CfgEdgeKind = serde_json::from_str(&s).expect("deser");
            prop_assert_eq!(k, back);
        }

        #[test]
        fn prop_dfg_kind_serde(k in arb_dfg_edge_kind()) {
            let s = serde_json::to_string(&k).expect("ser");
            let back: DfgEdgeKind = serde_json::from_str(&s).expect("deser");
            prop_assert_eq!(k, back);
        }
    }
}
