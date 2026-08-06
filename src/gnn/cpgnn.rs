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

use super::GraphNeuralNetwork;
use crate::{CodePropertyGraph, CpgNodeKind, NodeId};

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
            CpgNodeKind::If | CpgNodeKind::While | CpgNodeKind::For | CpgNodeKind::Loop => {
                features[5] = 1.0
            }
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
                    let current = embeddings
                        .get(&node.id)
                        .cloned()
                        .unwrap_or_else(|| Array1::zeros(self.embedding_dim));

                    // Aggregate neighbor embeddings
                    let mut aggregated = current.clone();
                    let mut neighbor_count = 0;

                    // Aggregate from AST neighbors
                    for child_id in self.cpg.ast_children(node.id) {
                        if let Some(child_emb) = embeddings.get(&child_id) {
                            aggregated += child_emb;
                            neighbor_count += 1;
                        }
                    }
                    if let Some(parent_id) = self.cpg.ast_parent(node.id) {
                        if let Some(parent_emb) = embeddings.get(&parent_id) {
                            aggregated += parent_emb;
                            neighbor_count += 1;
                        }
                    }

                    // Aggregate from CFG neighbors
                    for (succ_id, _) in self.cpg.cfg_successors(node.id) {
                        if let Some(succ_emb) = embeddings.get(&succ_id) {
                            aggregated += succ_emb;
                            neighbor_count += 1;
                        }
                    }
                    for (pred_id, _) in self.cpg.cfg_predecessors(node.id) {
                        if let Some(pred_emb) = embeddings.get(&pred_id) {
                            aggregated += pred_emb;
                            neighbor_count += 1;
                        }
                    }

                    // Aggregate from DFG neighbors
                    for (succ_id, _) in self.cpg.dfg_successors(node.id) {
                        if let Some(succ_emb) = embeddings.get(&succ_id) {
                            aggregated += succ_emb;
                            neighbor_count += 1;
                        }
                    }
                    for (pred_id, _) in self.cpg.dfg_predecessors(node.id) {
                        if let Some(pred_emb) = embeddings.get(&pred_id) {
                            aggregated += pred_emb;
                            neighbor_count += 1;
                        }
                    }

                    // Mean aggregation
                    if neighbor_count > 0 {
                        aggregated /= neighbor_count as f32 + 1.0;
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
                sum += emb;
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
    use crate::{CpgEdgeKind, CpgNode, Language, SourceRange};

    #[test]
    fn test_cpgnn_creation() {
        let cpg = CodePropertyGraph::new(Language::Rust);
        let gnn = CpgGnn::new(cpg).with_embedding_dim(64).with_num_layers(2);

        assert_eq!(gnn.embedding_dim(), 64);
        assert!(!gnn.is_initialized());
    }

    #[test]
    fn test_cpgnn_propagation() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let n1 = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Root,
            SourceRange::default(),
        ));
        let n2 = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::If,
            SourceRange::default(),
        ));
        cpg.connect(n1, n2, CpgEdgeKind::AstChild);

        let mut gnn = CpgGnn::new(cpg).with_embedding_dim(32);
        gnn.propagate(3);

        assert!(gnn.is_initialized());
        assert!(gnn.node_embedding(n1).is_some());
        assert!(gnn.node_embedding(n2).is_some());
    }

    #[test]
    fn test_cpgnn_builders_and_cpg_accessor() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Root,
            SourceRange::default(),
        ));
        cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::If,
            SourceRange::default(),
        ));

        // The builder chain returns `Self`; `cpg()` hands back the moved-in graph
        // unchanged (same node count), and `with_embedding_dim` is observable.
        let gnn = CpgGnn::new(cpg)
            .with_embedding_dim(16)
            .with_num_layers(5)
            .with_dropout(0.3);
        assert_eq!(gnn.cpg().node_count(), 2);
        assert_eq!(gnn.embedding_dim(), 16);
        assert!(!gnn.is_initialized());

        // `with_num_layers` / `with_dropout` set private fields with no public
        // getter and no effect on `propagate` (which takes an explicit iteration
        // count) — so verify they mutate state via the derived `Debug` view,
        // without coupling to the exact format string.
        let base = CpgGnn::new(CodePropertyGraph::new(Language::Rust));
        let more_layers = CpgGnn::new(CodePropertyGraph::new(Language::Rust)).with_num_layers(9);
        let more_dropout = CpgGnn::new(CodePropertyGraph::new(Language::Rust)).with_dropout(0.75);
        assert_ne!(format!("{base:?}"), format!("{more_layers:?}"));
        assert_ne!(format!("{base:?}"), format!("{more_dropout:?}"));
    }

    #[test]
    fn test_cpgnn_subgraph_embedding_and_reset() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let n1 = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Root,
            SourceRange::default(),
        ));
        let n2 = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::If,
            SourceRange::default(),
        ));
        cpg.connect(n1, n2, CpgEdgeKind::AstChild);

        let mut gnn = CpgGnn::new(cpg).with_embedding_dim(16);

        // Empty subgraph ⇒ zero vector of length `embedding_dim`, regardless of
        // whether propagation has run.
        let empty = gnn.subgraph_embedding(&[]);
        assert_eq!(empty.len(), 16);
        assert!(empty.iter().all(|&x| x == 0.0));

        // Before propagation the embedding table is `None`, so pooling over real
        // nodes still returns zeros of the configured dimension.
        let pre = gnn.subgraph_embedding(&[n1, n2]);
        assert_eq!(pre.len(), 16);

        gnn.propagate(2);
        assert!(gnn.is_initialized());
        assert_eq!(gnn.embedding_dim(), 16);
        assert!(gnn.node_embedding(n1).is_some());
        assert!(gnn.node_embedding(n2).is_some());

        let post = gnn.subgraph_embedding(&[n1, n2]);
        assert_eq!(post.len(), 16);
        // Pooled from ReLU'd node embeddings ⇒ every component is non-negative.
        assert!(post.iter().all(|&x| x >= 0.0));

        // `reset` clears the table and the initialized flag.
        gnn.reset();
        assert!(!gnn.is_initialized());
        assert!(gnn.node_embedding(n1).is_none());
        let after = gnn.subgraph_embedding(&[n1, n2]);
        assert_eq!(after.len(), 16);
    }
}

