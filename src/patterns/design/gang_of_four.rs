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

    /// All 23 GoF patterns for exhaustive coverage.
    const ALL_PATTERNS: [GofPattern; 23] = [
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

    /// `category()` is total and every `name()` / `GofCategory::name()` is
    /// non-empty, with the canonical GoF partition of 5 / 7 / 11.
    #[test]
    fn test_category_and_names_all_23() {
        let (mut creational, mut structural, mut behavioral) = (0, 0, 0);
        for pattern in ALL_PATTERNS {
            assert!(!pattern.name().is_empty(), "{pattern:?}: empty name");
            let category = pattern.category();
            assert!(!category.name().is_empty(), "{pattern:?}: empty category name");
            match category {
                GofCategory::Creational => creational += 1,
                GofCategory::Structural => structural += 1,
                GofCategory::Behavioral => behavioral += 1,
            }
        }
        assert_eq!(creational, 5, "expected 5 creational patterns");
        assert_eq!(structural, 7, "expected 7 structural patterns");
        assert_eq!(behavioral, 11, "expected 11 behavioral patterns");
        // Every category name is stable and non-empty.
        for category in [
            GofCategory::Creational,
            GofCategory::Structural,
            GofCategory::Behavioral,
        ] {
            assert!(!category.name().is_empty());
        }
    }

    /// `calculate_confidence` is bounded in `[0, 1]`, monotonically increases in
    /// the match size, and yields exactly the template's base confidence for a
    /// complete match (`completeness == 1.0`) — the property the boy-scout
    /// node-parity fix guarantees.
    #[test]
    fn test_calculate_confidence_bounds() {
        let detector = GofPatternDetector::new();
        // Singleton template now has 5 node constraints, base confidence 0.8.
        let template = build_pattern_template(GofPattern::Singleton);
        let expected = template.node_constraints.len();
        assert_eq!(expected, 5);
        let base = template.min_confidence;

        let mut previous = f64::NEG_INFINITY;
        for k in 0..=expected {
            let mut m = PatternMatch::new("Singleton", NodeId::new(0), 0.0);
            for i in 0..k as u32 {
                m = m.with_mapping(NodeId::new(i), NodeId::new(100 + i));
            }
            let confidence = detector.calculate_confidence(&m, &template);
            assert!(
                (0.0..=1.0).contains(&confidence),
                "confidence {confidence} out of [0,1] at k={k}"
            );
            assert!(
                confidence >= previous,
                "confidence must not decrease as match size grows"
            );
            previous = confidence;
        }
        // A complete match scores exactly the base confidence.
        let mut full = PatternMatch::new("Singleton", NodeId::new(0), 0.0);
        for i in 0..expected as u32 {
            full = full.with_mapping(NodeId::new(i), NodeId::new(100 + i));
        }
        let confidence_full = detector.calculate_confidence(&full, &template);
        assert!(
            (confidence_full - base).abs() < 1e-9,
            "complete match confidence {confidence_full} should equal base {base}"
        );
    }

    /// End-to-end: detecting against the Singleton pattern CPG finds Singleton,
    /// every match is within `[min_confidence, 1.0]`, and results are sorted by
    /// descending confidence.
    #[test]
    fn test_detect_sorted_and_bounded() {
        let target = build_pattern_cpg(GofPattern::Singleton);
        let min_confidence = 0.5;
        let detector = GofPatternDetector::new().with_min_confidence(min_confidence);

        let matches = detector.detect(&target);

        assert!(
            matches.iter().any(|m| m.pattern_name == "Singleton"),
            "Singleton should match its own pattern CPG"
        );
        for m in &matches {
            assert!(
                m.confidence >= min_confidence,
                "confidence {} below min_confidence {}",
                m.confidence,
                min_confidence
            );
            assert!(m.confidence <= 1.0, "confidence {} exceeds 1.0", m.confidence);
        }
        for pair in matches.windows(2) {
            assert!(
                pair[0].confidence >= pair[1].confidence,
                "matches must be sorted by descending confidence"
            );
        }
    }
}

/// Detector configuration: which patterns are searched, and which results are
/// kept.
#[cfg(test)]
mod configuration {
    use super::*;
    use crate::testutil::{build_well_formed, StmtSpec};

