use proptest::prelude::*;

use super::*;
use crate::{
    CpgEdgeKind, CpgNode, DfgEdgeKind, Language, MethodSignature, ScopeId, SourceRange, Visibility,
};

fn add(cpg: &mut CodePropertyGraph, kind: CpgNodeKind) -> NodeId {
    let offset = u32::try_from(cpg.node_count()).unwrap_or(u32::MAX / 2);
    cpg.add_node(CpgNode::new(
        NodeId::new(0),
        kind,
        SourceRange::from_bytes(offset.saturating_mul(3), offset.saturating_mul(3) + 1),
    ))
}

fn ast(cpg: &mut CodePropertyGraph, parent: NodeId, child: NodeId) {
    cpg.connect(parent, child, CpgEdgeKind::AstChild)
        .expect("secret fixture AST endpoints exist");
    cpg.node_mut(parent)
        .expect("secret fixture parent exists")
        .children
        .push(child);
    cpg.node_mut(child)
        .expect("secret fixture child exists")
        .parent = Some(parent);
}

fn function(cpg: &mut CodePropertyGraph, name: &str) -> NodeId {
    add(
        cpg,
        CpgNodeKind::Function {
            signature: MethodSignature {
                name: name.into(),
                params: Default::default(),
                return_type: None,
                is_static: false,
                is_async: false,
                visibility: Visibility::Private,
            },
        },
    )
}

fn call(cpg: &mut CodePropertyGraph, owner: NodeId, path: &str) -> NodeId {
    let call = add(
        cpg,
        CpgNodeKind::Call {
            target: None,
            is_method: path.contains("::") || path.contains('.'),
        },
    );
    ast(cpg, owner, call);
    let callee = add(
        cpg,
        CpgNodeKind::Identifier {
            name: path.into(),
            definition: None,
        },
    );
    ast(cpg, call, callee);
    call
}

fn literal_argument(cpg: &mut CodePropertyGraph, call: NodeId, value: &str) -> NodeId {
    let literal = add(
        cpg,
        CpgNodeKind::Literal {
            kind: LiteralKind::String(value.into()),
        },
    );
    ast(cpg, call, literal);
    literal
}

#[test]
fn high_entropy_literal_reaching_authentication_is_reported_without_its_value() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let owner = function(&mut cpg, "entry");
    let sink = call(&mut cpg, owner, "client::authenticate");
    let source = literal_argument(&mut cpg, sink, "A7z!qP2#mN9$vR4@xK8&");
    let shape = (cpg.node_count(), cpg.edge_count());

    let report = analyze_builtin_secrets(
        &cpg,
        &CallGraphResolution::default(),
        &SecretAnalysisConfig::default(),
    );

    assert!(report.is_complete(), "{report:#?}");
    assert_eq!(report.findings.len(), 1, "{report:#?}");
    let finding = &report.findings[0];
    assert_eq!((finding.source, finding.sink), (source, sink));
    assert_eq!(finding.family, SecretSinkFamily::Authentication);
    assert!(matches!(
        finding.literal_evidence,
        SecretLiteralEvidence::HighEntropy { byte_len: 20, .. }
    ));
    assert_eq!((cpg.node_count(), cpg.edge_count()), shape);
}

#[test]
fn typed_binding_hint_qualifies_a_low_entropy_network_flow() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let owner = function(&mut cpg, "entry");
    let binding = add(
        &mut cpg,
        CpgNodeKind::Variable {
            name: "access_token".into(),
            var_type: None,
            scope: ScopeId::new(1),
            is_mutable: false,
        },
    );
    ast(&mut cpg, owner, binding);
    let source = add(
        &mut cpg,
        CpgNodeKind::Literal {
            kind: LiteralKind::String("aaaa".into()),
        },
    );
    ast(&mut cpg, binding, source);
    let sink = call(&mut cpg, owner, "transport::send");
    let argument = add(
        &mut cpg,
        CpgNodeKind::Identifier {
            name: "value".into(),
            definition: Some(binding),
        },
    );
    ast(&mut cpg, sink, argument);
    cpg.connect(source, argument, CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse))
        .expect("secret fixture DFG endpoints exist");

    let report = analyze_builtin_secrets(
        &cpg,
        &CallGraphResolution::default(),
        &SecretAnalysisConfig::default(),
    );

    assert!(report.is_complete(), "{report:#?}");
    let finding = report.findings.first().expect("hinted secret reaches send");
    assert_eq!(finding.family, SecretSinkFamily::Network);
    assert!(matches!(
        &finding.literal_evidence,
        SecretLiteralEvidence::NameHint { context, .. } if *context == binding
    ));
}

#[test]
fn ordinary_low_entropy_literal_and_ungoverned_argument_are_clean() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let owner = function(&mut cpg, "entry");
    let network = call(&mut cpg, owner, "transport::send");
    literal_argument(&mut cpg, network, "hello");
    let crypto = call(&mut cpg, owner, "Cipher::with_key");
    literal_argument(&mut cpg, crypto, "public-label");
    literal_argument(&mut cpg, crypto, "A7z!qP2#mN9$vR4@xK8&");

    let report = analyze_builtin_secrets(
        &cpg,
        &CallGraphResolution::default(),
        &SecretAnalysisConfig::default(),
    );

    assert!(report.is_complete(), "{report:#?}");
    assert!(report.findings.is_empty(), "{report:#?}");
}