/// Pooling and lifecycle edge cases.
#[cfg(all(test, feature = "gnn"))]
mod pooling_edges {
    use super::*;
    use crate::testutil::{arb_well_formed_cpg, build_well_formed, node_ids, StmtSpec};
    use proptest::prelude::*;

    fn small_graph() -> CodePropertyGraph {
        build_well_formed(vec![
            StmtSpec::Let("x".into()),
            StmtSpec::If(vec![StmtSpec::Use("x".into())]),
        ])
    }

    /// Pooling over no nodes is the zero vector of the configured width — not
    /// a panic and not a division by zero.
    #[test]
    fn pooling_over_no_nodes_is_the_zero_vector() {
        let mut gnn = CpgGnn::new(small_graph()).with_embedding_dim(8);
        gnn.propagate(1);
        let pooled = gnn.subgraph_embedding(&[]);
        assert_eq!(pooled.len(), 8, "the zero vector still has the right width");
        assert!(pooled.iter().all(|v| *v == 0.0));
    }

    /// Pooling over ids that are not in the graph yields the zero vector too:
    /// the running sum stays empty, so the `count > 0` normalization is skipped.
    #[test]
    fn pooling_over_unknown_ids_is_the_zero_vector() {
        let mut gnn = CpgGnn::new(small_graph()).with_embedding_dim(8);
        gnn.propagate(1);
        let absent = [NodeId::new(9_998), NodeId::new(9_999)];
        let pooled = gnn.subgraph_embedding(&absent);
        assert_eq!(pooled.len(), 8);
        assert!(pooled.iter().all(|v| *v == 0.0));
        assert!(gnn.node_embedding(absent[0]).is_none());
    }

    /// Pooling before any propagation is also the zero vector — the embeddings
    /// table has not been built yet.
    #[test]
    fn pooling_before_propagation_is_the_zero_vector() {
        let cpg = small_graph();
        let ids = node_ids(&cpg);
        let gnn = CpgGnn::new(cpg).with_embedding_dim(8);
        assert!(!gnn.is_initialized());
        let pooled = gnn.subgraph_embedding(&ids);
        assert!(pooled.iter().all(|v| *v == 0.0));
    }

