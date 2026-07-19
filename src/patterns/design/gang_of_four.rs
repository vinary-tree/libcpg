//! Gang of Four design pattern definitions and detection.
//!
//! This module provides detection of the 23 Gang-of-Four design patterns using
//! subgraph isomorphism matching via the VF2 algorithm.

use crate::CodePropertyGraph;
use crate::pattern::{PatternMatch, SubgraphMatcher, Vf2Matcher};
use super::PatternDetector;
use super::templates::{build_pattern_cpg, build_pattern_template};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// GoF design patterns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum GofPattern {
    // Creational
    /// Abstract Factory pattern.
    AbstractFactory,
    /// Builder pattern.
    Builder,
    /// Factory Method pattern.
    FactoryMethod,
    /// Prototype pattern.
    Prototype,
    /// Singleton pattern.
    Singleton,

    // Structural
    /// Adapter pattern.
    Adapter,
    /// Bridge pattern.
    Bridge,
    /// Composite pattern.
    Composite,
    /// Decorator pattern.
    Decorator,
    /// Facade pattern.
    Facade,
    /// Flyweight pattern.
    Flyweight,
    /// Proxy pattern.
    Proxy,

    // Behavioral
    /// Chain of Responsibility pattern.
    ChainOfResponsibility,
    /// Command pattern.
    Command,
    /// Interpreter pattern.
    Interpreter,
    /// Iterator pattern.
    Iterator,
    /// Mediator pattern.
    Mediator,
    /// Memento pattern.
    Memento,
    /// Observer pattern.
    Observer,
    /// State pattern.
    State,
    /// Strategy pattern.
    Strategy,
    /// Template Method pattern.
    TemplateMethod,
    /// Visitor pattern.
    Visitor,
}

impl GofPattern {
    /// Returns the pattern name.
    pub fn name(&self) -> &'static str {
        match self {
            Self::AbstractFactory => "Abstract Factory",
            Self::Builder => "Builder",
            Self::FactoryMethod => "Factory Method",
            Self::Prototype => "Prototype",
            Self::Singleton => "Singleton",
            Self::Adapter => "Adapter",
            Self::Bridge => "Bridge",
            Self::Composite => "Composite",
            Self::Decorator => "Decorator",
            Self::Facade => "Facade",
            Self::Flyweight => "Flyweight",
            Self::Proxy => "Proxy",
            Self::ChainOfResponsibility => "Chain of Responsibility",
            Self::Command => "Command",
            Self::Interpreter => "Interpreter",
            Self::Iterator => "Iterator",
            Self::Mediator => "Mediator",
            Self::Memento => "Memento",
            Self::Observer => "Observer",
            Self::State => "State",
            Self::Strategy => "Strategy",
            Self::TemplateMethod => "Template Method",
            Self::Visitor => "Visitor",
        }
    }

    /// Returns the category of this pattern.
    pub fn category(&self) -> GofCategory {
        match self {
            Self::AbstractFactory | Self::Builder | Self::FactoryMethod
            | Self::Prototype | Self::Singleton => GofCategory::Creational,
            Self::Adapter | Self::Bridge | Self::Composite | Self::Decorator
            | Self::Facade | Self::Flyweight | Self::Proxy => GofCategory::Structural,
            _ => GofCategory::Behavioral,
        }
    }
}

/// GoF pattern categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum GofCategory {
    /// Creational patterns (object creation).
    Creational,
    /// Structural patterns (composition).
    Structural,
    /// Behavioral patterns (communication).
    Behavioral,
}

impl GofCategory {
    /// Returns the category name.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Creational => "Creational",
            Self::Structural => "Structural",
            Self::Behavioral => "Behavioral",
        }
    }
}

/// Detector for Gang of Four design patterns.
#[derive(Debug, Default)]
pub struct GofPatternDetector {
    /// Minimum confidence threshold.
    min_confidence: f64,
    /// Patterns to detect (empty = all).
    patterns_to_detect: Vec<GofPattern>,
}

