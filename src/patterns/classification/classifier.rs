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

    fn add_field(
        cpg: &mut CodePropertyGraph,
        class_id: NodeId,
        name: &str,
        ty: Option<&str>,
    ) -> NodeId {
        use crate::TypeInfo;
        let mut node = CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Field {
                name: Arc::from(name),
                field_type: ty.map(TypeInfo::new),
                visibility: Visibility::Private,
            },
            SourceRange::default(),
        );
        node.parent = Some(class_id);
        let id = cpg.add_node(node);
        cpg.connect(class_id, id, CpgEdgeKind::AstChild);
        id
    }

    fn add_trait(cpg: &mut CodePropertyGraph, name: &str) -> NodeId {
        cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Trait { name: Arc::from(name) },
            SourceRange::default(),
        ))
    }

    /// End-to-end Singleton detection: static `getInstance` + self-typed
    /// `instance` field.
    #[test]
    fn test_singleton_detection() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let logger = create_class(&mut cpg, "Logger");
        add_method(&mut cpg, logger, "getInstance", true, Visibility::Public);
        add_field(&mut cpg, logger, "instance", Some("Logger"));

        let classifier = PatternClassifier::new().with_min_confidence(0.6);
        let matches = classifier.classify(&cpg);

        let singleton = matches
            .iter()
            .find(|m| m.pattern_name == "Singleton")
            .expect("Expected Singleton to be detected");
        assert!(singleton.confidence >= 0.6);
        assert!(singleton.confidence <= 1.0);
    }

    /// End-to-end Decorator detection: implements a trait AND holds a field of
    /// that trait's type.
    #[test]
    fn test_decorator_detection() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let decorator = create_class(&mut cpg, "LoggingDecorator");
        let component = add_trait(&mut cpg, "Component");
        cpg.connect(decorator, component, CpgEdgeKind::Implements);
        add_field(&mut cpg, decorator, "inner", Some("Component"));

        let classifier = PatternClassifier::new().with_min_confidence(0.7);
        let matches = classifier.classify(&cpg);

        assert!(
            matches.iter().any(|m| m.pattern_name == "Decorator"),
            "Expected Decorator pattern to be detected"
        );
        for m in &matches {
            assert!(m.confidence <= 1.0);
        }
    }

    /// The Strategy detector's scoring logic, exercised directly (both the
    /// firing and the non-firing branch).
    #[test]
    fn test_detect_strategy_scoring() {
        let cpg = CodePropertyGraph::new(Language::Rust);
        let classifier = PatternClassifier::new();
        let class_id = NodeId::new(0);

        // Fires: holds interface references but implements none (a context).
        let fires = FeatureVector {
            interface_field_count: 2,
            interface_count: 0,
            ..FeatureVector::default()
        };
        let pm = classifier
            .detect_strategy(&cpg, class_id, &fires, "Context")
            .expect("Strategy should fire");
        assert_eq!(pm.pattern_name, "Strategy");
        assert!(pm.confidence >= 0.6);
        assert!(pm.confidence <= 1.0);

        // Does not fire when the class also implements an interface.
        let quiet = FeatureVector {
            interface_field_count: 1,
            interface_count: 1,
            ..FeatureVector::default()
        };
        assert!(classifier.detect_strategy(&cpg, class_id, &quiet, "Context").is_none());
    }

    /// Hybrid mode agrees with rule-based and never lowers the confidence below
    /// the rule score, staying within `[0, 1]`.
    #[test]
    fn test_hybrid_mode_boost() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let factory = create_class(&mut cpg, "WidgetFactory");
        add_method(&mut cpg, factory, "create_a", false, Visibility::Public);
        add_method(&mut cpg, factory, "create_b", false, Visibility::Public);

        let classifier = PatternClassifier::new()
            .with_mode(ClassificationMode::Hybrid)
            .with_min_confidence(0.5);
        let matches = classifier.classify(&cpg);

        let factory_match = matches
            .iter()
            .find(|m| m.pattern_name == "Factory")
            .expect("Expected Factory under hybrid mode");
        // Rule score was 0.7; hybrid boost = (0.7+0.7)/2 * 1.1 = 0.77.
        assert!(factory_match.confidence >= 0.7 - 1e-9);
        assert!(factory_match.confidence <= 1.0);
    }

    /// `with_min_confidence` filters out matches below the threshold.
    #[test]
    fn test_min_confidence_filtering() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let factory = create_class(&mut cpg, "SimpleFactory");
        // A single create method -> Factory score 0.6.
        add_method(&mut cpg, factory, "create_widget", false, Visibility::Public);

        let strict = PatternClassifier::new().with_min_confidence(0.7);
        assert!(
            strict.classify(&cpg).is_empty(),
            "0.6-confidence match should be filtered at threshold 0.7"
        );

        let lax = PatternClassifier::new().with_min_confidence(0.5);
        let matches = lax.classify(&cpg);
        assert!(
            matches
                .iter()
                .any(|m| m.pattern_name == "Factory" && (m.confidence - 0.6).abs() < 1e-9),
            "0.6-confidence match should pass at threshold 0.5"
        );
    }

    /// Results are sorted by descending confidence across multiple classes.
    #[test]
    fn test_classify_sorted_descending() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let strong = create_class(&mut cpg, "StrongFactory");
        for name in ["create_a", "create_b", "create_c"] {
            add_method(&mut cpg, strong, name, false, Visibility::Public);
        }
        let weak = create_class(&mut cpg, "WeakFactory");
        add_method(&mut cpg, weak, "create_x", false, Visibility::Public);

        let classifier = PatternClassifier::new().with_min_confidence(0.5);
        let matches = classifier.classify(&cpg);

        assert!(matches.len() >= 2, "expected matches from both factories");
        for pair in matches.windows(2) {
            assert!(
                pair[0].confidence >= pair[1].confidence,
                "matches must be sorted by descending confidence"
            );
        }
    }
}

