//! Shared proptest strategies and helpers for the crate's own unit tests.
//!
//! Gated `#[cfg(test)]` and `pub(crate)`, so every inline `mod tests` can
//! `use crate::testutil::*`. Integration tests under `tests/` cannot see
//! `pub(crate)` items and keep their own copies in `tests/common/`.
//!
//! Two structural generators exist because [`CodePropertyGraph::connect`] wires
//! only the petgraph edge — it does **not** set `node.parent`/`node.children`,
//! which the AST-ancestor analyses (DFG, PDG, metrics, algorithm detection)
//! read:
//!
//! * [`arb_cpg_raw`] — random nodes + random edges. For analyses that read only
//!   node kinds and petgraph adjacency (serde, VF2, similarity, ids).
//! * [`arb_well_formed_cpg`] — a `Function → Block → statements…` tree with the
//!   `AstChild` edges, parent pointers, and child lists all set (mirroring the
//!   real builder). Unlocks CFG/DFG/PDG/metrics/algorithm-detection properties
//!   without needing a language grammar.
//! * [`arb_pdg_graph`] — nodes + random control/data-dependence edges, for the
//!   program-slicing properties.

#![allow(missing_docs)]
#![allow(dead_code)]

use crate::{
    CfgEdgeKind, CodePropertyGraph, CpgEdgeKind, CpgNode, CpgNodeKind, DfgEdgeKind, Language,
    LiteralKind, MethodSignature, NodeId, ScopeId, SourceRange, Visibility,
};
use proptest::prelude::*;

// ============================ leaf strategies ============================

/// A short identifier-like name (`[a-z][a-z0-9_]{0,7}`).
pub(crate) fn arb_name() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_]{0,7}"
}

/// A source range with `start <= end`.
pub(crate) fn arb_source_range() -> impl Strategy<Value = SourceRange> {
    (any::<u32>(), any::<u32>()).prop_map(|(a, b)| {
        let (s, e) = if a <= b { (a, b) } else { (b, a) };
        SourceRange::from_bytes(s, e)
    })
}

/// A literal kind. Floats are restricted to finite values so that `PartialEq`
/// and serde round-trips are well-defined (NaN != NaN; JSON has no NaN).
pub(crate) fn arb_literal_kind() -> impl Strategy<Value = LiteralKind> {
    prop_oneof![
        any::<i64>().prop_map(LiteralKind::Integer),
        any::<f64>()
            .prop_filter("finite", |f| f.is_finite())
            .prop_map(LiteralKind::Float),
        arb_name().prop_map(|s| LiteralKind::String(s.into())),
        any::<char>().prop_map(LiteralKind::Char),
        any::<bool>().prop_map(LiteralKind::Bool),
        Just(LiteralKind::Null),
        Just(LiteralKind::Array),
        Just(LiteralKind::Object),
        arb_name().prop_map(|s| LiteralKind::Regex(s.into())),
    ]
}

/// A method signature (public, no params/return type).
pub(crate) fn arb_method_signature() -> impl Strategy<Value = MethodSignature> {
    (arb_name(), any::<bool>(), any::<bool>()).prop_map(|(name, is_static, is_async)| {
        MethodSignature {
            name: name.into(),
            params: Default::default(),
            return_type: None,
            is_static,
            is_async,
            visibility: Visibility::Public,
        }
    })
}

/// A broad spread of node kinds — unit and data-bearing variants across every
/// category (structural, function-level, variable, statement, expression,
/// type, special).
pub(crate) fn arb_node_kind() -> impl Strategy<Value = CpgNodeKind> {
    prop_oneof![
        Just(CpgNodeKind::Root),
        arb_name().prop_map(|n| CpgNodeKind::Module { name: n.into() }),
        (arb_name(), any::<bool>())
            .prop_map(|(n, a)| CpgNodeKind::Class { name: n.into(), is_abstract: a }),
        arb_name().prop_map(|n| CpgNodeKind::Struct { name: n.into() }),
        arb_name().prop_map(|n| CpgNodeKind::Enum { name: n.into() }),
        arb_name().prop_map(|n| CpgNodeKind::Trait { name: n.into() }),
        arb_method_signature().prop_map(|signature| CpgNodeKind::Function { signature }),
        (arb_name(), any::<bool>()).prop_map(|(n, m)| CpgNodeKind::Variable {
            name: n.into(),
            var_type: None,
            scope: ScopeId::GLOBAL,
            is_mutable: m,
        }),
        Just(CpgNodeKind::Block { scope: ScopeId::GLOBAL }),
        Just(CpgNodeKind::If),
        Just(CpgNodeKind::Else),
        Just(CpgNodeKind::While),
        Just(CpgNodeKind::For),
        Just(CpgNodeKind::Loop),
        Just(CpgNodeKind::Match),
        Just(CpgNodeKind::Return),
        Just(CpgNodeKind::Break),
        Just(CpgNodeKind::Continue),
        arb_name().prop_map(|o| CpgNodeKind::BinaryOp { operator: o.into() }),
        arb_name().prop_map(|o| CpgNodeKind::Assignment { operator: o.into() }),
        any::<bool>().prop_map(|m| CpgNodeKind::Call { target: None, is_method: m }),
        arb_name().prop_map(|n| CpgNodeKind::Identifier { name: n.into(), definition: None }),
        arb_literal_kind().prop_map(|kind| CpgNodeKind::Literal { kind }),
        Just(CpgNodeKind::IndexAccess),
        (arb_name(), any::<bool>()).prop_map(|(n, doc)| {
            let _ = n;
            CpgNodeKind::Comment { is_doc: doc }
        }),
        arb_name().prop_map(|p| CpgNodeKind::Import { path: p.into() }),
        arb_name().prop_map(|m| CpgNodeKind::Error { message: m.into() }),
    ]
}

