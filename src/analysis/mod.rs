//! Exact structural analyses over Code Property Graph projections.
//!
//! Unlike the feature-gated algorithm-family detector, analyses in this module
//! compute graph properties exactly. They are available with `default = []`.

mod basic_blocks;
pub(crate) mod cfg_scope;
mod condensation;
pub mod dataflow;
mod dominators;
mod dsm;
mod induction;
mod loops;
mod projection;
mod reachability;
mod resolve;
mod scc;

use crate::NodeId;

pub use basic_blocks::{block_cfg, BasicBlock, BlockCfg, BlockEdge, BlockId};

pub use condensation::{
    coffman_graham, condensation_metrics, CgLayering, CondensationMetrics, CondensationOptions,
    MAX_CONDENSATION_COMPONENTS,
};

pub use dataflow::{
    available_expressions, dead_stores, expression_facts, liveness, sccp, solve, value_ranges,
    variable_facts, very_busy_expressions, AvailableExpressions, BitSetDomain, Bound, Confluence,
    ConstLattice, ConstValue, DataflowAnalysis, DataflowSolution, DeadStore, DeadStoreReport,
    Direction, ExprFacts, ExprKey, ExprOperand, FactUniverse, Interval, IntervalEnv,
    IntervalReport, Lattice, LivenessResult, MustBits, SccpResult, SolverConfig, VarFacts,
    VariableDefinition, VeryBusyExpressions,
};

pub use projection::{
    call_graph_projection, cfg_projection, dfg_projection, module_graph_projection, pdg_projection,
    projection_of, GraphProjection, ProjectionError, ProjectionKind, UndirectedProjection,
};

pub use dominators::{dominator_tree, post_dominator_tree, DominatorTree, PostDominatorTree};

pub use dsm::{
    analyze_dsm, classify_core_periphery, CorePeriphery, CorePeripheryResult, DsmAnalysis,
    MAX_DSM_NODES,
};

pub use induction::{
    loop_induction, InductionAnalysis, InductionVariable, IvDirection, LoopBoundEvidence,
    LoopInvariants, TripEvidence,
};

pub use loops::{loop_forest, LoopExitEdge, LoopForest, NaturalLoop};

pub use reachability::{unreachable_code, DeadBranch, ReachabilityReport};

pub use resolve::{
    build_symbol_table, resolve_calls, CallGraphResolution, CallResolution, Confidence,
    ResolveConfig, SymbolCandidates, SymbolTable,
};

pub(crate) use scc::function_scope_nodes;
pub use scc::{
    call_graph_sccs, control_flow_sccs, projection_sccs, SccAnalysisError, SccComponent,
    SccCondensationEdge, SccDecomposition, SccProjection,
};

/// Errors shared by exact structural analyses.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AnalysisError {
    /// The requested node does not exist in the analyzed projection or CPG.
    #[error("analysis node does not exist: {0:?}")]
    UnknownNode(NodeId),
    /// The requested node exists but is not a function.
    #[error("analysis requires a function node: {0:?}")]
    NotAFunction(NodeId),
    /// A fixed-point analysis did not converge within its iteration cap.
    #[error("{analysis} exceeded its iteration limit of {limit}")]
    IterationLimitExceeded {
        /// Stable analysis name used in diagnostics.
        analysis: &'static str,
        /// Configured iteration cap.
        limit: u64,
    },
    /// An analysis would exceed an explicit resource budget.
    #[error("{analysis} needs {needed} {unit}, exceeding the configured cap of {cap}")]
    BudgetExceeded {
        /// Stable analysis name used in diagnostics.
        analysis: &'static str,
        /// Resource unit, such as `states` or `bytes`.
        unit: &'static str,
        /// Resource amount required for the requested operation.
        needed: u64,
        /// Configured maximum resource amount.
        cap: u64,
    },
    /// Two composable analysis results were produced for different functions.
    #[error("analysis result function mismatch: expected {expected:?}, got {actual:?}")]
    FunctionMismatch {
        /// Function selected by the receiving result.
        expected: NodeId,
        /// Function selected by the supplied refinement.
        actual: NodeId,
    },
    /// An analysis received a structurally invalid input or option.
    #[error("{analysis} received invalid {input}: {reason}")]
    InvalidInput {
        /// Stable analysis name used in diagnostics.
        analysis: &'static str,
        /// Input or option whose contract was violated.
        input: &'static str,
        /// Human-readable description of the violated contract.
        reason: &'static str,
    },
    /// Projection construction failed before the analysis could run.
    #[error(transparent)]
    Projection(#[from] ProjectionError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projection_errors_convert_without_losing_their_cause() {
        let node = NodeId::new(41);
        let error = AnalysisError::from(ProjectionError::UnknownNode(node));
        assert_eq!(
            error,
            AnalysisError::Projection(ProjectionError::UnknownNode(node))
        );
        assert!(error.to_string().contains("does not exist"));
    }

    #[test]
    fn resource_errors_report_analysis_limits_and_units() {
        let iteration = AnalysisError::IterationLimitExceeded {
            analysis: "pagerank",
            limit: 50,
        };
        assert_eq!(
            iteration.to_string(),
            "pagerank exceeded its iteration limit of 50"
        );

        let budget = AnalysisError::BudgetExceeded {
            analysis: "paths",
            unit: "states",
            needed: 11,
            cap: 10,
        };
        assert_eq!(
            budget.to_string(),
            "paths needs 11 states, exceeding the configured cap of 10"
        );

        let invalid = AnalysisError::InvalidInput {
            analysis: "coffman_graham",
            input: "width",
            reason: "must be at least one",
        };
        assert_eq!(
            invalid.to_string(),
            "coffman_graham received invalid width: must be at least one"
        );
    }
}
