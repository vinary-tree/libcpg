//! Graph Neural Network support for CPGs.
//!
//! This module provides GNN-based message passing and embedding
//! generation for Code Property Graphs.

mod cpgnn;
mod embeddings;

pub use cpgnn::CpgGnn;
pub use embeddings::{NodeEmbedding, SubgraphEmbedding};

use crate::NodeId;

#[cfg(feature = "gnn")]
use ndarray::Array1;

/// Trait for graph neural network operations on CPGs.
///
/// Implementations of this trait can compute node and subgraph
/// embeddings using message passing algorithms.
pub trait GraphNeuralNetwork: Send + Sync {
    /// Performs message passing propagation.
    ///
    /// # Arguments
    /// * `iterations` - Number of message passing iterations
    fn propagate(&mut self, iterations: usize);

    /// Returns the embedding for a specific node.
    #[cfg(feature = "gnn")]
    fn node_embedding(&self, node: NodeId) -> Option<Array1<f32>>;

    /// Returns an aggregated embedding for a subgraph.
    #[cfg(feature = "gnn")]
    fn subgraph_embedding(&self, nodes: &[NodeId]) -> Array1<f32>;

    /// Returns the embedding dimensionality.
    fn embedding_dim(&self) -> usize;

    /// Returns true if embeddings have been computed.
    fn is_initialized(&self) -> bool;

    /// Resets the embeddings.
    fn reset(&mut self);
}
