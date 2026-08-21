use proptest::prelude::*;

use super::*;
use crate::{
    CfgEdgeKind, CpgNode, DfgEdgeKind, Language, MethodSignature, SourceRange, Visibility,
};

fn function_kind(name: &str) -> CpgNodeKind {
    CpgNodeKind::Function {
        signature: Box::new(MethodSignature {
            name: name.into(),
            params: Default::default(),
            return_type: None,
            is_static: false,
            is_async: false,
            visibility: Visibility::Private,
        }),
    }
}

fn add(cpg: &mut CodePropertyGraph, kind: CpgNodeKind) -> NodeId {
    let offset = u32::try_from(cpg.node_count()).unwrap_or(u32::MAX / 2);
    cpg.add_node(CpgNode::new(
        NodeId::new(0),
        kind,
        SourceRange::from_bytes(offset.saturating_mul(4), offset.saturating_mul(4) + 1),
    ))
}

fn ast(cpg: &mut CodePropertyGraph, parent: NodeId, child: NodeId) {
    cpg.connect(parent, child, CpgEdgeKind::AstChild)
        .expect("crypto fixture AST endpoints exist");
}

fn function(cpg: &mut CodePropertyGraph, name: &str) -> NodeId {
    add(cpg, function_kind(name))
}

fn direct_call(cpg: &mut CodePropertyGraph, parent: NodeId, path: &str) -> NodeId {
    let site = add(
        cpg,
        CpgNodeKind::Call {
            target: None,
            is_method: path.contains("::") || path.contains('.'),
        },
    );
    ast(cpg, parent, site);
    let callee = add(
        cpg,
        CpgNodeKind::Identifier {
            name: path.into(),
            definition: None,
        },
    );
    ast(cpg, site, callee);
    site
}

fn method_call(
    cpg: &mut CodePropertyGraph,
    parent: NodeId,
    receiver_definition: NodeId,
    member: &str,
) -> (NodeId, NodeId) {
    let site = add(
        cpg,
        CpgNodeKind::Call {
            target: None,
            is_method: true,
        },
    );
    ast(cpg, parent, site);
    let callee = add(
        cpg,
        CpgNodeKind::MemberAccess {
            member: member.into(),
        },
    );
    ast(cpg, site, callee);
    let receiver = add(
        cpg,
        CpgNodeKind::Identifier {
            name: "cipher".into(),
            definition: Some(receiver_definition),
        },
    );
    ast(cpg, callee, receiver);
    cpg.connect(
        receiver_definition,
        receiver,
        CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse),
    )
    .expect("crypto receiver DFG endpoints exist");
    (site, receiver)
}

fn string_argument(cpg: &mut CodePropertyGraph, call: NodeId, value: &str) -> NodeId {
    let literal = add(
        cpg,
        CpgNodeKind::Literal {
            kind: LiteralKind::String(value.into()),
        },
    );
    ast(cpg, call, literal);
    literal
}

fn defined_argument(
    cpg: &mut CodePropertyGraph,
    call: NodeId,
    name: &str,
    definition: NodeId,
) -> NodeId {
    let argument = add(
        cpg,
        CpgNodeKind::Identifier {
            name: name.into(),
            definition: Some(definition),
        },
    );
    ast(cpg, call, argument);
    cpg.connect(
        definition,
        argument,
        CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse),
    )
    .expect("crypto argument DFG endpoints exist");
    argument
}

fn sequence(cpg: &mut CodePropertyGraph, points: &[NodeId]) {
    for pair in points.windows(2) {
        cpg.connect(
            pair[0],
            pair[1],
            CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
        )
        .expect("crypto fixture CFG endpoints exist");
    }
}

