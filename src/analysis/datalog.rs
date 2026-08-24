//! Deterministic extensional-database exports for Datalog engines.
//!
//! libcpg owns canonical facts; an external engine such as Soufflé owns rule
//! parsing, planning, and fixed-point evaluation. The exporter is feature-free,
//! side-effect free, iterative, and independent of native call-stack depth.

use crate::{
    CfgEdgeKind, CodePropertyGraph, CpgEdge, CpgEdgeKind, CpgNodeKind, CpgNodeKindTag, DfgEdgeKind,
    EdgeId, HeapEdgeKind, NameFlowEdgeKind, NodeId, RewriteDepEdgeKind, TypeInfo,
};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Example Doop-style allocation/assignment closure shipped with libcpg.
pub const DOOP_LITE_RULES: &str = include_str!("../../examples/datalog/doop_lite.dl");
/// Example explicit/implicit information-flow query shipped with libcpg.
pub const IFC_RULES: &str = include_str!("../../examples/datalog/ifc.dl");

/// One canonical binary relation tuple.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct BinaryFact {
    /// Relation source.
    pub source: NodeId,
    /// Relation target.
    pub target: NodeId,
}

/// One canonical control-flow tuple.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CfgFact {
    /// Control-flow source.
    pub source: NodeId,
    /// Control-flow target.
    pub target: NodeId,
    /// Exact control-flow edge kind.
    pub kind: CfgEdgeKind,
}

/// One canonical data-flow tuple.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct DfgFact {
    /// Data-flow source.
    pub source: NodeId,
    /// Data-flow target.
    pub target: NodeId,
    /// Exact data-flow edge kind.
    pub kind: DfgEdgeKind,
}

/// Classification carried by a call tuple.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum DatalogCallKind {
    /// Statically selected callee.
    Static,
    /// Dynamically selected callee.
    Dynamic,
    /// Resolution edge from a call site to a callee.
    CallSite,
}

/// One canonical call tuple.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CallFact {
    /// Calling node or call site.
    pub site: NodeId,
    /// Callee node.
    pub callee: NodeId,
    /// Resolution tier represented by the edge.
    pub kind: DatalogCallKind,
}

/// One stable CPG-node classification tuple.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct NodeFact {
    /// Stable CPG node identifier.
    pub node: NodeId,
    /// Complete payload-free node discriminant.
    pub kind: CpgNodeKindTag,
}

/// Semantic role of a nominal type embedded in a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum DeclaredTypeRole {
    /// Function result type.
    Return,
    /// Function parameter type.
    Parameter,
    /// Variable type.
    Variable,
    /// Field type.
    Field,
    /// Explicit type annotation.
    Annotation,
    /// Type allocated by an expression.
    Allocation,
}

/// One nominal type embedded directly in a CPG node.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct DeclaredTypeFact {
    /// Typed node.
    pub node: NodeId,
    /// Type role at this node.
    pub role: DeclaredTypeRole,
    /// Nominal type name.
    pub name: String,
    /// Whether the type is a reference.
    pub is_reference: bool,
    /// Whether the type is mutable.
    pub is_mutable: bool,
}

/// A lossless canonical edge tuple.
///
/// Relation-specific vectors use Datalog set semantics. This master tuple
/// retains edge identity, labels, and parallel multiplicity for exact overlay
/// reconstruction.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct DatalogEdgeFact {
    /// Stable edge identifier.
    pub edge: EdgeId,
    /// Edge source.
    pub source: NodeId,
    /// Edge target.
    pub target: NodeId,
    /// Exact CPG edge kind.
    pub kind: CpgEdgeKind,
    /// Optional edge label.
    pub label: Option<String>,
}

impl DatalogEdgeFact {
    /// Reconstructs the exact CPG edge represented by this fact.
    pub fn to_cpg_edge(&self) -> CpgEdge {
        let mut edge = CpgEdge::new(self.edge, self.source, self.target, self.kind.clone());
        edge.label = self.label.clone().map(String::into_boxed_str);
        edge
    }
}

/// One in-memory Soufflé `.facts` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SouffleFactFile {
    /// Relation name and file stem.
    pub relation: &'static str,
    /// Canonical RFC 4180 tab-separated fact rows.
    pub contents: String,
}