pub(crate) fn arb_cfg_edge_kind() -> impl Strategy<Value = CfgEdgeKind> {
    prop_oneof![
        Just(CfgEdgeKind::Sequential),
        Just(CfgEdgeKind::ConditionalTrue),
        Just(CfgEdgeKind::ConditionalFalse),
        Just(CfgEdgeKind::LoopBack),
        Just(CfgEdgeKind::LoopExit),
        Just(CfgEdgeKind::Break),
        Just(CfgEdgeKind::Continue),
        Just(CfgEdgeKind::Return),
        Just(CfgEdgeKind::Throw),
        Just(CfgEdgeKind::Catch),
        Just(CfgEdgeKind::Call),
        Just(CfgEdgeKind::CallReturn),
        Just(CfgEdgeKind::Case),
        Just(CfgEdgeKind::DefaultCase),
    ]
}

pub(crate) fn arb_dfg_edge_kind() -> impl Strategy<Value = DfgEdgeKind> {
    prop_oneof![
        Just(DfgEdgeKind::DefUse),
        Just(DfgEdgeKind::UseDef),
        Just(DfgEdgeKind::ReachingDef),
        Just(DfgEdgeKind::DataDependency),
        Just(DfgEdgeKind::Parameter),
        Just(DfgEdgeKind::ReturnValue),
        Just(DfgEdgeKind::FieldRead),
        Just(DfgEdgeKind::FieldWrite),
        Just(DfgEdgeKind::IndexRead),
        Just(DfgEdgeKind::IndexWrite),
        Just(DfgEdgeKind::Alias),
        Just(DfgEdgeKind::Dereference),
        Just(DfgEdgeKind::AddressOf),
    ]
}

/// A broad spread of edge kinds across every category.
pub(crate) fn arb_edge_kind() -> impl Strategy<Value = CpgEdgeKind> {
    prop_oneof![
        Just(CpgEdgeKind::AstChild),
        Just(CpgEdgeKind::AstParent),
        Just(CpgEdgeKind::AstNextSibling),
        arb_cfg_edge_kind().prop_map(CpgEdgeKind::ControlFlow),
        arb_dfg_edge_kind().prop_map(CpgEdgeKind::DataFlow),
        Just(CpgEdgeKind::ControlDependence),
        Just(CpgEdgeKind::DataDependence),
        Just(CpgEdgeKind::StaticCall),
        Just(CpgEdgeKind::DynamicCall),
        Just(CpgEdgeKind::CallSite),
        Just(CpgEdgeKind::TypeOf),
        Just(CpgEdgeKind::Inherits),
        Just(CpgEdgeKind::Implements),
        Just(CpgEdgeKind::Reference),
        Just(CpgEdgeKind::EnclosingScope),
        Just(CpgEdgeKind::Imports),
    ]
}

// ======================= (a1) raw structural graph =======================

/// A random graph: 1–8 nodes of random kinds joined by random edges. Edges may
/// form cycles (including in `AstChild`); use only for analyses that do not walk
/// AST-ancestor pointers (serde, VF2, similarity, ids, structural ops).
pub(crate) fn arb_cpg_raw() -> impl Strategy<Value = CodePropertyGraph> {
    prop::collection::vec(arb_node_kind(), 1..=8)
        .prop_flat_map(|kinds| {
            let n = kinds.len();
            let edges = prop::collection::vec((0..n, 0..n, arb_edge_kind()), 0..=(2 * n));
            (Just(kinds), edges)
        })
        .prop_map(|(kinds, edges)| {
            let mut cpg = CodePropertyGraph::new(Language::Rust);
            let ids: Vec<NodeId> = kinds
                .into_iter()
                .map(|k| cpg.add_node(CpgNode::new(NodeId::new(0), k, SourceRange::default())))
                .collect();
            for (s, t, k) in edges {
                cpg.connect(ids[s], ids[t], k);
            }
            cpg
        })
}

