//! Deterministic dense fact universes shared by data-flow analyses.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use crate::{CodePropertyGraph, CpgNodeKind, DefinitionKind, LiteralKind, NodeId, UseKind};

use super::super::function_scope_nodes;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Sorted, deduplicated values with stable dense `u32` numbering.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct FactUniverse<T> {
    values: Vec<T>,
}

impl<T: Ord> FactUniverse<T> {
    /// Constructs a canonical universe by sorting and deduplicating `values`.
    ///
    /// # Panics
    ///
    /// Panics if $`2^{32}`$ or more distinct facts are supplied.
    pub fn new(values: impl IntoIterator<Item = T>) -> Self {
        let mut values: Vec<_> = values.into_iter().collect();
        values.sort_unstable();
        values.dedup();
        let _ = u32::try_from(values.len()).expect("fact universe exceeds the u32 id domain");
        Self { values }
    }

    /// Returns the number of facts.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Returns whether the universe is empty.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Returns the fact with dense identifier `id`.
    pub fn get(&self, id: u32) -> Option<&T> {
        self.values.get(id as usize)
    }

    /// Returns the dense identifier of `fact`.
    pub fn id_of(&self, fact: &T) -> Option<u32> {
        self.values
            .binary_search(fact)
            .ok()
            .map(|index| index as u32)
    }

    /// Iterates facts in dense-id order.
    pub fn iter(&self) -> impl Iterator<Item = (u32, &T)> {
        self.values
            .iter()
            .enumerate()
            .map(|(index, value)| (index as u32, value))
    }

    /// Returns the sorted fact slice.
    pub fn as_slice(&self) -> &[T] {
        &self.values
    }
}

impl<T: Ord> FromIterator<T> for FactUniverse<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        Self::new(iter)
    }
}

/// One variable definition attached to a CPG node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct VariableDefinition {
    /// Dense variable identifier in [`VarFacts::universe`].
    pub variable: u32,
    /// Syntactic definition classification.
    pub kind: DefinitionKind,
}

/// Canonical variable definitions and uses for one function scope.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct VarFacts {
    /// Sorted variable-name universe.
    pub universe: FactUniverse<Arc<str>>,
    /// Definitions keyed by the node that performs the write.
    pub definitions: BTreeMap<NodeId, Vec<VariableDefinition>>,
    /// Dense variables read at each identifier node.
    pub uses: BTreeMap<NodeId, Vec<u32>>,
}

impl VarFacts {
    /// Returns definitions performed by `node`.
    pub fn definitions_at(&self, node: NodeId) -> &[VariableDefinition] {
        self.definitions.get(&node).map_or(&[], Vec::as_slice)
    }

    /// Returns dense variables used by `node`.
    pub fn uses_at(&self, node: NodeId) -> &[u32] {
        self.uses.get(&node).map_or(&[], Vec::as_slice)
    }

    /// Returns the sorted variable name for a dense id.
    pub fn name(&self, variable: u32) -> Option<&Arc<str>> {
        self.universe.get(variable)
    }
}

