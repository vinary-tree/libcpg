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
//! - **Architectural Layering**: Reverse condensation longest-path levels and
//!   classify cyclic, skip-layer, and upward dependency violations
//! - **Architectural Smells**: Report cycles, two-sided dependency hubs,
//!   unstable dependencies, and module-size outliers with numerical evidence
//! - **Martin Package Metrics**: Measure afferent/efferent coupling,
//!   instability, abstractness, main-sequence distance, and stable-dependency
//!   violations over explicit module assignments
//! - **Code Metrics**: Compute CK, LCOM1–5, Halstead, cognitive complexity,
//!   and maintainability from resolved CPG evidence
//! - **Reflexion Models**: Compare ordered path-layer rules with observed
//!   dependency paths as convergences, divergences, uncovered edges, and
//!   declared absences
//! - **CFG Reachability**: Report unreachable nodes and structural dead branches
//! - **Basic-block CFGs**: Group reachable CFG nodes without losing CPG ids
//! - **Monotone Data Flow**: Solve forward/backward analyses over block CFGs
//! - **Sparse Constant Propagation**: Prove scalar constants and executable CFG edges
//! - **Interval Value Ranges**: Bound integers with widening and narrowing
//! - **Loop Induction**: Find invariant computations, induction variables, and counted guards
//! - **Graph Projections**: Build deterministic forward/reverse CSR views
//! - **Graph Cohesion**: Compute cores, trusses, cut structure, weighted seams,
//!   endpoint assortativity, and bounded attack traces
//! - **Spectral Connectivity**: Approximate the combinatorial Laplacian's
//!   second eigenpair with bounded deterministic multi-start iteration,
//!   aligned Fiedler vectors, and sign partitions
//! - **Simple Cycles**: Enumerate canonical directed cycles with explicit
//!   length and result-count bounds
//! - **Motif Census**: Count all 16 directed triads and bounded four-node
//!   cliques/out-stars
//! - **Graph Edit Distance**: Compute deterministic bipartite typed edit-script
//!   upper bounds with an explicit 512-node compatibility boundary
//! - **Structural Clones**: Refine bounded Weisfeiler-Lehman projection labels
//! - **Function Clones**: Classify Type-1/2 AST, Type-3 multiset, and bounded
//!   Type-4 PDG WL/VF2 evidence
//! - **Multi-file Programs**: Merge per-file CPGs with collision-free ids and
//!   emit evidence-bounded import/export topology for cross-file calls
//! - **IFDS/IDE Summaries**: Solve caller-defined finite summary domains in
//!   deterministic callee-first SCC order under explicit resource caps
//! - **Effect Summaries**: Compose typed global/parameter/field effects,
//!   transitive purity, return dependencies, and checked scalar constants
//! - **Advisory Taint and Maybe-Null Analysis**: Classify data-driven sources,
//!   sinks, sanitizers, external calls, and dereferences with bounded witnesses
//! - **System Dependence Slicing**: Materialize effect-derived labeled summary
//!   edges and compute deterministic context-sensitive two-pass slices
//! - **Channel Topology**: Extract typed message events through def-use channel
//!   identity and report orphan, blocked, and contended linear consumption
//! - **Deadlock Candidates**: Reuse deterministic SCC projections for initially
//!   blocked communication waits and typed interprocedural lock order
//! - **May-Happen-in-Parallel**: Derive canonical symmetric execution-site
//!   pairs from typed parallel operand subtrees under an explicit output cap
//! - **Session Protocol Sketches** (`rholang`): Compare ordered per-channel
//!   endpoint actions for advisory binary direction and payload-type duality
//! - **Advisory Bug Detectors**: Project resource leaks and uninitialized uses,
//!   propagate bounded exception summaries, and report uncaught root flow and
//!   dead catches with explicit completeness evidence
//! - **Population Anomaly Detectors**: Infer taint-selected missing checks,
//!   call-order conventions, and return-check conventions from exact peer
//!   fractions with qualified advisory evidence
//! - **Clone and Feasibility Advisories**: Diagnose typed divergences in exact
//!   low-edit Type-2/3 clone pairs and prove infeasible branches, redundant
//!   guards, and integer contradictions with SCCP and interval evidence
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
    DfgEdgeKind, EdgeId, Language, LiteralKind, LockKind, LockMode, MethodSignature, MsgKind,
    NodeId, Paradigm, PropertyKey, PropertyList, PropertyListIter, PropertyValue,
    PropertyValueError, PropertyValueKind, PropertyValueLimit, PropertyValueLimits,
    PropertyValueMetrics, PropertyValueRef, ScopeId, SourceRange, TypeInfo, Visibility,
};