// ===================== (a1') PDG graph for slicing =====================

/// A graph of 1–8 generic `Block` nodes joined by random `ControlDependence` /
/// `DataDependence` edges — the substrate the slicer walks.
pub(crate) fn arb_pdg_graph() -> impl Strategy<Value = CodePropertyGraph> {
    (1usize..=8)
        .prop_flat_map(|n| {
            let edges = prop::collection::vec(
                (
                    0..n,
                    0..n,
                    prop_oneof![
                        Just(CpgEdgeKind::ControlDependence),
                        Just(CpgEdgeKind::DataDependence),
                    ],
                ),
                0..=(2 * n),
            );
            (Just(n), edges)
        })
        .prop_map(|(n, edges)| {
            let mut cpg = CodePropertyGraph::new(Language::Rust);
            let ids: Vec<NodeId> = (0..n)
                .map(|_| {
                    cpg.add_node(CpgNode::new(
                        NodeId::new(0),
                        CpgNodeKind::Block { scope: ScopeId::GLOBAL },
                        SourceRange::default(),
                    ))
                })
                .collect();
            for (s, t, k) in edges {
                cpg.connect(ids[s], ids[t], k);
            }
            cpg
        })
}

// ==================== (a2) well-formed CPG (AST tree) ====================

/// A statement spec used to build a well-formed AST deterministically.
#[derive(Debug, Clone)]
pub(crate) enum StmtSpec {
    Let(String),
    Assign(String),
    Use(String),
    CallStmt(String),
    Return,
    If(Vec<StmtSpec>),
    While(Vec<StmtSpec>),
    For(Vec<StmtSpec>),
}

/// A single statement, with bounded nesting of `if`/`while`/`for` bodies.
pub(crate) fn arb_stmt() -> impl Strategy<Value = StmtSpec> {
    let leaf = prop_oneof![
        arb_name().prop_map(StmtSpec::Let),
        arb_name().prop_map(StmtSpec::Assign),
        arb_name().prop_map(StmtSpec::Use),
        arb_name().prop_map(StmtSpec::CallStmt),
        Just(StmtSpec::Return),
    ];
    leaf.prop_recursive(3, 24, 3, |inner| {
        let body = prop::collection::vec(inner, 0..=3);
        prop_oneof![
            body.clone().prop_map(StmtSpec::If),
            body.clone().prop_map(StmtSpec::While),
            body.prop_map(StmtSpec::For),
        ]
    })
}

/// A well-formed CPG: `Function "f" → Block → statements…`, with `AstChild`
/// edges, parent pointers, and child lists set exactly like the real builder.
/// The `Function` is the first node, so it is also the graph root.
pub(crate) fn arb_well_formed_cpg() -> impl Strategy<Value = CodePropertyGraph> {
    prop::collection::vec(arb_stmt(), 0..=6).prop_map(build_well_formed)
}

/// Adds `kind` as an AST child of `parent`, wiring the `AstChild` edge, the
/// child's parent pointer, and the parent's child list.
pub(crate) fn wf_child(cpg: &mut CodePropertyGraph, parent: NodeId, kind: CpgNodeKind) -> NodeId {
    let id = cpg.add_node(CpgNode::new(NodeId::new(0), kind, SourceRange::default()));
    cpg.connect(parent, id, CpgEdgeKind::AstChild);
    if let Some(n) = cpg.node_mut(id) {
        n.parent = Some(parent);
    }
    if let Some(p) = cpg.node_mut(parent) {
        p.children.push(id);
    }
    id
}

