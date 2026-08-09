//! Exact structural analyses over Code Property Graph projections.
//!
//! Unlike the feature-gated algorithm-family detector, analyses in this module
//! compute graph properties exactly. They are available with `default = []`.

mod projection;
mod scc;

use crate::NodeId;

pub use projection::{
    call_graph_projection, cfg_projection, dfg_projection, module_graph_projection, pdg_projection,
    projection_of, FunctionCfgView, GraphProjection, ProjectionError, ProjectionKind,
    UndirectedProjection,
};

pub use scc::{
    call_graph_sccs, control_flow_sccs, SccAnalysisError, SccComponent, SccCondensationEdge,
    SccDecomposition, SccProjection,
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
    }
}