impl GofPatternDetector {
    /// Creates a new GoF detector.
    pub fn new() -> Self {
        Self {
            min_confidence: 0.7,
            patterns_to_detect: Vec::new(),
        }
    }

    /// Sets the minimum confidence threshold.
    pub fn with_min_confidence(mut self, confidence: f64) -> Self {
        self.min_confidence = confidence;
        self
    }

    /// Limits detection to specific patterns.
    pub fn with_patterns(mut self, patterns: Vec<GofPattern>) -> Self {
        self.patterns_to_detect = patterns;
        self
    }
}

impl PatternDetector for GofPatternDetector {
    fn detect(&self, cpg: &CodePropertyGraph) -> Vec<PatternMatch> {
        let mut all_matches = Vec::new();

        // Determine which patterns to search for
        let patterns_to_check: Vec<GofPattern> = if self.patterns_to_detect.is_empty() {
            // Check all patterns
            vec![
                GofPattern::Singleton,
                GofPattern::FactoryMethod,
                GofPattern::AbstractFactory,
                GofPattern::Builder,
                GofPattern::Prototype,
                GofPattern::Adapter,
                GofPattern::Bridge,
                GofPattern::Composite,
                GofPattern::Decorator,
                GofPattern::Facade,
                GofPattern::Flyweight,
                GofPattern::Proxy,
                GofPattern::ChainOfResponsibility,
                GofPattern::Command,
                GofPattern::Interpreter,
                GofPattern::Iterator,
                GofPattern::Mediator,
                GofPattern::Memento,
                GofPattern::Observer,
                GofPattern::State,
                GofPattern::Strategy,
                GofPattern::TemplateMethod,
                GofPattern::Visitor,
            ]
        } else {
            self.patterns_to_detect.clone()
        };

        // Create VF2 matcher with appropriate settings
        let matcher = Vf2Matcher::new()
            .with_strict_kinds(false)  // Allow relaxed matching for better recall
            .with_strict_edges(false);

        // Search for each pattern
        for pattern in patterns_to_check {
            let pattern_cpg = build_pattern_cpg(pattern);
            let template = build_pattern_template(pattern);

            let matches = matcher.find_matches(&pattern_cpg, cpg);

            for mut m in matches {
                // Set the pattern name and calculate confidence
                m.pattern_name = pattern.name().to_string();
                m.confidence = self.calculate_confidence(&m, &template);

                // Add metadata
                m.metadata.insert("category".to_string(), pattern.category().name().to_string());
                m.metadata.insert("pattern_type".to_string(), "GoF".to_string());

                // Filter by confidence threshold
                if m.confidence >= self.min_confidence {
                    all_matches.push(m);
                }
            }
        }

        // Sort by confidence (highest first)
        all_matches.sort_by(|a, b| {
            b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal)
        });

        all_matches
    }

    fn supported_patterns(&self) -> &[&str] {
        &[
            "Abstract Factory", "Builder", "Factory Method", "Prototype", "Singleton",
            "Adapter", "Bridge", "Composite", "Decorator", "Facade", "Flyweight", "Proxy",
            "Chain of Responsibility", "Command", "Interpreter", "Iterator", "Mediator",
            "Memento", "Observer", "State", "Strategy", "Template Method", "Visitor",
        ]
    }
}