fn build_stmt(cpg: &mut CodePropertyGraph, parent: NodeId, spec: &StmtSpec) {
    match spec {
        StmtSpec::Let(name) => {
            wf_child(
                cpg,
                parent,
                CpgNodeKind::Variable {
                    name: name.clone().into(),
                    var_type: None,
                    scope: ScopeId::GLOBAL,
                    is_mutable: true,
                },
            );
        }
        StmtSpec::Assign(name) => {
            let a = wf_child(cpg, parent, CpgNodeKind::Assignment { operator: "=".into() });
            wf_child(
                cpg,
                a,
                CpgNodeKind::Identifier { name: name.clone().into(), definition: None },
            );
        }
        StmtSpec::Use(name) => {
            wf_child(
                cpg,
                parent,
                CpgNodeKind::Identifier { name: name.clone().into(), definition: None },
            );
        }
        StmtSpec::CallStmt(name) => {
            let c = wf_child(cpg, parent, CpgNodeKind::Call { target: None, is_method: false });
            wf_child(
                cpg,
                c,
                CpgNodeKind::Identifier { name: name.clone().into(), definition: None },
            );
        }
        StmtSpec::Return => {
            wf_child(cpg, parent, CpgNodeKind::Return);
        }
        StmtSpec::If(body) => build_control(cpg, parent, CpgNodeKind::If, body),
        StmtSpec::While(body) => build_control(cpg, parent, CpgNodeKind::While, body),
        StmtSpec::For(body) => build_control(cpg, parent, CpgNodeKind::For, body),
    }
}

/// Builds a control-flow construct: the keyword node, a condition identifier,
/// then a `Block` body holding the nested statements (last child = body, as the
/// CFG extractor expects).
fn build_control(
    cpg: &mut CodePropertyGraph,
    parent: NodeId,
    keyword: CpgNodeKind,
    body: &[StmtSpec],
) {
    let head = wf_child(cpg, parent, keyword);
    wf_child(
        cpg,
        head,
        CpgNodeKind::Identifier { name: "cond".into(), definition: None },
    );
    let block = wf_child(cpg, head, CpgNodeKind::Block { scope: ScopeId::GLOBAL });
    for s in body {
        build_stmt(cpg, block, s);
    }
}

pub(crate) fn build_well_formed(stmts: Vec<StmtSpec>) -> CodePropertyGraph {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let func = cpg.add_node(CpgNode::new(
        NodeId::new(0),
        CpgNodeKind::Function {
            signature: MethodSignature {
                name: "f".into(),
                params: Default::default(),
                return_type: None,
                is_static: false,
                is_async: false,
                visibility: Visibility::Public,
            },
        },
        SourceRange::default(),
    ));
    let body = wf_child(&mut cpg, func, CpgNodeKind::Block { scope: ScopeId::GLOBAL });
    for s in &stmts {
        build_stmt(&mut cpg, body, s);
    }
    cpg
}

// ======================= source-based generator =======================

/// A small, syntactically-valid Rust function whose body has bounded nesting of
/// `if`/`while`/`for`. Used for parser-backed properties (`build` ≡
/// `build_from_tree`, cyclomatic identity on real CFGs). It only needs to
/// *parse* (tree-sitter is error-tolerant), so fixed identifiers keep it free of
/// keyword collisions while the statement *structure* varies.
pub(crate) fn arb_rust_source() -> impl Strategy<Value = String> {
    prop::collection::vec(arb_stmt(), 0..=6).prop_map(|stmts| {
        let mut body = String::new();
        for s in &stmts {
            render_stmt(s, 1, &mut body);
        }
        format!("fn f(x: i64) -> i64 {{\n{body}    x\n}}\n")
    })
}

fn render_stmt(spec: &StmtSpec, indent: usize, out: &mut String) {
    use std::fmt::Write;
    let pad = "    ".repeat(indent);
    match spec {
        StmtSpec::Let(_) => {
            let _ = writeln!(out, "{pad}let a = x + 1;");
        }
        StmtSpec::Assign(_) => {
            let _ = writeln!(out, "{pad}let mut a = x; a = a + 1;");
        }
        StmtSpec::Use(_) => {
            let _ = writeln!(out, "{pad}let _ = x + 2;");
        }
        StmtSpec::CallStmt(_) => {
            let _ = writeln!(out, "{pad}g(x);");
        }
        StmtSpec::Return => {
            let _ = writeln!(out, "{pad}if x < 0 {{ return x; }}");
        }
        StmtSpec::If(body) => {
            let _ = writeln!(out, "{pad}if x > 0 {{");
            for s in body {
                render_stmt(s, indent + 1, out);
            }
            let _ = writeln!(out, "{pad}}}");
        }
        StmtSpec::While(body) => {
            let _ = writeln!(out, "{pad}while x > 100 {{");
            for s in body {
                render_stmt(s, indent + 1, out);
            }
            let _ = writeln!(out, "{pad}}}");
        }
        StmtSpec::For(body) => {
            let _ = writeln!(out, "{pad}for i in 0..x {{");
            for s in body {
                render_stmt(s, indent + 1, out);
            }
            let _ = writeln!(out, "{pad}}}");
        }
    }
}

