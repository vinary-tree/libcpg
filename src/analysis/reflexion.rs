//! Deterministic reflexion-model conformance over path dependency edges.
//!
//! A caller declares ordered path-prefix layers and permitted directed
//! cross-layer dependencies. [`reflexion`] compares that model with observed
//! `(source_path, target_path)` edges without reading the filesystem or CPG.

use std::collections::BTreeSet;
use std::sync::Arc;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// One named architectural layer and the path prefixes assigned to it.
///
/// Prefixes are matched textually with [`str::starts_with`]. Layer order is
/// semantic: the first layer containing a matching prefix owns the path.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct LayerSpec {
    /// Stable layer name used by permitted dependency pairs and results.
    pub name: Arc<str>,
    /// Project-relative path prefixes owned by this layer.
    pub prefixes: Vec<Arc<str>>,
}

/// Declared layered architecture for reflexion-model comparison.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ReflexionRules {
    /// Layers in first-match precedence order.
    pub layers: Vec<LayerSpec>,
    /// Permitted directed cross-layer `(source, target)` pairs.
    pub allow: Vec<(Arc<str>, Arc<str>)>,
}

impl ReflexionRules {
    /// Returns whether the rules declare no layers.
    pub fn is_empty(&self) -> bool {
        self.layers.is_empty()
    }

    /// Returns the first declared layer whose prefix matches `path`.
    ///
    /// The path is not normalized or resolved on the filesystem. Returns
    /// `None` when no prefix covers it.
    pub fn layer_of(&self, path: &str) -> Option<&str> {
        self.layers
            .iter()
            .find(|layer| {
                layer
                    .prefixes
                    .iter()
                    .any(|prefix| path.starts_with(prefix.as_ref()))
            })
            .map(|layer| layer.name.as_ref())
    }

    /// Classifies one observed directed path dependency.
    pub fn classify(&self, source_path: &str, target_path: &str) -> ReflexionVerdict {
        match (self.layer_of(source_path), self.layer_of(target_path)) {
            (Some(source), Some(target)) if source == target => ReflexionVerdict::SameLayer,
            (Some(source), Some(target))
                if self.allow.iter().any(|(allowed_source, allowed_target)| {
                    allowed_source.as_ref() == source && allowed_target.as_ref() == target
                }) =>
            {
                ReflexionVerdict::Allowed
            }
            (Some(_), Some(_)) => ReflexionVerdict::Divergence,
            _ => ReflexionVerdict::Unlayered,
        }
    }

    /// Returns declared permitted pairs that no observed edge realized.
    ///
    /// Duplicate declarations are collapsed and the result is sorted
    /// lexicographically by `(source_layer, target_layer)`.
    pub fn absences(
        &self,
        realized_pairs: &BTreeSet<(Arc<str>, Arc<str>)>,
    ) -> Vec<(Arc<str>, Arc<str>)> {
        self.allow
            .iter()
            .filter(|pair| !realized_pairs.contains(*pair))
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }
}

/// Reflexion classification for one observed path dependency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ReflexionVerdict {
    /// Both endpoints belong to the same layer, which is always permitted.
    SameLayer,
    /// The endpoints belong to a declared permitted cross-layer pair.
    Allowed,
    /// Both endpoints are layered, but their directed pair is not permitted.
    Divergence,
    /// At least one endpoint has no matching declared layer.
    Unlayered,
}

/// One observed dependency that diverges from the declared architecture.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ReflexionDivergence {
    /// Observed source path.
    pub src_path: Arc<str>,
    /// Observed target path.
    pub dst_path: Arc<str>,
    /// Declared layer owning `src_path`.
    pub from_layer: Arc<str>,
    /// Declared layer owning `dst_path`.
    pub to_layer: Arc<str>,
}

/// Deterministic conformance summary for a complete observed edge set.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ReflexionSummary {
    /// Violating edges sorted by `(src_path, dst_path)`.
    pub divergences: Vec<ReflexionDivergence>,
    /// Number of permitted observed edges, including same-layer edges.
    pub convergences: usize,
    /// Number of observed edges with at least one unlayered endpoint.
    pub unlayered: usize,
    /// Distinct permitted cross-layer pairs realized by observed edges.
    pub realized_pairs: BTreeSet<(Arc<str>, Arc<str>)>,
}

