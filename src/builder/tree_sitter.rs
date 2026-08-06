//! Tree-sitter based CPG builder.
//!
//! This module provides CPG construction from tree-sitter ASTs.
//! It converts tree-sitter parse trees into unified CPGs with
//! AST, CFG, and DFG edges.
//!
//! ## Architecture
//!
//! The CPG construction pipeline:
//! 1. **Parsing**: Tree-sitter parses source code into a concrete syntax tree
//! 2. **AST Construction**: Convert tree-sitter nodes to CPG nodes with AST edges
//! 3. **CFG Extraction**: [`CfgExtractor`] analyzes control flow constructs
//! 4. **DFG Extraction**: [`DfgExtractor`] performs reaching definitions analysis
//!
//! ## Example
//!
//! ```ignore
//! use libcpg::builder::TreeSitterCpgBuilder;
//! use libcpg::Language;
//!
//! let builder = TreeSitterCpgBuilder::new();
//! let source = "fn main() { println!(\"Hello, world!\"); }";
//! let cpg = builder.build(source, Language::Rust)?;
//!
//! // The CPG now contains AST nodes with parent-child edges,
//! // CFG edges for control flow, and DFG edges for data flow.
//! ```

use super::{CfgExtractor, CpgBuilder, CpgBuilderConfig, DfgExtractor, NodeMapper, ParserRegistry};
use crate::{
    CodePropertyGraph, CpgEdgeKind, CpgNode, CpgNodeKind, Error, Language, NodeId, Result,
    SourceRange,
};

/// CPG builder using tree-sitter for parsing.
///
/// This builder supports multiple programming languages through
/// tree-sitter's unified parsing interface.
#[derive(Debug, Clone)]
pub struct TreeSitterCpgBuilder {
    /// Configuration options.
    config: CpgBuilderConfig,
}

impl TreeSitterCpgBuilder {
    /// Creates a new tree-sitter CPG builder with default configuration.
    pub fn new() -> Self {
        Self {
            config: CpgBuilderConfig::default(),
        }
    }

    /// Creates a builder with custom configuration.
    pub fn with_config(config: CpgBuilderConfig) -> Self {
        Self { config }
    }

    /// Returns the configuration.
    pub fn config(&self) -> &CpgBuilderConfig {
        &self.config
    }

    /// Sets a new configuration.
    pub fn set_config(&mut self, config: CpgBuilderConfig) {
        self.config = config;
    }

    /// Builds a CPG from a caller-supplied tree-sitter parse tree, skipping the
    /// internal parse step.
    ///
    /// This is the "Mode B" entry point: a caller that has already parsed
    /// `source` with a tree-sitter grammar — for example to avoid a second
    /// parse, or to reuse a grammar crate it already links — hands the
    /// [`tree_sitter::Tree`] directly instead of a source string. `language`
    /// selects the [`NodeMapper`]; the caller must guarantee `tree` was produced
    /// by a grammar whose node-kind strings that mapper understands (otherwise
    /// nodes fall through to the generic mapping).
    ///
    /// The post-parse pipeline is identical to [`CpgBuilder::build`]: AST
    /// construction, then config-gated CFG and DFG extraction. Unlike `build`,
    /// no `max_file_size` check is applied — the caller already owns the tree.
    pub fn build_from_tree(
        &self,
        tree: &tree_sitter::Tree,
        source: &str,
        language: Language,
    ) -> Result<CodePropertyGraph> {
        let mut cpg = CodePropertyGraph::new(language);

        if self.config.retain_source {
            cpg = cpg.with_source_code(source);
        }

        // Phase 1: Build AST from tree-sitter parse tree
        let mapper = NodeMapper::new(language);
        self.build_ast(tree, source, &mapper, &mut cpg)?;

        // Phase 2: Extract CFG
        if self.config.build_cfg {
            let cfg_extractor = CfgExtractor::new();
            cfg_extractor.extract(&mut cpg);
        }

        // Phase 3: Extract DFG
        if self.config.build_dfg {
            let dfg_extractor = DfgExtractor::new();
            dfg_extractor.extract(&mut cpg);
        }

        Ok(cpg)
    }
}

