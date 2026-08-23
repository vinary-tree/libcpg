#![cfg(feature = "lang-rust")]

use libcpg::{export_datalog, CpgBuilder, Language, TreeSitterCpgBuilder};

#[test]
fn parser_built_rust_exports_deterministically_without_graph_mutation() {
    let source = r#"
        fn allocate(flag: bool) -> i32 {
            let value = if flag { 1 } else { 2 };
            value
        }
    "#;
    let builder = TreeSitterCpgBuilder::new();
    let cpg = builder
        .build(source, Language::Rust)
        .expect("build Rust fixture");
    let before_nodes = cpg.node_count();
    let before_edges = cpg.edge_count();

    let first = export_datalog(&cpg);
    let second = export_datalog(&cpg);

    assert_eq!(first, second);
    assert_eq!(first.cpg_edges().len(), cpg.edge_count());
    assert!(!first.ast_child.is_empty());
    assert!(!first.cfg.is_empty());
    assert_eq!(cpg.node_count(), before_nodes);
    assert_eq!(cpg.edge_count(), before_edges);
}