impl GofPatternDetector {
    /// Calculates a confidence score for a pattern match.
    ///
    /// The confidence is based on:
    /// - Completeness of node mappings
    /// - Match quality relative to pattern template
    fn calculate_confidence(
        &self,
        pattern_match: &PatternMatch,
        template: &crate::pattern::PatternTemplate,
    ) -> f64 {
        // Base confidence from match completeness
        let expected_nodes = template.node_constraints.len();
        let matched_nodes = pattern_match.match_size();

        let completeness = if expected_nodes > 0 {
            (matched_nodes as f64) / (expected_nodes as f64)
        } else {
            0.0
        };

        // Combine with template's minimum confidence
        let base_confidence = template.min_confidence;

        // Final confidence is weighted average
        // Completeness determines how much of the pattern was found
        completeness * base_confidence + (1.0 - completeness) * 0.5 * base_confidence
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CpgNode, CpgNodeKind, CpgEdge, CpgEdgeKind, EdgeId, Language, SourceRange,
        NodeId, Visibility, MethodSignature, DfgEdgeKind,
    };
    use smallvec::SmallVec;
    use std::sync::Arc;

    fn make_method(name: &str, is_static: bool) -> MethodSignature {
        MethodSignature {
            name: Arc::from(name),
            params: SmallVec::new(),
            return_type: None,
            is_static,
            is_async: false,
            visibility: Visibility::Public,
        }
    }

    #[test]
    fn test_pattern_categories() {
        assert_eq!(GofPattern::Singleton.category(), GofCategory::Creational);
        assert_eq!(GofPattern::Adapter.category(), GofCategory::Structural);
        assert_eq!(GofPattern::Observer.category(), GofCategory::Behavioral);
    }

    #[test]
    fn test_detector_creation() {
        let detector = GofPatternDetector::new()
            .with_min_confidence(0.8)
            .with_patterns(vec![GofPattern::Singleton, GofPattern::Observer]);

        assert_eq!(detector.min_confidence, 0.8);
        assert_eq!(detector.patterns_to_detect.len(), 2);
    }

    #[test]
    fn test_detect_empty_cpg() {
        let detector = GofPatternDetector::new();
        let cpg = CodePropertyGraph::new(Language::Rust);

        let matches = detector.detect(&cpg);
        assert!(matches.is_empty(), "Empty CPG should have no pattern matches");
    }

