//! Public-API integration coverage for stack-safe tree-sitter CPG and CFG construction.

#![cfg(feature = "lang-rust")]

use std::sync::Arc;

use libcpg::{
    CodePropertyGraph, CpgBuilder, CpgBuilderConfig, CpgEdgeKind, CpgNodeKind, Language,
    TreeSitterCpgBuilder, TypeInfo,
};

fn collect_type_strings<'a>(type_info: &'a TypeInfo, output: &mut Vec<&'a Arc<str>>) {
    output.push(&type_info.name);
    output.extend(&type_info.generics);
}

fn strings_equal_to<'a>(cpg: &'a CodePropertyGraph, expected: &str) -> Vec<&'a Arc<str>> {
    let mut output = Vec::new();
    for node in cpg.nodes() {
        match &node.kind {
            CpgNodeKind::Module { name }
            | CpgNodeKind::Class { name, .. }
            | CpgNodeKind::Struct { name }
            | CpgNodeKind::Enum { name }
            | CpgNodeKind::Trait { name }
            | CpgNodeKind::MemberAccess { member: name }
            | CpgNodeKind::Identifier { name, .. }
            | CpgNodeKind::GenericParam { name }
            | CpgNodeKind::Import { path: name }
            | CpgNodeKind::Attribute { name }
            | CpgNodeKind::Macro { name }
            | CpgNodeKind::Error { message: name }
            | CpgNodeKind::Unknown { kind: name } => output.push(name),
            CpgNodeKind::Impl {
                for_type,
                trait_name,
            } => output.extend(for_type.iter().chain(trait_name.iter())),
            CpgNodeKind::Function { signature } => {
                output.push(&signature.name);
                for parameter in &signature.params {
                    collect_type_strings(parameter, &mut output);
                }
                if let Some(return_type) = &signature.return_type {
                    collect_type_strings(return_type, &mut output);
                }
            }
            CpgNodeKind::Parameter {
                name, param_type, ..
            } => {
                output.push(name);
                if let Some(param_type) = param_type {
                    collect_type_strings(param_type, &mut output);
                }
            }
            CpgNodeKind::Variable { name, var_type, .. } => {
                output.push(name);
                if let Some(var_type) = var_type {
                    collect_type_strings(var_type, &mut output);
                }
            }
            CpgNodeKind::Field {
                name, field_type, ..
            } => {
                output.push(name);
                if let Some(field_type) = field_type {
                    collect_type_strings(field_type, &mut output);
                }
            }
            CpgNodeKind::BinaryOp { operator }
            | CpgNodeKind::UnaryOp { operator }
            | CpgNodeKind::Assignment { operator } => output.push(operator),
            CpgNodeKind::Literal {
                kind: libcpg::LiteralKind::String(value) | libcpg::LiteralKind::Regex(value),
            } => output.push(value),
            CpgNodeKind::TypeAnnotation { type_info } => {
                collect_type_strings(type_info, &mut output);
            }
            _ => {}
        }
        if let Some(text) = &node.text {
            output.push(text);
        }
        if let Some(type_info) = &node.allocation_type {
            collect_type_strings(type_info, &mut output);
        }
    }
    output.retain(|value| value.as_ref() == expected);
    output
}

#[test]
fn public_mode_a_builder_and_cfg_handle_depth_100_000_on_a_small_native_stack() {
    const DEPTH: usize = 100_000;

    let handle = std::thread::Builder::new()
        .name("public-cpg-builder-small-stack".into())
        .stack_size(256 * 1024)
        .spawn(|| {
            let mut source = String::with_capacity(32 + DEPTH * 2);
            source.push_str("fn deep(){let _x=");
            source.push_str(&"(".repeat(DEPTH));
            source.push('0');
            source.push_str(&")".repeat(DEPTH));
            source.push_str(";}");

            let builder = TreeSitterCpgBuilder::with_config(
                CpgBuilderConfig::new().with_cfg(true).with_dfg(false),
            );
            let cpg = builder
                .build(&source, Language::Rust)
                .expect("public Mode-A builder handles deep syntax");
            assert!(cpg.node_count() >= DEPTH);
            assert_eq!(
                cpg.cfg_entries().len(),
                1,
                "the parsed function is a CFG entry"
            );
            assert!(
                !cpg.cfg_exits().is_empty(),
                "the deep expression has a CFG exit"
            );
            assert!(
                cpg.stats().cfg_edges >= DEPTH,
                "CFG extraction traverses the complete parser-built nesting"
            );

            let wrappers = strings_equal_to(&cpg, "parenthesized_expression");
            assert!(
                wrappers.len() >= DEPTH,
                "anti-vacuity: deep wrapper strings are represented"
            );
            let canonical = wrappers[0];
            assert!(
                wrappers
                    .iter()
                    .all(|wrapper| Arc::ptr_eq(canonical, wrapper)),
                "depth-100k wrapper labels share one heap allocation"
            );

            for edge in cpg
                .edges()
                .filter(|edge| matches!(edge.kind, CpgEdgeKind::AstChild))
            {
                assert_eq!(
                    cpg.node(edge.target).expect("AST child exists").parent,
                    Some(edge.source)
                );
            }
        })
        .expect("spawn public builder small-stack test");

    handle
        .join()
        .expect("public builder small-stack test completes");
}