// ==================== parser-backed mapping helpers ====================
//
// The per-language `NodeMapper::map_*` tables are the widest single surface in
// the crate, and each arm is a *claim about a grammar* ("tree-sitter emits
// `switch_statement` here"). A claim about an external artifact can silently
// rot when that artifact is upgraded, and a rotted arm fails soft: the mapper
// returns `Unknown` and the CPG quietly loses a construct. These helpers turn
// each table into an executable, self-checking specification — [`assert_maps`]
// fails both when the mapping is wrong *and* when the snippet no longer parses
// to the kind the arm names (grammar drift), so an upgrade cannot silently
// invalidate the table.

/// Parses `src` with the grammar registered for `lang`.
///
/// Panics if no grammar is registered: every caller is gated on the matching
/// `lang-*` feature, so a missing grammar means the feature/registry wiring
/// drifted — precisely what the test must report.
pub(crate) fn parse_with(src: &str, lang: Language) -> tree_sitter::Tree {
    let ts_lang = crate::builder::ParserRegistry::global()
        .get(lang)
        .unwrap_or_else(|| panic!("no tree-sitter grammar registered for {lang:?}"));
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&ts_lang)
        .unwrap_or_else(|e| panic!("set {lang:?} grammar: {e}"));
    parser
        .parse(src, None)
        .unwrap_or_else(|| panic!("parse {lang:?} snippet"))
}

/// Every `(tree-sitter kind, mapped CPG kind)` pair the mapper for `lang`
/// produces over the whole parse tree of `src` — including anonymous nodes, so
/// keyword-shaped arms (`"true"`, `"import"`, `"nil"`) are exercised too.
pub(crate) fn mapped_kinds(src: &str, lang: Language) -> Vec<(String, CpgNodeKind)> {
    let tree = parse_with(src, lang);
    let mapper = crate::builder::NodeMapper::new(lang);
    // Tree-sitter emits roughly one node per 3 source bytes for the dense
    // snippets used here; preallocating avoids repeated reallocation.
    let mut out = Vec::with_capacity(src.len() / 3 + 16);
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        out.push((
            node.kind().to_string(),
            mapper.map_kind(node.kind(), &node, src),
        ));
        let mut cursor = node.walk();
        let children: Vec<_> = node.children(&mut cursor).collect();
        stack.extend(children);
    }
    out
}

/// Asserts that the snippet contains at least one `ts_kind` node **and** that
/// some such node maps to a `CpgNodeKind` satisfying `pred`.
///
/// The two assertions catch the two distinct ways a mapping table can be wrong:
/// the arm maps to the wrong CPG kind, or the grammar no longer produces
/// `ts_kind` at all (so the arm is unreachable and the construct silently
/// degrades to `Unknown`).
#[track_caller]
pub(crate) fn assert_maps(
    pairs: &[(String, CpgNodeKind)],
    ts_kind: &str,
    expected: &str,
    pred: impl Fn(&CpgNodeKind) -> bool,
) {
    let seen: Vec<&CpgNodeKind> = pairs
        .iter()
        .filter(|(k, _)| k == ts_kind)
        .map(|(_, v)| v)
        .collect();
    assert!(
        !seen.is_empty(),
        "the snippet parses to no `{ts_kind}` node, so that mapper arm is \
         unreachable — either the snippet or the grammar drifted"
    );
    assert!(
        seen.iter().any(|k| pred(k)),
        "`{ts_kind}` must map to `{expected}`, but mapped to {seen:?}"
    );
}

/// Table-driven form of [`assert_maps`]: `maps!(&pairs, "if_statement" =>
/// CpgNodeKind::If, …)` asserts one row per tree-sitter kind.
///
/// Only the `lang-*`-gated mapping-table tests use it, so it is unused under a
/// feature set that enables no grammar.
#[allow(unused_macros)]
macro_rules! maps {
    ($pairs:expr, $($ts:literal => $pat:pat),+ $(,)?) => {
        $( $crate::testutil::assert_maps($pairs, $ts, stringify!($pat), |k| matches!(k, $pat)); )+
    };
}
pub(crate) use maps;

/// Builds a CPG from source through the public tree-sitter pipeline.
pub(crate) fn build_source(src: &str, lang: Language) -> CodePropertyGraph {
    use crate::builder::CpgBuilder;
    crate::builder::TreeSitterCpgBuilder::new()
        .build(src, lang)
        .unwrap_or_else(|e| panic!("build {lang:?} CPG: {e}"))
}

// ============================== helpers ==============================

/// All node ids in the graph.
pub(crate) fn node_ids(cpg: &CodePropertyGraph) -> Vec<NodeId> {
    cpg.node_ids().collect()
}
