use proptest::prelude::*;

use super::*;

#[test]
fn pattern_variants_have_distinct_documented_semantics() {
    assert!(Pattern::Exact("eval".into()).matches("eval"));
    assert!(!Pattern::Exact("eval".into()).matches("js::eval"));
    assert!(Pattern::Suffix("::eval".into()).matches("js::eval"));
    assert!(Pattern::Substring("process".into()).matches("std::process::Command"));
    assert!(
        Pattern::AllSubstrings(vec!["rusqlite".into(), "execute".into()])
            .matches("rusqlite::Connection::execute")
    );
    assert!(
        !Pattern::AllSubstrings(vec!["rusqlite".into(), "execute".into()])
            .matches("rusqlite::Connection::prepare")
    );
    assert!(!Pattern::AllSubstrings(Vec::new()).matches("anything"));
    assert!(Pattern::Segment("eval".into()).matches("js::eval::run"));
    assert!(!Pattern::Segment("eval".into()).matches("evaluate"));
    assert_eq!(Pattern::Exact("x".into()).confidence(), 1.0);
    assert_eq!(Pattern::Segment("x".into()).confidence(), 0.95);
    assert_eq!(Pattern::Suffix("x".into()).confidence(), 0.9);
    assert_eq!(Pattern::Substring("x".into()).confidence(), 0.8);
    assert_eq!(Pattern::AllSubstrings(vec!["x".into()]).confidence(), 0.85);
}

#[test]
fn generic_catalog_preserves_open_source_sink_and_cwe_classes() {
    let catalog = TaintCatalog::builtin_generic();
    let env = catalog.classify("std::env::var");
    assert_eq!(env.sources[0].class.as_ref(), "env");
    let command = catalog.classify("std::process::Command::new");
    assert!(command
        .sinks
        .iter()
        .any(|matched| matched.class.as_ref() == "command" && matched.cwe == Some(78)));
    let sql = catalog.classify("sqlx::query");
    assert!(sql
        .sinks
        .iter()
        .any(|matched| matched.class.as_ref() == "sql" && matched.cwe == Some(89)));
    let deserialize = catalog.classify("pickle.load");
    assert!(deserialize
        .sinks
        .iter()
        .any(|matched| matched.class.as_ref() == "deserialize" && matched.cwe == Some(502)));
    assert!(catalog.classify("safe::sum").sources.is_empty());
    assert!(!catalog.classify("input.parse::<i64>").sanitizers.is_empty());
    assert!(matches!(
        catalog.external_model("rusqlite::Connection::execute"),
        ExternalModel::Sink { cwe: Some(89), .. }
    ));
    assert_eq!(
        catalog.external_model("rusqlite::Connection::prepare"),
        ExternalModel::Passthrough
    );
}

#[test]
fn literal_source_rules_match_only_their_typed_categories() {
    let catalog = TaintCatalog {
        literal_sources: vec![
            LiteralSourceRule {
                pattern: LiteralPattern::String,
                class: "embedded-text".into(),
                cwe: Some(798),
            },
            LiteralSourceRule {
                pattern: LiteralPattern::Array,
                class: "embedded-bytes".into(),
                cwe: Some(798),
            },
        ],
        ..TaintCatalog::empty()
    };

    assert_eq!(
        catalog.classify_literal(&LiteralKind::String("secret".into())),
        vec![CatalogMatch {
            class: "embedded-text".into(),
            cwe: Some(798),
            confidence: 1.0,
        }]
    );
    assert_eq!(
        catalog.classify_literal(&LiteralKind::Array)[0]
            .class
            .as_ref(),
        "embedded-bytes"
    );
    assert!(catalog
        .classify_literal(&LiteralKind::Integer(7))
        .is_empty());
}

#[test]
fn best_duplicate_match_and_external_specificity_are_deterministic() {
    let catalog = TaintCatalog {
        sources: vec![
            CatalogRule {
                pattern: Pattern::Substring("input".into()),
                class: "custom".into(),
                cwe: None,
            },
            CatalogRule {
                pattern: Pattern::Exact("input".into()),
                class: "custom".into(),
                cwe: None,
            },
        ],
        externals: vec![
            (Pattern::Substring("db".into()), ExternalModel::Havoc),
            (
                Pattern::Exact("db::query".into()),
                ExternalModel::Sink {
                    class: "query".into(),
                    cwe: Some(89),
                },
            ),
        ],
        ..TaintCatalog::empty()
    };
    assert_eq!(catalog.classify("input").sources[0].confidence, 1.0);
    assert!(matches!(
        catalog.external_model("db::query"),
        ExternalModel::Sink { cwe: Some(89), .. }
    ));
    assert_eq!(
        catalog.external_model("unknown::call"),
        ExternalModel::Passthrough
    );
}

#[cfg(feature = "serde")]
#[test]
fn catalog_round_trips_with_arc_classes() {
    let catalog = TaintCatalog::builtin_generic();
    let json = serde_json::to_string(&catalog).expect("catalog serializes");
    let decoded: TaintCatalog = serde_json::from_str(&json).expect("catalog deserializes");
    assert_eq!(decoded, catalog);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn classification_is_repeatable_for_arbitrary_paths(path in ".{0,96}") {
        let catalog = TaintCatalog::builtin_generic();
        prop_assert_eq!(catalog.classify(&path), catalog.classify(&path));
        prop_assert_eq!(catalog.external_model(&path), catalog.external_model(&path));
    }

    #[test]
    fn literal_classification_is_repeatable_for_arbitrary_strings(value in ".{0,96}") {
        let catalog = TaintCatalog {
            literal_sources: vec![LiteralSourceRule {
                pattern: LiteralPattern::String,
                class: "literal".into(),
                cwe: Some(798),
            }],
            ..TaintCatalog::empty()
        };
        let literal = LiteralKind::String(value.into());
        prop_assert_eq!(
            catalog.classify_literal(&literal),
            catalog.classify_literal(&literal)
        );
    }

    #[test]
    fn segment_matching_agrees_with_a_direct_boundary_oracle(
        segment in "[A-Za-z_][A-Za-z0-9_]{0,12}",
        left in "[A-Za-z_][A-Za-z0-9_]{0,12}",
        right in "[A-Za-z_][A-Za-z0-9_]{0,12}",
    ) {
        let path = format!("{left}::{segment}.{right}");
        prop_assert!(Pattern::Segment(segment.into()).matches(&path));
    }

    #[test]
    fn flat_conjunction_agrees_with_a_direct_substring_oracle(
        path in ".{0,96}",
        fragments in prop::collection::vec(".{0,12}", 0..8),
    ) {
        let expected = !fragments.is_empty()
            && fragments.iter().all(|fragment| path.contains(fragment));
        let pattern = Pattern::AllSubstrings(
            fragments.into_iter().map(Arc::<str>::from).collect()
        );
        prop_assert_eq!(pattern.matches(&path), expected);
    }
}
