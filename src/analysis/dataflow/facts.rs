//! Deterministic dense fact universes shared by data-flow analyses.

use std::{
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet, BinaryHeap},
    sync::Arc,
};

use crate::{CodePropertyGraph, CpgNodeKind, DefinitionKind, LiteralKind, NodeId, UseKind};

use super::super::function_scope_nodes;
use super::{
    expr_key::{ExprArenaBuilder, ExprBuildOperand},
    ExprKey, ExprKeyError, ExprKeyLimit, ExprKeyLimits,
};

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
        heap_sort_dedup(&mut values);
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

pub(super) fn heap_sort_dedup<T: Ord>(values: &mut Vec<T>) {
    let mut heap = BinaryHeap::with_capacity(values.len());
    for value in std::mem::take(values) {
        heap.push(Reverse(value));
    }
    values.reserve(heap.len());
    while let Some(Reverse(value)) = heap.pop() {
        if values.last().is_none_or(|previous| previous != &value) {
            values.push(value);
        }
    }
}

/// Canonical structural-expression universe backed by one shared arena.
///
/// Dense identifiers follow deterministic structural-class creation order in
/// the postorder arena. Equivalent occurrences share one value without
/// recursively comparing or copying their logical subtrees.
#[derive(Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ExprUniverse {
    values: Vec<ExprKey>,
    #[cfg_attr(feature = "serde", serde(skip))]
    class_ids: BTreeMap<u64, u32>,
}

impl std::fmt::Debug for ExprUniverse {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ExprUniverse")
            .field("values", &self.values)
            .finish()
    }
}

impl PartialEq for ExprUniverse {
    fn eq(&self, other: &Self) -> bool {
        self.values == other.values
    }
}

impl Eq for ExprUniverse {}

impl ExprUniverse {
    fn try_new(values: impl IntoIterator<Item = ExprKey>) -> Result<Self, ExprKeyError> {
        let mut by_class = BTreeMap::new();
        for value in values {
            by_class.entry(value.class_id()).or_insert(value);
        }
        let values: Vec<_> = by_class.into_values().collect();
        let mut class_ids = BTreeMap::new();
        for (index, value) in values.iter().enumerate() {
            let id = u32::try_from(index).map_err(|_| ExprKeyError::ArithmeticOverflow {
                context: "expression-universe dense identifier",
            })?;
            class_ids.insert(value.class_id(), id);
        }
        Ok(Self { values, class_ids })
    }

    /// Returns the number of distinct structural expressions.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Returns whether the expression universe is empty.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Returns the expression with dense identifier `id`.
    pub fn get(&self, id: u32) -> Option<&ExprKey> {
        self.values.get(id as usize)
    }

    /// Returns the dense identifier of a structural expression.
    pub fn id_of(&self, expression: &ExprKey) -> Option<u32> {
        if !self.class_ids.is_empty()
            && self
                .values
                .first()
                .is_some_and(|value| value.shares_arena_with(expression))
        {
            return self.class_ids.get(&expression.class_id()).copied();
        }
        self.values
            .iter()
            .position(|candidate| candidate == expression)
            .and_then(|index| u32::try_from(index).ok())
    }

    /// Iterates expressions in dense-id order.
    pub fn iter(&self) -> impl Iterator<Item = (u32, &ExprKey)> {
        self.values
            .iter()
            .enumerate()
            .map(|(index, value)| (index as u32, value))
    }