/// Re-derives variable definitions and uses with the DFG sweep's binding rules.
///
/// Variable declarations and parameters define their declared names. A simple
/// assignment defines the name of its first identifier child; a compound
/// assignment also reads that child. Declaration/parameter binder identifiers
/// and a plain `=` target are not uses. Nested functions are excluded.
pub fn variable_facts(cpg: &CodePropertyGraph, function: NodeId) -> VarFacts {
    let scope = function_scope_nodes(cpg, function);
    let mut nodes: Vec<_> = scope.iter().copied().collect();
    nodes.sort_unstable();

    let mut raw_definitions = Vec::new();
    let mut raw_uses = Vec::new();
    let mut names = BTreeSet::new();
    let mut non_use_identifiers = BTreeSet::new();

    for &node in &nodes {
        match cpg.node(node).map(|node| &node.kind) {
            Some(CpgNodeKind::Variable { name, .. } | CpgNodeKind::Parameter { name, .. }) => {
                if let Some(binder) = binder_identifier(cpg, node, name) {
                    non_use_identifiers.insert(binder);
                }
            }
            Some(CpgNodeKind::Assignment { operator }) if &**operator == "=" => {
                if let Some(target) = cpg.ast_children(node).first().copied() {
                    if matches!(
                        cpg.node(target).map(|node| &node.kind),
                        Some(CpgNodeKind::Identifier { .. })
                    ) {
                        non_use_identifiers.insert(target);
                    }
                }
            }
            _ => {}
        }
    }

    for node in nodes {
        let Some(cpg_node) = cpg.node(node) else {
            continue;
        };
        match &cpg_node.kind {
            CpgNodeKind::Variable { name, .. } if !name.is_empty() => {
                names.insert(name.clone());
                raw_definitions.push((node, name.clone(), DefinitionKind::Declaration));
            }
            CpgNodeKind::Parameter { name, .. } if !name.is_empty() => {
                names.insert(name.clone());
                raw_definitions.push((node, name.clone(), DefinitionKind::Parameter));
            }
            CpgNodeKind::Assignment { .. } => {
                if let Some(name) = assignment_target_name(cpg, node) {
                    if !name.is_empty() {
                        names.insert(name.clone());
                        raw_definitions.push((node, name, DefinitionKind::Assignment));
                    }
                }
            }
            CpgNodeKind::Identifier { name, .. }
                if !name.is_empty() && !non_use_identifiers.contains(&node) =>
            {
                names.insert(name.clone());
                raw_uses.push((node, name.clone(), UseKind::Read));
            }
            _ => {}
        }
    }

    let universe = FactUniverse::new(names);
    let mut definitions: BTreeMap<NodeId, Vec<VariableDefinition>> = BTreeMap::new();
    for (node, name, kind) in raw_definitions {
        if let Some(variable) = universe.id_of(&name) {
            definitions
                .entry(node)
                .or_default()
                .push(VariableDefinition { variable, kind });
        }
    }
    for at_node in definitions.values_mut() {
        at_node.sort_unstable_by_key(|definition| definition.variable);
        at_node.dedup();
    }

    let mut uses: BTreeMap<NodeId, Vec<u32>> = BTreeMap::new();
    for (node, name, _) in raw_uses {
        if let Some(variable) = universe.id_of(&name) {
            uses.entry(node).or_default().push(variable);
        }
    }
    for at_node in uses.values_mut() {
        at_node.sort_unstable();
        at_node.dedup();
    }

    VarFacts {
        universe,
        definitions,
        uses,
    }
}

fn binder_identifier(cpg: &CodePropertyGraph, parent: NodeId, name: &str) -> Option<NodeId> {
    if name.is_empty() {
        return None;
    }
    cpg.ast_children(parent).into_iter().find(|&child| {
        matches!(
            cpg.node(child).map(|node| &node.kind),
            Some(CpgNodeKind::Identifier { name: child_name, .. }) if &**child_name == name
        )
    })
}

fn assignment_target_name(cpg: &CodePropertyGraph, assignment: NodeId) -> Option<Arc<str>> {
    let target = cpg.ast_children(assignment).first().copied()?;
    match cpg.node(target).map(|node| &node.kind) {
        Some(CpgNodeKind::Identifier { name, .. }) => Some(name.clone()),
        _ => None,
    }
}

/// One canonical expression operand.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ExprOperand {
    /// Variable read by name.
    Variable(Arc<str>),
    /// Signed integer literal.
    Integer(i64),
    /// IEEE-754 floating literal represented by exact bits.
    Float(u64),
    /// Boolean literal.
    Bool(bool),
    /// String literal.
    String(Arc<str>),
    /// Character literal.
    Char(char),
    /// Null/nil literal.
    Null,
    /// Array/list literal.
    Array,
    /// Object/map literal.
    Object,
    /// Regular-expression literal.
    Regex(Arc<str>),
    /// Nested unary or binary expression.
    Expression(Box<ExprKey>),
    /// Unsupported operand retained by identity to avoid an unsound match.
    Opaque(NodeId),
}

/// Structural key for one unary or binary expression.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ExprKey {
    /// Operator spelling.
    pub operator: Arc<str>,
    /// Ordered structural operands.
    pub operands: Vec<ExprOperand>,
}

