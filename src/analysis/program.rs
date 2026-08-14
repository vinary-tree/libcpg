//! Deterministic multi-file CPG composition and import linkage.
//!
//! Per-file graphs keep their own AST roots and source-coordinate spaces. A
//! [`ProgramCpg`] combines those graphs by assigning fresh global identifiers
//! while retaining an explicit [`FileScope`] for every input. Import linkage is
//! then emitted as typed `Imports` and `Exports` edges over that shared node set;
//! no parallel graph or copied declaration nodes are introduced.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::{CodePropertyGraph, CpgEdge, CpgEdgeKind, CpgNodeKind, EdgeId, Language, NodeId};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[cfg(feature = "serde")]
use crate::graph::serde_util::{arc_str, option_arc_str};

/// The translated identity and source metadata for one input file.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct FileScope {
    /// Source path retained from the input graph, when one was supplied.
    #[cfg_attr(feature = "serde", serde(with = "option_arc_str"))]
    pub source_path: Option<Arc<str>>,
    /// Retained source text from the input graph, when available.
    #[cfg_attr(feature = "serde", serde(with = "option_arc_str"))]
    pub source_code: Option<Arc<str>>,
    /// Language recorded by the input graph.
    pub language: Language,
    /// Translated AST root for this file.
    pub root: Option<NodeId>,
    /// All translated nodes belonging to this file, in ascending identifier order.
    pub nodes: Vec<NodeId>,
    /// Deterministic source-to-program node identifier map.
    pub node_map: BTreeMap<NodeId, NodeId>,
}

impl FileScope {
    /// Returns whether `node` belongs to this file.
    pub fn contains(&self, node: NodeId) -> bool {
        self.nodes.binary_search(&node).is_ok()
    }

    /// Translates an input-file node identifier into the program identifier.
    pub fn translated(&self, source: NodeId) -> Option<NodeId> {
        self.node_map.get(&source).copied()
    }
}

/// A CPG containing one or more translated per-file graphs.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ProgramCpg {
    /// Unified append-only graph containing every input node and edge.
    pub cpg: CodePropertyGraph,
    /// Input files in caller-supplied order, with their translated identities.
    pub files: Vec<FileScope>,
}

/// Evidence classification for one import statement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ImportResolutionKind {
    /// One represented file has the strongest matching path or module alias.
    Exact,
    /// Multiple represented files tie for the strongest alias match.
    Ambiguous,
    /// No represented file matches the import path.
    External,
}

/// Deterministic linkage evidence for one import statement.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ImportResolution {
    /// Import-statement node.
    pub import: NodeId,
    /// Index of the containing [`FileScope`] in [`ProgramCpg::files`].
    pub importer: usize,
    /// Original import text/path stored by the language mapper.
    #[cfg_attr(feature = "serde", serde(with = "arc_str"))]
    pub path: Arc<str>,
    /// Resolution classification.
    pub kind: ImportResolutionKind,
    /// Strongest normalized alias, empty when no alias matched.
    pub matched_alias: Vec<String>,
    /// Sorted candidate file indices at the strongest evidence level.
    pub target_files: Vec<usize>,
    /// Sorted declarations connected to this import for an exact match.
    pub imported_nodes: Vec<NodeId>,
}

/// One exact file-to-file dependency supported by an import statement.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ImportDependency {
    /// Index of the importing file.
    pub importer: usize,
    /// Index of the represented target file.
    pub exporter: usize,
    /// Import node supporting this dependency.
    pub import: NodeId,
}

/// Out-of-graph summary of the import/export edges emitted into a program CPG.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ImportGraph {
    /// Namespace-visible declarations per file, ordered by file then node id.
    pub exports: BTreeMap<usize, Vec<NodeId>>,
    /// Exact file dependencies, including their supporting import statements.
    pub dependencies: Vec<ImportDependency>,
    /// Resolution evidence for every represented import node.
    pub resolutions: BTreeMap<NodeId, ImportResolution>,
    /// Number of new `Imports`/`Exports` edges created by this invocation.
    pub emitted_edges: usize,
}