/// Canonical extensional database (EDB) exported from one CPG.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct DatalogFacts {
    /// Stable node kinds.
    pub nodes: Vec<NodeFact>,
    /// Lossless typed edge snapshot.
    pub edges: Vec<DatalogEdgeFact>,
    /// Parent-to-child AST edges.
    pub ast_child: Vec<BinaryFact>,
    /// Control-flow edges.
    pub cfg: Vec<CfgFact>,
    /// Data-flow edges.
    pub dfg: Vec<DfgFact>,
    /// Control-dependence edges.
    pub control_dependence: Vec<BinaryFact>,
    /// Data-dependence edges.
    pub data_dependence: Vec<BinaryFact>,
    /// Static, dynamic, and call-site resolution edges.
    pub call: Vec<CallFact>,
    /// Explicit node-to-type-node edges.
    pub type_of: Vec<BinaryFact>,
    /// Inheritance edges.
    pub inherits: Vec<BinaryFact>,
    /// Trait/interface implementation edges.
    pub implements: Vec<BinaryFact>,
    /// Generic-instantiation edges.
    pub generic_instance: Vec<BinaryFact>,
    /// Use-to-definition reference edges.
    pub reference: Vec<BinaryFact>,
    /// Definition edges.
    pub definition: Vec<BinaryFact>,
    /// Declaration edges.
    pub declaration: Vec<BinaryFact>,
    /// Enclosing-scope edges.
    pub enclosing_scope: Vec<BinaryFact>,
    /// Containment edges.
    pub contained_in: Vec<BinaryFact>,
    /// Import edges.
    pub imports: Vec<BinaryFact>,
    /// Export edges.
    pub exports: Vec<BinaryFact>,
    /// Nominal types embedded directly in nodes.
    pub declared_type: Vec<DeclaredTypeFact>,
}

impl DatalogFacts {
    /// Reconstructs the complete canonical CPG edge snapshot.
    pub fn cpg_edges(&self) -> Vec<CpgEdge> {
        self.edges
            .iter()
            .map(DatalogEdgeFact::to_cpg_edge)
            .collect()
    }

    /// Renders every public EDB relation as one canonical Soufflé fact file.
    ///
    /// Callers choose their own atomic-write and sandbox policy; libcpg does
    /// not write these buffers automatically. Input directives that consume
    /// these files must set `rfc4180=true` and `delimiter="\t"`.
    pub fn souffle_files(&self) -> Vec<SouffleFactFile> {
        vec![
            SouffleFactFile {
                relation: "node",
                contents: render_nodes(&self.nodes),
            },
            SouffleFactFile {
                relation: "cpg_edge",
                contents: render_edges(&self.edges),
            },
            binary_file("ast_child", &self.ast_child),
            SouffleFactFile {
                relation: "cfg",
                contents: render_cfg(&self.cfg),
            },
            SouffleFactFile {
                relation: "dfg",
                contents: render_dfg(&self.dfg),
            },
            binary_file("cdep", &self.control_dependence),
            binary_file("ddep", &self.data_dependence),
            SouffleFactFile {
                relation: "call",
                contents: render_calls(&self.call),
            },
            binary_file("type_of", &self.type_of),
            binary_file("inherits", &self.inherits),
            binary_file("implements", &self.implements),
            binary_file("generic_instance", &self.generic_instance),
            binary_file("reference", &self.reference),
            binary_file("definition", &self.definition),
            binary_file("declaration", &self.declaration),
            binary_file("enclosing_scope", &self.enclosing_scope),
            binary_file("contained_in", &self.contained_in),
            binary_file("imports", &self.imports),
            binary_file("exports", &self.exports),
            SouffleFactFile {
                relation: "declared_type",
                contents: render_declared_types(&self.declared_type),
            },
        ]
    }
}