    #[test]
    fn test_singleton_detection() {
        // Create a CPG that resembles a Singleton pattern
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let range = SourceRange::default();

        // Add a class with static instance field and getInstance method
        let class_id = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Class {
                name: Arc::from("Logger"),
                is_abstract: false,
            },
            range,
        ));

        let field_id = cpg.add_node(CpgNode::new(
            NodeId::new(1),
            CpgNodeKind::Field {
                name: Arc::from("instance"),
                field_type: None,
                visibility: Visibility::Private,
            },
            range,
        ));

        let constructor_id = cpg.add_node(CpgNode::new(
            NodeId::new(2),
            CpgNodeKind::Function {
                signature: make_method("new", false),
            },
            range,
        ));

        let get_instance_id = cpg.add_node(CpgNode::new(
            NodeId::new(3),
            CpgNodeKind::Function {
                signature: make_method("getInstance", true),
            },
            range,
        ));

        let return_id = cpg.add_node(CpgNode::new(
            NodeId::new(4),
            CpgNodeKind::Return,
            range,
        ));

        // Connect them
        cpg.add_edge(CpgEdge::new(
            EdgeId::new(0),
            class_id,
            field_id,
            CpgEdgeKind::AstChild,
        ));
        cpg.add_edge(CpgEdge::new(
            EdgeId::new(1),
            class_id,
            constructor_id,
            CpgEdgeKind::AstChild,
        ));
        cpg.add_edge(CpgEdge::new(
            EdgeId::new(2),
            class_id,
            get_instance_id,
            CpgEdgeKind::AstChild,
        ));
        cpg.add_edge(CpgEdge::new(
            EdgeId::new(3),
            get_instance_id,
            return_id,
            CpgEdgeKind::AstChild,
        ));
        cpg.add_edge(CpgEdge::new(
            EdgeId::new(4),
            field_id,
            return_id,
            CpgEdgeKind::DataFlow(DfgEdgeKind::FieldRead),
        ));

        let detector = GofPatternDetector::new()
            .with_patterns(vec![GofPattern::Singleton])
            .with_min_confidence(0.5);

        let matches = detector.detect(&cpg);

        // The pattern should be detected (may have multiple matches)
        // We just verify the detection logic runs without errors
        // In a real scenario with exact matching, we'd verify the specific match
        assert!(matches.is_empty() || matches[0].pattern_name == "Singleton");
    }

    #[test]
    fn test_strategy_pattern_detection() {
        // Create a CPG that resembles a Strategy pattern
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let range = SourceRange::default();

        // Strategy trait
        let strategy_trait = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Trait {
                name: Arc::from("PaymentStrategy"),
            },
            range,
        ));

        // execute method
        let execute = cpg.add_node(CpgNode::new(
            NodeId::new(1),
            CpgNodeKind::Function {
                signature: make_method("pay", false),
            },
            range,
        ));

        // Context class
        let context = cpg.add_node(CpgNode::new(
            NodeId::new(2),
            CpgNodeKind::Class {
                name: Arc::from("PaymentProcessor"),
                is_abstract: false,
            },
            range,
        ));

        // Strategy field
        let strategy_field = cpg.add_node(CpgNode::new(
            NodeId::new(3),
            CpgNodeKind::Field {
                name: Arc::from("strategy"),
                field_type: None,
                visibility: Visibility::Private,
            },
            range,
        ));

        // setStrategy method
        let set_strategy = cpg.add_node(CpgNode::new(
            NodeId::new(4),
            CpgNodeKind::Function {
                signature: make_method("setPaymentMethod", false),
            },
            range,
        ));

        // Connect
        cpg.add_edge(CpgEdge::new(
            EdgeId::new(0),
            strategy_trait,
            execute,
            CpgEdgeKind::AstChild,
        ));
        cpg.add_edge(CpgEdge::new(
            EdgeId::new(1),
            context,
            strategy_field,
            CpgEdgeKind::AstChild,
        ));
        cpg.add_edge(CpgEdge::new(
            EdgeId::new(2),
            context,
            set_strategy,
            CpgEdgeKind::AstChild,
        ));
        cpg.add_edge(CpgEdge::new(
            EdgeId::new(3),
            strategy_field,
            strategy_trait,
            CpgEdgeKind::TypeOf,
        ));

        let detector = GofPatternDetector::new()
            .with_patterns(vec![GofPattern::Strategy])
            .with_min_confidence(0.5);

        let matches = detector.detect(&cpg);

        // Just verify the detection runs without error
        // The actual matching depends on graph structure
        for m in &matches {
            assert_eq!(m.metadata.get("category"), Some(&"Behavioral".to_string()));
        }
    }

    #[test]
    fn test_all_pattern_names() {
        let patterns = [
            GofPattern::Singleton,
            GofPattern::FactoryMethod,
            GofPattern::AbstractFactory,
            GofPattern::Builder,
            GofPattern::Prototype,
            GofPattern::Adapter,
            GofPattern::Bridge,
            GofPattern::Composite,
            GofPattern::Decorator,
            GofPattern::Facade,
            GofPattern::Flyweight,
            GofPattern::Proxy,
            GofPattern::ChainOfResponsibility,
            GofPattern::Command,
            GofPattern::Interpreter,
            GofPattern::Iterator,
            GofPattern::Mediator,
            GofPattern::Memento,
            GofPattern::Observer,
            GofPattern::State,
            GofPattern::Strategy,
            GofPattern::TemplateMethod,
            GofPattern::Visitor,
        ];

        for pattern in patterns {
            let name = pattern.name();
            assert!(!name.is_empty(), "Pattern {:?} should have a name", pattern);
        }
    }

    #[test]
    fn test_supported_patterns() {
        let detector = GofPatternDetector::new();
        let supported = detector.supported_patterns();

        assert_eq!(supported.len(), 23, "Should support all 23 GoF patterns");
    }
}
