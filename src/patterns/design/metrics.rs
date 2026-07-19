//! Code metrics for pattern detection.

use crate::{CodePropertyGraph, CpgNodeKind, CpgEdgeKind, NodeId};
use rustc_hash::{FxHashMap, FxHashSet};

/// Code metrics useful for pattern detection.
#[derive(Debug, Clone, Default)]
pub struct PatternMetrics {
    /// Number of classes.
    pub class_count: usize,
    /// Number of interfaces/traits.
    pub interface_count: usize,
    /// Number of inheritance relationships.
    pub inheritance_count: usize,
    /// Number of composition relationships.
    pub composition_count: usize,
    /// Average methods per class.
    pub avg_methods_per_class: f64,
    /// Cohesion metric (LCOM).
    pub cohesion: f64,
    /// Coupling metric (CBO).
    pub coupling: f64,
}

impl PatternMetrics {
    /// Computes metrics for a CPG.
    ///
    /// Analyzes the code property graph to compute:
    /// - Class/struct and trait/interface counts
    /// - Inheritance and composition relationships
    /// - Average methods per class
    /// - LCOM (Lack of Cohesion of Methods) metric
    /// - CBO (Coupling Between Objects) metric
    pub fn compute(cpg: &CodePropertyGraph) -> Self {
        let mut class_count = 0;
        let mut interface_count = 0;
        let mut inheritance_count = 0;
        let composition_count;

        // Track class/struct nodes and their methods
        let mut class_nodes: Vec<NodeId> = Vec::new();
        let mut class_methods: FxHashMap<NodeId, Vec<NodeId>> = FxHashMap::default();
        let mut class_fields: FxHashMap<NodeId, Vec<NodeId>> = FxHashMap::default();

        // Collect class, struct, and trait nodes
        for node in cpg.nodes() {
            match &node.kind {
                CpgNodeKind::Class { .. } | CpgNodeKind::Struct { .. } => {
                    class_count += 1;
                    class_nodes.push(node.id);
                }
                CpgNodeKind::Trait { .. } => {
                    interface_count += 1;
                }
                _ => {}
            }
        }

        // For each class/struct, collect methods and fields
        for &class_id in &class_nodes {
            let mut methods = Vec::new();
            let mut fields = Vec::new();

            Self::collect_class_members(cpg, class_id, &mut methods, &mut fields);

            class_methods.insert(class_id, methods);
            class_fields.insert(class_id, fields);
        }

        // Count inheritance edges
        for edge in cpg.edges() {
            match &edge.kind {
                CpgEdgeKind::Inherits | CpgEdgeKind::Implements => {
                    inheritance_count += 1;
                }
                _ => {}
            }
        }

        // Count composition relationships (fields that reference other classes)
        composition_count = Self::count_composition(cpg, &class_nodes);

        // Compute average methods per class
        let total_methods: usize = class_methods.values().map(|m| m.len()).sum();
        let avg_methods_per_class = if class_count > 0 {
            total_methods as f64 / class_count as f64
        } else {
            0.0
        };

        // Compute LCOM (Lack of Cohesion of Methods)
        let cohesion = Self::compute_lcom(cpg, &class_methods, &class_fields);

        // Compute CBO (Coupling Between Objects)
        let coupling = Self::compute_cbo(cpg, &class_nodes);

        Self {
            class_count,
            interface_count,
            inheritance_count,
            composition_count,
            avg_methods_per_class,
            cohesion,
            coupling,
        }
    }

    /// Collects methods and fields for a class/struct by traversing AST children.
    fn collect_class_members(
        cpg: &CodePropertyGraph,
        class_id: NodeId,
        methods: &mut Vec<NodeId>,
        fields: &mut Vec<NodeId>,
    ) {
        let children = cpg.ast_children(class_id);

        for child_id in children {
            if let Some(child) = cpg.node(child_id) {
                match &child.kind {
                    CpgNodeKind::Function { .. } => {
                        methods.push(child_id);
                    }
                    CpgNodeKind::Field { .. } => {
                        fields.push(child_id);
                    }
                    CpgNodeKind::Impl { .. } => {
                        // Recursively collect methods from impl blocks
                        Self::collect_class_members(cpg, child_id, methods, fields);
                    }
                    CpgNodeKind::Block { .. } => {
                        // Recursively check block contents
                        Self::collect_class_members(cpg, child_id, methods, fields);
                    }
                    _ => {}
                }
            }
        }
    }

