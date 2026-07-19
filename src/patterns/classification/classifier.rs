//! Pattern classification using rule-based heuristics and optional ML.
//!
//! This module provides pattern classification with two modes:
//! - **Rule-based**: Hand-crafted heuristics for common design patterns
//! - **ML-based**: Uses trained models (requires `ml-linfa` feature)
//!
//! The classifier can operate in hybrid mode, combining both approaches.

use crate::{CodePropertyGraph, CpgNodeKind, CpgEdgeKind, NodeId};
use crate::pattern::PatternMatch;
use rustc_hash::{FxHashMap, FxHashSet};

/// Classification mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClassificationMode {
    /// Use only rule-based heuristics.
    #[default]
    RuleBased,
    /// Use only ML-based classification (requires ml-linfa feature).
    MachineLearning,
    /// Combine both approaches, boosting confidence when they agree.
    Hybrid,
}

/// Feature vector for ML classification.
#[derive(Debug, Clone, Default)]
pub struct FeatureVector {
    /// Number of methods in the class.
    pub method_count: usize,
    /// Number of fields in the class.
    pub field_count: usize,
    /// Ratio of methods to fields.
    pub method_field_ratio: f64,
    /// Inheritance depth.
    pub inheritance_depth: usize,
    /// Number of interfaces/traits implemented.
    pub interface_count: usize,
    /// Number of static methods.
    pub static_method_count: usize,
    /// Whether class has a private constructor.
    pub has_private_constructor: bool,
    /// Number of factory-like methods (create*, make*, build*).
    pub factory_method_count: usize,
    /// Number of observer-like methods (register*, subscribe*, notify*, update*).
    pub observer_method_count: usize,
    /// Number of fields that reference interfaces/traits.
    pub interface_field_count: usize,
    /// Whether class implements an interface it also holds a reference to.
    pub is_decorator_candidate: bool,
}

impl FeatureVector {
    /// Converts the feature vector to an array for ML models.
    ///
    /// Used when the `ml-linfa` feature is enabled for training/prediction.
    #[allow(dead_code)]
    pub fn to_array(&self) -> [f64; 12] {
        [
            self.method_count as f64,
            self.field_count as f64,
            self.method_field_ratio,
            self.inheritance_depth as f64,
            self.interface_count as f64,
            self.static_method_count as f64,
            if self.has_private_constructor { 1.0 } else { 0.0 },
            self.factory_method_count as f64,
            self.observer_method_count as f64,
            self.interface_field_count as f64,
            if self.is_decorator_candidate { 1.0 } else { 0.0 },
            // Computed features
            (self.static_method_count as f64 / self.method_count.max(1) as f64),
        ]
    }
}

/// Classifier for design patterns using rules and/or ML.
#[derive(Debug)]
pub struct PatternClassifier {
    /// Minimum confidence threshold.
    min_confidence: f64,
    /// Classification mode.
    mode: ClassificationMode,
}

impl Default for PatternClassifier {
    fn default() -> Self {
        Self::new()
    }
}

impl PatternClassifier {
    /// Creates a new classifier with default settings.
    pub fn new() -> Self {
        Self {
            min_confidence: 0.7,
            mode: ClassificationMode::RuleBased,
        }
    }

    /// Sets the minimum confidence threshold.
    pub fn with_min_confidence(mut self, confidence: f64) -> Self {
        self.min_confidence = confidence;
        self
    }

    /// Sets the classification mode.
    pub fn with_mode(mut self, mode: ClassificationMode) -> Self {
        self.mode = mode;
        self
    }