/// Exports every represented CPG overlay as deterministic Datalog EDB facts.
///
/// The implementation performs two linear scans followed by canonical sorts.
/// It neither mutates the graph nor evaluates any rules.
pub fn export_datalog(cpg: &CodePropertyGraph) -> DatalogFacts {
    let mut facts = DatalogFacts {
        nodes: Vec::with_capacity(cpg.node_count()),
        edges: Vec::with_capacity(cpg.edge_count()),
        ..DatalogFacts::default()
    };
    for node in cpg.nodes() {
        facts.nodes.push(NodeFact {
            node: node.id,
            kind: node.kind.tag(),
        });
        collect_declared_types(
            node.id,
            &node.kind,
            node.allocation_type.as_deref(),
            &mut facts,
        );
    }
    for edge in cpg.edges() {
        let pair = BinaryFact {
            source: edge.source,
            target: edge.target,
        };
        match edge.kind {
            CpgEdgeKind::AstChild => facts.ast_child.push(pair),
            CpgEdgeKind::ControlFlow(kind) => facts.cfg.push(CfgFact {
                source: edge.source,
                target: edge.target,
                kind,
            }),
            CpgEdgeKind::DataFlow(kind) => facts.dfg.push(DfgFact {
                source: edge.source,
                target: edge.target,
                kind,
            }),
            CpgEdgeKind::ControlDependence => facts.control_dependence.push(pair),
            CpgEdgeKind::DataDependence => facts.data_dependence.push(pair),
            CpgEdgeKind::Heap(_) => {}
            CpgEdgeKind::NameFlow(_) => {}
            CpgEdgeKind::RewriteDep(_) => {}
            CpgEdgeKind::StaticCall => facts.call.push(CallFact {
                site: edge.source,
                callee: edge.target,
                kind: DatalogCallKind::Static,
            }),
            CpgEdgeKind::DynamicCall => facts.call.push(CallFact {
                site: edge.source,
                callee: edge.target,
                kind: DatalogCallKind::Dynamic,
            }),
            CpgEdgeKind::CallSite => facts.call.push(CallFact {
                site: edge.source,
                callee: edge.target,
                kind: DatalogCallKind::CallSite,
            }),
            CpgEdgeKind::TypeOf => facts.type_of.push(pair),
            CpgEdgeKind::Inherits => facts.inherits.push(pair),
            CpgEdgeKind::Implements => facts.implements.push(pair),
            CpgEdgeKind::GenericInstance => facts.generic_instance.push(pair),
            CpgEdgeKind::Reference => facts.reference.push(pair),
            CpgEdgeKind::Definition => facts.definition.push(pair),
            CpgEdgeKind::Declaration => facts.declaration.push(pair),
            CpgEdgeKind::EnclosingScope => facts.enclosing_scope.push(pair),
            CpgEdgeKind::ContainedIn => facts.contained_in.push(pair),
            CpgEdgeKind::Imports => facts.imports.push(pair),
            CpgEdgeKind::Exports => facts.exports.push(pair),
            CpgEdgeKind::AstParent | CpgEdgeKind::AstNextSibling | CpgEdgeKind::AstPrevSibling => {}
        }
        facts.edges.push(DatalogEdgeFact {
            edge: edge.id,
            source: edge.source,
            target: edge.target,
            kind: edge.kind.clone(),
            label: edge.label.as_deref().map(str::to_owned),
        });
    }
    canonicalize(&mut facts);
    facts
}

fn collect_declared_types(
    node: NodeId,
    kind: &CpgNodeKind,
    allocation: Option<&TypeInfo>,
    facts: &mut DatalogFacts,
) {
    let embedded = match kind {
        CpgNodeKind::Function { signature } => signature
            .return_type
            .as_ref()
            .map(|value| (DeclaredTypeRole::Return, value)),
        CpgNodeKind::Parameter { param_type, .. } => param_type
            .as_deref()
            .map(|value| (DeclaredTypeRole::Parameter, value)),
        CpgNodeKind::Variable { var_type, .. } => var_type
            .as_deref()
            .map(|value| (DeclaredTypeRole::Variable, value)),
        CpgNodeKind::Field { field_type, .. } => field_type
            .as_deref()
            .map(|value| (DeclaredTypeRole::Field, value)),
        CpgNodeKind::TypeAnnotation { type_info } => {
            Some((DeclaredTypeRole::Annotation, type_info.as_ref()))
        }
        _ => None,
    };
    if let Some((role, type_info)) = embedded {
        facts.declared_type.push(type_fact(node, role, type_info));
    }
    if let Some(type_info) = allocation {
        facts
            .declared_type
            .push(type_fact(node, DeclaredTypeRole::Allocation, type_info));
    }
}

fn type_fact(node: NodeId, role: DeclaredTypeRole, value: &TypeInfo) -> DeclaredTypeFact {
    DeclaredTypeFact {
        node,
        role,
        name: value.name.to_string(),
        is_reference: value.is_reference,
        is_mutable: value.is_mutable,
    }
}