pub use analysis::{
    accumulate_for_sources, algebraic_connectivity, analyze_api_usage_mining,
    analyze_deviant_behavior, analyze_dsm, analyze_effects, analyze_exception_flow,
    analyze_inconsistent_clones, analyze_infeasible_paths, analyze_missing_checks,
    analyze_null_flows, analyze_null_flows_report, analyze_resource_leaks, analyze_taint,
    analyze_taint_report, analyze_typestate, analyze_uninitialized_uses, api_usage_misuses,
    architecture_smells, attack_simulation, available_expressions, betweenness_centrality,
    block_cfg, build_import_graph, build_summary_edges, build_symbol_table, burt_constraint,
    call_graph_projection, call_graph_sccs, cfg_projection, channel_report, check_typestate,
    ck_metrics, classify_core_periphery, closeness_centrality, coffman_graham,
    communication_deadlocks, condensation_metrics, control_flow_sccs, cut_vertices_and_bridges,
    dead_catches, dead_stores, degree_assortativity, deviant_behaviors, dfg_projection,
    dominator_tree, edge_betweenness, effect_summaries, eigenvector_centrality, expression_facts,
    expression_facts_with_limits, extract_events, function_metrics, global_min_cut,
    harmonic_centrality, hierarchy_metrics, hits, inconsistent_clone_bugs, infeasible_paths,
    infer_layering, interprocedural_backward_slice, interprocedural_forward_slice,
    k_core_decomposition, k_truss_decomposition, katz_centrality, liveness, loop_forest,
    loop_induction, louvain, martin_metrics, may_happen_in_parallel, merge_programs,
    method_field_graph, missing_checks, module_graph_projection, motif_census, pagerank,
    pdg_projection, personalized_pagerank, post_dominator_tree, projection_of, projection_sccs,
    refine_connected, reflexion, resolve_calls, resource_leaks, reverse_pagerank, sccp,
    sdp_violations, simple_cycles, solve, solve_summaries, solve_summaries_with_resolver,
    structural_clone_classes, try_algebraic_connectivity, try_refine_connected,
    two_edge_connected_components, uncaught_exceptions, uninitialized_uses, unreachable_code,
    value_ranges, variable_facts, very_busy_expressions, wl_labels, AmbientFlowKey, AnalysisError,
    AnalyzedCondition, ApiDataShape, ApiIdentity, ApiLiteralShape, ApiUsageEdge, ApiUsageEdgeKind,
    ApiUsageGraph, ApiUsageMiningAnalysis, ApiUsageMiningConfig, ApiUsageMiningIssue,
    ApiUsageMisuse, ApiUsageMisuseKind, ApiUsageNode, ApiUsageNodeId, ApiUsageNodeKind,
    ApiUsagePattern, ApiUsagePatternKind, ArchSmell, ArchSmellKind, AttackResult, AttackStep,
    AvailableExpressions, BasicBlock, BetweennessOptions, BitSetDomain, BlockCfg, BlockEdge,
    BlockId, Bound, BranchOutcome, CallGraphResolution, CallOrderBelief, CallResolution, CalleeRef,
    CatalogClassification, CatalogMatch, CatalogRule, CentralityBudget, CentralityKind,
    CentralityScores, CgLayering, ChannelEvent, ChannelFinding, ChannelFindingKind, ChannelId,
    CheckFingerprint, CheckFingerprintNode, CheckNodeKind, CkOptions, ClassMetrics,
    CloneBugAnalysis, CloneBugConfig, CloneBugIssue, CloneGuardValue, ClonePairEvidence,
    CommunityAssignment, CommunityKind, CondensationMetrics, CondensationOptions, Confidence,
    Confluence, ConstLattice, ConstValue, ControlContext, CorePeriphery, CorePeripheryResult,
    CutStructure, CycleOptions, CycleSet, DataflowAnalysis, DataflowSolution, DeadBranch,
    DeadCatch, DeadStore, DeadStoreReport, DeadlockFinding, DeadlockFindingKind, DeadlockWitness,
    DeviantBehavior, DeviantBehaviorAnalysis, DeviantBehaviorConfig, DeviantBehaviorIssue,
    DeviantBehaviorKind, Direction, DominatorTree, DsmAnalysis, EdgeCentralityScores,
    EffectAnalysis, EffectSummary, ExceptionFlowAnalysis, ExceptionFlowIssue, ExceptionSummary,
    ExceptionType, ExprFacts, ExprKey, ExprKeyError, ExprKeyLimit, ExprKeyLimits, ExprKeyMetrics,
    ExprKeyRef, ExprOperand, ExprOperandRef, ExprOperands, ExprOperandsIter, ExprUniverse,
    ExternalModel, FactUniverse, FeasibilityProofSource, FileScope, FunctionMetrics, GlobalMinCut,
    GraphProjection, HalsteadMetrics, HierarchyMetrics, HitsScores, IfdsCapEvent, IfdsCaps,
    IfdsIssue, IfdsLattice, IfdsOutput, ImportDependency, ImportGraph, ImportResolution,
    ImportResolutionKind, InconsistentCloneBug, InconsistentCloneKind, InductionAnalysis,
    InductionVariable, InfeasiblePathAnalysis, InfeasiblePathConfig, InfeasiblePathFinding,
    InfeasiblePathIssue, InfeasiblePathKind, Interval, IntervalContradiction, IntervalEnv,
    IntervalPredicateEvidence, IntervalReport, IterationOptions, IvDirection, KCoreDecomposition,
    KTrussDecomposition, KatzOptions, Lattice, LayerSpec, LayeringReport, LayeringViolation,
    LayeringViolationKind, LcomSuite, LivenessResult, LoopBoundEvidence, LoopExitEdge, LoopForest,
    LoopInvariants, LouvainOptions, MartinMetrics, MaybeNull, MethodCall, MethodFieldAccess,
    MethodFieldGraph, MhpPair, MhpRelation, MissingCheck, MissingCheckAnalysis, MissingCheckConfig,
    MissingCheckIssue, ModuleAssignment, MotifCensus, MotifOptions, MustBits,
    NamedTemporalProperty, NaturalLoop, NullAnalysis, NullFlow, NullFlowKey, NullOriginKind,
    NullSummary, PageRankOptions, ParamSinkKey, Pattern, PeerScope, Place, PopulationFraction,
    PopulationThreshold, PostDominatorTree, ProgramCpg, ProjectionError, ProjectionKind,
    PropertyEvent, PropertyFsm, PropertyFsmIssue, PropertyState, Purity, ReachabilityReport,
    ReflexionDivergence, ReflexionRules, ReflexionSummary, ReflexionVerdict, RenameDirection,
    ResolveConfig, ResourceLeak, ResourceLeakAnalysis, ResourceLeakIssue, ReturnCheckBelief,
    ReturnSourceKey, SccAnalysisError, SccComponent, SccCondensationEdge, SccDecomposition,
    SccProjection, SccpResult, SdpViolation, SinkClass, SliceCaps, SmellOptions, SolverConfig,
    SourceClass, Spectral, SpectralOptions, SummaryCtx, SummaryDomain, SymbolCandidates,
    SymbolTable, TaintAnalysis, TaintCatalog, TaintFact, TaintFlow, TaintSummary, TaintWitness,
    TemporalPropertyAnalysis, TemporalPropertyFinding, TemporalPropertyFindingKind, TripEvidence,
    TrussEdge, TwoEdgeConnectedComponents, TypestateAnalysis, TypestateCapEvent, TypestateIssue,
    TypestateTerminal, TypestateViolation, UncaughtException, UndirectedProjection,
    UninitializedUse, UninitializedUseAnalysis, VarFacts, VariableDefinition, VeryBusyExpressions,
    DEFAULT_CENTRALITY_WORK_LIMIT, MAX_API_USAGE_CALL_SITES, MAX_API_USAGE_CFG_NODES_PER_SITE,
    MAX_API_USAGE_GRAPH_EDGES, MAX_API_USAGE_GRAPH_NODES, MAX_ATTACK_STEPS,
    MAX_CENTRALITY_ITERATIONS, MAX_CLONE_ALIGNMENT_CELLS, MAX_CLONE_BUG_AST_NODES,
    MAX_CONDENSATION_COMPONENTS, MAX_CONDITION_NODES, MAX_CONDITION_STATES, MAX_CONDITION_STEPS,
    MAX_CYCLE_LENGTH, MAX_DSM_NODES, MAX_GRAPHLET_NODES, MAX_K_TRUSS_EDGES, MAX_LOUVAIN_LEVELS,
    MAX_LOUVAIN_PASSES, MAX_MHP_PAIRS, MAX_MIN_CUT_NODES, MAX_SIMPLE_CYCLES,
    MAX_SPECTRAL_ITERATIONS, MAX_SPECTRAL_STARTS, MAX_TEMPORAL_PROPERTIES, MAX_TRIAD_NODES,
    MAX_WL_ITERATIONS, SUMMARY_EDGE_LABEL, TRIAD_NAMES,
};

#[cfg(feature = "rholang")]
pub use analysis::{
    session_sketches, SessionAction, SessionConformance, SessionDirection, SessionEndpoint,
    SessionIssue, SessionIssueKind, SessionPayloadType, SessionSketch,
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