    /// Classifies patterns in a CPG.
    ///
    /// Analyzes class structures in the CPG and identifies design patterns
    /// using the configured classification mode.
    pub fn classify(&self, cpg: &CodePropertyGraph) -> Vec<PatternMatch> {
        let mut matches = Vec::new();

        // Find all class/struct nodes
        let class_nodes: Vec<NodeId> = cpg
            .nodes()
            .filter(|n| matches!(n.kind, CpgNodeKind::Class { .. } | CpgNodeKind::Struct { .. }))
            .map(|n| n.id)
            .collect();

        for &class_id in &class_nodes {
            // Extract features for this class
            let features = self.extract_features(cpg, class_id);

            // Classify based on mode
            let pattern_matches = match self.mode {
                ClassificationMode::RuleBased => self.classify_rule_based(cpg, class_id, &features),
                ClassificationMode::MachineLearning => self.classify_ml(cpg, class_id, &features),
                ClassificationMode::Hybrid => self.classify_hybrid(cpg, class_id, &features),
            };

            // Filter by confidence and add to results
            for pm in pattern_matches {
                if pm.confidence >= self.min_confidence {
                    matches.push(pm);
                }
            }
        }

        // Sort by confidence (highest first)
        matches.sort_by(|a, b| {
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        matches
    }

    /// Extracts feature vector for a class.
    fn extract_features(&self, cpg: &CodePropertyGraph, class_id: NodeId) -> FeatureVector {
        let mut features = FeatureVector::default();

        // Collect methods and fields from this class
        let descendants = cpg.ast_descendants(class_id);

        let mut methods: Vec<NodeId> = Vec::new();
        let mut fields: Vec<NodeId> = Vec::new();
        let mut implemented_traits: FxHashSet<String> = FxHashSet::default();

        for desc_id in &descendants {
            if let Some(node) = cpg.node(*desc_id) {
                match &node.kind {
                    CpgNodeKind::Function { signature } => {
                        methods.push(*desc_id);
                        if signature.is_static {
                            features.static_method_count += 1;
                        }

                        let name_lower = signature.name.to_lowercase();

                        // Check for factory-like methods
                        if name_lower.starts_with("create")
                            || name_lower.starts_with("make")
                            || name_lower.starts_with("build")
                            || name_lower.starts_with("new_")
                            || name_lower == "new"
                        {
                            features.factory_method_count += 1;
                        }

                        // Check for observer-like methods
                        if name_lower.starts_with("register")
                            || name_lower.starts_with("subscribe")
                            || name_lower.starts_with("notify")
                            || name_lower.starts_with("update")
                            || name_lower.starts_with("add_observer")
                            || name_lower.starts_with("remove_observer")
                        {
                            features.observer_method_count += 1;
                        }

                        // Check for private constructor
                        if (name_lower == "new" || name_lower.contains("init"))
                            && matches!(signature.visibility, crate::Visibility::Private)
                        {
                            features.has_private_constructor = true;
                        }
                    }
                    CpgNodeKind::Field { .. } => {
                        fields.push(*desc_id);
                    }
                    _ => {}
                }
            }
        }

        features.method_count = methods.len();
        features.field_count = fields.len();
        features.method_field_ratio = if fields.is_empty() {
            methods.len() as f64
        } else {
            methods.len() as f64 / fields.len() as f64
        };

        // Check for implemented interfaces
        for edge in cpg.outgoing_edges(class_id) {
            match &edge.kind {
                CpgEdgeKind::Implements => {
                    if let Some(target) = cpg.node(edge.target) {
                        if let CpgNodeKind::Trait { name, .. } = &target.kind {
                            implemented_traits.insert(name.to_string());
                        }
                    }
                    features.interface_count += 1;
                }
                CpgEdgeKind::Inherits => {
                    features.inheritance_depth += 1;
                }
                _ => {}
            }
        }

        // Check fields for interface references (for decorator detection)
        for &field_id in &fields {
            if let Some(node) = cpg.node(field_id) {
                if let CpgNodeKind::Field { field_type: Some(type_info), .. } = &node.kind {
                    // Check if field type is an implemented interface
                    let type_name = type_info.name.to_string();
                    if implemented_traits.contains(&type_name) {
                        features.is_decorator_candidate = true;
                        features.interface_field_count += 1;
                    }
                }
            }
        }

        features
    }

    /// Rule-based pattern classification.
    fn classify_rule_based(
        &self,
        cpg: &CodePropertyGraph,
        class_id: NodeId,
        features: &FeatureVector,
    ) -> Vec<PatternMatch> {
        let mut matches = Vec::new();

        // Get class name
        let class_name = if let Some(node) = cpg.node(class_id) {
            match &node.kind {
                CpgNodeKind::Class { name, .. } | CpgNodeKind::Struct { name, .. } => {
                    name.to_string()
                }
                _ => String::new(),
            }
        } else {
            String::new()
        };

        // Singleton detection
        if let Some(pm) = self.detect_singleton(cpg, class_id, features, &class_name) {
            matches.push(pm);
        }

        // Factory detection
        if let Some(pm) = self.detect_factory(cpg, class_id, features, &class_name) {
            matches.push(pm);
        }

        // Observer detection
        if let Some(pm) = self.detect_observer(cpg, class_id, features, &class_name) {
            matches.push(pm);
        }

        // Strategy detection
        if let Some(pm) = self.detect_strategy(cpg, class_id, features, &class_name) {
            matches.push(pm);
        }

        // Decorator detection
        if let Some(pm) = self.detect_decorator(cpg, class_id, features, &class_name) {
            matches.push(pm);
        }

        matches
    }

    /// Detects Singleton pattern.
    ///
    /// Heuristics:
    /// - Private instance field (static)
    /// - Static getInstance-like method
    /// - Optional: private constructor
    fn detect_singleton(
        &self,
        cpg: &CodePropertyGraph,
        class_id: NodeId,
        features: &FeatureVector,
        class_name: &str,
    ) -> Option<PatternMatch> {
        let mut score = 0.0;
        let mut evidence = Vec::new();

        // Check for static methods with instance-returning names
        let descendants = cpg.ast_descendants(class_id);
        let mut has_get_instance = false;
        let mut has_static_instance_field = false;

        for desc_id in &descendants {
            if let Some(node) = cpg.node(*desc_id) {
                match &node.kind {
                    CpgNodeKind::Function { signature } => {
                        let name_lower = signature.name.to_lowercase();
                        if signature.is_static
                            && (name_lower.contains("instance")
                                || name_lower.contains("singleton")
                                || name_lower == "get")
                        {
                            has_get_instance = true;
                            evidence.push(format!("Static method: {}", signature.name));
                        }
                    }
                    CpgNodeKind::Field { name, field_type, .. } => {
                        let name_lower = name.to_lowercase();
                        if name_lower.contains("instance")
                            || name_lower.contains("singleton")
                            || name_lower == "self"
                        {
                            // Check if field type matches class name
                            if let Some(ft) = field_type {
                                if ft.name.as_ref() == class_name {
                                    has_static_instance_field = true;
                                    evidence.push(format!("Instance field: {}", name));
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        if has_get_instance {
            score += 0.4;
        }
        if has_static_instance_field {
            score += 0.3;
        }
        if features.has_private_constructor {
            score += 0.2;
            evidence.push("Private constructor".to_string());
        }
        // Bonus for having few public constructors
        if features.static_method_count > 0 && features.method_count <= 5 {
            score += 0.1;
        }

        if score >= 0.5 {
            let mut pm = PatternMatch::new("Singleton", class_id, score);
            pm.metadata
                .insert("evidence".to_string(), evidence.join("; "));
            Some(pm)
        } else {
            None
        }
    }

    /// Detects Factory pattern.
    ///
    /// Heuristics:
    /// - Multiple create*/make*/build* methods
    /// - Methods return different types
    fn detect_factory(
        &self,
        _cpg: &CodePropertyGraph,
        class_id: NodeId,
        features: &FeatureVector,
        _class_name: &str,
    ) -> Option<PatternMatch> {
        if features.factory_method_count >= 2 {
            let score = 0.5 + (features.factory_method_count as f64 * 0.1).min(0.4);
            let mut pm = PatternMatch::new("Factory", class_id, score);
            pm.metadata.insert(
                "factory_methods".to_string(),
                features.factory_method_count.to_string(),
            );
            Some(pm)
        } else if features.factory_method_count == 1 && features.method_count <= 3 {
            // Simple factory with single create method
            let pm = PatternMatch::new("Factory", class_id, 0.6);
            Some(pm)
        } else {
            None
        }
    }

    /// Detects Observer pattern.
    ///
    /// Heuristics:
    /// - register/subscribe + notify/update methods
    /// - Collection field for observers
    fn detect_observer(
        &self,
        _cpg: &CodePropertyGraph,
        class_id: NodeId,
        features: &FeatureVector,
        _class_name: &str,
    ) -> Option<PatternMatch> {
        if features.observer_method_count >= 2 {
            let score = 0.5 + (features.observer_method_count as f64 * 0.15).min(0.4);
            let pm = PatternMatch::new("Observer", class_id, score);
            Some(pm)
        } else {
            None
        }
    }

    /// Detects Strategy pattern.
    ///
    /// Heuristics:
    /// - Class holds reference to an interface
    /// - Interface has multiple implementations
    fn detect_strategy(
        &self,
        _cpg: &CodePropertyGraph,
        class_id: NodeId,
        features: &FeatureVector,
        _class_name: &str,
    ) -> Option<PatternMatch> {
        if features.interface_field_count >= 1 && features.interface_count == 0 {
            // Class holds interface reference but doesn't implement it (context)
            let score = 0.6 + (features.interface_field_count as f64 * 0.1).min(0.3);
            let pm = PatternMatch::new("Strategy", class_id, score);
            Some(pm)
        } else {
            None
        }
    }

    /// Detects Decorator pattern.
    ///
    /// Heuristics:
    /// - Class implements an interface AND holds a reference to the same interface
    fn detect_decorator(
        &self,
        _cpg: &CodePropertyGraph,
        class_id: NodeId,
        features: &FeatureVector,
        _class_name: &str,
    ) -> Option<PatternMatch> {
        if features.is_decorator_candidate {
            let score = 0.75;
            let pm = PatternMatch::new("Decorator", class_id, score);
            Some(pm)
        } else {
            None
        }
    }

    /// ML-based classification (requires ml-linfa feature).
    #[cfg(feature = "ml-linfa")]
    fn classify_ml(
        &self,
        cpg: &CodePropertyGraph,
        class_id: NodeId,
        features: &FeatureVector,
    ) -> Vec<PatternMatch> {
        // Note: In a real implementation, this would load a trained model
        // and use it for prediction. For now, we fall back to rule-based.
        self.classify_rule_based(cpg, class_id, features)
    }

    /// ML-based classification stub when feature is disabled.
    #[cfg(not(feature = "ml-linfa"))]
    fn classify_ml(
        &self,
        cpg: &CodePropertyGraph,
        class_id: NodeId,
        features: &FeatureVector,
    ) -> Vec<PatternMatch> {
        // Fall back to rule-based when ML is not available
        self.classify_rule_based(cpg, class_id, features)
    }

    /// Hybrid classification combining rules and ML.
    fn classify_hybrid(
        &self,
        cpg: &CodePropertyGraph,
        class_id: NodeId,
        features: &FeatureVector,
    ) -> Vec<PatternMatch> {
        let rule_matches = self.classify_rule_based(cpg, class_id, features);
        let ml_matches = self.classify_ml(cpg, class_id, features);

        // Merge results, boosting confidence when both agree
        let mut merged: FxHashMap<String, PatternMatch> = FxHashMap::default();

        for pm in rule_matches {
            merged.insert(pm.pattern_name.clone(), pm);
        }

        for pm in ml_matches {
            if let Some(existing) = merged.get_mut(&pm.pattern_name) {
                // Boost confidence when both methods agree
                let boosted = (existing.confidence + pm.confidence) / 2.0 * 1.1;
                existing.confidence = boosted.min(1.0);
            } else {
                merged.insert(pm.pattern_name.clone(), pm);
            }
        }

        merged.into_values().collect()
    }

    /// Returns the list of patterns this classifier can detect.
    pub fn supported_patterns(&self) -> &[&str] {
        &["Singleton", "Factory", "Observer", "Strategy", "Decorator"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CpgNode, Language, SourceRange, MethodSignature, Visibility};
    use std::sync::Arc;

    fn create_class(cpg: &mut CodePropertyGraph, name: &str) -> NodeId {
        cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Class {
                name: Arc::from(name),
                is_abstract: false,
            },
            SourceRange::default(),
        ))
    }

    fn add_method(
        cpg: &mut CodePropertyGraph,
        class_id: NodeId,
        name: &str,
        is_static: bool,
        visibility: Visibility,
    ) -> NodeId {
        use smallvec::smallvec;

        let mut node = CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Function {
                signature: MethodSignature {
                    name: Arc::from(name),
                    params: smallvec![],
                    return_type: None,
                    is_static,
                    is_async: false,
                    visibility,
                },
            },
            SourceRange::default(),
        );
        node.parent = Some(class_id);
        let method_id = cpg.add_node(node);
        cpg.connect(class_id, method_id, CpgEdgeKind::AstChild);
        method_id
    }

    #[test]
    fn test_classifier_creation() {
        let classifier = PatternClassifier::new();
        assert_eq!(classifier.min_confidence, 0.7);
        assert_eq!(classifier.mode, ClassificationMode::RuleBased);
    }

    #[test]
    fn test_classifier_with_settings() {
        let classifier = PatternClassifier::new()
            .with_min_confidence(0.8)
            .with_mode(ClassificationMode::Hybrid);
        assert_eq!(classifier.min_confidence, 0.8);
        assert_eq!(classifier.mode, ClassificationMode::Hybrid);
    }

    #[test]
    fn test_empty_cpg_classification() {
        let cpg = CodePropertyGraph::new(Language::Rust);
        let classifier = PatternClassifier::new();
        let matches = classifier.classify(&cpg);
        assert!(matches.is_empty());
    }

    #[test]
    fn test_feature_vector_to_array() {
        let features = FeatureVector {
            method_count: 5,
            field_count: 2,
            method_field_ratio: 2.5,
            inheritance_depth: 1,
            interface_count: 2,
            static_method_count: 1,
            has_private_constructor: true,
            factory_method_count: 0,
            observer_method_count: 0,
            interface_field_count: 1,
            is_decorator_candidate: false,
        };

        let arr = features.to_array();
        assert_eq!(arr[0], 5.0); // method_count
        assert_eq!(arr[6], 1.0); // has_private_constructor
    }

    #[test]
    fn test_supported_patterns() {
        let classifier = PatternClassifier::new();
        let patterns = classifier.supported_patterns();
        assert!(patterns.contains(&"Singleton"));
        assert!(patterns.contains(&"Factory"));
        assert!(patterns.contains(&"Observer"));
    }

    #[test]
    fn test_factory_detection() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let class_id = create_class(&mut cpg, "ProductFactory");

        add_method(&mut cpg, class_id, "create_product_a", false, Visibility::Public);
        add_method(&mut cpg, class_id, "create_product_b", false, Visibility::Public);
        add_method(&mut cpg, class_id, "make_product_c", false, Visibility::Public);

        let classifier = PatternClassifier::new().with_min_confidence(0.5);
        let matches = classifier.classify(&cpg);

        assert!(
            matches.iter().any(|m| m.pattern_name == "Factory"),
            "Expected Factory pattern to be detected"
        );
    }

    #[test]
    fn test_observer_detection() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let class_id = create_class(&mut cpg, "EventEmitter");

        add_method(&mut cpg, class_id, "register_listener", false, Visibility::Public);
        add_method(&mut cpg, class_id, "notify_all", false, Visibility::Public);
        add_method(&mut cpg, class_id, "update_state", false, Visibility::Public);

        let classifier = PatternClassifier::new().with_min_confidence(0.5);
        let matches = classifier.classify(&cpg);

        assert!(
            matches.iter().any(|m| m.pattern_name == "Observer"),
            "Expected Observer pattern to be detected"
        );
    }

    #[test]
    fn test_classification_mode_default() {
        let mode = ClassificationMode::default();
        assert_eq!(mode, ClassificationMode::RuleBased);
    }
}