/// Canonical expression occurrences and variable dependencies.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ExprFacts {
    /// Sorted structural expression universe.
    pub universe: FactUniverse<ExprKey>,
    /// Sorted variable universe referenced by expressions.
    pub variables: FactUniverse<Arc<str>>,
    /// Expression ids evaluated at each CPG node.
    pub generated: BTreeMap<NodeId, Vec<u32>>,
    /// Occurrence nodes for each expression id, in ascending order.
    pub occurrences: Vec<Vec<NodeId>>,
    operand_variables: Vec<Vec<u32>>,
}

impl ExprFacts {
    /// Returns expression ids evaluated at `node`.
    pub fn generated_at(&self, node: NodeId) -> &[u32] {
        self.generated.get(&node).map_or(&[], Vec::as_slice)
    }

    /// Returns dense variable ids read by expression `expression`.
    pub fn variables_of(&self, expression: u32) -> &[u32] {
        self.operand_variables
            .get(expression as usize)
            .map_or(&[], Vec::as_slice)
    }

    /// Returns whether defining `variable` kills `expression`.
    pub fn killed_by_var(&self, expression: u32, variable: u32) -> bool {
        self.variables_of(expression)
            .binary_search(&variable)
            .is_ok()
    }
}

/// Re-derives unary/binary expression facts from one function's AST scope.
///
/// Keys are structural across occurrences: operators, variable names, exact
/// literal values, and nested expression keys participate. Unsupported operand
/// nodes remain identity-qualified, conservatively preventing false CSE/PRE
/// equivalence.
pub fn expression_facts(cpg: &CodePropertyGraph, function: NodeId) -> ExprFacts {
    let scope = function_scope_nodes(cpg, function);
    let mut nodes: Vec<_> = scope.iter().copied().collect();
    nodes.sort_unstable();
    let mut raw = Vec::new();
    let mut all_variables = BTreeSet::new();

    for node in nodes {
        let Some(kind) = cpg.node(node).map(|node| &node.kind) else {
            continue;
        };
        if !matches!(
            kind,
            CpgNodeKind::BinaryOp { .. } | CpgNodeKind::UnaryOp { .. }
        ) {
            continue;
        }
        let mut on_path = BTreeSet::new();
        if let Some(key) = expression_key(cpg, node, &mut on_path) {
            let mut variables = BTreeSet::new();
            collect_operand_variables(&key, &mut variables);
            all_variables.extend(variables.iter().cloned());
            raw.push((node, key, variables));
        }
    }

    let universe = FactUniverse::new(raw.iter().map(|(_, key, _)| key.clone()));
    let variables = FactUniverse::new(all_variables);
    let mut generated: BTreeMap<NodeId, Vec<u32>> = BTreeMap::new();
    let mut occurrences = vec![Vec::new(); universe.len()];
    let mut operand_names: Vec<BTreeSet<Arc<str>>> = vec![BTreeSet::new(); universe.len()];
    for (node, key, names) in raw {
        let expression = universe
            .id_of(&key)
            .expect("raw expression belongs to its universe");
        generated.entry(node).or_default().push(expression);
        occurrences[expression as usize].push(node);
        operand_names[expression as usize].extend(names);
    }
    for expressions in generated.values_mut() {
        expressions.sort_unstable();
        expressions.dedup();
    }
    for nodes in &mut occurrences {
        nodes.sort_unstable();
        nodes.dedup();
    }
    let operand_variables = operand_names
        .into_iter()
        .map(|names| {
            names
                .into_iter()
                .filter_map(|name| variables.id_of(&name))
                .collect()
        })
        .collect();

    ExprFacts {
        universe,
        variables,
        generated,
        occurrences,
        operand_variables,
    }
}

fn expression_key(
    cpg: &CodePropertyGraph,
    node: NodeId,
    on_path: &mut BTreeSet<NodeId>,
) -> Option<ExprKey> {
    if !on_path.insert(node) {
        return None;
    }
    let operator = match cpg.node(node).map(|node| &node.kind) {
        Some(CpgNodeKind::BinaryOp { operator } | CpgNodeKind::UnaryOp { operator }) => {
            operator.clone()
        }
        _ => {
            on_path.remove(&node);
            return None;
        }
    };
    let operands = cpg
        .ast_children(node)
        .into_iter()
        .map(|child| expression_operand(cpg, child, on_path))
        .collect();
    on_path.remove(&node);
    Some(ExprKey { operator, operands })
}