/// Merges per-file CPGs into one deterministic program graph.
///
/// Input order is retained as program file order and therefore determines the
/// contiguous fresh identifier range assigned to each file. Within a file,
/// [`CodePropertyGraph::merge_from`] sorts source identifiers before assigning
/// fresh identifiers. The merged graph records the common input language when
/// every file agrees and [`Language::Unknown`] for an empty or heterogeneous
/// program.
pub fn merge_programs(files: Vec<CodePropertyGraph>) -> ProgramCpg {
    let language = files
        .first()
        .map(CodePropertyGraph::language)
        .filter(|first| files.iter().all(|file| file.language() == *first))
        .unwrap_or(Language::Unknown);
    let single_source = (files.len() == 1).then(|| {
        let file = &files[0];
        (
            file.source_path().map(Arc::<str>::from),
            file.source_code().map(Arc::<str>::from),
        )
    });

    let mut cpg = CodePropertyGraph::new(language);
    if let Some((Some(path), _)) = &single_source {
        cpg = cpg.with_source_path(Arc::clone(path));
    }
    if let Some((_, Some(source))) = &single_source {
        cpg = cpg.with_source_code(Arc::clone(source));
    }

    let mut scopes = Vec::with_capacity(files.len());
    for file in files {
        let source_path = file.source_path().map(Arc::<str>::from);
        let source_code = file.source_code().map(Arc::<str>::from);
        let file_language = file.language();
        let source_root = file.root();
        let remap = cpg.merge_from(&file);
        let node_map: BTreeMap<_, _> = remap.into_iter().collect();
        let mut nodes: Vec<_> = node_map.values().copied().collect();
        nodes.sort_unstable();
        let root = source_root.and_then(|root| node_map.get(&root).copied());
        scopes.push(FileScope {
            source_path,
            source_code,
            language: file_language,
            root,
            nodes,
            node_map,
        });
    }

    ProgramCpg { cpg, files: scopes }
}

/// Resolves represented imports and emits `Imports`/`Exports` overlay edges.
///
/// A file is identified by aliases derived from its extensionless source path
/// and its `Module` nodes. An import resolves only when one non-importing file
/// has the longest matching contiguous alias in the import text. Ties remain
/// [`ImportResolutionKind::Ambiguous`]; absence remains
/// [`ImportResolutionKind::External`]. Exact matches emit:
///
/// - `Exports(file_root, declaration)` for namespace-visible declarations;
/// - `Imports(importing_root, exporting_root)` for the file dependency; and
/// - `Imports(import_node, declaration)` for imported symbols, labelled with
///   the symbol's local binding name.
///
/// When the import text explicitly names exported declarations after the
/// matched module path, only those declarations are connected. Module imports,
/// wildcards, and aliases connect every namespace-visible declaration so
/// qualified member calls can be resolved. Edge insertion is idempotent.
pub fn build_import_graph(program: &mut ProgramCpg) -> ImportGraph {
    let exports = collect_exports(program);
    let aliases = alias_index(program);
    let mut result = ImportGraph {
        exports,
        ..ImportGraph::default()
    };
    let mut import_edges = BTreeSet::new();
    let mut export_edges = BTreeSet::new();

    for (&file, declarations) in &result.exports {
        if let Some(root) = program.files[file].root {
            for &declaration in declarations {
                if root != declaration {
                    export_edges.insert((root, declaration));
                }
            }
        }
    }

    for importer in 0..program.files.len() {
        let mut imports: Vec<_> = program.files[importer]
            .nodes
            .iter()
            .filter_map(
                |&node| match program.cpg.node(node).map(|node| &node.kind) {
                    Some(CpgNodeKind::Import { path }) => Some((node, Arc::clone(path))),
                    _ => None,
                },
            )
            .collect();
        imports.sort_by_key(|(node, _)| *node);

        for (import, path) in imports {
            let tokens = identifier_tokens(&path);
            let matches = strongest_file_matches(&tokens, importer, &aliases);
            let kind = match matches.len() {
                0 => ImportResolutionKind::External,
                1 => ImportResolutionKind::Exact,
                _ => ImportResolutionKind::Ambiguous,
            };
            let target_files: Vec<_> = matches.iter().map(|matched| matched.file).collect();
            let matched_alias = matches
                .first()
                .map(|matched| matched.alias.clone())
                .unwrap_or_default();
            let mut imported_nodes = Vec::new();

            if let [matched] = matches.as_slice() {
                let exporter = matched.file;
                imported_nodes = selected_exports(
                    &program.cpg,
                    result
                        .exports
                        .get(&exporter)
                        .map(Vec::as_slice)
                        .unwrap_or(&[]),
                    &path,
                    &tokens,
                    matched,
                );
                result.dependencies.push(ImportDependency {
                    importer,
                    exporter,
                    import,
                });

                if let (Some(source), Some(target)) =
                    (program.files[importer].root, program.files[exporter].root)
                {
                    if source != target {
                        import_edges.insert((source, target, None));
                    }
                }
                for &target in &imported_nodes {
                    if import != target {
                        for binding in imported_binding_names(&program.cpg, target, &tokens) {
                            import_edges.insert((import, target, Some(binding)));
                        }
                    }
                }
            }

            result.resolutions.insert(
                import,
                ImportResolution {
                    import,
                    importer,
                    path,
                    kind,
                    matched_alias,
                    target_files,
                    imported_nodes,
                },
            );
        }
    }

    result.dependencies.sort();
    result.dependencies.dedup();
    for (source, target) in export_edges {
        result.emitted_edges += usize::from(connect_new(
            &mut program.cpg,
            source,
            target,
            CpgEdgeKind::Exports,
        ));
    }
    for (source, target, label) in import_edges {
        result.emitted_edges += usize::from(connect_labeled_new(
            &mut program.cpg,
            source,
            target,
            CpgEdgeKind::Imports,
            label.as_deref(),
        ));
    }
    result
}

