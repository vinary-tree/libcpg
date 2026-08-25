//! Deterministic, feature-free exports for external machine-learning systems.
//!
//! libcpg owns graph construction and structural feature extraction. Model
//! training, tensor runtimes, and learned weights remain outside the core.

mod export;
mod hetero;

pub use export::{
    ast_paths, edge_kind_class, export_pyg, export_pyg_with_options, node_kind_class,
    AstPathOptions, PathContext, PygExport, PygExportOptions, EDGE_KIND_CLASS_COUNT,
    EDGE_KIND_CLASS_NAMES, MAX_AST_PATH_ANCESTOR_STEPS, MAX_AST_PATH_CONTEXTS, MAX_AST_PATH_LENGTH,
    MAX_AST_PATH_PAIR_CHECKS, MAX_AST_PATH_TERMINALS, MAX_AST_PATH_TOTAL_NODES,
    PYG_NODE_FEATURE_COUNT, PYG_NODE_FEATURE_NAMES,
};
pub use hetero::{
    export_heterogeneous_gnn, HeteroGnnExport, HeteroRelationEdges, HETERO_NODE_FEATURE_COUNT,
    HETERO_TOKEN_EMBEDDING_DIM,
};

#[cfg(test)]
mod tests;
