#![cfg(feature = "approximate-matching")]

use libcpg::{
    ApproximateCancellation, ApproximateConfig, ApproximateId, ApproximateRecord, ApproximateTape,
};

#[cfg(all(feature = "lang-rust", feature = "lang-python"))]
use libcpg::{approximate_revision_alignments, ApproximateCompletion, ApproximateMethod};

#[cfg(all(feature = "lang-rust", feature = "lang-python"))]
#[test]
fn parser_built_rust_query_aligns_with_python_function() {
    use libcpg::{CpgBuilder, Language, TreeSitterCpgBuilder};

    let rust = TreeSitterCpgBuilder::new()
        .build(
            "fn calculate_total(values: &[i32]) -> i32 { values.iter().sum() }",
            Language::Rust,
        )
        .expect("parser-built Rust query graph");
    let python = TreeSitterCpgBuilder::new()
        .build(
            "def calculate_totl(values):\n    return sum(values)\n",
            Language::Python,
        )
        .expect("parser-built Python candidate graph");
    let rust_function = rust
        .nodes()
        .find(|node| node.name() == Some("calculate_total"))
        .expect("Rust function name");
    let query = ApproximateRecord::new(
        ApproximateId::CpgNode {
            graph: 1,
            node: rust_function.id,
        },
        ApproximateTape::text("calculate_total"),
    );
    let config = ApproximateConfig {
        method: ApproximateMethod::DamerauLevenshtein,
        max_distance: 2,
        ..ApproximateConfig::default()
    };
    let before = (python.node_count(), python.edge_count());
    let report = approximate_revision_alignments(
        &query,
        &python,
        2,
        &config,
        &ApproximateCancellation::default(),
    )
    .expect("bounded cross-language candidate generation");

    assert_eq!(report.completion, ApproximateCompletion::Complete);
    let candidate = report.evidence.first().expect("one close Python name");
    let ApproximateId::CpgNode { graph, node } = candidate.candidate else {
        panic!("parser-built candidate should retain its CPG identity");
    };
    assert_eq!(graph, 2);
    assert_eq!(
        python.node(node).and_then(|item| item.name()),
        Some("calculate_totl")
    );
    assert_eq!((python.node_count(), python.edge_count()), before);
}

#[test]
fn public_text_search_is_unicode_scalar_based() {
    let query = ApproximateRecord::new(
        ApproximateId::External { corpus: 1, item: 0 },
        ApproximateTape::text("café"),
    );
    let candidates = [ApproximateRecord::new(
        ApproximateId::External { corpus: 1, item: 1 },
        ApproximateTape::text("cafe"),
    )];
    let report = libcpg::approximate_candidates(
        &query,
        &candidates,
        libcpg::ApproximateConsumer::RevisionAlignment,
        &ApproximateConfig::default(),
        &ApproximateCancellation::default(),
    )
    .expect("Unicode candidate search");

    assert_eq!(report.evidence[0].raw_cost.numerator, 1);
}

