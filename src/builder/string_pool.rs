//! Build-local canonical storage for mapped CPG strings.

use std::sync::Arc;

use rustc_hash::FxHashSet;

use crate::{CpgNode, CpgNodeKind, LiteralKind, MethodSignature, TypeInfo};

/// Canonicalizes string ownership during one CPG construction.
///
/// The pool never escapes the builder and owns no graph state after the build.
/// Every graph field remains an `Arc<str>` with its original contents; repeated
/// contents merely share one allocation. The visitor is a fixed-depth match plus
/// bounded loops over flat parameter and generic vectors, so source nesting never
/// consumes native call-stack space.
#[derive(Default)]
pub(super) struct CpgStringPool {
    strings: FxHashSet<Arc<str>>,
}

impl CpgStringPool {
    /// Canonicalizes every string field populated by the syntax-tree builder.
    pub(super) fn intern_node(&mut self, node: &mut CpgNode) {
        self.intern_kind(&mut node.kind);
        if let Some(text) = &mut node.text {
            self.intern(text);
        }
        if let Some(type_info) = &mut node.allocation_type {
            self.intern_type(type_info);
        }
    }

    fn intern(&mut self, value: &mut Arc<str>) {
        if let Some(canonical) = self.strings.get(value.as_ref()) {
            if !Arc::ptr_eq(value, canonical) {
                *value = Arc::clone(canonical);
            }
        } else {
            self.strings.insert(Arc::clone(value));
        }
    }

    fn intern_type(&mut self, type_info: &mut TypeInfo) {
        self.intern(&mut type_info.name);
        for generic in &mut type_info.generics {
            self.intern(generic);
        }
    }

    fn intern_signature(&mut self, signature: &mut MethodSignature) {
        self.intern(&mut signature.name);
        for parameter in &mut signature.params {
            self.intern_type(parameter);
        }
        if let Some(return_type) = &mut signature.return_type {
            self.intern_type(return_type);
        }
    }

    fn intern_kind(&mut self, kind: &mut CpgNodeKind) {
        match kind {
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
            | CpgNodeKind::Unknown { kind: name } => self.intern(name),
            CpgNodeKind::Impl {
                for_type,
                trait_name,
            } => {
                if let Some(for_type) = for_type {
                    self.intern(for_type);
                }
                if let Some(trait_name) = trait_name {
                    self.intern(trait_name);
                }
            }
            CpgNodeKind::Function { signature } => self.intern_signature(signature),
            CpgNodeKind::Parameter {
                name, param_type, ..
            } => {
                self.intern(name);
                if let Some(param_type) = param_type {
                    self.intern_type(param_type);
                }
            }
            CpgNodeKind::Variable { name, var_type, .. } => {
                self.intern(name);
                if let Some(var_type) = var_type {
                    self.intern_type(var_type);
                }
            }
            CpgNodeKind::Field {
                name, field_type, ..
            } => {
                self.intern(name);
                if let Some(field_type) = field_type {
                    self.intern_type(field_type);
                }
            }
            CpgNodeKind::BinaryOp { operator }
            | CpgNodeKind::UnaryOp { operator }
            | CpgNodeKind::Assignment { operator } => self.intern(operator),
            CpgNodeKind::Literal {
                kind: LiteralKind::String(value) | LiteralKind::Regex(value),
            } => self.intern(value),
            CpgNodeKind::Literal { .. } => {}
            CpgNodeKind::TypeAnnotation { type_info } => self.intern_type(type_info),
            CpgNodeKind::Root
            | CpgNodeKind::Block { .. }
            | CpgNodeKind::Return
            | CpgNodeKind::If
            | CpgNodeKind::Else
            | CpgNodeKind::While
            | CpgNodeKind::For
            | CpgNodeKind::Loop
            | CpgNodeKind::Match
            | CpgNodeKind::MatchArm
            | CpgNodeKind::Break
            | CpgNodeKind::Continue
            | CpgNodeKind::Throw
            | CpgNodeKind::Try
            | CpgNodeKind::Catch
            | CpgNodeKind::Finally
            | CpgNodeKind::Call { .. }
            | CpgNodeKind::IndexAccess
            | CpgNodeKind::Lambda { .. }
            | CpgNodeKind::Await
            | CpgNodeKind::Yield
            | CpgNodeKind::Comment { .. } => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NodeId, ScopeId, SourceRange};
    use proptest::prelude::*;

    fn unknown(value: &str) -> CpgNode {
        CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Unknown {
                kind: Arc::from(value),
            },
            SourceRange::default(),
        )
    }

    fn unknown_value(node: &CpgNode) -> &Arc<str> {
        let CpgNodeKind::Unknown { kind } = &node.kind else {
            unreachable!("test fixture always constructs Unknown nodes")
        };
        kind
    }

    #[test]
    fn nested_type_text_and_allocation_fields_share_one_unicode_allocation() {
        let repeated = "λ::資料::🦀";
        let mut node = CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Variable {
                name: Arc::from(repeated),
                var_type: Some(Box::new(
                    TypeInfo::new(repeated).with_generic(Arc::<str>::from(repeated)),
                )),
                scope: ScopeId::GLOBAL,
                is_mutable: false,
            },
            SourceRange::default(),
        )
        .with_text(Arc::<str>::from(repeated))
        .with_allocation_type(TypeInfo::new(repeated));

        CpgStringPool::default().intern_node(&mut node);

        let CpgNodeKind::Variable {
            name,
            var_type: Some(var_type),
            ..
        } = &node.kind
        else {
            panic!("variable fixture changed shape")
        };
        for value in [
            &var_type.name,
            &var_type.generics[0],
            node.text.as_ref().expect("text"),
            &node.allocation_type.as_ref().expect("allocation type").name,
        ] {
            assert_eq!(value.as_ref(), repeated);
            assert!(Arc::ptr_eq(name, value));
        }
    }

    #[test]
    fn large_repeated_strings_are_canonicalized_without_truncation() {
        let large = "資料🦀".repeat(32 * 1024);
        let mut nodes: Vec<_> = (0..32).map(|_| unknown(&large)).collect();
        let mut pool = CpgStringPool::default();
        for node in &mut nodes {
            pool.intern_node(node);
        }

        let canonical = unknown_value(&nodes[0]);
        assert_eq!(canonical.as_ref(), large);
        for node in &nodes[1..] {
            assert!(Arc::ptr_eq(canonical, unknown_value(node)));
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn equal_contents_have_equal_ownership_and_distinct_contents_do_not_change(
            values in prop::collection::vec(any::<String>(), 0..256),
        ) {
            let expected = values.clone();
            let mut nodes: Vec<_> = values.iter().map(|value| unknown(value)).collect();
            let mut pool = CpgStringPool::default();
            for node in &mut nodes {
                pool.intern_node(node);
            }

            let mut canonical = std::collections::BTreeMap::<String, Arc<str>>::new();
            for (node, expected) in nodes.iter().zip(&expected) {
                let actual = unknown_value(node);
                prop_assert_eq!(actual.as_ref(), expected);
                if let Some(first) = canonical.get(expected) {
                    prop_assert!(Arc::ptr_eq(first, actual));
                } else {
                    canonical.insert(expected.clone(), Arc::clone(actual));
                }
            }
        }
    }
}
