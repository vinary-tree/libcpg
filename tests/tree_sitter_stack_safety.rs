//! Public-API integration coverage for stack-safe tree-sitter CPG construction.

#![cfg(feature = "lang-rust")]

use libcpg::{CpgBuilder, CpgBuilderConfig, CpgEdgeKind, Language, TreeSitterCpgBuilder};

#[test]
fn public_mode_a_builder_handles_deep_source_on_a_small_native_stack() {
    const DEPTH: usize = 20_000;

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
                CpgBuilderConfig::new().with_cfg(false).with_dfg(false),
            );
            let cpg = builder
                .build(&source, Language::Rust)
                .expect("public Mode-A builder handles deep syntax");
            assert!(cpg.node_count() >= DEPTH);

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