    /// Counts composition relationships (fields that reference other classes/structs).
    fn count_composition(cpg: &CodePropertyGraph, class_nodes: &[NodeId]) -> usize {
        let mut composition_count = 0;

        // Build a set of class/struct type names for quick lookup
        let mut class_type_names: FxHashSet<&str> = FxHashSet::default();
        for &class_id in class_nodes {
            if let Some(node) = cpg.node(class_id) {
                match &node.kind {
                    CpgNodeKind::Class { name, .. } | CpgNodeKind::Struct { name, .. } => {
                        class_type_names.insert(name.as_ref());
                    }
                    _ => {}
                }
            }
        }

        // Check each field to see if its type references another class
        for node in cpg.nodes() {
            if let CpgNodeKind::Field { field_type: Some(type_info), .. } = &node.kind {
                if class_type_names.contains(type_info.name.as_ref()) {
                    composition_count += 1;
                }
            }
        }

        composition_count
    }

    /// Computes LCOM (Lack of Cohesion of Methods) metric.
    ///
    /// LCOM measures the cohesiveness of a class based on shared field access.
    /// Higher values indicate lower cohesion.
    ///
    /// For each class:
    /// 1. Get all methods and fields
    /// 2. For each method pair (m1, m2):
    ///    - If they share field access → increase cohesion count
    ///    - Otherwise → increase non-cohesion count
    /// 3. LCOM = max(0, non_shared - shared)
    ///
    /// Returns an average LCOM across all classes.
    fn compute_lcom(
        cpg: &CodePropertyGraph,
        class_methods: &FxHashMap<NodeId, Vec<NodeId>>,
        class_fields: &FxHashMap<NodeId, Vec<NodeId>>,
    ) -> f64 {
        let mut total_lcom = 0.0;
        let mut class_count = 0;

        for (class_id, methods) in class_methods {
            let fields = class_fields.get(class_id).map(|f| f.as_slice()).unwrap_or(&[]);

            if methods.len() < 2 || fields.is_empty() {
                // If less than 2 methods or no fields, LCOM is 0 (perfectly cohesive)
                continue;
            }

            // Build field access sets for each method
            let mut method_field_access: FxHashMap<NodeId, FxHashSet<NodeId>> = FxHashMap::default();

            for &method_id in methods {
                let accessed_fields = Self::find_accessed_fields(cpg, method_id, fields);
                method_field_access.insert(method_id, accessed_fields);
            }

            // Count shared vs non-shared method pairs
            let mut shared = 0;
            let mut not_shared = 0;

            for i in 0..methods.len() {
                for j in (i + 1)..methods.len() {
                    let m1_fields = method_field_access.get(&methods[i]).unwrap();
                    let m2_fields = method_field_access.get(&methods[j]).unwrap();

                    if m1_fields.intersection(m2_fields).next().is_some() {
                        shared += 1;
                    } else {
                        not_shared += 1;
                    }
                }
            }

            // LCOM = max(0, not_shared - shared) normalized by total pairs
            let total_pairs = shared + not_shared;
            let lcom = if total_pairs > 0 {
                (not_shared as f64 - shared as f64).max(0.0) / total_pairs as f64
            } else {
                0.0
            };

            total_lcom += lcom;
            class_count += 1;
        }

        if class_count > 0 {
            total_lcom / class_count as f64
        } else {
            0.0
        }
    }

    /// Finds which fields from the given list are accessed within a method.
    fn find_accessed_fields(
        cpg: &CodePropertyGraph,
        method_id: NodeId,
        fields: &[NodeId],
    ) -> FxHashSet<NodeId> {
        let mut accessed = FxHashSet::default();

        // Get field names for matching
        let mut field_names: FxHashMap<&str, NodeId> = FxHashMap::default();
        for &field_id in fields {
            if let Some(node) = cpg.node(field_id) {
                if let CpgNodeKind::Field { name, .. } = &node.kind {
                    field_names.insert(name.as_ref(), field_id);
                }
            }
        }

        // Traverse method descendants looking for field access
        let descendants = cpg.ast_descendants(method_id);
        for desc_id in descendants {
            if let Some(node) = cpg.node(desc_id) {
                match &node.kind {
                    CpgNodeKind::MemberAccess { member } => {
                        if let Some(&field_id) = field_names.get(member.as_ref()) {
                            accessed.insert(field_id);
                        }
                    }
                    CpgNodeKind::Identifier { name, .. } => {
                        // Also check direct identifier references to fields
                        if let Some(&field_id) = field_names.get(name.as_ref()) {
                            accessed.insert(field_id);
                        }
                    }
                    _ => {}
                }
            }
        }

        accessed
    }

