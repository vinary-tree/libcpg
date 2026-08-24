#![cfg(feature = "metta")]

use libcpg::{
    analyze_critical_pairs, dependency_pairs, materialize_rewrite_dep_overlay, terminates,
    Language, ProjectionKind, TerminationVerdict, TreeSitterCpgBuilder,
};

fn build_metta(source: &str) -> libcpg::CodePropertyGraph {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_metta::LANGUAGE.into())
        .expect("set MeTTa grammar");
    let tree = parser.parse(source, None).expect("parse MeTTa");
    TreeSitterCpgBuilder::new()
        .build_from_tree(&tree, source, Language::MeTTa)
        .expect("build MeTTa CPG")
}

#[test]
fn public_rewrite_workflow_proves_termination_and_materializes_conflicts() {
    let terminating = build_metta("(= (f (S $x)) (g $x))\n(= (g (S $x)) (f $x))\n");
    let graph = dependency_pairs(&terminating);
    assert!(graph.complete);
    assert_eq!(graph.projection.kind, ProjectionKind::RewriteDependency);
    assert!(matches!(
        terminates(&terminating),
        TerminationVerdict::Terminates(_)
    ));

    let mut conflicting = build_metta("(= (pick $x) A)\n(= (pick $x) B)\n");
    let dependencies = dependency_pairs(&conflicting);
    let critical = analyze_critical_pairs(&conflicting);
    assert!(critical.complete);
    assert!(!critical.pairs.is_empty());

    let inserted = materialize_rewrite_dep_overlay(&mut conflicting, &dependencies, &critical)
        .expect("all analysis endpoints belong to the CPG");
    assert!(inserted > 0);
    assert_eq!(conflicting.stats().rewrite_dep_edges, inserted);
}
