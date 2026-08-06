//! Exact structural analyses over Code Property Graph projections.
//!
//! Unlike the feature-gated algorithm-family detector, analyses in this module
//! compute graph properties exactly. They are available with `default = []`.

mod scc;

pub use scc::{
    call_graph_sccs, control_flow_sccs, SccAnalysisError, SccComponent, SccCondensationEdge,
    SccDecomposition, SccProjection,
};
