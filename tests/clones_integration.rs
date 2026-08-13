use std::sync::Arc;

use libcpg::analysis::AnalysisError;
use libcpg::pattern::{detect_clones, CloneOptions, CloneType};
use libcpg::{
    CodePropertyGraph, CpgEdgeKind, CpgNode, CpgNodeKind, Language, LiteralKind, MethodSignature,
    NodeId, SourceRange, Visibility,
};
use smallvec::smallvec;

fn function(name: &str) -> CpgNodeKind {
    CpgNodeKind::Function {
        signature: MethodSignature {
            name: Arc::from(name),
            params: smallvec![],
            return_type: None,
            is_static: false,
            is_async: false,
            visibility: Visibility::Private,
        },
    }
}

fn identifier(name: &str) -> CpgNodeKind {
    CpgNodeKind::Identifier {
        name: Arc::from(name),
        definition: None,
    }
}

fn integer(value: i64) -> CpgNodeKind {
    CpgNodeKind::Literal {
        kind: LiteralKind::Integer(value),
    }
}

fn add_function(
    cpg: &mut CodePropertyGraph,
    body: impl IntoIterator<Item = CpgNodeKind>,
) -> (NodeId, Vec<NodeId>) {
    let root = cpg.add_node(CpgNode::new(
        NodeId::new(0),
        function("same"),
        SourceRange::default(),
    ));
    let mut children = Vec::new();
    for kind in body {
        let child = cpg.add_node(CpgNode::new(NodeId::new(0), kind, SourceRange::default()));
        cpg.connect(root, child, CpgEdgeKind::AstChild);
        children.push(child);
    }
    (root, children)
}

fn classified_pair(left: Vec<CpgNodeKind>, right: Vec<CpgNodeKind>, dependence: bool) -> CloneType {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let (left_root, left_body) = add_function(&mut cpg, left);
    let (right_root, right_body) = add_function(&mut cpg, right);
    if dependence {
        for child in left_body {
            cpg.connect(left_root, child, CpgEdgeKind::ControlDependence);
        }
        for child in right_body {
            cpg.connect(right_root, child, CpgEdgeKind::ControlDependence);
        }
    }
    let classes = detect_clones(&cpg, &CloneOptions::default()).unwrap();
    assert_eq!(classes.len(), 1, "classes: {classes:?}");
    assert_eq!(classes[0].members, vec![left_root, right_root]);
    classes[0].clone_type
}

#[test]
fn public_api_distinguishes_all_four_clone_types_without_features() {
    assert_eq!(
        classified_pair(
            vec![identifier("x"), integer(1)],
            vec![identifier("x"), integer(1)],
            false,
        ),
        CloneType::Type1
    );
    assert_eq!(
        classified_pair(
            vec![identifier("x"), integer(1)],
            vec![identifier("y"), integer(2)],
            false,
        ),
        CloneType::Type2
    );
    assert_eq!(
        classified_pair(
            vec![
                identifier("a"),
                identifier("b"),
                identifier("c"),
                identifier("d"),
            ],
            vec![
                identifier("a"),
                identifier("b"),
                identifier("c"),
                identifier("d"),
                CpgNodeKind::Return,
            ],
            false,
        ),
        CloneType::Type3
    );
    assert_eq!(
        classified_pair(
            vec![identifier("x"), CpgNodeKind::Return],
            vec![CpgNodeKind::Return, identifier("x")],
            true,
        ),
        CloneType::Type4
    );
}

#[test]
fn public_api_reports_candidate_budget_exhaustion() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    add_function(&mut cpg, vec![identifier("x")]);
    add_function(&mut cpg, vec![CpgNodeKind::Return]);

    let error = detect_clones(
        &cpg,
        &CloneOptions {
            type3_threshold: 0.0,
            max_pairs: 0,
            ..CloneOptions::default()
        },
    )
    .unwrap_err();
    assert!(matches!(
        error,
        AnalysisError::BudgetExceeded {
            unit: "candidate pairs",
            needed: 1,
            cap: 0,
            ..
        }
    ));
}