fn canonicalize(facts: &mut DatalogFacts) {
    sort_dedup(&mut facts.nodes);
    facts.edges.sort_by(|left, right| {
        edge_sort_key(left)
            .cmp(&edge_sort_key(right))
            .then_with(|| left.label.cmp(&right.label))
    });
    sort_dedup(&mut facts.ast_child);
    facts
        .cfg
        .sort_by_key(|fact| (fact.source, fact.target, cfg_name(fact.kind)));
    facts.cfg.dedup();
    facts
        .dfg
        .sort_by_key(|fact| (fact.source, fact.target, dfg_name(fact.kind)));
    facts.dfg.dedup();
    sort_dedup(&mut facts.control_dependence);
    sort_dedup(&mut facts.data_dependence);
    sort_dedup(&mut facts.call);
    sort_dedup(&mut facts.type_of);
    sort_dedup(&mut facts.inherits);
    sort_dedup(&mut facts.implements);
    sort_dedup(&mut facts.generic_instance);
    sort_dedup(&mut facts.reference);
    sort_dedup(&mut facts.definition);
    sort_dedup(&mut facts.declaration);
    sort_dedup(&mut facts.enclosing_scope);
    sort_dedup(&mut facts.contained_in);
    sort_dedup(&mut facts.imports);
    sort_dedup(&mut facts.exports);
    sort_dedup(&mut facts.declared_type);
}

fn sort_dedup<T: Ord>(values: &mut Vec<T>) {
    values.sort_unstable();
    values.dedup();
}

fn edge_sort_key(fact: &DatalogEdgeFact) -> (u32, u32, u32, &'static str, &'static str) {
    let (relation, detail) = edge_names(&fact.kind);
    (
        fact.edge.as_u32(),
        fact.source.as_u32(),
        fact.target.as_u32(),
        relation,
        detail,
    )
}

fn binary_file(relation: &'static str, facts: &[BinaryFact]) -> SouffleFactFile {
    SouffleFactFile {
        relation,
        contents: render_binary(facts),
    }
}

fn render_nodes(facts: &[NodeFact]) -> String {
    let mut output = String::with_capacity(facts.len().saturating_mul(20));
    for fact in facts {
        number(&mut output, fact.node.as_u32());
        output.push('\t');
        symbol(&mut output, node_name(fact.kind));
        output.push('\n');
    }
    output
}

fn render_edges(facts: &[DatalogEdgeFact]) -> String {
    let mut output = String::with_capacity(facts.len().saturating_mul(48));
    for fact in facts {
        let (relation, detail) = edge_names(&fact.kind);
        number(&mut output, fact.edge.as_u32());
        output.push('\t');
        number(&mut output, fact.source.as_u32());
        output.push('\t');
        number(&mut output, fact.target.as_u32());
        output.push('\t');
        symbol(&mut output, relation);
        output.push('\t');
        symbol(&mut output, detail);
        output.push('\t');
        symbol(&mut output, fact.label.as_deref().unwrap_or(""));
        output.push('\n');
    }
    output
}

fn render_binary(facts: &[BinaryFact]) -> String {
    let mut output = String::with_capacity(facts.len().saturating_mul(24));
    for fact in facts {
        number(&mut output, fact.source.as_u32());
        output.push('\t');
        number(&mut output, fact.target.as_u32());
        output.push('\n');
    }
    output
}

fn render_cfg(facts: &[CfgFact]) -> String {
    let mut output = String::with_capacity(facts.len().saturating_mul(36));
    for fact in facts {
        number(&mut output, fact.source.as_u32());
        output.push('\t');
        number(&mut output, fact.target.as_u32());
        output.push('\t');
        symbol(&mut output, cfg_name(fact.kind));
        output.push('\n');
    }
    output
}

fn render_dfg(facts: &[DfgFact]) -> String {
    let mut output = String::with_capacity(facts.len().saturating_mul(36));
    for fact in facts {
        number(&mut output, fact.source.as_u32());
        output.push('\t');
        number(&mut output, fact.target.as_u32());
        output.push('\t');
        symbol(&mut output, dfg_name(fact.kind));
        output.push('\n');
    }
    output
}

fn render_calls(facts: &[CallFact]) -> String {
    let mut output = String::with_capacity(facts.len().saturating_mul(32));
    for fact in facts {
        number(&mut output, fact.site.as_u32());
        output.push('\t');
        number(&mut output, fact.callee.as_u32());
        output.push('\t');
        symbol(&mut output, call_name(fact.kind));
        output.push('\n');
    }
    output
}