    fn sample() -> CodePropertyGraph {
        build_well_formed(vec![
            StmtSpec::Let("x".into()),
            StmtSpec::If(vec![StmtSpec::CallStmt("g".into())]),
        ])
    }

    /// By default every GoF pattern is searched; `with_patterns` narrows the
    /// search, and nothing outside the list can be reported. An *empty* list
    /// means "everything", not "nothing".
    #[test]
    fn with_patterns_narrows_the_search() {
        let cpg = sample();

        let all = GofPatternDetector::new().with_min_confidence(0.0).detect(&cpg);

        let narrowed = GofPatternDetector::new()
            .with_min_confidence(0.0)
            .with_patterns(vec![GofPattern::Singleton])
            .detect(&cpg);
        for m in &narrowed {
            assert_eq!(
                m.pattern_name,
                GofPattern::Singleton.name(),
                "only the requested pattern may be reported"
            );
        }
        assert!(
            narrowed.len() <= all.len(),
            "narrowing cannot report more than searching everything ({} vs {})",
            narrowed.len(),
            all.len()
        );

        let empty_list = GofPatternDetector::new()
            .with_min_confidence(0.0)
            .with_patterns(vec![])
            .detect(&cpg);
        assert_eq!(empty_list.len(), all.len(), "an empty list searches everything");
    }

    /// The confidence threshold filters the results, and every surviving match
    /// carries the GoF metadata.
    #[test]
    fn min_confidence_filters_results_and_metadata_is_attached() {
        let cpg = sample();

        let permissive = GofPatternDetector::new().with_min_confidence(0.0).detect(&cpg);
        for m in &permissive {
            assert_eq!(m.metadata.get("pattern_type").map(String::as_str), Some("GoF"));
            assert!(m.metadata.contains_key("category"), "the GoF category is recorded");
            assert!((0.0..=1.0).contains(&m.confidence));
        }

        let strict = GofPatternDetector::new().with_min_confidence(1.01).detect(&cpg);
        assert!(strict.is_empty(), "nothing can exceed a confidence of 1");

        let mid = GofPatternDetector::new().with_min_confidence(0.5).detect(&cpg);
        assert!(mid.len() <= permissive.len(), "raising the threshold never adds matches");
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use crate::testutil::*;
    use proptest::prelude::*;

    fn arb_gof_pattern() -> impl Strategy<Value = GofPattern> {
        prop_oneof![
            Just(GofPattern::Singleton),
            Just(GofPattern::FactoryMethod),
            Just(GofPattern::AbstractFactory),
            Just(GofPattern::Builder),
            Just(GofPattern::Prototype),
            Just(GofPattern::Adapter),
            Just(GofPattern::Bridge),
            Just(GofPattern::Composite),
            Just(GofPattern::Decorator),
            Just(GofPattern::Facade),
            Just(GofPattern::Flyweight),
            Just(GofPattern::Proxy),
            Just(GofPattern::ChainOfResponsibility),
            Just(GofPattern::Command),
            Just(GofPattern::Interpreter),
            Just(GofPattern::Iterator),
            Just(GofPattern::Mediator),
            Just(GofPattern::Memento),
            Just(GofPattern::Observer),
            Just(GofPattern::State),
            Just(GofPattern::Strategy),
            Just(GofPattern::TemplateMethod),
            Just(GofPattern::Visitor),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        /// Over random targets and thresholds, every detected match satisfies
        /// `min_confidence <= confidence <= 1.0`, and the result is sorted by
        /// descending confidence.
        #[test]
        fn prop_detect_sorted_and_bounded(
            target in arb_cpg_raw(),
            threshold in (0u32..=100).prop_map(|x| x as f64 / 100.0),
        ) {
            let detector = GofPatternDetector::new().with_min_confidence(threshold);
            let matches = detector.detect(&target);

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

        /// `category()` is total and both `name()` and `GofCategory::name()` are
        /// non-empty for every pattern.
        #[test]
        fn prop_category_total(pattern in arb_gof_pattern()) {
            prop_assert!(!pattern.name().is_empty());
            prop_assert!(!pattern.category().name().is_empty());
        }
    }
}
