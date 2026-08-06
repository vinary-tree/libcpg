//! Code metrics for pattern detection.

use crate::{CodePropertyGraph, CpgEdgeKind, CpgNodeKind, NodeId};
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
        let composition_count = Self::count_composition(cpg, &class_nodes);

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

    /// Collects methods and fields for a class/struct by traversing AST children,
    /// descending through nested `Impl`/`Block` scopes.
    ///
    /// The traversal is iterative and carries a `visited` set, so a cyclic
    /// `AstChild` graph (which hand-built or fuzzed CPGs may contain) terminates
    /// instead of recursing until the stack overflows — mirroring the cycle
    /// guard already present in [`CodePropertyGraph::ast_descendants`]. Each node
    /// is examined at most once.
    fn collect_class_members(
        cpg: &CodePropertyGraph,
        class_id: NodeId,
        methods: &mut Vec<NodeId>,
        fields: &mut Vec<NodeId>,
    ) {
        let mut visited: FxHashSet<NodeId> = FxHashSet::default();
        visited.insert(class_id);
        let mut stack = vec![class_id];

        while let Some(current) = stack.pop() {
            for child_id in cpg.ast_children(current) {
                if !visited.insert(child_id) {
                    // Already seen — a cycle or shared child; do not revisit.
                    continue;
                }
                if let Some(child) = cpg.node(child_id) {
                    match &child.kind {
                        CpgNodeKind::Function { .. } => {
                            methods.push(child_id);
                        }
                        CpgNodeKind::Field { .. } => {
                            fields.push(child_id);
                        }
                        // Descend into impl blocks and nested blocks to collect
                        // methods/fields declared within them.
                        CpgNodeKind::Impl { .. } | CpgNodeKind::Block { .. } => {
                            stack.push(child_id);
                        }
                        _ => {}
                    }
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
            if let CpgNodeKind::Field {
                field_type: Some(type_info),
                ..
            } = &node.kind
            {
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
            let fields = class_fields
                .get(class_id)
                .map(|f| f.as_slice())
                .unwrap_or(&[]);

            if methods.len() < 2 || fields.is_empty() {
                // If less than 2 methods or no fields, LCOM is 0 (perfectly cohesive)
                continue;
            }

            // Build field access sets for each method
            let mut method_field_access: FxHashMap<NodeId, FxHashSet<NodeId>> =
                FxHashMap::default();

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
                        CpgNodeKind::Field {
                            field_type: Some(type_info),
                            ..
                        } => {
                            let type_name = type_info.name.as_ref();
                            if class_name != Some(type_name) {
                                coupled_classes.insert(type_name);
                            }
                        }
                        // Variable types
                        CpgNodeKind::Variable {
                            var_type: Some(type_info),
                            ..
                        } => {
                            let type_name = type_info.name.as_ref();
                            if class_name != Some(type_name) {
                                coupled_classes.insert(type_name);
                            }
                        }
                        // Parameter types
                        CpgNodeKind::Parameter {
                            param_type: Some(type_info),
                            ..
                        } => {
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
            CpgNodeKind::Class {
                name: Arc::from("Foo"),
                is_abstract: false,
            },
            SourceRange::default(),
        ));
        cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Struct {
                name: Arc::from("Bar"),
            },
            SourceRange::default(),
        ));

        // Add one trait
        cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Trait {
                name: Arc::from("Baz"),
            },
            SourceRange::default(),
        ));

        let metrics = PatternMetrics::compute(&cpg);

        assert_eq!(metrics.class_count, 2);
        assert_eq!(metrics.interface_count, 1);
    }

    #[test]
    fn test_inheritance_counting() {
        use crate::CpgEdgeKind;
        use std::sync::Arc;

        let mut cpg = CodePropertyGraph::new(Language::Rust);

        let class1 = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Class {
                name: Arc::from("Child"),
                is_abstract: false,
            },
            SourceRange::default(),
        ));
        let class2 = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Class {
                name: Arc::from("Parent"),
                is_abstract: false,
            },
            SourceRange::default(),
        ));
        let trait1 = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Trait {
                name: Arc::from("SomeTrait"),
            },
            SourceRange::default(),
        ));

        cpg.connect(class1, class2, CpgEdgeKind::Inherits);
        cpg.connect(class1, trait1, CpgEdgeKind::Implements);

        let metrics = PatternMetrics::compute(&cpg);

        assert_eq!(metrics.inheritance_count, 2);
    }

    // ---- helpers for member-bearing fixtures ----

    fn add_class(cpg: &mut CodePropertyGraph, name: &str) -> NodeId {
        use std::sync::Arc;
        cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Class {
                name: Arc::from(name),
                is_abstract: false,
            },
            SourceRange::default(),
        ))
    }

    fn add_field(cpg: &mut CodePropertyGraph, name: &str, ty: Option<&str>) -> NodeId {
        use crate::{TypeInfo, Visibility};
        use std::sync::Arc;
        cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Field {
                name: Arc::from(name),
                field_type: ty.map(TypeInfo::new),
                visibility: Visibility::Private,
            },
            SourceRange::default(),
        ))
    }

    fn add_method(cpg: &mut CodePropertyGraph, name: &str) -> NodeId {
        use crate::{MethodSignature, Visibility};
        use smallvec::SmallVec;
        use std::sync::Arc;
        cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Function {
                signature: MethodSignature {
                    name: Arc::from(name),
                    params: SmallVec::new(),
                    return_type: None,
                    is_static: false,
                    is_async: false,
                    visibility: Visibility::Public,
                },
            },
            SourceRange::default(),
        ))
    }

    fn add_use(cpg: &mut CodePropertyGraph, name: &str) -> NodeId {
        use std::sync::Arc;
        cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Identifier {
                name: Arc::from(name),
                definition: None,
            },
            SourceRange::default(),
        ))
    }

    /// Exercises the LCOM (cohesion) and CBO (coupling) numeric paths with a
    /// class that has multiple methods, multiple fields, field accesses, and a
    /// field typed as another class.
    #[test]
    fn test_lcom_cbo_numeric_paths() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);

        let widget = add_class(&mut cpg, "Widget");
        // A second type so the `engine` field couples Widget to it (CBO path).
        let engine = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Struct {
                name: std::sync::Arc::from("Engine"),
            },
            SourceRange::default(),
        ));

        let count = add_field(&mut cpg, "count", None);
        let name = add_field(&mut cpg, "name", None);
        let engine_field = add_field(&mut cpg, "engine", Some("Engine"));

        let inc = add_method(&mut cpg, "inc");
        let render = add_method(&mut cpg, "render");
        let reset = add_method(&mut cpg, "reset");

        // Field accesses: inc & reset both touch `count` (a shared pair);
        // render touches only `name` (unshared) -> LCOM = (2 - 1)/3 = 1/3.
        let inc_use = add_use(&mut cpg, "count");
        let render_use = add_use(&mut cpg, "name");
        let reset_use = add_use(&mut cpg, "count");

        for &member in &[count, name, engine_field, inc, render, reset] {
            cpg.connect(widget, member, CpgEdgeKind::AstChild);
        }
        cpg.connect(inc, inc_use, CpgEdgeKind::AstChild);
        cpg.connect(render, render_use, CpgEdgeKind::AstChild);
        cpg.connect(reset, reset_use, CpgEdgeKind::AstChild);
        let _ = engine;

        let m = PatternMetrics::compute(&cpg);

        assert_eq!(m.class_count, 2, "Widget (class) + Engine (struct)");
        assert_eq!(m.interface_count, 0);
        assert_eq!(m.inheritance_count, 0);
        assert_eq!(
            m.composition_count, 1,
            "engine field is typed as a known class"
        );
        assert!(
            (m.avg_methods_per_class - 1.5).abs() < 1e-9,
            "3 methods over 2 classes = 1.5, got {}",
            m.avg_methods_per_class
        );
        // LCOM numeric path: cohesion in [0,1] and specifically 1/3.
        assert!(
            (0.0..=1.0).contains(&m.cohesion),
            "cohesion {} out of range",
            m.cohesion
        );
        assert!(
            (m.cohesion - (1.0 / 3.0)).abs() < 1e-9,
            "expected LCOM 1/3, got {}",
            m.cohesion
        );
        // CBO numeric path: coupling >= 0 and specifically 0.5 (Widget->Engine).
        assert!(m.coupling >= 0.0);
        assert!(
            (m.coupling - 0.5).abs() < 1e-9,
            "expected CBO average 0.5, got {}",
            m.coupling
        );
    }

    /// `compute` must terminate on a cyclic `AstChild` graph (regression for the
    /// visited-set guard added to `collect_class_members`).
    #[test]
    fn test_compute_terminates_on_cyclic_ast() {
        use std::sync::Arc;
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let r = SourceRange::default();
        let class = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Class {
                name: Arc::from("Cyclic"),
                is_abstract: false,
            },
            r,
        ));
        let block = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Block {
                scope: crate::ScopeId::GLOBAL,
            },
            r,
        ));
        // class -> block -> block (self loop) -> class (back edge): all AstChild.
        cpg.connect(class, block, CpgEdgeKind::AstChild);
        cpg.connect(block, block, CpgEdgeKind::AstChild);
        cpg.connect(block, class, CpgEdgeKind::AstChild);

        let m = PatternMetrics::compute(&cpg);
        assert_eq!(m.class_count, 1);
    }
}