fn connect_labeled_new(
    cpg: &mut CodePropertyGraph,
    source: NodeId,
    target: NodeId,
    kind: CpgEdgeKind,
    label: Option<&str>,
) -> bool {
    if cpg
        .edges_between(source, target)
        .into_iter()
        .any(|edge| edge.kind == kind && edge.label.as_deref() == label)
    {
        return false;
    }
    let mut edge = CpgEdge::new(EdgeId::new(0), source, target, kind);
    if let Some(label) = label {
        edge = edge.with_label(label);
    }
    cpg.add_edge(edge).is_some()
}

fn connect_new(
    cpg: &mut CodePropertyGraph,
    source: NodeId,
    target: NodeId,
    kind: CpgEdgeKind,
) -> bool {
    if cpg
        .edges_between(source, target)
        .into_iter()
        .any(|edge| edge.kind == kind)
    {
        return false;
    }
    cpg.connect(source, target, kind).is_some()
}

fn collect_exports(program: &ProgramCpg) -> BTreeMap<usize, Vec<NodeId>> {
    let mut exports = BTreeMap::new();
    for (file, scope) in program.files.iter().enumerate() {
        let mut declarations: Vec<_> = scope
            .nodes
            .iter()
            .copied()
            .filter(|&node| namespace_visible(&program.cpg, scope.root, node))
            .collect();
        declarations.sort_unstable();
        declarations.dedup();
        exports.insert(file, declarations);
    }
    exports
}