/// Feature-extraction and scoring vocabularies.
///
/// The rule-based classifier recognizes roles by *method and field naming
/// conventions*: a factory method is one whose name starts with `create`,
/// `make`, `build`, `new_`, or is exactly `new`. Every alternative is a
/// separate rule, and a dropped one silently stops recognizing every class
/// that spells the role that way — so each is pinned individually.
#[cfg(test)]
mod vocabulary {
    use super::*;
    use crate::{CpgNode, Language, MethodSignature, SourceRange, TypeInfo, Visibility};
    use std::sync::Arc;
    use smallvec::smallvec;

    fn class(cpg: &mut CodePropertyGraph, name: &str) -> NodeId {
        cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Class { name: Arc::from(name), is_abstract: false },
            SourceRange::default(),
        ))
    }

    fn method(
        cpg: &mut CodePropertyGraph,
        class_id: NodeId,
        name: &str,
        is_static: bool,
        visibility: Visibility,
    ) -> NodeId {
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
        let id = cpg.add_node(node);
        cpg.connect(class_id, id, CpgEdgeKind::AstChild);
        if let Some(c) = cpg.node_mut(class_id) {
            c.children.push(id);
        }
        id
    }

    fn field(
        cpg: &mut CodePropertyGraph,
        class_id: NodeId,
        name: &str,
        field_type: Option<&str>,
    ) -> NodeId {
        let mut node = CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Field {
                name: Arc::from(name),
                field_type: field_type.map(TypeInfo::new),
                visibility: Visibility::Private,
            },
            SourceRange::default(),
        );
        node.parent = Some(class_id);
        let id = cpg.add_node(node);
        cpg.connect(class_id, id, CpgEdgeKind::AstChild);
        if let Some(c) = cpg.node_mut(class_id) {
            c.children.push(id);
        }
        id
    }

    /// A class with exactly the named methods, and the extracted features.
    fn features_of(method_names: &[(&str, bool, Visibility)]) -> FeatureVector {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let c = class(&mut cpg, "C");
        for (name, is_static, vis) in method_names {
            method(&mut cpg, c, name, *is_static, *vis);
        }
        PatternClassifier::new().extract_features(&cpg, c)
    }

    /// Every factory-method spelling increments the factory count.
    #[test]
    fn every_factory_method_name_is_recognized() {
        for name in ["create", "create_widget", "make", "make_it", "build", "builder", "new_with", "new"] {
            let f = features_of(&[(name, false, Visibility::Public)]);
            assert_eq!(f.factory_method_count, 1, "`{name}` is a factory method");
        }
        // A name outside the vocabulary is not one. `renew` is deliberate: it
        // *contains* "new" but does not start with it and is not exactly "new".
        for name in ["run", "renew", "newton"] {
            let f = features_of(&[(name, false, Visibility::Public)]);
            assert_eq!(f.factory_method_count, 0, "`{name}` is not a factory method");
        }
    }

    /// Every observer-method spelling increments the observer count.
    #[test]
    fn every_observer_method_name_is_recognized() {
        for name in [
            "register", "register_all", "subscribe", "subscribe_to", "notify", "notify_all",
            "update", "update_state", "add_observer", "remove_observer",
        ] {
            let f = features_of(&[(name, false, Visibility::Public)]);
            assert_eq!(f.observer_method_count, 1, "`{name}` is an observer method");
        }
        for name in ["run", "observe"] {
            let f = features_of(&[(name, false, Visibility::Public)]);
            assert_eq!(f.observer_method_count, 0, "`{name}` is not an observer method");
        }
    }

    /// A private constructor is a private `new`, or a private name containing
    /// `init`; a *public* one of either spelling is not.
    #[test]
    fn a_private_constructor_needs_both_the_name_and_the_visibility() {
        for name in ["new", "init", "do_init", "initialize"] {
            let private = features_of(&[(name, false, Visibility::Private)]);
            assert!(private.has_private_constructor, "private `{name}`");
            let public = features_of(&[(name, false, Visibility::Public)]);
            assert!(!public.has_private_constructor, "public `{name}`");
        }
        let other = features_of(&[("run", false, Visibility::Private)]);
        assert!(!other.has_private_constructor, "private `run` is not a constructor");
    }

    /// Every singleton accessor spelling scores, as does every instance-field
    /// spelling *whose type is the class itself*.
    #[test]
    fn singleton_evidence_requires_the_right_name_and_type() {
        let detect = |accessor: &str, field_name: &str, field_type: Option<&str>| {
            let mut cpg = CodePropertyGraph::new(Language::Rust);
            let c = class(&mut cpg, "Registry");
            method(&mut cpg, c, accessor, true, Visibility::Public);
            method(&mut cpg, c, "new", false, Visibility::Private);
            field(&mut cpg, c, field_name, field_type);
            let features = PatternClassifier::new().extract_features(&cpg, c);
            let name = "Registry".to_string();
            PatternClassifier::new().detect_singleton(&cpg, c, &features, &name)
        };

        // Accessor spellings.
        for accessor in ["instance", "get_instance", "singleton", "the_singleton", "get"] {
            assert!(
                detect(accessor, "instance", Some("Registry")).is_some(),
                "`{accessor}` is a singleton accessor"
            );
        }
        // Field spellings.
        for field_name in ["instance", "the_instance", "singleton", "self"] {
            assert!(
                detect("get_instance", field_name, Some("Registry")).is_some(),
                "`{field_name}` is an instance field"
            );
        }
        // A field of the right name but the wrong type is not evidence, and an
        // untyped field cannot be checked at all.
        let wrong_type = detect("frobnicate", "instance", Some("SomethingElse"));
        let untyped = detect("frobnicate", "instance", None);
        assert!(
            wrong_type.is_none() && untyped.is_none(),
            "an instance field must be typed as the class itself"
        );
    }

    /// The factory detector has two arms: several factory methods, or exactly
    /// one in a small class.
    #[test]
    fn factory_detection_has_a_bulk_and_a_single_method_arm() {
        let detect = |names: &[&str]| {
            let mut cpg = CodePropertyGraph::new(Language::Rust);
            let c = class(&mut cpg, "F");
            for n in names {
                method(&mut cpg, c, n, false, Visibility::Public);
            }
            let features = PatternClassifier::new().extract_features(&cpg, c);
            PatternClassifier::new().detect_factory(&cpg, c, &features, "F")
        };

        // Two or more factory methods: score grows with the count.
        let two = detect(&["create_a", "create_b"]).expect("two factory methods");
        let three = detect(&["create_a", "create_b", "make_c"]).expect("three factory methods");
        assert!(three.confidence > two.confidence, "more evidence ⇒ more confidence");
        assert!(two.confidence >= 0.5);
        assert!(three.confidence <= 1.0);

        // Exactly one, in a small class.
        let single = detect(&["create_a", "run"]).expect("a single-method factory");
        assert!((single.confidence - 0.6).abs() < 1e-9);

        // Exactly one, in a large class: not a factory.
        assert!(
            detect(&["create_a", "a", "b", "c"]).is_none(),
            "one factory method among many is not a factory"
        );
        // None at all.
        assert!(detect(&["a", "b"]).is_none());
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use crate::testutil::*;
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]

        /// Under both rule-based and hybrid modes, every classified match has
        /// `min_confidence <= confidence <= 1.0`, and results are sorted by
        /// descending confidence.
        #[test]
        fn prop_classify_bounded_and_sorted(
            cpg in arb_cpg_raw(),
            threshold in (0u32..=100).prop_map(|x| x as f64 / 100.0),
            hybrid in any::<bool>(),
        ) {
            let mode = if hybrid {
                ClassificationMode::Hybrid
            } else {
                ClassificationMode::RuleBased
            };
            let classifier = PatternClassifier::new()
                .with_mode(mode)
                .with_min_confidence(threshold);
            let matches = classifier.classify(&cpg);

            for m in &matches {
                prop_assert!(
                    m.confidence >= threshold,
                    "confidence {} < min_confidence {}",
                    m.confidence,
                    threshold
                );
                prop_assert!(m.confidence <= 1.0, "confidence {} > 1.0", m.confidence);
            }
            for pair in matches.windows(2) {
                prop_assert!(
                    pair[0].confidence >= pair[1].confidence,
                    "not sorted descending"
                );
            }
        }
    }
}