    /// Returns expressions in dense-id order.
    pub fn as_slice(&self) -> &[ExprKey] {
        &self.values
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
    let nodes = super::super::scc::sorted_nodes(scope.iter().copied());

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
    debug_assert!(definitions.values().all(|at_node| at_node.len() == 1));

    let mut uses: BTreeMap<NodeId, Vec<u32>> = BTreeMap::new();
    for (node, name, _) in raw_uses {
        if let Some(variable) = universe.id_of(&name) {
            uses.entry(node).or_default().push(variable);
        }
    }
    debug_assert!(uses.values().all(|at_node| at_node.len() == 1));

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
    cpg.ast_children(parent).iter().copied().find(|&child| {
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

/// Canonical expression occurrences and variable dependencies.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ExprFacts {
    /// Deduplicated structural roots in one shared flat arena.
    pub universe: ExprUniverse,
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
///
/// # Errors
///
/// Returns [`ExprKeyError`] if default node, operand, string-byte,
/// encoded-byte, or work limits are exceeded, checked arithmetic overflows, or
/// a required arena/worklist reservation fails. The function never returns a
/// partial fact universe.
pub fn expression_facts(
    cpg: &CodePropertyGraph,
    function: NodeId,
) -> Result<ExprFacts, ExprKeyError> {
    expression_facts_with_limits(cpg, function, ExprKeyLimits::default())
}

/// Re-derives expression facts with explicit arena and logical-key limits.
///
/// Any exceeded cap fails the entire derivation. No partial universe is
/// returned because omitting an expression could make a Must analysis report
/// an unsound negative claim.
///
/// # Errors
///
/// Returns [`ExprKeyError`] for a named limit, checked arithmetic, or fallible
/// allocation failure while building the shared postorder arena.
pub fn expression_facts_with_limits(
    cpg: &CodePropertyGraph,
    function: NodeId,
    limits: ExprKeyLimits,
) -> Result<ExprFacts, ExprKeyError> {
    let scope = function_scope_nodes(cpg, function);
    let nodes = super::super::scc::sorted_nodes(scope.iter().copied());
    let expression_nodes: Vec<_> = nodes
        .iter()
        .copied()
        .filter(|&node| {
            matches!(
                cpg.node(node).map(|node| &node.kind),
                Some(CpgNodeKind::BinaryOp { .. } | CpgNodeKind::UnaryOp { .. })
            )
        })
        .collect();

    let BuiltExpressionArena {
        arena,
        roots,
        variable_names,
    } = build_expression_arena(cpg, &expression_nodes, limits)?;
    let mut raw = Vec::new();
    let mut all_variables = BTreeSet::new();

    raw.try_reserve(expression_nodes.len())
        .map_err(|error| ExprKeyError::AllocationFailed {
            context: "expression-fact occurrences",
            message: error.to_string(),
        })?;
    for node in expression_nodes {
        let root = roots[&node];
        let variables = variable_names[root as usize].clone();
        all_variables.extend(variables.iter().cloned());
        raw.push((node, ExprKey::from_shared(arena.clone(), root), variables));
    }

    let universe = ExprUniverse::try_new(raw.iter().map(|(_, key, _)| key.clone()))?;
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
    debug_assert!(generated.values().all(|expressions| expressions.len() == 1));
    debug_assert!(occurrences
        .iter()
        .all(|nodes| nodes.windows(2).all(|pair| pair[0] < pair[1])));
    let operand_variables = operand_names
        .into_iter()
        .map(|names| {
            names
                .into_iter()
                .filter_map(|name| variables.id_of(&name))
                .collect()
        })
        .collect();

    Ok(ExprFacts {
        universe,
        variables,
        generated,
        occurrences,
        operand_variables,
    })
}

struct BuiltExpressionArena {
    arena: Arc<super::expr_key::ExprArena>,
    roots: BTreeMap<NodeId, u64>,
    variable_names: Vec<BTreeSet<Arc<str>>>,
}

fn build_expression_arena(
    cpg: &CodePropertyGraph,
    expression_nodes: &[NodeId],
    limits: ExprKeyLimits,
) -> Result<BuiltExpressionArena, ExprKeyError> {
    struct Frame {
        node: NodeId,
        operator: Arc<str>,
        children: Vec<NodeId>,
        next: usize,
        operands: Vec<ExprBuildOperand>,
        variables: BTreeSet<Arc<str>>,
    }

    fn frame(cpg: &CodePropertyGraph, node: NodeId) -> Result<Frame, ExprKeyError> {
        let operator = match cpg.node(node).map(|node| &node.kind) {
            Some(CpgNodeKind::BinaryOp { operator } | CpgNodeKind::UnaryOp { operator }) => {
                operator.clone()
            }
            _ => {
                return Err(ExprKeyError::ArithmeticOverflow {
                    context: "expression frame requires an operator node",
                })
            }
        };
        let children = cpg.ast_children(node).to_vec();
        let mut operands = Vec::new();
        operands
            .try_reserve(children.len())
            .map_err(|error| ExprKeyError::AllocationFailed {
                context: "expression extraction frame",
                message: error.to_string(),
            })?;
        Ok(Frame {
            node,
            operator,
            children,
            next: 0,
            operands,
            variables: BTreeSet::new(),
        })
    }

    fn limit(dimension: ExprKeyLimit, actual: usize, maximum: usize) -> Result<(), ExprKeyError> {
        if actual > maximum {
            Err(ExprKeyError::LimitExceeded {
                limit: dimension,
                actual,
                maximum,
            })
        } else {
            Ok(())
        }
    }

    let mut builder = ExprArenaBuilder::new(limits);
    let mut roots = BTreeMap::new();
    let mut variables_by_node: Vec<BTreeSet<Arc<str>>> = Vec::new();
    let mut active = BTreeSet::new();
    let mut work = Vec::new();
    let mut scheduled_nodes = 0usize;
    let mut scheduled_operands = 0usize;

    for &root in expression_nodes {
        if roots.contains_key(&root) {
            continue;
        }
        scheduled_nodes =
            scheduled_nodes
                .checked_add(1)
                .ok_or(ExprKeyError::ArithmeticOverflow {
                    context: "scheduled expression nodes",
                })?;
        limit(ExprKeyLimit::Nodes, scheduled_nodes, limits.max_nodes)?;
        let root_frame = frame(cpg, root)?;
        scheduled_operands = scheduled_operands
            .checked_add(root_frame.children.len())
            .ok_or(ExprKeyError::ArithmeticOverflow {
                context: "scheduled expression operands",
            })?;
        limit(
            ExprKeyLimit::Operands,
            scheduled_operands,
            limits.max_operands,
        )?;
        active.insert(root);
        work.push(root_frame);

        while let Some(current) = work.last_mut() {
            if let Some(&child) = current.children.get(current.next) {
                current.next += 1;
                match cpg.node(child).map(|node| &node.kind) {
                    Some(CpgNodeKind::Identifier { name, .. }) => {
                        current.variables.insert(name.clone());
                        current
                            .operands
                            .push(ExprBuildOperand::Variable(name.clone()));
                    }
                    Some(CpgNodeKind::Literal { kind }) => {
                        current.operands.push(literal_operand(kind));
                    }
                    Some(CpgNodeKind::BinaryOp { .. } | CpgNodeKind::UnaryOp { .. }) => {
                        if active.contains(&child) {
                            current.operands.push(ExprBuildOperand::Opaque(child));
                        } else if let Some(&child_root) = roots.get(&child) {
                            current
                                .variables
                                .extend(variable_names_for(&variables_by_node, child_root));
                            current
                                .operands
                                .push(ExprBuildOperand::Expression(child_root));
                        } else {
                            scheduled_nodes = scheduled_nodes.checked_add(1).ok_or(
                                ExprKeyError::ArithmeticOverflow {
                                    context: "scheduled expression nodes",
                                },
                            )?;
                            limit(ExprKeyLimit::Nodes, scheduled_nodes, limits.max_nodes)?;
                            let child_frame = frame(cpg, child)?;
                            scheduled_operands = scheduled_operands
                                .checked_add(child_frame.children.len())
                                .ok_or(ExprKeyError::ArithmeticOverflow {
                                    context: "scheduled expression operands",
                                })?;
                            limit(
                                ExprKeyLimit::Operands,
                                scheduled_operands,
                                limits.max_operands,
                            )?;
                            active.insert(child);
                            work.push(child_frame);
                        }
                    }
                    _ => current.operands.push(ExprBuildOperand::Opaque(child)),
                }
                continue;
            }

            let completed = work.pop().expect("one active expression frame");
            active.remove(&completed.node);
            let variables = completed.variables;
            let node = builder.push(completed.operator, completed.operands)?;
            debug_assert_eq!(node as usize, variables_by_node.len());
            variables_by_node.push(variables.clone());
            roots.insert(completed.node, node);
            if let Some(parent) = work.last_mut() {
                parent.variables.extend(variables);
                parent.operands.push(ExprBuildOperand::Expression(node));
            }
        }
    }

    Ok(BuiltExpressionArena {
        arena: builder.finish(),
        roots,
        variable_names: variables_by_node,
    })
}

fn variable_names_for(
    variables_by_node: &[BTreeSet<Arc<str>>],
    node: u64,
) -> impl Iterator<Item = Arc<str>> + '_ {
    variables_by_node[node as usize].iter().cloned()
}

fn literal_operand(kind: &LiteralKind) -> ExprBuildOperand {
    match kind {
        LiteralKind::Integer(value) => ExprBuildOperand::Integer(*value),
        LiteralKind::Float(value) => ExprBuildOperand::Float(value.to_bits()),
        LiteralKind::String(value) => ExprBuildOperand::String(value.clone()),
        LiteralKind::Char(value) => ExprBuildOperand::Char(*value),
        LiteralKind::Bool(value) => ExprBuildOperand::Bool(*value),
        LiteralKind::Null => ExprBuildOperand::Null,
        LiteralKind::Array => ExprBuildOperand::Array,
        LiteralKind::Object => ExprBuildOperand::Object,
        LiteralKind::Regex(value) => ExprBuildOperand::Regex(value.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CpgEdgeKind, CpgNode, ExprOperandRef, Language, MethodSignature, ScopeId, SourceRange,
        Visibility,
    };
    use proptest::prelude::*;

    fn node(id: u32, kind: CpgNodeKind) -> CpgNode {
        CpgNode::new(NodeId::new(id), kind, SourceRange::default())
    }

    fn function() -> CpgNodeKind {
        CpgNodeKind::Function {
            signature: Box::new(MethodSignature {
                name: "facts".into(),
                params: Default::default(),
                return_type: None,
                is_static: false,
                is_async: false,
                visibility: Visibility::Private,
            }),
        }
    }

    fn connect_ast(cpg: &mut CodePropertyGraph, parent: NodeId, child: NodeId) {
        cpg.connect(parent, child, CpgEdgeKind::AstChild);
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

        let facts = expression_facts(&cpg, function).expect("expression facts");
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
        let function = cpg.add_node_with_id(node(100, function()));
        let root = cpg.add_node_with_id(node(
            101,
            CpgNodeKind::BinaryOp {
                operator: "all".into(),
            },
        ));
        connect_ast(&mut cpg, function, root);
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
        for (index, kind) in literal_kinds.into_iter().enumerate() {
            let id = cpg.add_node_with_id(node(index as u32, CpgNodeKind::Literal { kind }));
            connect_ast(&mut cpg, root, id);
        }

        let nested = cpg.add_node_with_id(node(
            20,
            CpgNodeKind::UnaryOp {
                operator: "-".into(),
            },
        ));
        connect_ast(&mut cpg, nested, NodeId::new(0));
        connect_ast(&mut cpg, root, nested);

        let opaque = cpg.add_node_with_id(node(21, CpgNodeKind::Return));
        connect_ast(&mut cpg, root, opaque);

        cpg.connect(nested, nested, CpgEdgeKind::AstChild);
        let facts = expression_facts(&cpg, function).expect("iterative expression facts");
        let root_id = facts.generated_at(root)[0];
        let root_key = facts.universe.get(root_id).expect("root key");
        let operands: Vec<_> = root_key.operands().iter().collect();
        assert_eq!(operands[0], ExprOperandRef::Integer(-2));
        assert_eq!(operands[1], ExprOperandRef::Float(1.25f64.to_bits()));
        assert_eq!(operands[5], ExprOperandRef::Null);
        assert_eq!(operands[8], ExprOperandRef::Regex("a+"));
        let ExprOperandRef::Expression(nested_key) = operands[9] else {
            panic!("nested operator remains structural");
        };
        assert!(nested_key
            .operands()
            .iter()
            .any(|operand| operand == ExprOperandRef::Opaque(nested)));
        assert_eq!(operands[10], ExprOperandRef::Opaque(opaque));
    }

    #[test]
    fn shared_subexpressions_reuse_arena_ids_but_encode_as_canonical_trees() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let function = cpg.add_node_with_id(node(0, function()));
        let root = cpg.add_node_with_id(node(
            1,
            CpgNodeKind::BinaryOp {
                operator: "+".into(),
            },
        ));
        let shared = cpg.add_node_with_id(node(
            2,
            CpgNodeKind::UnaryOp {
                operator: "-".into(),
            },
        ));
        let variable = cpg.add_node_with_id(node(
            3,
            CpgNodeKind::Identifier {
                name: "x".into(),
                definition: None,
            },
        ));
        connect_ast(&mut cpg, function, root);
        connect_ast(&mut cpg, root, shared);
        cpg.connect(root, shared, CpgEdgeKind::AstChild);
        connect_ast(&mut cpg, shared, variable);

        let facts = expression_facts(&cpg, function).expect("shared expression facts");
        let key = facts
            .universe
            .get(facts.generated_at(root)[0])
            .expect("root key");
        let operands: Vec<_> = key.operands().iter().collect();
        let (ExprOperandRef::Expression(left), ExprOperandRef::Expression(right)) =
            (operands[0], operands[1])
        else {
            panic!("both operands are shared expressions");
        };
        assert_eq!(left, right);
        assert_eq!(key.metrics().nodes, 3);
        let decoded = ExprKey::decode(&key.encode().expect("canonical shared encode"))
            .expect("canonical shared decode");
        assert_eq!(decoded, *key);
        assert_eq!(decoded.metrics().nodes, 3);
    }

    #[test]
    fn expression_fact_caps_fail_the_whole_derivation_monotonically() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let function = cpg.add_node_with_id(node(0, function()));
        let expression = cpg.add_node_with_id(node(
            1,
            CpgNodeKind::BinaryOp {
                operator: "+".into(),
            },
        ));
        let variable = cpg.add_node_with_id(node(
            2,
            CpgNodeKind::Identifier {
                name: "variable".into(),
                definition: None,
            },
        ));
        let literal = cpg.add_node_with_id(node(
            3,
            CpgNodeKind::Literal {
                kind: LiteralKind::Integer(1),
            },
        ));
        connect_ast(&mut cpg, function, expression);
        connect_ast(&mut cpg, expression, variable);
        connect_ast(&mut cpg, expression, literal);
        let facts = expression_facts(&cpg, function).expect("uncapped facts");
        let metrics = facts.universe.get(0).expect("one key").metrics();

        let cases = [
            (ExprKeyLimit::Nodes, metrics.nodes),
            (ExprKeyLimit::Operands, metrics.operands),
            (ExprKeyLimit::StringBytes, metrics.string_bytes),
            (ExprKeyLimit::EncodedBytes, metrics.encoded_bytes),
            (ExprKeyLimit::Work, metrics.work),
        ];
        for (dimension, exact) in cases {
            let mut limits = ExprKeyLimits::UNBOUNDED;
            match dimension {
                ExprKeyLimit::Nodes => limits.max_nodes = exact,
                ExprKeyLimit::Operands => limits.max_operands = exact,
                ExprKeyLimit::StringBytes => limits.max_string_bytes = exact,
                ExprKeyLimit::EncodedBytes => limits.max_encoded_bytes = exact,
                ExprKeyLimit::Work => limits.max_work = exact,
            }
            assert!(expression_facts_with_limits(&cpg, function, limits).is_ok());
            match dimension {
                ExprKeyLimit::Nodes => limits.max_nodes -= 1,
                ExprKeyLimit::Operands => limits.max_operands -= 1,
                ExprKeyLimit::StringBytes => limits.max_string_bytes -= 1,
                ExprKeyLimit::EncodedBytes => limits.max_encoded_bytes -= 1,
                ExprKeyLimit::Work => limits.max_work -= 1,
            }
            assert!(matches!(
                expression_facts_with_limits(&cpg, function, limits),
                Err(ExprKeyError::LimitExceeded { limit, .. }) if limit == dimension
            ));
        }
    }

    #[cfg(feature = "serde")]
    #[test]
    fn fact_types_round_trip_through_serde() {
        let universe = FactUniverse::new([Arc::<str>::from("a"), Arc::from("b")]);
        let json = serde_json::to_string(&universe).expect("serialize universe");
        let decoded: FactUniverse<Arc<str>> =
            serde_json::from_str(&json).expect("deserialize universe");
        assert_eq!(decoded, universe);

        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let function = cpg.add_node_with_id(node(0, function()));
        let expression = cpg.add_node_with_id(node(
            1,
            CpgNodeKind::UnaryOp {
                operator: "-".into(),
            },
        ));
        let variable = cpg.add_node_with_id(node(
            2,
            CpgNodeKind::Identifier {
                name: "x".into(),
                definition: None,
            },
        ));
        connect_ast(&mut cpg, function, expression);
        connect_ast(&mut cpg, expression, variable);
        let facts = expression_facts(&cpg, function).expect("expression facts");
        let json = serde_json::to_vec(&facts).expect("serialize expression facts");
        let decoded: ExprFacts =
            serde_json::from_slice(&json).expect("deserialize expression facts");
        assert_eq!(decoded, facts);
        let original_key = facts.universe.get(0).expect("original key");
        assert_eq!(decoded.universe.id_of(original_key), Some(0));
    }
}