fn render_declared_types(facts: &[DeclaredTypeFact]) -> String {
    let mut output = String::with_capacity(facts.len().saturating_mul(48));
    for fact in facts {
        number(&mut output, fact.node.as_u32());
        output.push('\t');
        symbol(&mut output, role_name(fact.role));
        output.push('\t');
        symbol(&mut output, &fact.name);
        output.push('\t');
        output.push(if fact.is_reference { '1' } else { '0' });
        output.push('\t');
        output.push(if fact.is_mutable { '1' } else { '0' });
        output.push('\n');
    }
    output
}

fn number(output: &mut String, value: u32) {
    use std::fmt::Write as _;
    write!(output, "{value}").expect("writing to a String cannot fail");
}

fn symbol(output: &mut String, value: &str) {
    output.push('"');
    for character in value.chars() {
        if character == '"' {
            output.push_str("\"\"");
        } else {
            output.push(character);
        }
    }
    output.push('"');
}

fn edge_names(kind: &CpgEdgeKind) -> (&'static str, &'static str) {
    match kind {
        CpgEdgeKind::AstChild => ("ast_child", ""),
        CpgEdgeKind::AstParent => ("ast_parent", ""),
        CpgEdgeKind::AstNextSibling => ("ast_next_sibling", ""),
        CpgEdgeKind::AstPrevSibling => ("ast_prev_sibling", ""),
        CpgEdgeKind::ControlFlow(kind) => ("cfg", cfg_name(*kind)),
        CpgEdgeKind::DataFlow(kind) => ("dfg", dfg_name(*kind)),
        CpgEdgeKind::ControlDependence => ("cdep", ""),
        CpgEdgeKind::DataDependence => ("ddep", ""),
        CpgEdgeKind::Heap(kind) => ("heap", heap_name(*kind)),
        CpgEdgeKind::NameFlow(kind) => ("name_flow", name_flow_name(*kind)),
        CpgEdgeKind::RewriteDep(kind) => ("rewrite_dep", rewrite_dep_name(*kind)),
        CpgEdgeKind::StaticCall => ("call", "static"),
        CpgEdgeKind::DynamicCall => ("call", "dynamic"),
        CpgEdgeKind::CallSite => ("call", "call_site"),
        CpgEdgeKind::TypeOf => ("type_of", ""),
        CpgEdgeKind::Inherits => ("inherits", ""),
        CpgEdgeKind::Implements => ("implements", ""),
        CpgEdgeKind::GenericInstance => ("generic_instance", ""),
        CpgEdgeKind::Reference => ("reference", ""),
        CpgEdgeKind::Definition => ("definition", ""),
        CpgEdgeKind::Declaration => ("declaration", ""),
        CpgEdgeKind::EnclosingScope => ("enclosing_scope", ""),
        CpgEdgeKind::ContainedIn => ("contained_in", ""),
        CpgEdgeKind::Imports => ("imports", ""),
        CpgEdgeKind::Exports => ("exports", ""),
    }
}

fn heap_name(kind: HeapEdgeKind) -> &'static str {
    match kind {
        HeapEdgeKind::Allocates => "allocates",
        HeapEdgeKind::PointsTo => "points_to",
        HeapEdgeKind::Dereferences => "dereferences",
        HeapEdgeKind::Releases => "releases",
        HeapEdgeKind::RequiresValid => "requires_valid",
        HeapEdgeKind::EnsuresInvalid => "ensures_invalid",
    }
}

fn name_flow_name(kind: NameFlowEdgeKind) -> &'static str {
    match kind {
        NameFlowEdgeKind::Creates => "creates",
        NameFlowEdgeKind::References => "references",
        NameFlowEdgeKind::Quotes => "quotes",
        NameFlowEdgeKind::Drops => "drops",
        NameFlowEdgeKind::Carries => "carries",
        NameFlowEdgeKind::Receives => "receives",
        NameFlowEdgeKind::Communicates => "communicates",
    }
}

fn rewrite_dep_name(kind: RewriteDepEdgeKind) -> &'static str {
    match kind {
        RewriteDepEdgeKind::DependencyPair => "dependency_pair",
        RewriteDepEdgeKind::CriticalConflict => "critical_conflict",
    }
}

fn cfg_name(kind: CfgEdgeKind) -> &'static str {
    match kind {
        CfgEdgeKind::Sequential => "sequential",
        CfgEdgeKind::ConditionalTrue => "conditional_true",
        CfgEdgeKind::ConditionalFalse => "conditional_false",
        CfgEdgeKind::LoopBack => "loop_back",
        CfgEdgeKind::LoopExit => "loop_exit",
        CfgEdgeKind::Break => "break",
        CfgEdgeKind::Continue => "continue",
        CfgEdgeKind::Return => "return",
        CfgEdgeKind::Throw => "throw",
        CfgEdgeKind::Catch => "catch",
        CfgEdgeKind::Call => "call",
        CfgEdgeKind::CallReturn => "call_return",
        CfgEdgeKind::Case => "case",
        CfgEdgeKind::DefaultCase => "default_case",
    }
}