#[test]
fn invalid_policy_and_local_caps_are_fail_closed_and_explicit() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let owner = function(&mut cpg, "entry");
    let sink = call(&mut cpg, owner, "client::authenticate");
    literal_argument(&mut cpg, sink, "A7z!qP2#mN9$vR4@xK8&");
    literal_argument(&mut cpg, sink, "B8y@rQ3$nO0%wS5!zL9*");
    let mut rules = builtin_secret_rules();
    rules
        .sinks
        .push(SecretSinkRule::new("", SecretSinkFamily::Authentication));
    rules.name_hints.push(Pattern::Substring("".into()));
    let config = SecretAnalysisConfig {
        minimum_entropy_bytes: 0,
        minimum_entropy_millibits_per_byte: 8_001,
        max_literals: 1,
        ..SecretAnalysisConfig::default()
    };

    let report = analyze_secret_flows(&cpg, &CallGraphResolution::default(), &rules, &config);

    assert!(!report.is_complete());
    assert!(report.issues.contains(&SecretAnalysisIssue::EmptyRuleName));
    assert!(report
        .issues
        .contains(&SecretAnalysisIssue::ZeroEntropyLength));
    assert!(report
        .issues
        .contains(&SecretAnalysisIssue::EntropyThresholdOutOfRange { threshold: 8_001 }));
    assert!(report
        .issues
        .contains(&SecretAnalysisIssue::LiteralsCapped {
            observed: 2,
            cap: 1,
        }));
}

#[test]
fn binding_context_walk_is_stack_safe_at_one_hundred_thousand_nodes() {
    let handle = std::thread::Builder::new()
        .stack_size(64 * 1024)
        .spawn(|| {
            let mut cpg = CodePropertyGraph::new(Language::Unknown);
            let binding = add(
                &mut cpg,
                CpgNodeKind::Variable {
                    name: "private_key".into(),
                    var_type: None,
                    scope: ScopeId::new(1),
                    is_mutable: false,
                },
            );
            let mut parent = binding;
            for _ in 0..100_000 {
                let wrapper = add(
                    &mut cpg,
                    CpgNodeKind::Unknown {
                        kind: "wrapper".into(),
                    },
                );
                ast(&mut cpg, parent, wrapper);
                parent = wrapper;
            }
            let literal = add(
                &mut cpg,
                CpgNodeKind::Literal {
                    kind: LiteralKind::String("x".into()),
                },
            );
            ast(&mut cpg, parent, literal);
            name_hint_evidence(&cpg, literal, &builtin_secret_rules().name_hints, 100_002)
                .matched
                .is_some()
        })
        .expect("small-stack worker starts");
    assert!(handle.join().expect("small-stack worker completes"));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn entropy_is_bounded_and_deterministic(value in "[A-Za-z0-9]{0,96}") {
        let first = shannon_entropy_millibits(value.as_bytes());
        let second = shannon_entropy_millibits(value.as_bytes());
        prop_assert_eq!(first, second);
        prop_assert!(first <= 8_000);
    }

    #[test]
    fn analysis_is_deterministic_for_generated_literals(value in "[A-Za-z0-9]{1,48}") {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let owner = function(&mut cpg, "entry");
        let sink = call(&mut cpg, owner, "client::authenticate");
        literal_argument(&mut cpg, sink, &value);
        let config = SecretAnalysisConfig {
            minimum_entropy_bytes: 1,
            minimum_entropy_millibits_per_byte: 0,
            ..SecretAnalysisConfig::default()
        };

        let first = analyze_builtin_secrets(
            &cpg,
            &CallGraphResolution::default(),
            &config,
        );
        let second = analyze_builtin_secrets(
            &cpg,
            &CallGraphResolution::default(),
            &config,
        );
        prop_assert_eq!(first, second);
    }
}

#[cfg(feature = "serde")]
#[test]
fn policy_and_report_round_trip_through_json() {
    let rules = builtin_secret_rules();
    let encoded = serde_json::to_string(&rules).expect("policy serializes");
    let decoded: SecretRuleSet = serde_json::from_str(&encoded).expect("policy deserializes");
    assert_eq!(decoded, rules);

    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let owner = function(&mut cpg, "entry");
    let sink = call(&mut cpg, owner, "client::authenticate");
    literal_argument(&mut cpg, sink, "A7z!qP2#mN9$vR4@xK8&");
    let report = analyze_builtin_secrets(
        &cpg,
        &CallGraphResolution::default(),
        &SecretAnalysisConfig::default(),
    );
    let encoded = serde_json::to_string(&report).expect("report serializes");
    let decoded: SecretAnalysis = serde_json::from_str(&encoded).expect("report deserializes");
    assert_eq!(decoded, report);
}