fn expression_operand(
    cpg: &CodePropertyGraph,
    node: NodeId,
    on_path: &mut BTreeSet<NodeId>,
) -> ExprOperand {
    match cpg.node(node).map(|node| &node.kind) {
        Some(CpgNodeKind::Identifier { name, .. }) => ExprOperand::Variable(name.clone()),
        Some(CpgNodeKind::Literal { kind }) => match kind {
            LiteralKind::Integer(value) => ExprOperand::Integer(*value),
            LiteralKind::Float(value) => ExprOperand::Float(value.to_bits()),
            LiteralKind::String(value) => ExprOperand::String(value.clone()),
            LiteralKind::Char(value) => ExprOperand::Char(*value),
            LiteralKind::Bool(value) => ExprOperand::Bool(*value),
            LiteralKind::Null => ExprOperand::Null,
            LiteralKind::Array => ExprOperand::Array,
            LiteralKind::Object => ExprOperand::Object,
            LiteralKind::Regex(value) => ExprOperand::Regex(value.clone()),
        },
        Some(CpgNodeKind::BinaryOp { .. } | CpgNodeKind::UnaryOp { .. }) => {
            expression_key(cpg, node, on_path)
                .map(|key| ExprOperand::Expression(Box::new(key)))
                .unwrap_or(ExprOperand::Opaque(node))
        }
        _ => ExprOperand::Opaque(node),
    }
}