    /// Computes CBO (Coupling Between Objects) metric.
    ///
    /// CBO counts the number of distinct classes that a class is coupled with.
    /// Coupling occurs through:
    /// - Field types (composition)
    /// - Method parameter/return types
    /// - Inheritance/implementation relationships
    ///
    /// Returns an average CBO across all classes.
    fn compute_cbo(cpg: &CodePropertyGraph, class_nodes: &[NodeId]) -> f64 {
        if class_nodes.is_empty() {
            return 0.0;
        }

        let mut total_cbo = 0;

        for &class_id in class_nodes {
            let mut coupled_classes: FxHashSet<&str> = FxHashSet::default();

            // Get class name for self-reference exclusion
            let class_name = if let Some(node) = cpg.node(class_id) {
                match &node.kind {
                    CpgNodeKind::Class { name, .. } | CpgNodeKind::Struct { name, .. } => {
                        Some(name.as_ref())
                    }
                    _ => None,
                }
            } else {
                None
            };

            // Check all descendants for type references
            let descendants = cpg.ast_descendants(class_id);
            for desc_id in descendants {
                if let Some(node) = cpg.node(desc_id) {
                    match &node.kind {
                        // Field types
                        CpgNodeKind::Field { field_type: Some(type_info), .. } => {
                            let type_name = type_info.name.as_ref();
                            if class_name != Some(type_name) {
                                coupled_classes.insert(type_name);
                            }
                        }
                        // Variable types
                        CpgNodeKind::Variable { var_type: Some(type_info), .. } => {
                            let type_name = type_info.name.as_ref();
                            if class_name != Some(type_name) {
                                coupled_classes.insert(type_name);
                            }
                        }
                        // Parameter types
                        CpgNodeKind::Parameter { param_type: Some(type_info), .. } => {
                            let type_name = type_info.name.as_ref();
                            if class_name != Some(type_name) {
                                coupled_classes.insert(type_name);
                            }
                        }
                        // Type annotations
                        CpgNodeKind::TypeAnnotation { type_info } => {
                            let type_name = type_info.name.as_ref();
                            if class_name != Some(type_name) {
                                coupled_classes.insert(type_name);
                            }
                        }
                        _ => {}
                    }
                }
            }

            // Check for inheritance/implementation relationships
            for edge in cpg.outgoing_edges(class_id) {
                match &edge.kind {
                    CpgEdgeKind::Inherits | CpgEdgeKind::Implements => {
                        if let Some(target_node) = cpg.node(edge.target) {
                            let type_name = match &target_node.kind {
                                CpgNodeKind::Class { name, .. }
                                | CpgNodeKind::Struct { name, .. }
                                | CpgNodeKind::Trait { name, .. } => Some(name.as_ref()),
                                _ => None,
                            };
                            if let Some(name) = type_name {
                                if class_name != Some(name) {
                                    coupled_classes.insert(name);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }

            total_cbo += coupled_classes.len();
        }

        total_cbo as f64 / class_nodes.len() as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CpgNode, Language, SourceRange};

    #[test]
    fn test_empty_cpg_metrics() {
        let cpg = CodePropertyGraph::new(Language::Rust);
        let metrics = PatternMetrics::compute(&cpg);

        assert_eq!(metrics.class_count, 0);
        assert_eq!(metrics.interface_count, 0);
        assert_eq!(metrics.inheritance_count, 0);
        assert_eq!(metrics.composition_count, 0);
        assert_eq!(metrics.avg_methods_per_class, 0.0);
        assert_eq!(metrics.cohesion, 0.0);
        assert_eq!(metrics.coupling, 0.0);
    }

    #[test]
    fn test_class_counting() {
        use std::sync::Arc;

        let mut cpg = CodePropertyGraph::new(Language::Rust);

        // Add two classes
        cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Class { name: Arc::from("Foo"), is_abstract: false },
            SourceRange::default(),
        ));
        cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Struct { name: Arc::from("Bar") },
            SourceRange::default(),
        ));

        // Add one trait
        cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Trait { name: Arc::from("Baz") },
            SourceRange::default(),
        ));

        let metrics = PatternMetrics::compute(&cpg);

        assert_eq!(metrics.class_count, 2);
        assert_eq!(metrics.interface_count, 1);
    }

    #[test]
    fn test_inheritance_counting() {
        use std::sync::Arc;
        use crate::CpgEdgeKind;

        let mut cpg = CodePropertyGraph::new(Language::Rust);

        let class1 = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Class { name: Arc::from("Child"), is_abstract: false },
            SourceRange::default(),
        ));
        let class2 = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Class { name: Arc::from("Parent"), is_abstract: false },
            SourceRange::default(),
        ));
        let trait1 = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Trait { name: Arc::from("SomeTrait") },
            SourceRange::default(),
        ));

        cpg.connect(class1, class2, CpgEdgeKind::Inherits);
        cpg.connect(class1, trait1, CpgEdgeKind::Implements);

        let metrics = PatternMetrics::compute(&cpg);

        assert_eq!(metrics.inheritance_count, 2);
    }
}