fn dfg_name(kind: DfgEdgeKind) -> &'static str {
    match kind {
        DfgEdgeKind::DefUse => "def_use",
        DfgEdgeKind::UseDef => "use_def",
        DfgEdgeKind::ReachingDef => "reaching_def",
        DfgEdgeKind::DataDependency => "data_dependency",
        DfgEdgeKind::Parameter => "parameter",
        DfgEdgeKind::ReturnValue => "return_value",
        DfgEdgeKind::FieldRead => "field_read",
        DfgEdgeKind::FieldWrite => "field_write",
        DfgEdgeKind::IndexRead => "index_read",
        DfgEdgeKind::IndexWrite => "index_write",
        DfgEdgeKind::Alias => "alias",
        DfgEdgeKind::Dereference => "dereference",
        DfgEdgeKind::AddressOf => "address_of",
    }
}

fn call_name(kind: DatalogCallKind) -> &'static str {
    match kind {
        DatalogCallKind::Static => "static",
        DatalogCallKind::Dynamic => "dynamic",
        DatalogCallKind::CallSite => "call_site",
    }
}

fn role_name(role: DeclaredTypeRole) -> &'static str {
    match role {
        DeclaredTypeRole::Return => "return",
        DeclaredTypeRole::Parameter => "parameter",
        DeclaredTypeRole::Variable => "variable",
        DeclaredTypeRole::Field => "field",
        DeclaredTypeRole::Annotation => "annotation",
        DeclaredTypeRole::Allocation => "allocation",
    }
}