fn collect_operand_variables(key: &ExprKey, variables: &mut BTreeSet<Arc<str>>) {
    for operand in &key.operands {
        match operand {
            ExprOperand::Variable(name) => {
                variables.insert(name.clone());
            }
            ExprOperand::Expression(nested) => collect_operand_variables(nested, variables),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CpgEdgeKind, CpgNode, Language, MethodSignature, ScopeId, SourceRange, Visibility,
    };
    use proptest::prelude::*;

    fn node(id: u32, kind: CpgNodeKind) -> CpgNode {
        CpgNode::new(NodeId::new(id), kind, SourceRange::default())
    }

    fn function() -> CpgNodeKind {
        CpgNodeKind::Function {
            signature: MethodSignature {
                name: "facts".into(),
                params: Default::default(),
                return_type: None,
                is_static: false,
                is_async: false,
                visibility: Visibility::Private,
            },
        }
    }

    fn connect_ast(cpg: &mut CodePropertyGraph, parent: NodeId, child: NodeId) {
        cpg.connect(parent, child, CpgEdgeKind::AstChild);
        cpg.node_mut(parent).expect("parent").children.push(child);
        cpg.node_mut(child).expect("child").parent = Some(parent);
    }

    #[test]
    fn variable_facts_match_binding_and_assignment_use_rules() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let root = cpg.add_node_with_id(node(0, function()));
        let body = cpg.add_node_with_id(node(
            1,
            CpgNodeKind::Block {
                scope: ScopeId::GLOBAL,
            },
        ));
        connect_ast(&mut cpg, root, body);

        let parameter = cpg.add_node_with_id(node(
            2,
            CpgNodeKind::Parameter {
                name: "x".into(),
                param_type: None,
                is_variadic: false,
            },
        ));
        let parameter_binder = cpg.add_node_with_id(node(
            3,
            CpgNodeKind::Identifier {
                name: "x".into(),
                definition: None,
            },
        ));
        connect_ast(&mut cpg, body, parameter);
        connect_ast(&mut cpg, parameter, parameter_binder);

        let assignment = cpg.add_node_with_id(node(
            4,
            CpgNodeKind::Assignment {
                operator: "=".into(),
            },
        ));
        let target = cpg.add_node_with_id(node(
            5,
            CpgNodeKind::Identifier {
                name: "x".into(),
                definition: None,
            },
        ));
        let rhs = cpg.add_node_with_id(node(
            6,
            CpgNodeKind::Identifier {
                name: "y".into(),
                definition: None,
            },
        ));
        connect_ast(&mut cpg, body, assignment);
        connect_ast(&mut cpg, assignment, target);
        connect_ast(&mut cpg, assignment, rhs);

        let compound = cpg.add_node_with_id(node(
            7,
            CpgNodeKind::Assignment {
                operator: "+=".into(),
            },
        ));
        let compound_target = cpg.add_node_with_id(node(
            8,
            CpgNodeKind::Identifier {
                name: "x".into(),
                definition: None,
            },
        ));
        connect_ast(&mut cpg, body, compound);
        connect_ast(&mut cpg, compound, compound_target);

        let facts = variable_facts(&cpg, root);
        let x = facts.universe.id_of(&Arc::from("x")).expect("x fact");
        let y = facts.universe.id_of(&Arc::from("y")).expect("y fact");
        assert_eq!(
            facts.definitions_at(parameter)[0].kind,
            DefinitionKind::Parameter
        );
        assert_eq!(facts.definitions_at(assignment)[0].variable, x);
        assert_eq!(facts.definitions_at(compound)[0].variable, x);
        assert!(facts.uses_at(parameter_binder).is_empty());
        assert!(facts.uses_at(target).is_empty());
        assert_eq!(facts.uses_at(rhs), &[y]);
        assert_eq!(facts.uses_at(compound_target), &[x]);
        assert!(facts.definitions_at(NodeId::new(99)).is_empty());
        assert!(facts.uses_at(NodeId::new(99)).is_empty());
        assert_eq!(facts.name(99), None);
    }

    #[test]
    fn variable_facts_are_total_on_empty_and_non_identifier_bindings() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let root = cpg.add_node_with_id(node(0, function()));
        let empty_variable = cpg.add_node_with_id(node(
            1,
            CpgNodeKind::Variable {
                name: "".into(),
                var_type: None,
                is_mutable: false,
                scope: ScopeId::GLOBAL,
            },
        ));
        let empty_parameter = cpg.add_node_with_id(node(
            2,
            CpgNodeKind::Parameter {
                name: "".into(),
                param_type: None,
                is_variadic: false,
            },
        ));
        let no_target = cpg.add_node_with_id(node(
            3,
            CpgNodeKind::Assignment {
                operator: "=".into(),
            },
        ));
        let complex_target = cpg.add_node_with_id(node(
            4,
            CpgNodeKind::Assignment {
                operator: "=".into(),
            },
        ));
        let literal_target = cpg.add_node_with_id(node(
            5,
            CpgNodeKind::Literal {
                kind: LiteralKind::Integer(1),
            },
        ));
        let empty_use = cpg.add_node_with_id(node(
            6,
            CpgNodeKind::Identifier {
                name: "".into(),
                definition: None,
            },
        ));
        let nested = cpg.add_node_with_id(node(7, function()));
        let nested_use = cpg.add_node_with_id(node(
            8,
            CpgNodeKind::Identifier {
                name: "nested_only".into(),
                definition: None,
            },
        ));
        for child in [
            empty_variable,
            empty_parameter,
            no_target,
            complex_target,
            empty_use,
            nested,
        ] {
            connect_ast(&mut cpg, root, child);
        }
        connect_ast(&mut cpg, complex_target, literal_target);
        connect_ast(&mut cpg, nested, nested_use);

        let facts = variable_facts(&cpg, root);
        assert!(facts.universe.is_empty());
        assert!(facts.definitions.is_empty());
        assert!(facts.uses.is_empty());
    }

    #[test]
    fn repeated_structural_expressions_share_one_fact_and_kill_set() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let function = cpg.add_node_with_id(node(0, function()));
        for (base, variable) in [(10, "x"), (20, "x"), (30, "y")] {
            let expression = cpg.add_node_with_id(node(
                base,
                CpgNodeKind::BinaryOp {
                    operator: "+".into(),
                },
            ));
            let identifier = cpg.add_node_with_id(node(
                base + 1,
                CpgNodeKind::Identifier {
                    name: variable.into(),
                    definition: None,
                },
            ));
            let literal = cpg.add_node_with_id(node(
                base + 2,
                CpgNodeKind::Literal {
                    kind: LiteralKind::Integer(1),
                },
            ));
            connect_ast(&mut cpg, function, expression);
            connect_ast(&mut cpg, expression, identifier);
            connect_ast(&mut cpg, expression, literal);
        }

        let facts = expression_facts(&cpg, function);
        assert_eq!(facts.universe.len(), 2);
        assert_eq!(
            facts.generated_at(NodeId::new(10)),
            facts.generated_at(NodeId::new(20))
        );
        let x = facts.variables.id_of(&Arc::from("x")).expect("x variable");
        let x_expr = facts.generated_at(NodeId::new(10))[0];
        assert!(facts.killed_by_var(x_expr, x));
        let y = facts.variables.id_of(&Arc::from("y")).expect("y variable");
        assert!(!facts.killed_by_var(x_expr, y));
        assert!(facts.generated_at(NodeId::new(99)).is_empty());
        assert!(facts.variables_of(99).is_empty());
        assert_eq!(
            facts.occurrences[x_expr as usize],
            vec![NodeId::new(10), NodeId::new(20)]
        );
    }

    #[test]
    fn fact_universe_is_sorted_deduplicated_and_total() {
        let universe = FactUniverse::new(["z", "a", "z", "m"]);
        assert_eq!(universe.as_slice(), &["a", "m", "z"]);
        assert_eq!(universe.id_of(&"m"), Some(1));
        assert_eq!(universe.get(9), None);
        assert_eq!(
            universe.iter().map(|(id, _)| id).collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
        assert!(!universe.is_empty());
        assert_eq!(universe.id_of(&"missing"), None);
        let empty = FactUniverse::<u8>::new([]);
        assert!(empty.is_empty());
        let collected: FactUniverse<_> = [3, 1, 3].into_iter().collect();
        assert_eq!(collected.as_slice(), &[1, 3]);
    }

    proptest! {
        #[test]
        fn fact_universe_is_permutation_invariant(values in prop::collection::vec(any::<i16>(), 0..128)) {
            let forward = FactUniverse::new(values.iter().copied());
            let reverse = FactUniverse::new(values.iter().rev().copied());
            prop_assert_eq!(forward, reverse);
        }
    }

    #[test]
    fn expression_operands_cover_literals_nested_cycles_and_opaque_nodes() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let literal_kinds = [
            LiteralKind::Integer(-2),
            LiteralKind::Float(1.25),
            LiteralKind::String("text".into()),
            LiteralKind::Char('x'),
            LiteralKind::Bool(true),
            LiteralKind::Null,
            LiteralKind::Array,
            LiteralKind::Object,
            LiteralKind::Regex("a+".into()),
        ];
        let mut operands = Vec::new();
        for (index, kind) in literal_kinds.into_iter().enumerate() {
            let id = cpg.add_node_with_id(node(index as u32, CpgNodeKind::Literal { kind }));
            operands.push(expression_operand(&cpg, id, &mut BTreeSet::new()));
        }
        assert_eq!(operands[0], ExprOperand::Integer(-2));
        assert_eq!(operands[1], ExprOperand::Float(1.25f64.to_bits()));
        assert_eq!(operands[5], ExprOperand::Null);
        assert_eq!(operands[8], ExprOperand::Regex("a+".into()));

        let nested = cpg.add_node_with_id(node(
            20,
            CpgNodeKind::UnaryOp {
                operator: "-".into(),
            },
        ));
        connect_ast(&mut cpg, nested, NodeId::new(0));
        assert!(matches!(
            expression_operand(&cpg, nested, &mut BTreeSet::new()),
            ExprOperand::Expression(_)
        ));

        let opaque = cpg.add_node_with_id(node(21, CpgNodeKind::Return));
        assert_eq!(
            expression_operand(&cpg, opaque, &mut BTreeSet::new()),
            ExprOperand::Opaque(opaque)
        );
        assert!(expression_key(&cpg, opaque, &mut BTreeSet::new()).is_none());

        cpg.connect(nested, nested, CpgEdgeKind::AstChild);
        let mut on_path = BTreeSet::from([nested]);
        assert_eq!(
            expression_operand(&cpg, nested, &mut on_path),
            ExprOperand::Opaque(nested)
        );
    }

    #[cfg(feature = "serde")]
    #[test]
    fn fact_types_round_trip_through_serde() {
        let universe = FactUniverse::new([Arc::<str>::from("a"), Arc::from("b")]);
        let json = serde_json::to_string(&universe).expect("serialize universe");
        let decoded: FactUniverse<Arc<str>> =
            serde_json::from_str(&json).expect("deserialize universe");
        assert_eq!(decoded, universe);
    }
}