/// Compares observed path dependencies with a declared reflexion model.
///
/// Each input edge is classified exactly once. Same-layer and explicitly
/// permitted cross-layer edges are convergences; covered but forbidden edges
/// are divergences; and edges with an uncovered endpoint are unlayered. The
/// output is independent of input edge order. Duplicate observed edges remain
/// distinct observations in the counters and divergence list, while realized
/// layer pairs are deduplicated.
///
/// Use [`ReflexionRules::absences`] to derive permitted cross-layer relations
/// that were not realized. Layer membership is a lexical path-prefix policy:
/// this function performs no path canonicalization or filesystem access.
pub fn reflexion<I>(rules: &ReflexionRules, edges: I) -> ReflexionSummary
where
    I: IntoIterator<Item = (Arc<str>, Arc<str>)>,
{
    let allowed: BTreeSet<_> = rules.allow.iter().cloned().collect();
    let mut summary = ReflexionSummary::default();

    for (src_path, dst_path) in edges {
        let source_layer = rules.layer_of(&src_path);
        let target_layer = rules.layer_of(&dst_path);
        match (source_layer, target_layer) {
            (Some(source), Some(target)) if source == target => {
                summary.convergences += 1;
            }
            (Some(source), Some(target)) => {
                let pair = (Arc::<str>::from(source), Arc::<str>::from(target));
                if allowed.contains(&pair) {
                    summary.convergences += 1;
                    summary.realized_pairs.insert(pair);
                } else {
                    summary.divergences.push(ReflexionDivergence {
                        src_path,
                        dst_path,
                        from_layer: pair.0,
                        to_layer: pair.1,
                    });
                }
            }
            _ => summary.unlayered += 1,
        }
    }

    summary.divergences.sort_unstable_by(|left, right| {
        left.src_path
            .cmp(&right.src_path)
            .then_with(|| left.dst_path.cmp(&right.dst_path))
    });
    summary
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn rules() -> ReflexionRules {
        ReflexionRules {
            layers: vec![
                LayerSpec {
                    name: "api".into(),
                    prefixes: vec!["src/api/".into(), "src/mcp/".into()],
                },
                LayerSpec {
                    name: "domain".into(),
                    prefixes: vec!["src/graph/".into()],
                },
                LayerSpec {
                    name: "data".into(),
                    prefixes: vec!["src/db/".into()],
                },
            ],
            allow: vec![
                ("api".into(), "domain".into()),
                ("domain".into(), "data".into()),
            ],
        }
    }

    fn edge(source: &str, target: &str) -> (Arc<str>, Arc<str>) {
        (source.into(), target.into())
    }

    #[test]
    fn layered_oracle_classifies_convergences_divergences_and_absences() {
        let rules = rules();
        let summary = reflexion(
            &rules,
            [
                edge("src/util/x.rs", "src/db/query.rs"),
                edge("src/db/query.rs", "src/graph/model.rs"),
                edge("src/api/http.rs", "src/db/query.rs"),
                edge("src/api/http.rs", "src/mcp/server.rs"),
                edge("src/api/http.rs", "src/graph/model.rs"),
            ],
        );

        assert_eq!(summary.convergences, 2);
        assert_eq!(summary.unlayered, 1);
        assert_eq!(
            summary.realized_pairs,
            BTreeSet::from([("api".into(), "domain".into())])
        );
        assert_eq!(
            summary.divergences,
            vec![
                ReflexionDivergence {
                    src_path: "src/api/http.rs".into(),
                    dst_path: "src/db/query.rs".into(),
                    from_layer: "api".into(),
                    to_layer: "data".into(),
                },
                ReflexionDivergence {
                    src_path: "src/db/query.rs".into(),
                    dst_path: "src/graph/model.rs".into(),
                    from_layer: "data".into(),
                    to_layer: "domain".into(),
                },
            ]
        );
        assert_eq!(
            rules.absences(&summary.realized_pairs),
            vec![("domain".into(), "data".into())]
        );
    }

    #[test]
    fn first_prefix_match_and_single_edge_verdicts_follow_declared_order() {
        let mut rules = rules();
        rules.layers.insert(
            0,
            LayerSpec {
                name: "specific".into(),
                prefixes: vec!["src/api/internal/".into()],
            },
        );

        assert!(!rules.is_empty());
        assert_eq!(
            rules.layer_of("src/api/internal/token.rs"),
            Some("specific")
        );
        assert_eq!(rules.layer_of("src/api/public.rs"), Some("api"));
        assert_eq!(rules.layer_of("README.md"), None);
        assert_eq!(
            rules.classify("src/api/a.rs", "src/mcp/b.rs"),
            ReflexionVerdict::SameLayer
        );
        assert_eq!(
            rules.classify("src/api/a.rs", "src/graph/b.rs"),
            ReflexionVerdict::Allowed
        );
        assert_eq!(
            rules.classify("src/db/a.rs", "src/graph/b.rs"),
            ReflexionVerdict::Divergence
        );
        assert_eq!(
            rules.classify("README.md", "src/graph/b.rs"),
            ReflexionVerdict::Unlayered
        );
    }

    #[test]
    fn empty_duplicate_and_uncovered_public_values_are_total() {
        let empty = ReflexionRules::default();
        assert!(empty.is_empty());
        let empty_summary = reflexion(&empty, [edge("", "src/any.rs")]);
        assert_eq!(empty_summary.unlayered, 1);
        assert!(empty_summary.divergences.is_empty());
        assert!(empty.absences(&empty_summary.realized_pairs).is_empty());

        let mut duplicate = rules();
        duplicate.allow.push(("domain".into(), "data".into()));
        let summary = reflexion(
            &duplicate,
            [
                edge("src/api/b.rs", "src/db/z.rs"),
                edge("src/api/b.rs", "src/db/z.rs"),
            ],
        );
        assert_eq!(summary.divergences.len(), 2);
        assert_eq!(summary.convergences, 0);
        assert_eq!(summary.unlayered, 0);
        assert_eq!(
            duplicate.absences(&summary.realized_pairs),
            vec![
                ("api".into(), "domain".into()),
                ("domain".into(), "data".into()),
            ]
        );
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn generated_edge_sets_are_order_invariant_and_exhaustively_classified(
            raw_edges in prop::collection::vec((0usize..5, 0usize..5), 0..128),
        ) {
            let paths: [Arc<str>; 5] = [
                "src/api/a.rs".into(),
                "src/mcp/b.rs".into(),
                "src/graph/c.rs".into(),
                "src/db/d.rs".into(),
                "src/util/e.rs".into(),
            ];
            let edges: Vec<_> = raw_edges
                .into_iter()
                .map(|(source, target)| (paths[source].clone(), paths[target].clone()))
                .collect();
            let mut reversed = edges.clone();
            reversed.reverse();

            let rules = rules();
            let first = reflexion(&rules, edges.clone());
            let second = reflexion(&rules, reversed);
            prop_assert_eq!(&first, &second);
            prop_assert_eq!(
                first.convergences + first.divergences.len() + first.unlayered,
                edges.len()
            );
            let divergences_are_sorted = first.divergences.windows(2).all(|window| {
                (&window[0].src_path, &window[0].dst_path)
                    <= (&window[1].src_path, &window[1].dst_path)
            });
            prop_assert!(divergences_are_sorted);
            for pair in &first.realized_pairs {
                prop_assert!(rules.allow.contains(pair));
            }
        }
    }

    #[cfg(feature = "serde")]
    #[test]
    fn public_values_round_trip_through_json() {
        let rules = rules();
        let summary = reflexion(
            &rules,
            [
                edge("src/api/a.rs", "src/graph/b.rs"),
                edge("src/db/c.rs", "src/api/a.rs"),
            ],
        );
        let encoded_rules = serde_json::to_string(&rules).expect("serialize rules");
        let encoded_summary = serde_json::to_string(&summary).expect("serialize summary");
        assert_eq!(
            serde_json::from_str::<ReflexionRules>(&encoded_rules).expect("deserialize rules"),
            rules
        );
        assert_eq!(
            serde_json::from_str::<ReflexionSummary>(&encoded_summary)
                .expect("deserialize summary"),
            summary
        );
    }
}