impl Default for TreeSitterCpgBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl CpgBuilder for TreeSitterCpgBuilder {
    fn build(&self, source: &str, language: Language) -> Result<CodePropertyGraph> {
        // Check file size
        if source.len() > self.config.max_file_size {
            return Err(Error::Construction(format!(
                "Source file exceeds maximum size of {} bytes",
                self.config.max_file_size
            )));
        }

        // Get the tree-sitter language from the registry
        let registry = ParserRegistry::global();
        let ts_language = registry
            .get(language)
            .ok_or_else(|| Error::UnsupportedLanguage(language.name().to_string()))?;

        // Create parser and set language
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&ts_language)
            .map_err(|e| Error::Construction(format!("Failed to set parser language: {}", e)))?;

        // Parse the source code
        let tree = parser
            .parse(source, None)
            .ok_or_else(|| Error::Construction("Failed to parse source code".to_string()))?;

        // Delegate the post-parse pipeline (AST + CFG + DFG) to the shared
        // `build_from_tree` entry point so both paths stay identical.
        self.build_from_tree(&tree, source, language)
    }

    fn supported_languages(&self) -> &[Language] {
        // Return languages that have grammars registered
        // This is a static list of potentially supported languages;
        // actual support depends on enabled Cargo features
        &[
            Language::Rust,
            Language::Python,
            Language::JavaScript,
            Language::TypeScript,
            Language::Go,
            Language::Java,
            Language::C,
            Language::Cpp,
            Language::Ruby,
            Language::Bash,
            Language::Json,
            Language::Yaml,
            Language::Toml,
            Language::Html,
            Language::Css,
            Language::Markdown,
        ]
    }
}

impl TreeSitterCpgBuilder {
    /// Builds the AST portion of the CPG from a tree-sitter parse tree.
    fn build_ast(
        &self,
        tree: &tree_sitter::Tree,
        source: &str,
        mapper: &NodeMapper,
        cpg: &mut CodePropertyGraph,
    ) -> Result<()> {
        let root_node = tree.root_node();

        // Recursively convert tree-sitter nodes to CPG nodes
        self.convert_node(&root_node, None, source, mapper, cpg)?;

        Ok(())
    }

