//! Public-API integration tests for deterministic reflexion-model conformance.

use std::collections::BTreeSet;
use std::sync::Arc;

use libcpg::{reflexion, LayerSpec, ReflexionDivergence, ReflexionRules, ReflexionVerdict};

fn rules() -> ReflexionRules {
    ReflexionRules {
        layers: vec![
            LayerSpec {
                name: "presentation".into(),
                prefixes: vec!["src/http/".into(), "src/cli/".into()],
            },
            LayerSpec {
                name: "application".into(),
                prefixes: vec!["src/app/".into()],
            },
            LayerSpec {
                name: "storage".into(),
                prefixes: vec!["src/db/".into()],
            },
        ],
        allow: vec![
            ("presentation".into(), "application".into()),
            ("application".into(), "storage".into()),
        ],
    }
}

fn edge(source: &str, target: &str) -> (Arc<str>, Arc<str>) {
    (source.into(), target.into())
}

#[test]
fn feature_free_root_api_is_exact_order_invariant_and_path_only() {
    let rules = rules();
    let edges = vec![
        edge("generated/schema.rs", "src/db/records.rs"),
        edge("src/db/records.rs", "src/app/service.rs"),
        edge("src/http/routes.rs", "src/db/records.rs"),
        edge("src/http/routes.rs", "src/app/service.rs"),
        edge("src/http/routes.rs", "src/cli/main.rs"),
    ];
    let original = edges.clone();
    let mut reversed = edges.clone();
    reversed.reverse();

    let summary = reflexion(&rules, edges);
    assert_eq!(summary, reflexion(&rules, reversed));
    assert_eq!(summary.convergences, 2);
    assert_eq!(summary.unlayered, 1);
    assert_eq!(
        summary.realized_pairs,
        BTreeSet::from([("presentation".into(), "application".into())])
    );
    assert_eq!(
        summary.divergences,
        vec![
            ReflexionDivergence {
                src_path: "src/db/records.rs".into(),
                dst_path: "src/app/service.rs".into(),
                from_layer: "storage".into(),
                to_layer: "application".into(),
            },
            ReflexionDivergence {
                src_path: "src/http/routes.rs".into(),
                dst_path: "src/db/records.rs".into(),
                from_layer: "presentation".into(),
                to_layer: "storage".into(),
            },
        ]
    );
    assert_eq!(
        rules.absences(&summary.realized_pairs),
        vec![("application".into(), "storage".into())]
    );
    assert_eq!(
        rules.classify("src/http/routes.rs", "src/app/service.rs"),
        ReflexionVerdict::Allowed
    );
    assert_eq!(
        original,
        vec![
            edge("generated/schema.rs", "src/db/records.rs"),
            edge("src/db/records.rs", "src/app/service.rs"),
            edge("src/http/routes.rs", "src/db/records.rs"),
            edge("src/http/routes.rs", "src/app/service.rs"),
            edge("src/http/routes.rs", "src/cli/main.rs"),
        ],
        "caller-owned path values remain unchanged"
    );
}

#[cfg(feature = "serde")]
#[test]
fn serialized_rules_drive_the_same_public_conformance_result() {
    let rules = rules();
    let json = serde_json::to_string(&rules).expect("serialize caller rules");
    let decoded: ReflexionRules = serde_json::from_str(&json).expect("deserialize caller rules");
    let edges = [
        edge("src/http/routes.rs", "src/app/service.rs"),
        edge("src/app/service.rs", "src/db/records.rs"),
    ];

    assert_eq!(reflexion(&decoded, edges.clone()), reflexion(&rules, edges));
}
