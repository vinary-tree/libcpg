//! # libcpg - Code Property Graph Library
//!
//! A Rust library for constructing and analyzing Code Property Graphs (CPGs),
//! which combine Abstract Syntax Trees (AST), Control Flow Graphs (CFG), and
//! Data Flow Graphs (DFG) into a unified representation for program analysis.
//!
//! ## Features
//!
//! - **Graph Construction**: Build CPGs from source code via tree-sitter ASTs
//! - **SCC Analysis**: Decompose per-function CFGs and resolved call graphs
//! - **Condensation Analytics**: Measure SCC-DAG depth, width, critical paths,
//!   and bounded Coffman-Graham layers
//! - **CFG Reachability**: Report unreachable nodes and structural dead branches
//! - **Basic-block CFGs**: Group reachable CFG nodes without losing CPG ids
//! - **Monotone Data Flow**: Solve forward/backward analyses over block CFGs
//! - **Sparse Constant Propagation**: Prove scalar constants and executable CFG edges
//! - **Interval Value Ranges**: Bound integers with widening and narrowing
//! - **Loop Induction**: Find invariant computations, induction variables, and counted guards
//! - **Graph Projections**: Build deterministic forward/reverse CSR views
//! - **Graph Cohesion**: Compute cores, trusses, cut structure, weighted seams,
//!   endpoint assortativity, and bounded attack traces
//! - **Structural Clones**: Refine bounded Weisfeiler-Lehman projection labels
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

pub mod analysis;
pub mod builder;
pub mod graph;

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
    CfgEdgeKind, CodePropertyGraph, CpgEdge, CpgEdgeKind, CpgNode, CpgNodeKind, CpgStats,
    DfgEdgeKind, EdgeId, Language, LiteralKind, MethodSignature, NodeId, Paradigm, PropertyKey,
    PropertyValue, ScopeId, SourceRange, TypeInfo, Visibility,
};

pub use analysis::{
    accumulate_for_sources, analyze_dsm, attack_simulation, available_expressions,
    betweenness_centrality, block_cfg, build_symbol_table, burt_constraint, call_graph_projection,
    call_graph_sccs, cfg_projection, classify_core_periphery, closeness_centrality, coffman_graham,
    condensation_metrics, control_flow_sccs, cut_vertices_and_bridges, dead_stores,
    degree_assortativity, dfg_projection, dominator_tree, edge_betweenness, eigenvector_centrality,
    expression_facts, global_min_cut, harmonic_centrality, hits, k_core_decomposition,
    k_truss_decomposition, katz_centrality, liveness, loop_forest, loop_induction, louvain,
    module_graph_projection, pagerank, pdg_projection, personalized_pagerank, post_dominator_tree,
    projection_of, projection_sccs, refine_connected, resolve_calls, reverse_pagerank, sccp, solve,
    structural_clone_classes, try_refine_connected, two_edge_connected_components,
    unreachable_code, value_ranges, variable_facts, very_busy_expressions, wl_labels,
    AnalysisError, AttackResult, AttackStep, AvailableExpressions, BasicBlock, BetweennessOptions,
    BitSetDomain, BlockCfg, BlockEdge, BlockId, Bound, CallGraphResolution, CallResolution,
    CentralityBudget, CentralityKind, CentralityScores, CgLayering, CommunityAssignment,
    CommunityKind, CondensationMetrics, CondensationOptions, Confidence, Confluence, ConstLattice,
    ConstValue, CorePeriphery, CorePeripheryResult, CutStructure, DataflowAnalysis,
    DataflowSolution, DeadBranch, DeadStore, DeadStoreReport, Direction, DominatorTree,
    DsmAnalysis, EdgeCentralityScores, ExprFacts, ExprKey, ExprOperand, FactUniverse, GlobalMinCut,
    GraphProjection, HitsScores, InductionAnalysis, InductionVariable, Interval, IntervalEnv,
    IntervalReport, IterationOptions, IvDirection, KCoreDecomposition, KTrussDecomposition,
    KatzOptions, Lattice, LivenessResult, LoopBoundEvidence, LoopExitEdge, LoopForest,
    LoopInvariants, LouvainOptions, MustBits, NaturalLoop, PageRankOptions, PostDominatorTree,
    ProjectionError, ProjectionKind, ReachabilityReport, ResolveConfig, SccAnalysisError,
    SccComponent, SccCondensationEdge, SccDecomposition, SccProjection, SccpResult, SolverConfig,
    SymbolCandidates, SymbolTable, TripEvidence, TrussEdge, TwoEdgeConnectedComponents,
    UndirectedProjection, VarFacts, VariableDefinition, VeryBusyExpressions,
    DEFAULT_CENTRALITY_WORK_LIMIT, MAX_ATTACK_STEPS, MAX_CENTRALITY_ITERATIONS,
    MAX_CONDENSATION_COMPONENTS, MAX_DSM_NODES, MAX_K_TRUSS_EDGES, MAX_LOUVAIN_LEVELS,
    MAX_LOUVAIN_PASSES, MAX_MIN_CUT_NODES, MAX_WL_ITERATIONS,
};

pub use builder::{
    backward_slice, build_def_use_chains, forward_slice, BasicBlockIdentifier, CfgExtractor,
    CfgExtractorConfig, CpgBuilder, CpgBuilderConfig, DefUseChain, Definition, DefinitionKind,
    DfgExtractor, DfgExtractorConfig, PdgBuilder, TreeSitterCpgBuilder, Use, UseKind,
};

#[cfg(feature = "gnn")]
pub use gnn::GraphNeuralNetwork;

pub use pattern::{PatternMatch, SubgraphMatcher};

/// Result type alias for libcpg operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Error types for libcpg operations.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Error reported by an exact structural analysis.
    #[error(transparent)]
    Analysis(#[from] analysis::AnalysisError),

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