#[cfg(feature = "lang-rust")]
#[test]
fn parser_built_symbol_candidates_preserve_qualified_shadowed_and_generated_identities() {
    use std::collections::BTreeSet;

    use libcpg::{
        approximate_symbol_candidates, ApproximateConsumer, CpgBuilder, ExactConfirmation,
        Language, TreeSitterCpgBuilder,
    };

    let source = r#"
        trait Render { fn render(&self); }
        impl Render for i32 { fn render(&self) {} }
        impl Render for &str { fn render(&self) {} }
        mod left { pub fn render() {} }
        mod right { pub fn render() {} }
        fn generated_adapter_42() {}
        fn café() {}
        fn shadowing() {
            let shadowed = 1;
            { let shadowed = 2; let _ = shadowed; }
            let _ = shadowed;
        }
    "#;
    let cpg = TreeSitterCpgBuilder::new()
        .build(source, Language::Rust)
        .expect("parser-built qualified, overloaded, generated, and Unicode symbols");
    let before = (cpg.node_count(), cpg.edge_count());
    let config = ApproximateConfig {
        max_distance: 2,
        top_k: 128,
        ..ApproximateConfig::default()
    };

    let render_report = approximate_symbol_candidates(
        &ApproximateRecord::new(
            ApproximateId::External { corpus: 1, item: 1 },
            ApproximateTape::text("rendr"),
        ),
        &cpg,
        9,
        &config,
        &ApproximateCancellation::default(),
    )
    .expect("bounded overloaded symbol candidates");
    let render_ids = render_report
        .evidence
        .iter()
        .filter_map(|evidence| match evidence.candidate {
            ApproximateId::CpgNode { node, .. }
                if cpg.node(node).and_then(|candidate| candidate.name()) == Some("render") =>
            {
                Some(node)
            }
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    assert!(render_ids.len() >= 4, "{render_report:#?}");
    assert!(render_report.evidence.iter().all(|evidence| {
        evidence.consumer == ApproximateConsumer::SymbolNavigation
            && evidence.exact_confirmation == ExactConfirmation::UserSelectionOrExactLookup
    }));

    for (item, query, expected) in [
        (2, "generated_adapter_24", "generated_adapter_42"),
        (3, "café", "café"),
        (4, "shadowd", "shadowed"),
    ] {
        let report = approximate_symbol_candidates(
            &ApproximateRecord::new(
                ApproximateId::External { corpus: 1, item },
                ApproximateTape::text(query),
            ),
            &cpg,
            9,
            &config,
            &ApproximateCancellation::default(),
        )
        .expect("bounded parser-built symbol candidates");
        assert!(
            report.evidence.iter().any(|evidence| {
                let ApproximateId::CpgNode { graph, node } = evidence.candidate else {
                    return false;
                };
                graph == 9
                    && cpg.node(node).and_then(|candidate| candidate.name()) == Some(expected)
            }),
            "query {query:?}: {report:#?}"
        );
    }

    assert_eq!((cpg.node_count(), cpg.edge_count()), before);
}

#[cfg(feature = "lang-rust")]
#[test]
fn every_structural_consumer_operates_on_parser_built_immutable_projections() {
    use libcpg::{
        approximate_clone_candidates, approximate_diff_candidates,
        approximate_vulnerability_analogues, project_cpg_functions, ApproximateConsumer,
        CpgBuilder, ExactConfirmation, Language, TreeSitterCpgBuilder,
    };

    let source = r#"
        fn sanitize_input(input: &str) -> String {
            input.trim().replace('<', "&lt;")
        }
    "#;
    let query_graph = TreeSitterCpgBuilder::new()
        .build(source, Language::Rust)
        .expect("parser-built structural query graph");
    let candidate_graph = TreeSitterCpgBuilder::new()
        .build(source, Language::Rust)
        .expect("parser-built structural candidate graph");
    let query_projection =
        project_cpg_functions(&query_graph, 1, &ApproximateConfig::default().limits)
            .expect("immutable structural query projection");
    let query = query_projection
        .records
        .into_iter()
        .find(|record| match record.id {
            ApproximateId::CpgNode { node, .. } => {
                query_graph
                    .node(node)
                    .and_then(|candidate| candidate.name())
                    == Some("sanitize_input")
            }
            ApproximateId::ApiUsage { .. } | ApproximateId::External { .. } => false,
        })
        .expect("projected sanitize_input query");
    let before = (candidate_graph.node_count(), candidate_graph.edge_count());
    let config = ApproximateConfig {
        max_distance: 0,
        ..ApproximateConfig::default()
    };
    let cancellation = ApproximateCancellation::default();
    let reports = [
        (
            approximate_clone_candidates(&query, &candidate_graph, 2, &config, &cancellation)
                .expect("clone candidates"),
            ApproximateConsumer::CloneCandidate,
            ExactConfirmation::ExactCloneStructure,
        ),
        (
            approximate_diff_candidates(&query, &candidate_graph, 2, &config, &cancellation)
                .expect("semantic-diff candidates"),
            ApproximateConsumer::SemanticDiffCandidate,
            ExactConfirmation::ExactCpgDiff,
        ),
        (
            approximate_vulnerability_analogues(
                &query,
                &candidate_graph,
                2,
                &config,
                &cancellation,
            )
            .expect("vulnerability-analogue candidates"),
            ApproximateConsumer::VulnerabilityAnalogue,
            ExactConfirmation::ExactVulnerabilityStructure,
        ),
    ];

    for (report, consumer, confirmation) in reports {
        let evidence = report.evidence.first().expect("exact structural candidate");
        assert_eq!(evidence.consumer, consumer);
        assert_eq!(evidence.exact_confirmation, confirmation);
        assert_eq!(evidence.raw_cost.numerator, 0);
        let ApproximateId::CpgNode { graph, node } = evidence.candidate else {
            panic!("structural candidate must preserve its graph identity");
        };
        assert_eq!(graph, 2);
        assert_eq!(
            candidate_graph
                .node(node)
                .and_then(|candidate| candidate.name()),
            Some("sanitize_input")
        );
    }
    assert_eq!(
        (candidate_graph.node_count(), candidate_graph.edge_count()),
        before
    );
}

#[cfg(feature = "lang-rust")]
#[test]
fn parser_built_api_usage_graphs_drive_only_confirmable_sequence_candidates() {
    use libcpg::{
        analyze_api_usage_mining, approximate_api_sequence_neighbors, project_api_usages,
        ApiUsageMiningConfig, ApproximateConsumer, CpgBuilder, ExactConfirmation, Language,
        TreeSitterCpgBuilder,
    };

    let source = r#"
        fn open() -> i32 { 1 }
        fn read(_handle: &i32) {}
        fn close(_handle: i32) {}
        fn use_resource() {
            let handle = open();
            read(&handle);
            close(handle);
        }
    "#;
    let cpg = TreeSitterCpgBuilder::new()
        .build(source, Language::Rust)
        .expect("parser-built API usage corpus");
    let before = (cpg.node_count(), cpg.edge_count());
    let mining = analyze_api_usage_mining(&cpg, &[], &ApiUsageMiningConfig::default())
        .expect("bounded API usage mining");
    assert!(!mining.usage_graphs.is_empty(), "{mining:#?}");
    let projection = project_api_usages(
        &mining.usage_graphs,
        1,
        &ApproximateConfig::default().limits,
    )
    .expect("immutable API usage query projection");
    let query = projection
        .records
        .first()
        .cloned()
        .expect("one projected API usage query");
    let report = approximate_api_sequence_neighbors(
        &query,
        &mining.usage_graphs,
        2,
        &ApproximateConfig {
            max_distance: 0,
            ..ApproximateConfig::default()
        },
        &ApproximateCancellation::default(),
    )
    .expect("bounded API sequence candidates");
    let evidence = report
        .evidence
        .first()
        .expect("exact API sequence neighbor");

    assert_eq!(evidence.consumer, ApproximateConsumer::ApiSequenceNeighbor);
    assert_eq!(
        evidence.exact_confirmation,
        ExactConfirmation::ExactApiUsageTemporalProperty
    );
    assert_eq!(evidence.raw_cost.numerator, 0);
    assert!(matches!(
        evidence.candidate,
        ApproximateId::ApiUsage { graph: 2, .. }
    ));
    assert_eq!((cpg.node_count(), cpg.edge_count()), before);
}

#[test]
fn homoglyphs_are_compared_as_distinct_unicode_scalars() {
    let query = ApproximateRecord::new(
        ApproximateId::External { corpus: 4, item: 0 },
        ApproximateTape::text("parser"),
    );
    let candidates = [ApproximateRecord::new(
        ApproximateId::External { corpus: 4, item: 1 },
        ApproximateTape::text("pаrser"),
    )];
    let report = libcpg::approximate_candidates(
        &query,
        &candidates,
        libcpg::ApproximateConsumer::VulnerabilityAnalogue,
        &ApproximateConfig::default(),
        &ApproximateCancellation::default(),
    )
    .expect("homoglyph evidence remains explicit");

    assert_eq!(report.evidence[0].raw_cost.numerator, 1);
    assert_ne!(query.tape, candidates[0].tape);
}

#[cfg(feature = "serde")]
#[test]
fn evidence_json_is_deterministic_versioned_and_round_trips() {
    let query = ApproximateRecord::new(
        ApproximateId::External { corpus: 5, item: 0 },
        ApproximateTape::text("parser"),
    );
    let candidates = [ApproximateRecord::new(
        ApproximateId::External { corpus: 5, item: 1 },
        ApproximateTape::text("parse"),
    )];
    let report = libcpg::approximate_candidates(
        &query,
        &candidates,
        libcpg::ApproximateConsumer::RevisionAlignment,
        &ApproximateConfig::default(),
        &ApproximateCancellation::default(),
    )
    .expect("serializable approximate evidence");

    let first = serde_json::to_vec(&report).expect("serialize evidence");
    let second = serde_json::to_vec(&report).expect("repeat evidence serialization");
    let decoded: libcpg::ApproximateReport =
        serde_json::from_slice(&first).expect("deserialize evidence");

    assert_eq!(first, second);
    assert_eq!(decoded, report);
    assert_eq!(
        decoded.provenance.schema_version,
        libcpg::APPROXIMATE_EVIDENCE_SCHEMA_VERSION
    );
}