fn vulnerable_fixture() -> (CodePropertyGraph, NodeId, NodeId, NodeId, NodeId) {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let owner = function(&mut cpg, "vulnerable");
    let ecb = direct_call(&mut cpg, owner, "Cipher::getInstance");
    string_argument(&mut cpg, ecb, "AES/ECB/PKCS5Padding");
    let md5 = direct_call(&mut cpg, owner, "MD5::digest");
    let key = direct_call(&mut cpg, owner, "Cipher::with_key");
    string_argument(&mut cpg, key, "redacted-example-key");
    let iv = direct_call(&mut cpg, owner, "cipher::set_iv");
    string_argument(&mut cpg, iv, "fixed-example-iv");
    sequence(&mut cpg, &[owner, ecb, md5, key, iv]);
    (cpg, ecb, md5, key, iv)
}

#[test]
fn builtin_rules_report_ecb_md5_hardcoded_key_and_predictable_iv() {
    let (cpg, ecb, md5, key, iv) = vulnerable_fixture();
    let shape = (cpg.node_count(), cpg.edge_count());
    let analysis = analyze_builtin_crypto(
        &cpg,
        &CallGraphResolution::default(),
        &CryptoAnalysisConfig::default(),
    );

    assert!(analysis.is_complete(), "{analysis:#?}");
    assert!(analysis
        .findings
        .iter()
        .any(|finding| finding.site == ecb && finding.kind == CryptoMisuseKind::EcbMode));
    assert!(analysis
        .findings
        .iter()
        .any(|finding| finding.site == md5 && finding.kind == CryptoMisuseKind::WeakHash));
    let hardcoded = analysis
        .findings
        .iter()
        .find(|finding| finding.site == key && finding.kind == CryptoMisuseKind::HardcodedKey)
        .expect("literal key reaches the crypto sink");
    assert!(matches!(hardcoded.evidence, CryptoEvidence::Taint { .. }));
    assert!(analysis
        .findings
        .iter()
        .any(|finding| { finding.site == iv && finding.kind == CryptoMisuseKind::PredictableIv }));
    assert_eq!((cpg.node_count(), cpg.edge_count()), shape);
}

#[test]
fn aes_gcm_with_random_nonce_is_clean() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let owner = function(&mut cpg, "safe");
    let cipher = direct_call(&mut cpg, owner, "AesGcm::new");
    let nonce = direct_call(&mut cpg, owner, "OsRng::generate_nonce");
    let (encrypt, _) = method_call(&mut cpg, owner, cipher, "encrypt");
    defined_argument(&mut cpg, encrypt, "nonce", nonce);
    let plaintext = direct_call(&mut cpg, owner, "read_plaintext");
    defined_argument(&mut cpg, encrypt, "plaintext", plaintext);
    sequence(&mut cpg, &[owner, cipher, nonce, plaintext, encrypt]);

    let analysis = analyze_builtin_crypto(
        &cpg,
        &CallGraphResolution::default(),
        &CryptoAnalysisConfig::default(),
    );
    assert!(analysis.is_complete(), "{analysis:#?}");
    assert!(analysis.findings.is_empty(), "{analysis:#?}");
}

#[test]
fn finalized_cipher_use_is_a_typestate_finding() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let owner = function(&mut cpg, "bad_order");
    let cipher = direct_call(&mut cpg, owner, "AesGcm::new");
    let (finalize, _) = method_call(&mut cpg, owner, cipher, "finalize");
    let (update, _) = method_call(&mut cpg, owner, cipher, "update");
    sequence(&mut cpg, &[owner, cipher, finalize, update]);

    let analysis = analyze_builtin_crypto(
        &cpg,
        &CallGraphResolution::default(),
        &CryptoAnalysisConfig::default(),
    );
    assert!(analysis.is_complete(), "{analysis:#?}");
    assert!(analysis.findings.iter().any(|finding| {
        finding.site == update && finding.kind == CryptoMisuseKind::InvalidCallOrder
    }));
}