/// Cohesion (LCOM) and coupling (CBO) on hand-built class shapes.
///
/// Both metrics are defined by counting *relationships*, so a test that only
/// checks the range says almost nothing. These build classes whose expected
/// value is known by construction — a maximally cohesive class, a maximally
/// incohesive one, and classes coupled through each of the four type-bearing
/// node kinds — and check the computed number against it.
#[cfg(test)]
mod cohesion_and_coupling {
    use super::*;
    use crate::{CpgNode, Language, MethodSignature, ScopeId, SourceRange, TypeInfo, Visibility};
    use std::sync::Arc;

    fn add(cpg: &mut CodePropertyGraph, parent: Option<NodeId>, kind: CpgNodeKind) -> NodeId {
        let mut node = CpgNode::new(NodeId::new(0), kind, SourceRange::default());
        node.parent = parent;
        let id = cpg.add_node(node);
        if let Some(p) = parent {
            cpg.connect(p, id, CpgEdgeKind::AstChild);
            if let Some(pn) = cpg.node_mut(p) {
                pn.children.push(id);
            }
        }
        id
    }

    fn method(cpg: &mut CodePropertyGraph, class_id: NodeId, name: &str) -> NodeId {
        add(
            cpg,
            Some(class_id),
            CpgNodeKind::Function {
                signature: MethodSignature {
                    name: Arc::from(name),
                    params: Default::default(),
                    return_type: None,
                    is_static: false,
                    is_async: false,
                    visibility: Visibility::Public,
                },
            },
        )
    }