    /// Recursively converts a tree-sitter node and its children to CPG nodes.
    fn convert_node(
        &self,
        ts_node: &tree_sitter::Node,
        parent_id: Option<NodeId>,
        source: &str,
        mapper: &NodeMapper,
        cpg: &mut CodePropertyGraph,
    ) -> Result<Option<NodeId>> {
        let ts_kind = ts_node.kind();

        // Check if this node should be included. Uses the node-aware test so a
        // language mapper can distinguish an anonymous keyword/operator *token*
        // from a same-named semantic *rule* node (e.g. Rholang's `contract`).
        if !mapper.should_include_node(ts_node, self.config.include_comments) {
            // Still process children even if this node is skipped
            let mut cursor = ts_node.walk();
            for child in ts_node.children(&mut cursor) {
                self.convert_node(&child, parent_id, source, mapper, cpg)?;
            }
            return Ok(None);
        }

        // Map the tree-sitter node kind to CPG node kind
        let cpg_kind = mapper.map_kind(ts_kind, ts_node, source);

        // Create source range from tree-sitter positions
        let range = SourceRange::from_bytes(ts_node.start_byte() as u32, ts_node.end_byte() as u32);

        // Create the CPG node
        let cpg_node = CpgNode::new(NodeId::new(0), cpg_kind.clone(), range);
        let node_id = cpg.add_node(cpg_node);

        // Create the AST edge from parent to this node AND record the parent
        // pointer on the node itself. `ast_parent`/`ast_ancestors` read the
        // pointer, not the edge; leaving it unset made every parsed node look
        // like an AST root, silently breaking every ancestor-based analysis
        // (def/use classification, enclosing-statement lookup, program slicing).
        if let Some(parent) = parent_id {
            cpg.connect(parent, node_id, CpgEdgeKind::AstChild);
            if let Some(node) = cpg.node_mut(node_id) {
                node.parent = Some(parent);
            }
        }

        // Mark function nodes as CFG entry points
        if matches!(cpg_kind, CpgNodeKind::Function { .. }) {
            cpg.add_cfg_entry(node_id);
        }

        // Recursively convert children
        let mut cursor = ts_node.walk();
        for child in ts_node.children(&mut cursor) {
            self.convert_node(&child, Some(node_id), source, mapper, cpg)?;
        }

        Ok(Some(node_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_creation() {
        let builder = TreeSitterCpgBuilder::new();
        // Supported languages depend on enabled features
        let supported = builder.supported_languages();
        assert!(!supported.is_empty());
    }

    #[test]
    #[cfg(feature = "lang-rust")]
    fn test_build_rust() {
        let builder = TreeSitterCpgBuilder::new();
        let source = "fn main() { println!(\"hello\"); }";
        let cpg = builder
            .build(source, Language::Rust)
            .expect("build should succeed");

        // Should have multiple nodes (root, function, block, etc.)
        assert!(cpg.node_count() > 1);
        assert_eq!(cpg.language(), Language::Rust);
    }

    #[test]
    #[cfg(feature = "lang-python")]
    fn test_build_python() {
        let builder = TreeSitterCpgBuilder::new();
        let source = "def main():\n    print('hello')";
        let cpg = builder
            .build(source, Language::Python)
            .expect("build should succeed");

        assert!(cpg.node_count() > 1);
        assert_eq!(cpg.language(), Language::Python);
    }

    #[test]
    #[cfg(feature = "lang-javascript")]
    fn test_build_javascript() {
        let builder = TreeSitterCpgBuilder::new();
        let source = "function main() { console.log('hello'); }";
        let cpg = builder
            .build(source, Language::JavaScript)
            .expect("build should succeed");

        assert!(cpg.node_count() > 1);
        assert_eq!(cpg.language(), Language::JavaScript);
    }

    #[test]
    fn test_config() {
        let config = CpgBuilderConfig::new()
            .with_source(true)
            .with_cfg(true)
            .with_dfg(false)
            .with_comments(true);

        let builder = TreeSitterCpgBuilder::with_config(config);
        assert!(builder.config().retain_source);
        assert!(builder.config().build_cfg);
        assert!(!builder.config().build_dfg);
        assert!(builder.config().include_comments);
    }

    #[test]
    fn test_file_size_limit() {
        let config = CpgBuilderConfig::new().with_max_file_size(10);
        let builder = TreeSitterCpgBuilder::with_config(config);

        let source = "fn main() { }"; // > 10 bytes
        let result = builder.build(source, Language::Rust);

        assert!(result.is_err());
    }

    #[test]
    fn test_unsupported_language() {
        let builder = TreeSitterCpgBuilder::new();
        let result = builder.build("code", Language::Unknown);
        assert!(result.is_err());
    }

    #[test]
    #[cfg(feature = "lang-rust")]
    fn test_function_entry_points() {
        let builder = TreeSitterCpgBuilder::new();
        let source = r#"
            fn foo() {}
            fn bar() {}
        "#;
        let cpg = builder
            .build(source, Language::Rust)
            .expect("build should succeed");

        // Should have CFG entry points for both functions
        let entry_points: Vec<_> = cpg.cfg_entries().to_vec();
        assert_eq!(entry_points.len(), 2);
    }

    #[test]
    #[cfg(feature = "lang-rust")]
    fn test_build_from_tree_matches_build() {
        let source = "fn main() { let x = 1; let y = x; }";

        // Parse externally, exactly as a Mode-B caller (e.g. pgmcp) would.
        let ts_language = ParserRegistry::global()
            .get(Language::Rust)
            .expect("rust grammar should be registered");
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&ts_language)
            .expect("set language should succeed");
        let tree = parser.parse(source, None).expect("parse should succeed");

        let builder = TreeSitterCpgBuilder::new();
        let from_tree = builder
            .build_from_tree(&tree, source, Language::Rust)
            .expect("build_from_tree should succeed");
        let from_source = builder
            .build(source, Language::Rust)
            .expect("build should succeed");

        // The caller-supplied-tree path must produce a graph identical in shape
        // to the internal-parse path (same nodes, same edges, same language).
        assert!(from_tree.node_count() > 1);
        assert_eq!(from_tree.node_count(), from_source.node_count());
        assert_eq!(from_tree.edge_count(), from_source.edge_count());
        assert_eq!(from_tree.language(), Language::Rust);
    }

    #[test]
    #[cfg(feature = "lang-rust")]
    fn test_retain_source() {
        let config = CpgBuilderConfig::new().with_source(true);
        let builder = TreeSitterCpgBuilder::with_config(config);
        let source = "fn main() {}";
        let cpg = builder
            .build(source, Language::Rust)
            .expect("build should succeed");

        assert!(cpg.source_code().is_some());
        assert_eq!(cpg.source_code().unwrap(), source);
    }

    // ================================================================
    // Per-language mapper coverage: build one small snippet per grammar
    // (under `lang-all`) and assert node_count > 1 + a plausible kind.
    // ================================================================

    /// True if the CPG carries at least one *semantic* node — not merely the
    /// Root, an Unknown fall-through, an Error, or a Comment.
    /// Used only by the `lang-rust`-gated tests below.
    #[allow(dead_code)]
    fn has_meaningful_kind(cpg: &CodePropertyGraph) -> bool {
        cpg.nodes().any(|n| {
            !matches!(
                n.kind,
                CpgNodeKind::Root
                    | CpgNodeKind::Unknown { .. }
                    | CpgNodeKind::Error { .. }
                    | CpgNodeKind::Comment { .. }
            )
        })
    }

    #[test]
    #[cfg(feature = "lang-go")]
    fn build_go_maps_function() {
        let cpg = TreeSitterCpgBuilder::new()
            .build("package main\nfunc main() {}\n", Language::Go)
            .expect("build go");
        assert!(cpg.node_count() > 1);
        assert!(cpg.functions().count() >= 1, "go `func` ⇒ Function");
    }

    #[test]
    #[cfg(feature = "lang-java")]
    fn build_java_maps_class() {
        let cpg = TreeSitterCpgBuilder::new()
            .build("class A { void m() {} }", Language::Java)
            .expect("build java");
        assert!(cpg.node_count() > 1);
        assert!(cpg.classes().count() >= 1, "java `class` ⇒ Class");
    }

    #[test]
    #[cfg(feature = "lang-c")]
    fn build_c_maps_function() {
        let cpg = TreeSitterCpgBuilder::new()
            .build("int main() { return 0; }", Language::C)
            .expect("build c");
        assert!(cpg.node_count() > 1);
        assert!(cpg.functions().count() >= 1, "C function ⇒ Function");
    }

    #[test]
    #[cfg(feature = "lang-cpp")]
    fn build_cpp_maps_function() {
        let cpg = TreeSitterCpgBuilder::new()
            .build("int main() { return 0; }", Language::Cpp)
            .expect("build cpp");
        assert!(cpg.node_count() > 1);
        assert!(cpg.functions().count() >= 1, "C++ function ⇒ Function");
    }

    #[test]
    #[cfg(feature = "lang-ruby")]
    fn build_ruby_maps_method() {
        let cpg = TreeSitterCpgBuilder::new()
            .build("def m\n  1\nend\n", Language::Ruby)
            .expect("build ruby");
        assert!(cpg.node_count() > 1);
        assert!(cpg.functions().count() >= 1, "ruby `def` ⇒ Function");
    }

    #[test]
    #[cfg(feature = "lang-bash")]
    fn build_bash_maps_command() {
        let cpg = TreeSitterCpgBuilder::new()
            .build("echo hi\n", Language::Bash)
            .expect("build bash");
        assert!(cpg.node_count() > 1);
        assert!(cpg.calls().count() >= 1, "bash command ⇒ Call");
    }

    #[test]
    #[cfg(feature = "lang-json")]
    fn build_json_maps_meaningfully() {
        let cpg = TreeSitterCpgBuilder::new()
            .build("{\"a\": 1, \"b\": [true, null]}", Language::Json)
            .expect("build json");
        assert!(cpg.node_count() > 1);
        assert!(has_meaningful_kind(&cpg), "json ⇒ Object/Field/Literal");
    }

    #[test]
    #[cfg(feature = "lang-html")]
    fn build_html_maps_meaningfully() {
        let cpg = TreeSitterCpgBuilder::new()
            .build("<div class=\"x\">hi</div>", Language::Html)
            .expect("build html");
        assert!(cpg.node_count() > 1);
        assert!(
            has_meaningful_kind(&cpg),
            "html text/attribute ⇒ Literal/Attribute"
        );
    }

    #[test]
    #[cfg(feature = "lang-css")]
    fn build_css_maps_meaningfully() {
        let cpg = TreeSitterCpgBuilder::new()
            .build("a { color: red; }", Language::Css)
            .expect("build css");
        assert!(cpg.node_count() > 1);
        assert!(
            has_meaningful_kind(&cpg),
            "css rule ⇒ Block/Identifier/Variable"
        );
    }

    #[test]
    #[cfg(feature = "lang-toml")]
    fn build_toml_maps_meaningfully() {
        let cpg = TreeSitterCpgBuilder::new()
            .build("a = 1\nb = \"s\"\n", Language::Toml)
            .expect("build toml");
        assert!(cpg.node_count() > 1);
        assert!(has_meaningful_kind(&cpg), "toml pair ⇒ Field/Literal");
    }

    #[test]
    #[cfg(feature = "lang-yaml")]
    fn build_yaml_maps_meaningfully() {
        let cpg = TreeSitterCpgBuilder::new()
            .build("a: 1\nb: s\n", Language::Yaml)
            .expect("build yaml");
        assert!(cpg.node_count() > 1);
        assert!(has_meaningful_kind(&cpg), "yaml mapping ⇒ Field/Literal");
    }

    #[test]
    #[cfg(feature = "lang-markdown")]
    fn build_markdown_maps_meaningfully() {
        let cpg = TreeSitterCpgBuilder::new()
            .build("# Title\n\nSome text.\n", Language::Markdown)
            .expect("build markdown");
        assert!(cpg.node_count() > 1);
        assert!(
            has_meaningful_kind(&cpg),
            "markdown section/paragraph ⇒ Block"
        );
    }
}

#[cfg(all(test, feature = "lang-rust"))]
mod proptests {
    use super::*;
    use crate::testutil::*;
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        /// `build` ≡ `build_from_tree`: a caller-supplied tree (parsed with
        /// `tree_sitter_rust::LANGUAGE`) yields a graph identical in node count,
        /// edge count, and language to the internal-parse path.
        #[test]
        fn prop_build_eq_build_from_tree(src in arb_rust_source()) {
            let mut parser = tree_sitter::Parser::new();
            parser
                .set_language(&tree_sitter_rust::LANGUAGE.into())
                .expect("set rust grammar");
            let tree = parser.parse(&src, None).expect("parse");

            let builder = TreeSitterCpgBuilder::new();
            let from_tree = builder
                .build_from_tree(&tree, &src, Language::Rust)
                .expect("build_from_tree");
            let from_source = builder.build(&src, Language::Rust).expect("build");

            prop_assert_eq!(from_tree.node_count(), from_source.node_count());
            prop_assert_eq!(from_tree.edge_count(), from_source.edge_count());
            prop_assert_eq!(from_tree.language(), from_source.language());
            prop_assert_eq!(from_tree.language(), Language::Rust);
        }

        /// Parent ↔ AstChild consistency in a `build`-produced CPG: every
        /// `AstChild` edge `p→c` has `node(c).parent == Some(p)`, and no node has
        /// more than one incoming AST-child (parent) edge.
        #[test]
        fn prop_parent_astchild_consistent(src in arb_rust_source()) {
            let cpg = TreeSitterCpgBuilder::new()
                .build(&src, Language::Rust)
                .expect("build");

            for e in cpg.edges() {
                if matches!(e.kind, CpgEdgeKind::AstChild) {
                    let child = cpg.node(e.target).expect("edge target exists");
                    prop_assert_eq!(child.parent, Some(e.source));
                }
            }
            for id in cpg.node_ids() {
                let ast_parents = cpg
                    .incoming_edges(id)
                    .filter(|e| matches!(e.kind, CpgEdgeKind::AstChild))
                    .count();
                prop_assert!(ast_parents <= 1);
            }
        }
    }
}
