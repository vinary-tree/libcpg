//! CPGNN (Code Property Graph Neural Network) implementation.
//!
//! This module implements message passing on CPGs using the
//! structure from "Devign: Effective Vulnerability Identification"
//! and related work on GNN-based code understanding.

use rustc_hash::FxHashMap;

#[cfg(feature = "gnn")]
use ndarray::Array1;

#[cfg(feature = "gnn")]
use rand::Rng;

use crate::{CodePropertyGraph, NodeId, CpgNodeKind};
use super::GraphNeuralNetwork;

/// CPGNN - Graph Neural Network for Code Property Graphs.
///
/// Uses message passing along AST, CFG, and DFG edges to
/// learn contextual node representations.
#[derive(Debug)]
pub struct CpgGnn {
    /// The underlying CPG.
    cpg: CodePropertyGraph,
    /// Embedding dimension.
    embedding_dim: usize,
    /// Node embeddings (after propagation).
    #[cfg(feature = "gnn")]
    embeddings: Option<FxHashMap<NodeId, Array1<f32>>>,
    /// Whether embeddings have been computed.
    initialized: bool,
    /// Number of GNN layers.
    num_layers: usize,
    /// Dropout rate for training.
    dropout: f32,
}

impl CpgGnn {
    /// Creates a new CPGNN for the given graph.
    pub fn new(cpg: CodePropertyGraph) -> Self {
        Self {
            cpg,
            embedding_dim: 128,
            #[cfg(feature = "gnn")]
            embeddings: None,
            initialized: false,
            num_layers: 3,
            dropout: 0.1,
        }
    }

    /// Sets the embedding dimension.
    pub fn with_embedding_dim(mut self, dim: usize) -> Self {
        self.embedding_dim = dim;
        self
    }

    /// Sets the number of GNN layers.
    pub fn with_num_layers(mut self, layers: usize) -> Self {
        self.num_layers = layers;
        self
    }

    /// Sets the dropout rate.
    pub fn with_dropout(mut self, dropout: f32) -> Self {
        self.dropout = dropout;
        self
    }

    /// Returns a reference to the underlying CPG.
    pub fn cpg(&self) -> &CodePropertyGraph {
        &self.cpg
    }

    /// Initializes node embeddings based on node types.
    #[cfg(feature = "gnn")]
    fn initialize_embeddings(&mut self) {
        let mut rng = rand::thread_rng();
        let mut embeddings = FxHashMap::default();

        for node in self.cpg.nodes() {
            // Initialize with random values, seeded by node kind
            let mut embedding = Array1::zeros(self.embedding_dim);
            for i in 0..self.embedding_dim {
                embedding[i] = rng.gen_range(-0.1..0.1);
            }

            // Add type-specific features
            let type_features = self.node_type_features(&node.kind);
            for (i, &f) in type_features.iter().enumerate() {
                if i < self.embedding_dim {
                    embedding[i] += f;
                }
            }

            embeddings.insert(node.id, embedding);
        }

        self.embeddings = Some(embeddings);
    }

    /// Returns initial features based on node type.
    fn node_type_features(&self, kind: &CpgNodeKind) -> Vec<f32> {
        // Create a one-hot-like encoding for major node categories
        let mut features = vec![0.0; 16];

        match kind {
            CpgNodeKind::Root => features[0] = 1.0,
            CpgNodeKind::Module { .. } => features[1] = 1.0,
            CpgNodeKind::Class { .. } | CpgNodeKind::Struct { .. } => features[2] = 1.0,
            CpgNodeKind::Function { .. } => features[3] = 1.0,
            CpgNodeKind::Variable { .. } | CpgNodeKind::Field { .. } => features[4] = 1.0,
            CpgNodeKind::If | CpgNodeKind::While | CpgNodeKind::For | CpgNodeKind::Loop => features[5] = 1.0,
            CpgNodeKind::Return | CpgNodeKind::Break | CpgNodeKind::Continue => features[6] = 1.0,
            CpgNodeKind::Call { .. } => features[7] = 1.0,
            CpgNodeKind::BinaryOp { .. } | CpgNodeKind::UnaryOp { .. } => features[8] = 1.0,
            CpgNodeKind::Assignment { .. } => features[9] = 1.0,
            CpgNodeKind::Identifier { .. } => features[10] = 1.0,
            CpgNodeKind::Literal { .. } => features[11] = 1.0,
            CpgNodeKind::Block { .. } => features[12] = 1.0,
            CpgNodeKind::Parameter { .. } => features[13] = 1.0,
            CpgNodeKind::Try | CpgNodeKind::Catch | CpgNodeKind::Throw => features[14] = 1.0,
            _ => features[15] = 1.0,
        }

        features
    }
}