#[test]
fn public_builder_canonicalizes_repeated_unicode_names_without_changing_clone_contents() {
    let source = r#"
        struct 資料;
        fn repeated(資料: 資料) -> 資料 {
            let 資料: 資料 = 資料;
            資料
        }
    "#;
    let cpg = TreeSitterCpgBuilder::new()
        .build(source, Language::Rust)
        .expect("build repeated Unicode identifiers");
    let values = strings_equal_to(&cpg, "資料");
    assert!(
        values.len() >= 6,
        "fixture contains repeated mapped strings"
    );
    assert!(
        values.iter().all(|value| Arc::ptr_eq(values[0], value)),
        "one build owns one allocation for equal mapped strings"
    );

    let cloned = cpg.clone();
    let cloned_values = strings_equal_to(&cloned, "資料");
    assert_eq!(values.len(), cloned_values.len());
    for (original, cloned) in values.into_iter().zip(cloned_values) {
        assert_eq!(original, cloned);
        assert!(Arc::ptr_eq(original, cloned), "clone retains Arc ownership");
    }
}

#[cfg(feature = "serde")]
#[test]
fn public_builder_canonicalization_preserves_serde_bytes() {
    let source = "fn repeated(value: Value) -> Value { let value: Value = value; value }";
    let builder = TreeSitterCpgBuilder::new();
    let cpg = builder
        .build(source, Language::Rust)
        .expect("build serde fixture");
    let encoded = serde_json::to_vec(&cpg).expect("serialize canonical graph");
    let rebuilt = builder
        .build(source, Language::Rust)
        .expect("rebuild serde fixture");
    let rebuilt_bytes = serde_json::to_vec(&rebuilt).expect("serialize rebuilt graph");
    let cloned_bytes = serde_json::to_vec(&cpg.clone()).expect("serialize cloned graph");
    let decoded: CodePropertyGraph =
        serde_json::from_slice(&encoded).expect("deserialize canonical graph");

    assert_eq!(
        encoded, rebuilt_bytes,
        "independent builds have stable bytes"
    );
    assert_eq!(
        encoded, cloned_bytes,
        "allocation identity is not serialized"
    );
    assert_eq!(
        strings_equal_to(&cpg, "Value").len(),
        strings_equal_to(&decoded, "Value").len()
    );
    let mut original_ids = cpg.node_ids().collect::<Vec<_>>();
    let mut decoded_ids = decoded.node_ids().collect::<Vec<_>>();
    original_ids.sort_unstable();
    decoded_ids.sort_unstable();
    assert_eq!(original_ids, decoded_ids);
    for id in cpg.node_ids() {
        let original = cpg.node(id).expect("original node");
        let round_trip = decoded.node(id).expect("decoded node");
        assert_eq!(original.kind, round_trip.kind);
        assert_eq!(original.range, round_trip.range);
        assert_eq!(original.text, round_trip.text);
        assert_eq!(original.children, round_trip.children);
        assert_eq!(original.parent, round_trip.parent);
    }
    assert_eq!(
        cpg.edges().cloned().collect::<Vec<_>>(),
        decoded.edges().cloned().collect::<Vec<_>>()
    );
}

#[test]
fn public_builder_is_deterministic_across_comments_and_skipped_tokens() {
    let source = "// leading\nfn f() { /* nested */ let x = (((1))); x + 2; }";
    for include_comments in [false, true] {
        let builder = TreeSitterCpgBuilder::with_config(
            CpgBuilderConfig::new()
                .with_cfg(false)
                .with_dfg(false)
                .with_comments(include_comments),
        );
        let first = builder
            .build(source, Language::Rust)
            .expect("first deterministic build");
        let second = builder
            .build(source, Language::Rust)
            .expect("second deterministic build");

        assert_eq!(
            first.node_ids().collect::<Vec<_>>(),
            second.node_ids().collect::<Vec<_>>()
        );
        assert_eq!(
            first.edges().cloned().collect::<Vec<_>>(),
            second.edges().cloned().collect::<Vec<_>>()
        );
        for id in first.node_ids() {
            let left = first.node(id).expect("first node");
            let right = second.node(id).expect("second node");
            assert_eq!(left.kind, right.kind);
            assert_eq!(left.range, right.range);
            assert_eq!(left.parent, right.parent);
            assert_eq!(left.message_kind, right.message_kind);
        }
    }
}