#[test]
fn invalid_rules_and_local_caps_are_explicit() {
    let (cpg, ..) = vulnerable_fixture();
    let mut rules = builtin_crypto_rules();
    rules
        .key_parameters
        .push(CryptoArgumentRule::new("invalid"));
    let config = CryptoAnalysisConfig {
        max_calls: 0,
        max_origin_nodes_per_argument: 0,
        ..CryptoAnalysisConfig::default()
    };
    let analysis = analyze_crypto_misuse(&cpg, &CallGraphResolution::default(), &rules, &config);
    assert!(!analysis.is_complete());
    assert!(analysis.capped);
    assert!(analysis.issues.iter().any(|issue| matches!(
        issue,
        CryptoAnalysisIssue::MissingCallPattern { rule } if rule.as_ref() == "invalid"
    )));
    assert!(analysis
        .issues
        .iter()
        .any(|issue| matches!(issue, CryptoAnalysisIssue::CallsCapped { cap: 0, .. })));
}

#[test]
fn argument_origin_cap_never_becomes_a_clean_negative_result() {
    let (cpg, ..) = vulnerable_fixture();
    let config = CryptoAnalysisConfig {
        max_origin_nodes_per_argument: 0,
        ..CryptoAnalysisConfig::default()
    };
    let analysis = analyze_builtin_crypto(&cpg, &CallGraphResolution::default(), &config);
    assert!(!analysis.is_complete());
    assert!(analysis.capped);
    assert!(analysis
        .issues
        .iter()
        .any(|issue| matches!(issue, CryptoAnalysisIssue::OriginNodesCapped { cap: 0, .. })));
}

#[test]
fn origin_machine_handles_one_hundred_thousand_definitions_on_a_small_stack() {
    std::thread::Builder::new()
        .name("crypto-origin-small-stack".into())
        .stack_size(64 * 1024)
        .spawn(|| {
            let mut cpg = CodePropertyGraph::new(Language::Rust);
            let literal = add(
                &mut cpg,
                CpgNodeKind::Literal {
                    kind: LiteralKind::String("fixed-iv".into()),
                },
            );
            let mut previous = literal;
            for index in 0..100_000u32 {
                previous = add(
                    &mut cpg,
                    CpgNodeKind::Identifier {
                        name: format!("v{index}").into(),
                        definition: Some(previous),
                    },
                );
            }
            let evidence =
                trace_origins(&cpg, &CallGraphResolution::default(), previous, 100_001, 64);
            assert!(!evidence.capped);
            assert_eq!(evidence.literals, BTreeSet::from([literal]));
        })
        .expect("small-stack crypto-origin thread spawns")
        .join()
        .expect("small-stack crypto-origin thread succeeds");
}

#[cfg(feature = "serde")]
#[test]
fn rules_and_analysis_round_trip_canonically() {
    let (cpg, ..) = vulnerable_fixture();
    let rules = builtin_crypto_rules();
    let encoded = serde_json::to_string(&rules).expect("crypto rules serialize");
    let decoded: CryptoRuleSet = serde_json::from_str(&encoded).expect("crypto rules deserialize");
    assert_eq!(decoded, rules);

    let analysis = analyze_builtin_crypto(
        &cpg,
        &CallGraphResolution::default(),
        &CryptoAnalysisConfig::default(),
    );
    let encoded = serde_json::to_string(&analysis).expect("crypto analysis serializes");
    let decoded: CryptoAnalysis =
        serde_json::from_str(&encoded).expect("crypto analysis deserializes");
    assert_eq!(decoded, analysis);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn analysis_is_deterministic_for_bounded_cap_variation(
        max_calls in 1usize..64,
        max_origins in 1usize..256,
    ) {
        let (cpg, ..) = vulnerable_fixture();
        let config = CryptoAnalysisConfig {
            max_calls,
            max_origin_nodes_per_argument: max_origins,
            ..CryptoAnalysisConfig::default()
        };
        let first = analyze_builtin_crypto(
            &cpg,
            &CallGraphResolution::default(),
            &config,
        );
        let second = analyze_builtin_crypto(
            &cpg,
            &CallGraphResolution::default(),
            &config,
        );
        prop_assert_eq!(first, second);
    }
}