impl GraphNeuralNetwork for CpgGnn {
    fn propagate(&mut self, iterations: usize) {
        #[cfg(feature = "gnn")]
        {
            if self.embeddings.is_none() {
                self.initialize_embeddings();
            }

            let embeddings = self.embeddings.as_mut().expect("embeddings initialized");

            // Message passing iterations
            for _ in 0..iterations {
                let mut new_embeddings = FxHashMap::default();

                for node in self.cpg.nodes() {
                    let current = embeddings.get(&node.id).cloned()
                        .unwrap_or_else(|| Array1::zeros(self.embedding_dim));

                    // Aggregate neighbor embeddings
                    let mut aggregated = current.clone();
                    let mut neighbor_count = 0;

                    // Aggregate from AST neighbors
                    for child_id in self.cpg.ast_children(node.id) {
                        if let Some(child_emb) = embeddings.get(&child_id) {
                            aggregated = aggregated + child_emb;
                            neighbor_count += 1;
                        }
                    }
                    if let Some(parent_id) = self.cpg.ast_parent(node.id) {
                        if let Some(parent_emb) = embeddings.get(&parent_id) {
                            aggregated = aggregated + parent_emb;
                            neighbor_count += 1;
                        }
                    }

                    // Aggregate from CFG neighbors
                    for (succ_id, _) in self.cpg.cfg_successors(node.id) {
                        if let Some(succ_emb) = embeddings.get(&succ_id) {
                            aggregated = aggregated + succ_emb;
                            neighbor_count += 1;
                        }
                    }
                    for (pred_id, _) in self.cpg.cfg_predecessors(node.id) {
                        if let Some(pred_emb) = embeddings.get(&pred_id) {
                            aggregated = aggregated + pred_emb;
                            neighbor_count += 1;
                        }
                    }

                    // Aggregate from DFG neighbors
                    for (succ_id, _) in self.cpg.dfg_successors(node.id) {
                        if let Some(succ_emb) = embeddings.get(&succ_id) {
                            aggregated = aggregated + succ_emb;
                            neighbor_count += 1;
                        }
                    }
                    for (pred_id, _) in self.cpg.dfg_predecessors(node.id) {
                        if let Some(pred_emb) = embeddings.get(&pred_id) {
                            aggregated = aggregated + pred_emb;
                            neighbor_count += 1;
                        }
                    }

                    // Mean aggregation
                    if neighbor_count > 0 {
                        aggregated = aggregated / (neighbor_count as f32 + 1.0);
                    }

                    // Apply simple non-linearity (ReLU)
                    for i in 0..self.embedding_dim {
                        aggregated[i] = aggregated[i].max(0.0);
                    }

                    new_embeddings.insert(node.id, aggregated);
                }

                *embeddings = new_embeddings;
            }

            self.initialized = true;
        }

        #[cfg(not(feature = "gnn"))]
        {
            let _ = iterations;
            self.initialized = true;
        }
    }

    #[cfg(feature = "gnn")]
    fn node_embedding(&self, node: NodeId) -> Option<Array1<f32>> {
        self.embeddings.as_ref()?.get(&node).cloned()
    }

    #[cfg(feature = "gnn")]
    fn subgraph_embedding(&self, nodes: &[NodeId]) -> Array1<f32> {
        if nodes.is_empty() {
            return Array1::zeros(self.embedding_dim);
        }

        let embeddings = match &self.embeddings {
            Some(e) => e,
            None => return Array1::zeros(self.embedding_dim),
        };

        // Mean pooling over node embeddings
        let mut sum = Array1::zeros(self.embedding_dim);
        let mut count = 0;

        for &node_id in nodes {
            if let Some(emb) = embeddings.get(&node_id) {
                sum = sum + emb;
                count += 1;
            }
        }

        if count > 0 {
            sum / count as f32
        } else {
            sum
        }
    }

    fn embedding_dim(&self) -> usize {
        self.embedding_dim
    }

    fn is_initialized(&self) -> bool {
        self.initialized
    }

    fn reset(&mut self) {
        #[cfg(feature = "gnn")]
        {
            self.embeddings = None;
        }
        self.initialized = false;
    }
}

#[cfg(all(test, feature = "gnn"))]
mod tests {
    use super::*;
    use crate::{CpgNode, SourceRange, Language, CpgEdgeKind};

    #[test]
    fn test_cpgnn_creation() {
        let cpg = CodePropertyGraph::new(Language::Rust);
        let gnn = CpgGnn::new(cpg)
            .with_embedding_dim(64)
            .with_num_layers(2);

        assert_eq!(gnn.embedding_dim(), 64);
        assert!(!gnn.is_initialized());
    }

    #[test]
    fn test_cpgnn_propagation() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let n1 = cpg.add_node(CpgNode::new(NodeId::new(0), CpgNodeKind::Root, SourceRange::default()));
        let n2 = cpg.add_node(CpgNode::new(NodeId::new(0), CpgNodeKind::If, SourceRange::default()));
        cpg.connect(n1, n2, CpgEdgeKind::AstChild);

        let mut gnn = CpgGnn::new(cpg).with_embedding_dim(32);
        gnn.propagate(3);

        assert!(gnn.is_initialized());
        assert!(gnn.node_embedding(n1).is_some());
        assert!(gnn.node_embedding(n2).is_some());
    }
}