    /// `propagate` initializes once: a second call reuses the existing table
    /// rather than re-seeding it, and `reset` clears it again.
    #[test]
    fn propagation_initializes_once_and_reset_clears() {
        let cpg = small_graph();
        let ids = node_ids(&cpg);
        let mut gnn = CpgGnn::new(cpg).with_embedding_dim(8).with_num_layers(1);

        gnn.propagate(1);
        assert!(gnn.is_initialized());
        let widths: Vec<usize> = ids
            .iter()
            .map(|id| gnn.node_embedding(*id).expect("embedded").len())
            .collect();

        gnn.propagate(1); // second pass: no re-initialization
        assert!(gnn.is_initialized());
        for (i, id) in ids.iter().enumerate() {
            assert_eq!(
                gnn.node_embedding(*id).expect("still embedded").len(),
                widths[i]
            );
        }

        gnn.reset();
        assert!(!gnn.is_initialized());
        assert!(gnn.node_embedding(ids[0]).is_none());
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(32))]

        /// Mean pooling over every node is a genuine mean: each component lies
        /// within the range of that component across the pooled nodes.
        #[test]
        fn prop_pooling_is_a_mean(cpg in arb_well_formed_cpg()) {
            let ids = node_ids(&cpg);
            let mut gnn = CpgGnn::new(cpg).with_embedding_dim(6).with_num_layers(1);
            gnn.propagate(1);

            let pooled = gnn.subgraph_embedding(&ids);
            prop_assert_eq!(pooled.len(), 6);

            for c in 0..6 {
                let mut lo = f32::INFINITY;
                let mut hi = f32::NEG_INFINITY;
                for id in &ids {
                    if let Some(e) = gnn.node_embedding(*id) {
                        lo = lo.min(e[c]);
                        hi = hi.max(e[c]);
                    }
                }
                if lo.is_finite() {
                    prop_assert!(
                        pooled[c] >= lo - 1e-5 && pooled[c] <= hi + 1e-5,
                        "component {} = {} is outside [{}, {}]", c, pooled[c], lo, hi
                    );
                }
            }
        }
    }
}

#[cfg(all(test, feature = "gnn"))]
mod proptests {
    use super::*;
    use crate::testutil::*;
    use proptest::prelude::*;

    proptest! {
        // Heavier: each case builds a CPG and runs full message passing.
        #![proptest_config(ProptestConfig::with_cases(32))]

        /// After `propagate(k)` (k ≥ 1): the model is initialized, the embedding
        /// dimension is unchanged, EVERY node has an embedding of that dimension
        /// with all components ≥ 0 (final-layer ReLU), and `reset` clears state.
        #[test]
        fn prop_propagate_totality(cpg in arb_well_formed_cpg(), k in 1usize..=3) {
            let dim = 8usize;
            let ids = node_ids(&cpg);
            let mut gnn = CpgGnn::new(cpg).with_embedding_dim(dim);
            gnn.propagate(k);

            prop_assert!(gnn.is_initialized());
            prop_assert_eq!(gnn.embedding_dim(), dim);

            for id in &ids {
                let emb = gnn.node_embedding(*id);
                prop_assert!(emb.is_some());
                let emb = emb.expect("every node has an embedding after propagate");
                prop_assert_eq!(emb.len(), dim);
                prop_assert!(emb.iter().all(|&x| x >= 0.0), "ReLU invariant violated");
            }

            gnn.reset();
            prop_assert!(!gnn.is_initialized());
            if let Some(first) = ids.first() {
                prop_assert!(gnn.node_embedding(*first).is_none());
            }
        }

        /// Subgraph pooling always yields a vector of length `embedding_dim`:
        /// the empty slice pools to zeros; a non-empty slice pools to `dim`.
        /// Holds both before and after propagation.
        #[test]
        fn prop_subgraph_pooling(cpg in arb_well_formed_cpg(), do_prop in any::<bool>()) {
            let dim = 8usize;
            let ids = node_ids(&cpg);
            let mut gnn = CpgGnn::new(cpg).with_embedding_dim(dim);
            if do_prop {
                gnn.propagate(2);
            }

            let empty = gnn.subgraph_embedding(&[]);
            prop_assert_eq!(empty.len(), dim);
            prop_assert!(empty.iter().all(|&x| x == 0.0));

            let sg = gnn.subgraph_embedding(&ids);
            prop_assert_eq!(sg.len(), dim);
        }
    }
}