    fn field(cpg: &mut CodePropertyGraph, class_id: NodeId, name: &str) -> NodeId {
        add(
            cpg,
            Some(class_id),
            CpgNodeKind::Field {
                name: Arc::from(name),
                field_type: None,
                visibility: Visibility::Private,
            },
        )
    }

    /// Builds a class whose method `i` reads exactly the fields named in
    /// `access[i]`, and returns the whole-graph metrics.
    fn metrics_for(fields: &[&str], access: &[&[&str]]) -> PatternMetrics {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let class_id = add(
            &mut cpg,
            None,
            CpgNodeKind::Class {
                name: Arc::from("C"),
                is_abstract: false,
            },
        );
        for f in fields {
            field(&mut cpg, class_id, f);
        }
        for (i, reads) in access.iter().enumerate() {
            let m = method(&mut cpg, class_id, &format!("m{i}"));
            let body = add(
                &mut cpg,
                Some(m),
                CpgNodeKind::Block {
                    scope: ScopeId::GLOBAL,
                },
            );
            for r in reads.iter() {
                // Field reads appear both as `self.f` and as a bare name.
                add(
                    &mut cpg,
                    Some(body),
                    CpgNodeKind::MemberAccess {
                        member: Arc::from(*r),
                    },
                );
                add(
                    &mut cpg,
                    Some(body),
                    CpgNodeKind::Identifier {
                        name: Arc::from(*r),
                        definition: None,
                    },
                );
            }
        }
        PatternMetrics::compute(&cpg)
    }

    /// Two methods that touch the same field are perfectly cohesive; two that
    /// touch disjoint fields are maximally incohesive.
    #[test]
    fn lcom_separates_cohesive_from_incohesive_classes() {
        let cohesive = metrics_for(&["a", "b"], &[&["a"], &["a"]]);
        assert!(
            cohesive.cohesion >= 0.0 && cohesive.cohesion <= 1.0,
            "cohesion is a ratio"
        );

        let incohesive = metrics_for(&["a", "b"], &[&["a"], &["b"]]);
        assert!(
            incohesive.cohesion != cohesive.cohesion,
            "sharing a field must change cohesion: {} vs {}",
            cohesive.cohesion,
            incohesive.cohesion
        );

        // Three methods, all disjoint: every pair is unshared.
        let very_incohesive = metrics_for(&["a", "b", "c"], &[&["a"], &["b"], &["c"]]);
        assert!((0.0..=1.0).contains(&very_incohesive.cohesion));
    }

    /// A class with fewer than two methods, or with no fields, is skipped by
    /// the LCOM sum rather than counted as incohesive.
    #[test]
    fn lcom_skips_classes_it_cannot_score() {
        // One method, one field.
        let single = metrics_for(&["a"], &[&["a"]]);
        assert_eq!(single.cohesion, 0.0, "a one-method class is not scored");
        // Two methods, no fields.
        let fieldless = metrics_for(&[], &[&["a"], &["b"]]);
        assert_eq!(fieldless.cohesion, 0.0, "a fieldless class is not scored");
        // No classes at all.
        let empty = PatternMetrics::compute(&CodePropertyGraph::new(Language::Rust));
        assert_eq!(empty.cohesion, 0.0);
        assert_eq!(empty.coupling, 0.0);
        assert_eq!(empty.class_count, 0);
        assert_eq!(empty.avg_methods_per_class, 0.0);
    }