fn node_name(kind: CpgNodeKindTag) -> &'static str {
    match kind {
        CpgNodeKindTag::Root => "root",
        CpgNodeKindTag::Module => "module",
        CpgNodeKindTag::Class => "class",
        CpgNodeKindTag::Struct => "struct",
        CpgNodeKindTag::Enum => "enum",
        CpgNodeKindTag::Trait => "trait",
        CpgNodeKindTag::Impl => "impl",
        CpgNodeKindTag::Function => "function",
        CpgNodeKindTag::Parameter => "parameter",
        CpgNodeKindTag::Block => "block",
        CpgNodeKindTag::Variable => "variable",
        CpgNodeKindTag::Field => "field",
        CpgNodeKindTag::Return => "return",
        CpgNodeKindTag::If => "if",
        CpgNodeKindTag::Else => "else",
        CpgNodeKindTag::While => "while",
        CpgNodeKindTag::For => "for",
        CpgNodeKindTag::Loop => "loop",
        CpgNodeKindTag::Match => "match",
        CpgNodeKindTag::MatchArm => "match_arm",
        CpgNodeKindTag::Break => "break",
        CpgNodeKindTag::Continue => "continue",
        CpgNodeKindTag::Throw => "throw",
        CpgNodeKindTag::Try => "try",
        CpgNodeKindTag::Catch => "catch",
        CpgNodeKindTag::Finally => "finally",
        CpgNodeKindTag::BinaryOp => "binary_op",
        CpgNodeKindTag::UnaryOp => "unary_op",
        CpgNodeKindTag::Assignment => "assignment",
        CpgNodeKindTag::Call => "call",
        CpgNodeKindTag::MemberAccess => "member_access",
        CpgNodeKindTag::IndexAccess => "index_access",
        CpgNodeKindTag::Identifier => "identifier",
        CpgNodeKindTag::Literal => "literal",
        CpgNodeKindTag::Lambda => "lambda",
        CpgNodeKindTag::Await => "await",
        CpgNodeKindTag::Yield => "yield",
        CpgNodeKindTag::TypeAnnotation => "type_annotation",
        CpgNodeKindTag::GenericParam => "generic_param",
        CpgNodeKindTag::Comment => "comment",
        CpgNodeKindTag::Import => "import",
        CpgNodeKindTag::Attribute => "attribute",
        CpgNodeKindTag::Macro => "macro",
        CpgNodeKindTag::Error => "error",
        CpgNodeKindTag::Unknown => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeSet, VecDeque};

    use proptest::prelude::*;

    use super::*;
    use crate::{CpgNode, Language, ScopeId, SourceRange};

    fn node(id: u32, kind: CpgNodeKind) -> CpgNode {
        CpgNode::new(NodeId::new(id), kind, SourceRange::from_bytes(id, id + 1))
    }

    #[test]
    fn every_overlay_is_exported_and_losslessly_reconstructed() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        for id in 0..6 {
            cpg.add_node_with_id(node(
                id,
                CpgNodeKind::Unknown {
                    kind: "fixture".into(),
                },
            ));
        }
        let kinds = [
            CpgEdgeKind::AstChild,
            CpgEdgeKind::ControlFlow(CfgEdgeKind::ConditionalTrue),
            CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse),
            CpgEdgeKind::ControlDependence,
            CpgEdgeKind::DataDependence,
            CpgEdgeKind::Heap(HeapEdgeKind::PointsTo),
            CpgEdgeKind::NameFlow(NameFlowEdgeKind::References),
            CpgEdgeKind::RewriteDep(RewriteDepEdgeKind::DependencyPair),
            CpgEdgeKind::RewriteDep(RewriteDepEdgeKind::CriticalConflict),
            CpgEdgeKind::CallSite,
            CpgEdgeKind::TypeOf,
            CpgEdgeKind::Inherits,
            CpgEdgeKind::Implements,
            CpgEdgeKind::GenericInstance,
            CpgEdgeKind::Reference,
            CpgEdgeKind::Definition,
            CpgEdgeKind::Declaration,
            CpgEdgeKind::EnclosingScope,
            CpgEdgeKind::ContainedIn,
            CpgEdgeKind::Imports,
            CpgEdgeKind::Exports,
        ];
        for (index, kind) in kinds.into_iter().enumerate() {
            cpg.add_edge_with_id(CpgEdge::new(
                EdgeId::new(index as u32),
                NodeId::new((index % 5) as u32),
                NodeId::new(((index + 1) % 6) as u32),
                kind,
            ));
        }
        let facts = export_datalog(&cpg);
        let mut expected: Vec<_> = cpg.edges().cloned().collect();
        expected.sort_by_key(|edge| edge.id.as_u32());
        assert_eq!(facts.cpg_edges(), expected);
        for count in [
            facts.ast_child.len(),
            facts.cfg.len(),
            facts.dfg.len(),
            facts.control_dependence.len(),
            facts.data_dependence.len(),
            facts.call.len(),
            facts.type_of.len(),
            facts.inherits.len(),
            facts.implements.len(),
            facts.generic_instance.len(),
            facts.reference.len(),
            facts.definition.len(),
            facts.declaration.len(),
            facts.enclosing_scope.len(),
            facts.contained_in.len(),
            facts.imports.len(),
            facts.exports.len(),
        ] {
            assert_eq!(count, 1);
        }
        let edge_file = facts
            .souffle_files()
            .into_iter()
            .find(|file| file.relation == "cpg_edge")
            .expect("generic edge facts");
        assert!(edge_file.contents.contains("rewrite_dep"));
        assert!(edge_file.contents.contains("dependency_pair"));
        assert!(edge_file.contents.contains("critical_conflict"));
    }

    #[test]
    fn rejected_out_of_graph_edges_never_become_facts() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        cpg.add_node_with_id(node(
            0,
            CpgNodeKind::Unknown {
                kind: "fixture".into(),
            },
        ));
        assert_eq!(
            cpg.add_edge_with_id(CpgEdge::new(
                EdgeId::new(0),
                NodeId::new(0),
                NodeId::new(99),
                CpgEdgeKind::AstChild,
            )),
            None
        );

        let facts = export_datalog(&cpg);
        assert!(facts.edges.is_empty());
        assert!(facts.ast_child.is_empty());
        assert_eq!(facts.nodes.len(), 1);
    }

    #[test]
    fn symbols_use_rfc4180_tsv_and_rule_assets_opt_in() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        cpg.add_node(
            node(
                0,
                CpgNodeKind::Call {
                    target: None,
                    is_method: false,
                },
            )
            .with_allocation_type(TypeInfo::new("Vec\t\"T\"")),
        );
        cpg.add_node(node(
            1,
            CpgNodeKind::Variable {
                name: "v".into(),
                var_type: Some(Box::new(TypeInfo::new("Vec<T>"))),
                scope: ScopeId::GLOBAL,
                is_mutable: false,
            },
        ));
        let files = export_datalog(&cpg).souffle_files();
        let declared = files
            .iter()
            .find(|file| file.relation == "declared_type")
            .expect("declared_type facts");
        assert!(declared.contents.contains("\"Vec\t\"\"T\"\"\""));
        assert!(!declared.contents.contains("\\t"));
        assert!(DOOP_LITE_RULES.contains("rfc4180=true"));
        assert!(IFC_RULES.contains("rfc4180=true"));
        assert!(DOOP_LITE_RULES.contains("VarPointsTo(dst, heap)"));
        assert!(IFC_RULES.contains("flow(source, sink)"));
    }

    #[test]
    fn doop_lite_fixture_has_the_hand_computed_points_to_closure() {
        let allocations = BTreeSet::from([NodeId::new(1), NodeId::new(7)]);
        let assignments = [
            (NodeId::new(1), NodeId::new(2)),
            (NodeId::new(2), NodeId::new(3)),
            (NodeId::new(7), NodeId::new(8)),
        ];
        let mut points_to = BTreeSet::new();
        let mut queue = VecDeque::new();
        for (source, target) in assignments {
            if allocations.contains(&source) {
                points_to.insert((target, source));
                queue.push_back((target, source));
            }
        }
        while let Some((source, heap)) = queue.pop_front() {
            for &(from, target) in &assignments {
                if from == source && points_to.insert((target, heap)) {
                    queue.push_back((target, heap));
                }
            }
        }
        assert_eq!(
            points_to,
            BTreeSet::from([
                (NodeId::new(2), NodeId::new(1)),
                (NodeId::new(3), NodeId::new(1)),
                (NodeId::new(8), NodeId::new(7)),
            ])
        );
    }

    #[test]
    fn export_crosses_one_hundred_thousand_nodes_on_a_small_stack() {
        std::thread::Builder::new()
            .name("datalog-export-small-stack".into())
            .stack_size(256 * 1024)
            .spawn(|| {
                let count = 100_001u32;
                let mut cpg = CodePropertyGraph::with_capacity(
                    Language::Rust,
                    count as usize,
                    count as usize - 1,
                );
                for id in 0..count {
                    cpg.add_node_with_id(node(
                        id,
                        CpgNodeKind::Unknown {
                            kind: "deep".into(),
                        },
                    ));
                    if id > 0 {
                        cpg.add_edge_with_id(CpgEdge::new(
                            EdgeId::new(id - 1),
                            NodeId::new(id - 1),
                            NodeId::new(id),
                            CpgEdgeKind::AstChild,
                        ));
                    }
                }
                let facts = export_datalog(&cpg);
                assert_eq!(facts.nodes.len(), count as usize);
                assert_eq!(facts.ast_child.len(), count as usize - 1);
                assert_eq!(facts.souffle_files().len(), 20);
            })
            .expect("spawn small-stack exporter")
            .join()
            .expect("small-stack exporter");
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]

        #[test]
        fn insertion_permutations_have_byte_identical_facts(
            selectors in prop::collection::vec((0u8..5, 0u8..8, 0u8..8), 0..48)
        ) {
            let mut forward = CodePropertyGraph::new(Language::Rust);
            let mut reverse = CodePropertyGraph::new(Language::Rust);
            for id in 0..8u32 {
                forward.add_node_with_id(node(
                    id,
                    CpgNodeKind::Unknown { kind: "generated".into() },
                ));
            }
            for id in (0..8u32).rev() {
                reverse.add_node_with_id(node(
                    id,
                    CpgNodeKind::Unknown { kind: "generated".into() },
                ));
            }
            let edges: Vec<_> = selectors
                .iter()
                .enumerate()
                .map(|(index, &(selector, source, target))| {
                    let kind = match selector {
                        0 => CpgEdgeKind::AstChild,
                        1 => CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
                        2 => CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse),
                        3 => CpgEdgeKind::ControlDependence,
                        _ => CpgEdgeKind::CallSite,
                    };
                    CpgEdge::new(
                        EdgeId::new(index as u32),
                        NodeId::new(u32::from(source)),
                        NodeId::new(u32::from(target)),
                        kind,
                    )
                })
                .collect();
            for edge in &edges {
                forward.add_edge_with_id(edge.clone());
            }
            for edge in edges.iter().rev() {
                reverse.add_edge_with_id(edge.clone());
            }
            let forward = export_datalog(&forward);
            let reverse = export_datalog(&reverse);
            prop_assert_eq!(&forward, &reverse);
            prop_assert_eq!(forward.souffle_files(), reverse.souffle_files());
        }
    }
}