fn namespace_visible(cpg: &CodePropertyGraph, root: Option<NodeId>, node: NodeId) -> bool {
    let Some(candidate) = cpg.node(node) else {
        return false;
    };
    if !candidate.is_declaration() || candidate.name().is_none() {
        return false;
    }
    if root == Some(node) {
        return true;
    }

    let mut current = candidate.parent;
    let mut visited = BTreeSet::new();
    while let Some(parent) = current {
        if !visited.insert(parent) {
            return false;
        }
        if Some(parent) == root {
            return true;
        }
        let Some(parent_node) = cpg.node(parent) else {
            return false;
        };
        if !matches!(parent_node.kind, CpgNodeKind::Module { .. }) {
            return false;
        }
        current = parent_node.parent;
    }
    root.is_none() && candidate.parent.is_none()
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct AliasEntry {
    tokens: Vec<String>,
    file: usize,
}

fn alias_index(program: &ProgramCpg) -> BTreeMap<String, Vec<AliasEntry>> {
    let mut index: BTreeMap<String, Vec<AliasEntry>> = BTreeMap::new();
    for (file, scope) in program.files.iter().enumerate() {
        let mut aliases = BTreeSet::new();
        if let Some(path) = &scope.source_path {
            aliases.extend(source_path_aliases(path));
        }
        for &node in &scope.nodes {
            if let Some(CpgNodeKind::Module { name }) =
                program.cpg.node(node).map(|node| &node.kind)
            {
                let tokens = identifier_tokens(name);
                for start in 0..tokens.len() {
                    aliases.insert(tokens[start..].to_vec());
                }
            }
        }
        for tokens in aliases {
            if let Some(first) = tokens.first() {
                index
                    .entry(first.clone())
                    .or_default()
                    .push(AliasEntry { tokens, file });
            }
        }
    }
    for entries in index.values_mut() {
        entries.sort();
        entries.dedup();
    }
    index
}

fn source_path_aliases(path: &str) -> BTreeSet<Vec<String>> {
    let mut components: Vec<Vec<String>> = path
        .split(['/', '\\'])
        .filter(|component| !component.is_empty() && *component != ".")
        .map(identifier_tokens)
        .filter(|tokens| !tokens.is_empty())
        .collect();
    if let Some(last) = components.last_mut() {
        if last.len() > 1 {
            last.pop();
        } else if let Some(stem) = path
            .rsplit(['/', '\\'])
            .next()
            .and_then(|name| name.rsplit_once('.').map(|(stem, _)| stem))
        {
            *last = identifier_tokens(stem);
        }
    }

    if components.last().is_some_and(|component| {
        matches!(
            component.as_slice(),
            [name] if matches!(name.as_str(), "mod" | "lib" | "index" | "__init__")
        )
    }) {
        components.pop();
    }

    let flattened: Vec<_> = components.into_iter().flatten().collect();
    let mut aliases = BTreeSet::new();
    for start in 0..flattened.len() {
        let suffix = flattened[start..].to_vec();
        if suffix.len() >= 2 || start + 1 == flattened.len() {
            aliases.insert(suffix);
        }
    }
    aliases
}

fn identifier_tokens(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for character in text.chars() {
        if character == '_' || character == '$' || character.is_alphanumeric() {
            current.push(character);
        } else if !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileMatch {
    file: usize,
    alias: Vec<String>,
    start: usize,
    end: usize,
}

fn strongest_file_matches(
    tokens: &[String],
    importer: usize,
    aliases: &BTreeMap<String, Vec<AliasEntry>>,
) -> Vec<FileMatch> {
    let mut by_file: BTreeMap<usize, FileMatch> = BTreeMap::new();
    for start in 0..tokens.len() {
        let Some(entries) = aliases.get(&tokens[start]) else {
            continue;
        };
        for entry in entries {
            if entry.file == importer || start + entry.tokens.len() > tokens.len() {
                continue;
            }
            if tokens[start..start + entry.tokens.len()] != entry.tokens {
                continue;
            }
            let candidate = FileMatch {
                file: entry.file,
                alias: entry.tokens.clone(),
                start,
                end: start + entry.tokens.len(),
            };
            let replace = by_file.get(&entry.file).is_none_or(|prior| {
                candidate.alias.len() > prior.alias.len()
                    || (candidate.alias.len() == prior.alias.len()
                        && (candidate.alias.as_slice(), candidate.end)
                            < (prior.alias.as_slice(), prior.end))
            });
            if replace {
                by_file.insert(entry.file, candidate);
            }
        }
    }

    let best = by_file
        .values()
        .map(|matched| matched.alias.len())
        .max()
        .unwrap_or(0);
    by_file
        .into_values()
        .filter(|matched| matched.alias.len() == best)
        .collect()
}

fn selected_exports(
    cpg: &CodePropertyGraph,
    exports: &[NodeId],
    import_text: &str,
    tokens: &[String],
    matched: &FileMatch,
) -> Vec<NodeId> {
    let mut named = explicit_export_names(&tokens[matched.end..]);
    if named.is_empty() && import_text.contains('{') && import_text.contains('}') {
        if let Some(from) = tokens[..matched.start]
            .iter()
            .rposition(|token| token.eq_ignore_ascii_case("from"))
        {
            named = explicit_export_names(&tokens[..from]);
        }
    }
    let mut selected: Vec<_> = exports
        .iter()
        .copied()
        .filter(|&export| {
            cpg.node(export)
                .and_then(|node| node.name())
                .is_some_and(|name| named.contains(name))
        })
        .collect();
    if selected.is_empty() {
        selected.extend_from_slice(exports);
    }
    selected.sort_unstable();
    selected.dedup();
    selected
}

fn explicit_export_names(tokens: &[String]) -> BTreeSet<&str> {
    tokens
        .iter()
        .enumerate()
        .filter(|(index, token)| {
            !import_keyword(token)
                && (*index == 0 || !tokens[*index - 1].eq_ignore_ascii_case("as"))
        })
        .map(|(_, token)| token.as_str())
        .collect()
}

fn import_keyword(token: &str) -> bool {
    matches!(
        token.to_ascii_lowercase().as_str(),
        "as" | "from"
            | "import"
            | "use"
            | "include"
            | "require"
            | "export"
            | "type"
            | "crate"
            | "self"
            | "super"
            | "rs"
            | "py"
            | "js"
            | "jsx"
            | "ts"
            | "tsx"
            | "java"
            | "go"
            | "c"
            | "h"
            | "cc"
            | "cpp"
            | "hpp"
            | "cs"
            | "rb"
            | "php"
            | "swift"
            | "kt"
            | "kts"
            | "scala"
            | "lua"
            | "sh"
    )
}

fn imported_binding_names(
    cpg: &CodePropertyGraph,
    export: NodeId,
    import_tokens: &[String],
) -> BTreeSet<String> {
    let Some(exported_name) = cpg.node(export).and_then(|node| node.name()) else {
        return BTreeSet::new();
    };
    let mut bindings = BTreeSet::new();
    for window in import_tokens.windows(3) {
        if window[0] == exported_name && window[1].eq_ignore_ascii_case("as") {
            bindings.insert(window[2].clone());
        }
    }
    if bindings.is_empty() {
        bindings.insert(exported_name.to_owned());
    }
    bindings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{resolve_calls, CpgNode, MethodSignature, ResolveConfig, SourceRange, Visibility};
    use proptest::prelude::*;

    fn node(kind: CpgNodeKind) -> CpgNode {
        CpgNode::new(NodeId::new(0), kind, SourceRange::default())
    }

    fn function(name: &str) -> CpgNodeKind {
        CpgNodeKind::Function {
            signature: MethodSignature {
                name: name.into(),
                params: Default::default(),
                return_type: None,
                is_static: false,
                is_async: false,
                visibility: Visibility::Public,
            },
        }
    }

    fn child(cpg: &mut CodePropertyGraph, parent: NodeId, kind: CpgNodeKind) -> NodeId {
        let child = cpg.add_node(node(kind));
        cpg.connect(parent, child, CpgEdgeKind::AstChild);
        cpg.node_mut(child).expect("new child").parent = Some(parent);
        cpg.node_mut(parent).expect("parent").children.push(child);
        child
    }

    fn library(path: &str, symbol: &str) -> (CodePropertyGraph, NodeId) {
        let mut cpg = CodePropertyGraph::new(Language::Rust).with_source_path(path);
        let root = cpg.add_node(node(CpgNodeKind::Root));
        let declaration = child(&mut cpg, root, function(symbol));
        (cpg, declaration)
    }

    fn application(path: &str, import_path: &str, callee: &str) -> (CodePropertyGraph, NodeId) {
        let mut cpg = CodePropertyGraph::new(Language::Rust).with_source_path(path);
        let root = cpg.add_node(node(CpgNodeKind::Root));
        child(
            &mut cpg,
            root,
            CpgNodeKind::Import {
                path: import_path.into(),
            },
        );
        let caller = child(&mut cpg, root, function("caller"));
        let call = child(
            &mut cpg,
            caller,
            CpgNodeKind::Call {
                target: None,
                is_method: false,
            },
        );
        child(
            &mut cpg,
            call,
            CpgNodeKind::Identifier {
                name: callee.into(),
                definition: None,
            },
        );
        (cpg, call)
    }

    fn graph_shape(cpg: &CodePropertyGraph) -> (Vec<String>, Vec<String>) {
        let mut nodes: Vec<_> = cpg
            .nodes()
            .map(|node| {
                format!(
                    "{:?}|{:?}|{:?}|{:?}",
                    node.id, node.kind, node.parent, node.children
                )
            })
            .collect();
        nodes.sort();
        let mut edges: Vec<_> = cpg
            .edges()
            .map(|edge| {
                format!(
                    "{:?}|{:?}|{:?}|{:?}",
                    edge.id, edge.source, edge.target, edge.kind
                )
            })
            .collect();
        edges.sort();
        (nodes, edges)
    }

    fn embedded_references_resolve(cpg: &CodePropertyGraph) -> bool {
        cpg.nodes().all(|node| {
            node.parent.is_none_or(|target| cpg.contains_node(target))
                && node
                    .children
                    .iter()
                    .all(|&target| cpg.contains_node(target))
                && match &node.kind {
                    CpgNodeKind::Call { target, .. } => {
                        target.is_none_or(|target| cpg.contains_node(target))
                    }
                    CpgNodeKind::Identifier { definition, .. } => {
                        definition.is_none_or(|target| cpg.contains_node(target))
                    }
                    CpgNodeKind::Lambda { captures } => {
                        captures.iter().all(|&target| cpg.contains_node(target))
                    }
                    _ => true,
                }
        })
    }

    #[test]
    fn merge_programs_preserves_file_order_metadata_and_contiguous_identity_ranges() {
        let mut rust = library("src/math.rs", "add").0;
        rust = rust.with_source_code("pub fn add() {}");
        let python = CodePropertyGraph::new(Language::Python)
            .with_source_path("pkg/io.py")
            .with_source_code("def read(): pass");
        let expected_nodes = rust.node_count() + python.node_count();
        let program = merge_programs(vec![rust, python]);

        assert_eq!(program.cpg.language(), Language::Unknown);
        assert_eq!(program.cpg.node_count(), expected_nodes);
        assert_eq!(program.files.len(), 2);
        assert_eq!(program.files[0].source_path.as_deref(), Some("src/math.rs"));
        assert_eq!(
            program.files[0].source_code.as_deref(),
            Some("pub fn add() {}")
        );
        assert_eq!(program.files[1].source_path.as_deref(), Some("pkg/io.py"));
        assert!(program.files[0]
            .nodes
            .iter()
            .all(|&node| program.files[0].contains(node)));
        assert_eq!(
            program.files[0].translated(NodeId::new(0)),
            Some(NodeId::new(0))
        );
        let mut ids: Vec<_> = program.cpg.node_ids().collect();
        ids.sort_unstable();
        assert_eq!(
            ids,
            (0..expected_nodes as u32)
                .map(NodeId::new)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn single_file_metadata_is_retained_on_the_program_graph() {
        let source = CodePropertyGraph::new(Language::Rust)
            .with_source_path("src/lib.rs")
            .with_source_code("pub fn f() {}");
        let program = merge_programs(vec![source]);
        assert_eq!(program.cpg.language(), Language::Rust);
        assert_eq!(program.cpg.source_path(), Some("src/lib.rs"));
        assert_eq!(program.cpg.source_code(), Some("pub fn f() {}"));

        let empty = merge_programs(Vec::new());
        assert_eq!(empty.cpg.language(), Language::Unknown);
        assert!(empty.files.is_empty());
    }

    #[test]
    fn exact_import_emits_file_symbol_and_export_edges_idempotently() {
        let (app, _) = application("src/app.rs", "use crate::math::add;", "add");
        let (math, _) = library("src/math.rs", "add");
        let mut program = merge_programs(vec![app, math]);
        let app_root = program.files[0].root.expect("app root");
        let math_root = program.files[1].root.expect("math root");
        let import = program.files[0]
            .nodes
            .iter()
            .copied()
            .find(|&node| {
                matches!(
                    program.cpg.node(node).map(|node| &node.kind),
                    Some(CpgNodeKind::Import { .. })
                )
            })
            .expect("import");
        let add = program.files[1]
            .nodes
            .iter()
            .copied()
            .find(|&node| {
                program
                    .cpg
                    .node(node)
                    .is_some_and(|node| node.name() == Some("add"))
            })
            .expect("add");

        let first = build_import_graph(&mut program);
        assert!(first.emitted_edges >= 3);
        assert_eq!(first.dependencies.len(), 1);
        assert_eq!(first.resolutions[&import].kind, ImportResolutionKind::Exact);
        assert_eq!(first.resolutions[&import].target_files, vec![1]);
        assert_eq!(first.resolutions[&import].imported_nodes, vec![add]);
        assert!(program
            .cpg
            .edges_between(app_root, math_root)
            .iter()
            .any(|edge| edge.kind == CpgEdgeKind::Imports));
        assert!(program
            .cpg
            .edges_between(import, add)
            .iter()
            .any(|edge| edge.kind == CpgEdgeKind::Imports));
        assert!(program
            .cpg
            .edges_between(math_root, add)
            .iter()
            .any(|edge| edge.kind == CpgEdgeKind::Exports));
        let edges = program.cpg.edge_count();

        let second = build_import_graph(&mut program);
        assert_eq!(second.emitted_edges, 0);
        assert_eq!(second.dependencies, first.dependencies);
        assert_eq!(second.resolutions, first.resolutions);
        assert_eq!(second.exports, first.exports);
        assert_eq!(program.cpg.edge_count(), edges);
    }

    #[test]
    fn ambiguous_and_external_imports_emit_no_dependency_topology() {
        let (app, _) = application("src/app.rs", "import shared", "f");
        let (left, _) = library("src/left/shared.rs", "f");
        let (right, _) = library("src/right/shared.rs", "f");
        let mut program = merge_programs(vec![app, left, right]);
        let graph = build_import_graph(&mut program);
        let resolution = graph.resolutions.values().next().expect("resolution");
        assert_eq!(resolution.kind, ImportResolutionKind::Ambiguous);
        assert_eq!(resolution.target_files, vec![1, 2]);
        assert!(graph.dependencies.is_empty());

        let (app, _) = application("src/app.rs", "import absent", "f");
        let mut program = merge_programs(vec![app]);
        let graph = build_import_graph(&mut program);
        let resolution = graph.resolutions.values().next().expect("resolution");
        assert_eq!(resolution.kind, ImportResolutionKind::External);
        assert!(resolution.target_files.is_empty());
        assert!(graph.dependencies.is_empty());
        assert!(program
            .cpg
            .edges_by_kind(|kind| matches!(kind, CpgEdgeKind::Imports))
            .next()
            .is_none());
    }

    #[test]
    fn represented_import_evidence_disambiguates_duplicate_global_function_names() {
        let (app, source_call) = application("src/app.rs", "from math import add", "add");
        let (math, source_add) = library("src/math.rs", "add");
        let (rival, _) = library("src/rival.rs", "add");
        let mut program = merge_programs(vec![app, math, rival]);
        let call = program.files[0]
            .translated(source_call)
            .expect("translated call");
        let add = program.files[1]
            .translated(source_add)
            .expect("translated add");
        build_import_graph(&mut program);

        let resolution = resolve_calls(&mut program.cpg, &ResolveConfig::default());
        let record = &resolution.resolutions[&call];
        assert_eq!(record.confidence, crate::Confidence::ExactViaImport);
        assert_eq!(record.target, Some(add));
        assert_eq!(record.candidates, vec![add]);
        assert_eq!(record.candidate_count, 1);
        assert!(matches!(
            program.cpg.node(call).expect("call").kind,
            CpgNodeKind::Call {
                target: Some(target),
                ..
            } if target == add
        ));
    }

    #[test]
    fn repeated_import_statements_do_not_duplicate_the_same_function_candidate() {
        let (mut app, source_call) = application("src/app.rs", "use crate::math::add;", "add");
        let root = app.root().expect("app root");
        child(
            &mut app,
            root,
            CpgNodeKind::Import {
                path: "from math import add".into(),
            },
        );
        let (math, source_add) = library("src/math.rs", "add");
        let mut program = merge_programs(vec![app, math]);
        let call = program.files[0]
            .translated(source_call)
            .expect("translated call");
        let add = program.files[1]
            .translated(source_add)
            .expect("translated add");
        build_import_graph(&mut program);

        let resolution = resolve_calls(&mut program.cpg, &ResolveConfig::default());
        let record = &resolution.resolutions[&call];
        assert_eq!(record.confidence, crate::Confidence::ExactViaImport);
        assert_eq!(record.target, Some(add));
        assert_eq!(record.candidate_count, 1);
        assert_eq!(record.candidates, vec![add]);
    }

    #[test]
    fn renamed_import_uses_its_local_binding_for_cross_file_resolution() {
        let (app, source_call) = application("src/app.rs", "from math import add as sum", "sum");
        let (math, source_add) = library("src/math.rs", "add");
        let (rival, _) = library("src/rival.rs", "sum");
        let mut program = merge_programs(vec![app, math, rival]);
        let call = program.files[0]
            .translated(source_call)
            .expect("translated call");
        let add = program.files[1]
            .translated(source_add)
            .expect("translated add");

        let imports = build_import_graph(&mut program);
        let import = *imports
            .resolutions
            .keys()
            .next()
            .expect("import resolution");
        let symbol_edge = program
            .cpg
            .edges_between(import, add)
            .into_iter()
            .find(|edge| edge.kind == CpgEdgeKind::Imports)
            .expect("declaration import edge");
        assert_eq!(symbol_edge.label.as_deref(), Some("sum"));

        let resolution = resolve_calls(&mut program.cpg, &ResolveConfig::default());
        let record = &resolution.resolutions[&call];
        assert_eq!(record.confidence, crate::Confidence::ExactViaImport);
        assert_eq!(record.target, Some(add));
        assert_eq!(record.candidate_count, 1);
        assert_eq!(record.candidates, vec![add]);

        let edge_count = program.cpg.edge_count();
        let repeated = build_import_graph(&mut program);
        assert_eq!(repeated.emitted_edges, 0);
        assert_eq!(program.cpg.edge_count(), edge_count);
    }

    #[test]
    fn grouped_imports_retain_each_exported_local_name() {
        let mut app = CodePropertyGraph::new(Language::Rust).with_source_path("src/app.rs");
        let app_root = app.add_node(node(CpgNodeKind::Root));
        child(
            &mut app,
            app_root,
            CpgNodeKind::Import {
                path: "use crate::math::{add as sum, subtract};".into(),
            },
        );
        let mut math = CodePropertyGraph::new(Language::Rust).with_source_path("src/math.rs");
        let math_root = math.add_node(node(CpgNodeKind::Root));
        child(&mut math, math_root, function("add"));
        child(&mut math, math_root, function("sum"));
        child(&mut math, math_root, function("subtract"));
        let mut program = merge_programs(vec![app, math]);

        let imports = build_import_graph(&mut program);
        let import = *imports
            .resolutions
            .keys()
            .next()
            .expect("import resolution");
        let labels: BTreeSet<_> = program
            .cpg
            .outgoing_edges(import)
            .filter(|edge| edge.kind == CpgEdgeKind::Imports)
            .filter_map(|edge| edge.label.clone())
            .collect();
        assert_eq!(
            labels,
            BTreeSet::from(["subtract".to_owned(), "sum".to_owned()])
        );
        assert_eq!(imports.resolutions[&import].imported_nodes.len(), 2);
    }

    #[test]
    fn javascript_named_import_selects_symbols_before_from_without_alias_leakage() {
        let mut app = CodePropertyGraph::new(Language::JavaScript).with_source_path("src/app.js");
        let app_root = app.add_node(node(CpgNodeKind::Root));
        child(
            &mut app,
            app_root,
            CpgNodeKind::Import {
                path: "import { add as sum } from './math.js';".into(),
            },
        );
        let mut math = CodePropertyGraph::new(Language::JavaScript).with_source_path("src/math.js");
        let math_root = math.add_node(node(CpgNodeKind::Root));
        let add = child(&mut math, math_root, function("add"));
        child(&mut math, math_root, function("sum"));
        child(&mut math, math_root, function("multiply"));
        let mut program = merge_programs(vec![app, math]);
        let expected_add = program.files[1].translated(add).expect("translated add");

        let imports = build_import_graph(&mut program);
        let resolution = imports.resolutions.values().next().expect("resolution");
        assert_eq!(resolution.imported_nodes, vec![expected_add]);
        let edge = program
            .cpg
            .edges_between(resolution.import, expected_add)
            .into_iter()
            .find(|edge| edge.kind == CpgEdgeKind::Imports)
            .expect("declaration import");
        assert_eq!(edge.label.as_deref(), Some("sum"));
    }

    #[test]
    fn common_import_syntaxes_match_extensionless_source_aliases() {
        let mut app = CodePropertyGraph::new(Language::Rust).with_source_path("src/app.rs");
        let root = app.add_node(node(CpgNodeKind::Root));
        for path in [
            "use crate::math::add;",
            "from math import add",
            "import pkg.math.add",
            "#include \"math.h\"",
        ] {
            child(&mut app, root, CpgNodeKind::Import { path: path.into() });
        }
        let (math, _) = library("src/math.rs", "add");
        let mut program = merge_programs(vec![app, math]);
        let graph = build_import_graph(&mut program);
        assert_eq!(graph.resolutions.len(), 4);
        assert!(graph
            .resolutions
            .values()
            .all(|resolution| resolution.kind == ImportResolutionKind::Exact));
        assert!(graph
            .resolutions
            .values()
            .all(|resolution| resolution.target_files == vec![1]));
    }

    #[test]
    fn only_namespace_declarations_are_exported() {
        let mut library = CodePropertyGraph::new(Language::Rust).with_source_path("src/types.rs");
        let root = library.add_node(node(CpgNodeKind::Root));
        let top = child(&mut library, root, function("top"));
        let class = child(
            &mut library,
            root,
            CpgNodeKind::Class {
                name: "Container".into(),
                is_abstract: false,
            },
        );
        child(&mut library, class, function("method"));
        let mut program = merge_programs(vec![library]);
        let mapped_top = program.files[0].translated(top).expect("top");
        let mapped_class = program.files[0].translated(class).expect("class");
        let graph = build_import_graph(&mut program);
        assert_eq!(graph.exports[&0], vec![mapped_top, mapped_class]);
    }

    #[test]
    fn source_path_aliases_cover_nested_and_conventional_module_files() {
        assert!(source_path_aliases("src/math.rs").contains(&vec!["math".to_owned()]));
        assert!(source_path_aliases("src/numeric/math.rs")
            .contains(&vec!["numeric".to_owned(), "math".to_owned()]));
        assert!(source_path_aliases("src/numeric/mod.rs").contains(&vec!["numeric".to_owned()]));
        assert!(source_path_aliases("pkg/tools/__init__.py").contains(&vec!["tools".to_owned()]));
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn merging_generated_well_formed_files_is_deterministic_and_total(
            left in crate::testutil::arb_well_formed_cpg(),
            right in crate::testutil::arb_well_formed_cpg(),
        ) {
            let expected_nodes = left.node_count() + right.node_count();
            let expected_edges = left.edge_count() + right.edge_count();
            let first = merge_programs(vec![left.clone(), right.clone()]);
            let second = merge_programs(vec![left, right]);

            prop_assert_eq!(first.cpg.node_count(), expected_nodes);
            prop_assert_eq!(first.cpg.edge_count(), expected_edges);
            prop_assert_eq!(first.files, second.files);
            prop_assert_eq!(graph_shape(&first.cpg), graph_shape(&second.cpg));
            prop_assert!(embedded_references_resolve(&first.cpg));
            let mut ids: Vec<_> = first.cpg.node_ids().collect();
            ids.sort_unstable();
            prop_assert_eq!(
                ids,
                (0..expected_nodes as u32).map(NodeId::new).collect::<Vec<_>>()
            );
        }
    }

    #[cfg(feature = "serde")]
    #[test]
    fn program_and_import_results_round_trip_through_json() {
        let (app, _) = application("src/app.rs", "use crate::math::add;", "add");
        let (math, _) = library("src/math.rs", "add");
        let mut program = merge_programs(vec![app, math]);
        let graph = build_import_graph(&mut program);

        let encoded = serde_json::to_string(&program).expect("serialize program");
        let decoded: ProgramCpg = serde_json::from_str(&encoded).expect("deserialize program");
        assert_eq!(decoded.files, program.files);
        assert_eq!(decoded.cpg.node_count(), program.cpg.node_count());
        assert_eq!(decoded.cpg.edge_count(), program.cpg.edge_count());

        let encoded = serde_json::to_string(&graph).expect("serialize import graph");
        let decoded: ImportGraph =
            serde_json::from_str(&encoded).expect("deserialize import graph");
        assert_eq!(decoded, graph);
    }
}