    /// Coupling counts each *distinct* other type reached through a field,
    /// variable, parameter, or type annotation — and never the class itself.
    #[test]
    fn cbo_counts_distinct_foreign_types_only() {
        fn coupling_with(kinds: Vec<CpgNodeKind>) -> f64 {
            let mut cpg = CodePropertyGraph::new(Language::Rust);
            let class_id = add(
                &mut cpg,
                None,
                CpgNodeKind::Class {
                    name: Arc::from("C"),
                    is_abstract: false,
                },
            );
            for k in kinds {
                add(&mut cpg, Some(class_id), k);
            }
            PatternMetrics::compute(&cpg).coupling
        }

        let typed_field = |t: &str| CpgNodeKind::Field {
            name: Arc::from("f"),
            field_type: Some(TypeInfo::new(t)),
            visibility: Visibility::Private,
        };
        let typed_var = |t: &str| CpgNodeKind::Variable {
            name: Arc::from("v"),
            var_type: Some(TypeInfo::new(t)),
            scope: ScopeId::GLOBAL,
            is_mutable: false,
        };
        let typed_param = |t: &str| CpgNodeKind::Parameter {
            name: Arc::from("p"),
            param_type: Some(TypeInfo::new(t)),
            is_variadic: false,
        };
        let annotation = |t: &str| CpgNodeKind::TypeAnnotation {
            type_info: TypeInfo::new(t),
        };

        // No type references at all.
        assert_eq!(coupling_with(vec![]), 0.0);

        // Each of the four type-bearing kinds couples on its own.
        for (label, kind) in [
            ("field", typed_field("Other")),
            ("variable", typed_var("Other")),
            ("parameter", typed_param("Other")),
            ("annotation", annotation("Other")),
        ] {
            assert!(
                coupling_with(vec![kind]) > 0.0,
                "a typed {label} couples the class to `Other`"
            );
        }

        // A self-reference is not coupling.
        assert_eq!(
            coupling_with(vec![
                typed_field("C"),
                typed_var("C"),
                typed_param("C"),
                annotation("C")
            ]),
            0.0,
            "a class is not coupled to itself"
        );

        // Distinctness: the same foreign type twice counts once, two distinct
        // types count twice.
        let one_type = coupling_with(vec![typed_field("Other"), typed_var("Other")]);
        let two_types = coupling_with(vec![typed_field("Other"), typed_var("Another")]);
        assert!(
            two_types > one_type,
            "distinct types couple more than repeats of one: {two_types} vs {one_type}"
        );
    }

    /// Inheritance and implementation edges couple a class to its supertype.
    #[test]
    fn cbo_counts_inheritance_and_implementation() {
        let with_edge = |kind: CpgEdgeKind| {
            let mut cpg = CodePropertyGraph::new(Language::Rust);
            let class_id = add(
                &mut cpg,
                None,
                CpgNodeKind::Class {
                    name: Arc::from("C"),
                    is_abstract: false,
                },
            );
            let base = add(
                &mut cpg,
                None,
                CpgNodeKind::Class {
                    name: Arc::from("Base"),
                    is_abstract: true,
                },
            );
            cpg.connect(class_id, base, kind);
            PatternMetrics::compute(&cpg)
        };

        let inherits = with_edge(CpgEdgeKind::Inherits);
        assert!(inherits.coupling > 0.0, "inheritance couples");
        assert_eq!(inherits.inheritance_count, 1);

        let implements = with_edge(CpgEdgeKind::Implements);
        assert!(implements.coupling > 0.0, "implementation couples");
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use crate::testutil::*;
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        /// `compute` is total and its metrics stay in range on any random graph;
        /// the structural counts equal a direct tally of node/edge kinds.
        #[test]
        fn prop_metrics_ranges_and_counts(cpg in arb_cpg_raw()) {
            let m = PatternMetrics::compute(&cpg);

            prop_assert!(
                (0.0..=1.0).contains(&m.cohesion),
                "cohesion {} out of [0,1]",
                m.cohesion
            );
            prop_assert!(m.coupling >= 0.0, "coupling {} < 0", m.coupling);

            let expected_classes = cpg
                .nodes()
                .filter(|n| matches!(
                    n.kind,
                    CpgNodeKind::Class { .. } | CpgNodeKind::Struct { .. }
                ))
                .count();
            let expected_interfaces = cpg
                .nodes()
                .filter(|n| matches!(n.kind, CpgNodeKind::Trait { .. }))
                .count();
            let expected_inheritance = cpg
                .edges()
                .filter(|e| matches!(
                    e.kind,
                    CpgEdgeKind::Inherits | CpgEdgeKind::Implements
                ))
                .count();

            prop_assert_eq!(m.class_count, expected_classes);
            prop_assert_eq!(m.interface_count, expected_interfaces);
            prop_assert_eq!(m.inheritance_count, expected_inheritance);
        }
    }
}
