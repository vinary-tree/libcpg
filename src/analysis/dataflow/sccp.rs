//! Sparse conditional constant propagation over a function's CFG and DFG.
//!
//! The implementation follows Wegman and Zadeck's coupled fixed point: one
//! worklist discovers executable control-flow edges and another propagates the
//! height-two constant lattice over AST expression and DFG def-use relations.
//! Results are deterministic, feature-free, read-only, and entirely out of the
//! source CPG.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use rustc_hash::FxHashSet;

use crate::{
    CfgEdgeKind, CodePropertyGraph, CpgEdgeKind, CpgNodeKind, DfgEdgeKind, LiteralKind, NodeId,
};

use super::conditions::guard_expression;
use super::Lattice;
use crate::analysis::{
    cfg_projection,
    cfg_scope::{function_cfg_edges, function_cfg_view},
    function_scope_nodes, AnalysisError,
};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// A scalar constant represented without lossy normalization.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ConstValue {
    /// Signed 64-bit integer.
    Int(i64),
    /// IEEE-754 `f64` represented by its exact bit pattern.
    Float(u64),
    /// Boolean value.
    Bool(bool),
    /// Immutable string value.
    Str(Arc<str>),
    /// Unicode scalar value.
    Char(char),
    /// Null or nil value.
    Null,
}

/// Height-two lattice used by sparse conditional constant propagation.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ConstLattice {
    /// No executable definition has supplied a value yet; lattice bottom.
    Unknown,
    /// Every executable definition agrees on one scalar constant.
    Constant(ConstValue),
    /// Values disagree or an executable operation is not safely foldable.
    Overdefined,
}

impl Lattice for ConstLattice {
    fn join(&mut self, other: &Self) -> bool {
        let next = match (&*self, other) {
            (Self::Unknown, value) => value.clone(),
            (value, Self::Unknown) => value.clone(),
            (Self::Constant(left), Self::Constant(right)) if left == right => self.clone(),
            (Self::Constant(_), Self::Constant(_)) => Self::Overdefined,
            (Self::Overdefined, _) | (_, Self::Overdefined) => Self::Overdefined,
        };
        let changed = next != *self;
        *self = next;
        changed
    }

    fn leq(&self, other: &Self) -> bool {
        matches!(
            (self, other),
            (Self::Unknown, _)
                | (Self::Constant(_), Self::Overdefined)
                | (Self::Overdefined, Self::Overdefined)
        ) || matches!((self, other), (Self::Constant(left), Self::Constant(right)) if left == right)
    }
}

/// Deterministic SCCP facts for one function.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct SccpResult {
    /// Function whose intraprocedural overlays were analyzed.
    pub function: NodeId,
    /// Proven constants on executable definition and expression nodes.
    pub constants: BTreeMap<NodeId, ConstValue>,
    /// Executable intraprocedural CFG edges in canonical order.
    pub executable_edges: Vec<(NodeId, NodeId, CfgEdgeKind)>,
    /// Conditional CFG edges proved dead or contained in unreachable islands.
    pub dead_edges: Vec<(NodeId, NodeId, CfgEdgeKind)>,
    /// Function-scoped CFG nodes not reached by an executable edge.
    pub unreachable_nodes: Vec<NodeId>,
}

/// Computes sparse conditional constant propagation for `function`.
///
/// Literal, unary, binary, simple-assignment, and declaration-initializer
/// values are propagated over AST dependencies. Identifier uses join all
/// executable incoming [`DfgEdgeKind::DefUse`] definitions, which gives
/// multiple reaching definitions phi-like semantics. Calls, external inputs,
/// unsupported literals, and operations that cannot be folded safely become
/// [`ConstLattice::Overdefined`]. Signed integer arithmetic is checked; an
/// overflow, invalid shift, or division by zero is never reported as a
/// constant.
///
/// A boolean constant at a guard activates only its matching
/// [`CfgEdgeKind::ConditionalTrue`] or [`CfgEdgeKind::ConditionalFalse`] edge.
/// Other conditional vocabularies remain conservative because CPG case labels
/// do not carry a comparable scalar in the edge itself. The iteration budget
/// is `4 * (scope nodes + def-use edges)` and failure returns
/// [`AnalysisError::IterationLimitExceeded`]. The CPG is borrowed immutably.
pub fn sccp(cpg: &CodePropertyGraph, function: NodeId) -> Result<SccpResult, AnalysisError> {
    let view = function_cfg_view(cpg, function)?;
    let scoped_cfg = cfg_projection(cpg, function)?;
    let flow_edges = function_cfg_edges(cpg, &view.projection);
    let all_scoped_edges = function_cfg_edges(cpg, &scoped_cfg);
    let scope = function_scope_nodes(cpg, function);

    let mut universe_nodes = scoped_cfg.nodes.clone();
    universe_nodes.extend(view.projection.nodes.iter().copied());
    universe_nodes.sort_unstable();
    universe_nodes.dedup();

    // Ownership must recognize every scoped CFG endpoint, including a
    // disconnected island. Using only the entry-reachable view here would
    // attach an island's AST value cells to the function entry and could then
    // report constants for code that is not executable.
    let cfg_nodes: FxHashSet<_> = scoped_cfg.nodes.iter().copied().collect();
    let cells: BTreeSet<_> = scope
        .iter()
        .copied()
        .filter(|&node| is_value_cell(cpg, node))
        .collect();
    let ast_parents = ast_parent_map(cpg, &scope);
    let owners = execution_owners(&cells, &cfg_nodes, &ast_parents);
    let mut cells_by_owner: BTreeMap<NodeId, Vec<NodeId>> = BTreeMap::new();
    for (&cell, cell_owners) in &owners {
        for &owner in cell_owners {
            cells_by_owner.entry(owner).or_default().push(cell);
        }
    }

    let (def_use_predecessors, def_use_successors, dfg_edge_count) =
        def_use_maps(cpg, &scope, &cells);
    let mut outgoing: BTreeMap<NodeId, Vec<(NodeId, NodeId, CfgEdgeKind)>> = BTreeMap::new();
    for &edge @ (source, _, _) in &flow_edges {
        outgoing.entry(source).or_default().push(edge);
    }

    let mut guard_cells: BTreeMap<NodeId, NodeId> = BTreeMap::new();
    let mut guard_consumers: BTreeMap<NodeId, Vec<NodeId>> = BTreeMap::new();
    for (&source, edges) in &outgoing {
        if edges.iter().any(|(_, _, kind)| kind.is_conditional()) {
            if let Some(cell) = guard_expression(cpg, source).filter(|cell| cells.contains(cell)) {
                guard_cells.insert(source, cell);
                guard_consumers.entry(cell).or_default().push(source);
            }
        }
    }

    let mut values: BTreeMap<NodeId, ConstLattice> = cells
        .iter()
        .copied()
        .map(|node| (node, ConstLattice::Unknown))
        .collect();
    let mut executable_nodes = FxHashSet::default();
    let mut executable_edge_set = FxHashSet::default();
    let mut flow_work = BTreeSet::new();
    let mut value_work = BTreeSet::new();

    mark_node_executable(
        function,
        &mut executable_nodes,
        &mut flow_work,
        &mut value_work,
        &cells_by_owner,
    );

    let limit = 4u64.saturating_mul(
        u64::try_from(scope.len())
            .unwrap_or(u64::MAX)
            .saturating_add(u64::try_from(dfg_edge_count).unwrap_or(u64::MAX)),
    );
    let mut steps = 0u64;

    while !flow_work.is_empty() || !value_work.is_empty() {
        steps = steps.saturating_add(1);
        if steps > limit {
            return Err(AnalysisError::IterationLimitExceeded {
                analysis: "sccp",
                limit,
            });
        }

        if let Some(source) = flow_work.pop_first() {
            let guard = guard_cells.get(&source).and_then(|cell| values.get(cell));
            for &(edge_source, target, kind) in outgoing.get(&source).map_or(&[][..], Vec::as_slice)
            {
                if !edge_is_executable(kind, guard) {
                    continue;
                }
                let edge = (edge_source, target, kind);
                if executable_edge_set.insert(edge) {
                    mark_node_executable(
                        target,
                        &mut executable_nodes,
                        &mut flow_work,
                        &mut value_work,
                        &cells_by_owner,
                    );
                }
            }
            continue;
        }

        let Some(node) = value_work.pop_first() else {
            continue;
        };
        if !cell_is_executable(node, &owners, &executable_nodes) {
            continue;
        }
        let candidate = evaluate_cell(
            cpg,
            node,
            &values,
            &def_use_predecessors,
            &owners,
            &executable_nodes,
        );
        let changed = values
            .get_mut(&node)
            .expect("the worklist contains only value cells")
            .join(&candidate);
        if !changed {
            continue;
        }

        if let Some(parents) = ast_parents.get(&node) {
            value_work.extend(
                parents
                    .iter()
                    .copied()
                    .filter(|parent| cells.contains(parent)),
            );
        }
        if let Some(uses) = def_use_successors.get(&node) {
            value_work.extend(uses.iter().copied());
        }
        if let Some(consumers) = guard_consumers.get(&node) {
            flow_work.extend(
                consumers
                    .iter()
                    .copied()
                    .filter(|source| executable_nodes.contains(source)),
            );
        }
    }

    let constants = values
        .into_iter()
        .filter_map(|(node, value)| {
            (cell_is_executable(node, &owners, &executable_nodes))
                .then_some(value)
                .and_then(|value| match value {
                    ConstLattice::Constant(value) => Some((node, value)),
                    ConstLattice::Unknown | ConstLattice::Overdefined => None,
                })
        })
        .collect();
    let executable_edges = flow_edges
        .into_iter()
        .filter(|edge| executable_edge_set.contains(edge))
        .collect();
    let dead_edges = all_scoped_edges
        .into_iter()
        .filter(|edge| edge.2.is_conditional() && !executable_edge_set.contains(edge))
        .collect();
    let unreachable_nodes = universe_nodes
        .into_iter()
        .filter(|node| !executable_nodes.contains(node))
        .collect();

    Ok(SccpResult {
        function,
        constants,
        executable_edges,
        dead_edges,
        unreachable_nodes,
    })
}

fn is_value_cell(cpg: &CodePropertyGraph, node: NodeId) -> bool {
    cpg.node(node).is_some_and(|node| {
        node.is_expression()
            || matches!(
                node.kind,
                CpgNodeKind::Variable { .. } | CpgNodeKind::Parameter { .. }
            )
    })
}

fn ast_parent_map(
    cpg: &CodePropertyGraph,
    scope: &FxHashSet<NodeId>,
) -> BTreeMap<NodeId, Vec<NodeId>> {
    let mut parents: BTreeMap<NodeId, Vec<NodeId>> = BTreeMap::new();
    for edge in cpg.edges() {
        if matches!(edge.kind, CpgEdgeKind::AstChild)
            && scope.contains(&edge.source)
            && scope.contains(&edge.target)
        {
            parents.entry(edge.target).or_default().push(edge.source);
        }
    }
    for values in parents.values_mut() {
        values.sort_unstable();
        values.dedup();
    }
    parents
}

fn execution_owners(
    cells: &BTreeSet<NodeId>,
    cfg_nodes: &FxHashSet<NodeId>,
    ast_parents: &BTreeMap<NodeId, Vec<NodeId>>,
) -> BTreeMap<NodeId, Vec<NodeId>> {
    cells
        .iter()
        .copied()
        .map(|cell| {
            if cfg_nodes.contains(&cell) {
                return (cell, vec![cell]);
            }
            let mut owners = BTreeSet::new();
            let mut seen = FxHashSet::default();
            let mut work = ast_parents.get(&cell).cloned().unwrap_or_default();
            while let Some(parent) = work.pop() {
                if !seen.insert(parent) {
                    continue;
                }
                if cfg_nodes.contains(&parent) {
                    owners.insert(parent);
                } else if let Some(grandparents) = ast_parents.get(&parent) {
                    work.extend(grandparents.iter().copied());
                }
            }
            (cell, owners.into_iter().collect())
        })
        .collect()
}

type DefUseMap = BTreeMap<NodeId, Vec<NodeId>>;

fn def_use_maps(
    cpg: &CodePropertyGraph,
    scope: &FxHashSet<NodeId>,
    cells: &BTreeSet<NodeId>,
) -> (DefUseMap, DefUseMap, usize) {
    let mut predecessors: DefUseMap = BTreeMap::new();
    let mut successors: DefUseMap = BTreeMap::new();
    let mut count = 0usize;
    for edge in cpg.edges() {
        if !matches!(edge.kind, CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse))
            || !scope.contains(&edge.source)
            || !scope.contains(&edge.target)
            || !cells.contains(&edge.source)
            || !cells.contains(&edge.target)
        {
            continue;
        }
        predecessors
            .entry(edge.target)
            .or_default()
            .push(edge.source);
        successors.entry(edge.source).or_default().push(edge.target);
        count = count.saturating_add(1);
    }
    for values in predecessors.values_mut().chain(successors.values_mut()) {
        values.sort_unstable();
        values.dedup();
    }
    (predecessors, successors, count)
}

fn mark_node_executable(
    node: NodeId,
    executable_nodes: &mut FxHashSet<NodeId>,
    flow_work: &mut BTreeSet<NodeId>,
    value_work: &mut BTreeSet<NodeId>,
    cells_by_owner: &BTreeMap<NodeId, Vec<NodeId>>,
) {
    if !executable_nodes.insert(node) {
        return;
    }
    flow_work.insert(node);
    if let Some(cells) = cells_by_owner.get(&node) {
        value_work.extend(cells.iter().copied());
    }
}

fn cell_is_executable(
    node: NodeId,
    owners: &BTreeMap<NodeId, Vec<NodeId>>,
    executable_nodes: &FxHashSet<NodeId>,
) -> bool {
    owners
        .get(&node)
        .is_some_and(|owners| owners.iter().any(|owner| executable_nodes.contains(owner)))
}

fn edge_is_executable(kind: CfgEdgeKind, guard: Option<&ConstLattice>) -> bool {
    if !kind.is_conditional() {
        return true;
    }
    if matches!(kind, CfgEdgeKind::Case | CfgEdgeKind::DefaultCase) {
        return true;
    }
    match guard {
        Some(ConstLattice::Unknown) => false,
        Some(ConstLattice::Constant(ConstValue::Bool(value))) => match kind {
            CfgEdgeKind::ConditionalTrue => *value,
            CfgEdgeKind::ConditionalFalse => !*value,
            _ => true,
        },
        Some(ConstLattice::Constant(_)) | Some(ConstLattice::Overdefined) | None => true,
    }
}

fn evaluate_cell(
    cpg: &CodePropertyGraph,
    node: NodeId,
    values: &BTreeMap<NodeId, ConstLattice>,
    def_use_predecessors: &DefUseMap,
    owners: &BTreeMap<NodeId, Vec<NodeId>>,
    executable_nodes: &FxHashSet<NodeId>,
) -> ConstLattice {
    let Some(kind) = cpg.node(node).map(|node| &node.kind) else {
        return ConstLattice::Overdefined;
    };
    match kind {
        CpgNodeKind::Literal { kind } => literal_value(kind),
        CpgNodeKind::Identifier { .. } => {
            let Some(definitions) = def_use_predecessors.get(&node) else {
                return ConstLattice::Overdefined;
            };
            let mut result = ConstLattice::Unknown;
            for definition in definitions {
                if cell_is_executable(*definition, owners, executable_nodes) {
                    result.join(values.get(definition).unwrap_or(&ConstLattice::Unknown));
                }
            }
            result
        }
        CpgNodeKind::BinaryOp { operator } => {
            let operands = value_children(cpg, node);
            match operands.as_slice() {
                [left, right, ..] => fold_binary(
                    operator,
                    values.get(left).unwrap_or(&ConstLattice::Overdefined),
                    values.get(right).unwrap_or(&ConstLattice::Overdefined),
                ),
                _ => ConstLattice::Overdefined,
            }
        }
        CpgNodeKind::UnaryOp { operator } => value_children(cpg, node)
            .first()
            .and_then(|operand| values.get(operand))
            .map_or(ConstLattice::Overdefined, |operand| {
                fold_unary(operator, operand)
            }),
        CpgNodeKind::Assignment { operator } => {
            let children = value_children(cpg, node);
            match children.as_slice() {
                [_, rhs, ..] if &**operator == "=" => values
                    .get(rhs)
                    .cloned()
                    .unwrap_or(ConstLattice::Overdefined),
                [target, rhs, ..] => operator
                    .strip_suffix('=')
                    .filter(|operator| !operator.is_empty())
                    .map_or(ConstLattice::Overdefined, |operator| {
                        fold_binary(
                            operator,
                            values.get(target).unwrap_or(&ConstLattice::Overdefined),
                            values.get(rhs).unwrap_or(&ConstLattice::Overdefined),
                        )
                    }),
                _ => ConstLattice::Overdefined,
            }
        }
        CpgNodeKind::Variable { name, .. } => {
            let initializer = cpg.ast_children(node).iter().copied().rev().find(|child| {
                !matches!(
                    cpg.node(*child).map(|node| &node.kind),
                    Some(CpgNodeKind::Identifier { name: child_name, .. }) if child_name == name
                )
            });
            initializer
                .and_then(|initializer| values.get(&initializer))
                .cloned()
                .unwrap_or(ConstLattice::Overdefined)
        }
        CpgNodeKind::Parameter { .. } | CpgNodeKind::Call { .. } => ConstLattice::Overdefined,
        CpgNodeKind::MemberAccess { .. }
        | CpgNodeKind::IndexAccess
        | CpgNodeKind::Lambda { .. }
        | CpgNodeKind::Await
        | CpgNodeKind::Yield => ConstLattice::Overdefined,
        _ => ConstLattice::Overdefined,
    }
}

fn value_children(cpg: &CodePropertyGraph, node: NodeId) -> Vec<NodeId> {
    cpg.ast_children(node)
        .iter()
        .copied()
        .filter(|&child| is_value_cell(cpg, child))
        .collect()
}

pub(crate) fn literal_value(literal: &LiteralKind) -> ConstLattice {
    let value = match literal {
        LiteralKind::Integer(value) => ConstValue::Int(*value),
        LiteralKind::Float(value) => ConstValue::Float(value.to_bits()),
        LiteralKind::Bool(value) => ConstValue::Bool(*value),
        LiteralKind::String(value) => ConstValue::Str(value.clone()),
        LiteralKind::Char(value) => ConstValue::Char(*value),
        LiteralKind::Null => ConstValue::Null,
        LiteralKind::Array | LiteralKind::Object | LiteralKind::Regex(_) => {
            return ConstLattice::Overdefined;
        }
    };
    ConstLattice::Constant(value)
}

pub(crate) fn fold_unary(operator: &str, operand: &ConstLattice) -> ConstLattice {
    match operand {
        ConstLattice::Unknown => ConstLattice::Unknown,
        ConstLattice::Overdefined => ConstLattice::Overdefined,
        ConstLattice::Constant(ConstValue::Int(value)) => match operator {
            "+" => ConstLattice::Constant(ConstValue::Int(*value)),
            "-" => value
                .checked_neg()
                .map_or(ConstLattice::Overdefined, |value| {
                    ConstLattice::Constant(ConstValue::Int(value))
                }),
            "~" => ConstLattice::Constant(ConstValue::Int(!value)),
            _ => ConstLattice::Overdefined,
        },
        ConstLattice::Constant(ConstValue::Float(bits)) => match operator {
            "+" => ConstLattice::Constant(ConstValue::Float(*bits)),
            "-" => ConstLattice::Constant(ConstValue::Float((-f64::from_bits(*bits)).to_bits())),
            _ => ConstLattice::Overdefined,
        },
        ConstLattice::Constant(ConstValue::Bool(value)) => match operator {
            "!" | "not" => ConstLattice::Constant(ConstValue::Bool(!value)),
            _ => ConstLattice::Overdefined,
        },
        ConstLattice::Constant(_) => ConstLattice::Overdefined,
    }
}

pub(crate) fn fold_binary(
    operator: &str,
    left: &ConstLattice,
    right: &ConstLattice,
) -> ConstLattice {
    if matches!(operator, "&&" | "and")
        && (matches!(left, ConstLattice::Constant(ConstValue::Bool(false)))
            || matches!(right, ConstLattice::Constant(ConstValue::Bool(false))))
    {
        return ConstLattice::Constant(ConstValue::Bool(false));
    }
    if matches!(operator, "||" | "or")
        && (matches!(left, ConstLattice::Constant(ConstValue::Bool(true)))
            || matches!(right, ConstLattice::Constant(ConstValue::Bool(true))))
    {
        return ConstLattice::Constant(ConstValue::Bool(true));
    }
    match (left, right) {
        (ConstLattice::Overdefined, _) | (_, ConstLattice::Overdefined) => {
            ConstLattice::Overdefined
        }
        (ConstLattice::Unknown, _) | (_, ConstLattice::Unknown) => ConstLattice::Unknown,
        (ConstLattice::Constant(left), ConstLattice::Constant(right)) => {
            fold_constants(operator, left, right)
        }
    }
}

fn fold_constants(operator: &str, left: &ConstValue, right: &ConstValue) -> ConstLattice {
    if matches!(operator, "==" | "===") {
        return constants_equal(left, right).map_or(ConstLattice::Overdefined, |equal| {
            ConstLattice::Constant(ConstValue::Bool(equal))
        });
    }
    if matches!(operator, "!=" | "!==") {
        return constants_equal(left, right).map_or(ConstLattice::Overdefined, |equal| {
            ConstLattice::Constant(ConstValue::Bool(!equal))
        });
    }
    match (left, right) {
        (ConstValue::Int(left), ConstValue::Int(right)) => fold_ints(operator, *left, *right),
        (ConstValue::Float(left), ConstValue::Float(right)) => {
            fold_floats(operator, f64::from_bits(*left), f64::from_bits(*right))
        }
        (ConstValue::Bool(left), ConstValue::Bool(right)) => match operator {
            "&&" | "and" => ConstLattice::Constant(ConstValue::Bool(*left && *right)),
            "||" | "or" => ConstLattice::Constant(ConstValue::Bool(*left || *right)),
            "&" => ConstLattice::Constant(ConstValue::Bool(*left & *right)),
            "|" => ConstLattice::Constant(ConstValue::Bool(*left | *right)),
            "^" => ConstLattice::Constant(ConstValue::Bool(*left ^ *right)),
            _ => ConstLattice::Overdefined,
        },
        (ConstValue::Str(left), ConstValue::Str(right)) if operator == "+" => {
            let mut value = String::with_capacity(left.len().saturating_add(right.len()));
            value.push_str(left);
            value.push_str(right);
            ConstLattice::Constant(ConstValue::Str(Arc::from(value)))
        }
        (ConstValue::Char(left), ConstValue::Char(right)) => {
            fold_ordering(operator, left.cmp(right))
        }
        (ConstValue::Str(left), ConstValue::Str(right)) => fold_ordering(operator, left.cmp(right)),
        _ => ConstLattice::Overdefined,
    }
}

/// Compares constants only when the language-independent CPG value kinds agree.
///
/// Cross-kind equality is language-specific: for example, some frontends coerce
/// an integer and a float while others reject the expression. Returning `None`
/// keeps SCCP conservative instead of manufacturing a false constant.
fn constants_equal(left: &ConstValue, right: &ConstValue) -> Option<bool> {
    match (left, right) {
        (ConstValue::Float(left), ConstValue::Float(right)) => {
            Some(f64::from_bits(*left) == f64::from_bits(*right))
        }
        (ConstValue::Int(left), ConstValue::Int(right)) => Some(left == right),
        (ConstValue::Bool(left), ConstValue::Bool(right)) => Some(left == right),
        (ConstValue::Str(left), ConstValue::Str(right)) => Some(left == right),
        (ConstValue::Char(left), ConstValue::Char(right)) => Some(left == right),
        (ConstValue::Null, ConstValue::Null) => Some(true),
        _ => None,
    }
}

fn fold_ints(operator: &str, left: i64, right: i64) -> ConstLattice {
    let checked = match operator {
        "+" => left.checked_add(right),
        "-" => left.checked_sub(right),
        "*" => left.checked_mul(right),
        "/" => left.checked_div(right),
        "%" => left.checked_rem(right),
        "**" => u32::try_from(right)
            .ok()
            .and_then(|right| left.checked_pow(right)),
        "<<" => u32::try_from(right)
            .ok()
            .and_then(|right| left.checked_shl(right)),
        ">>" => u32::try_from(right)
            .ok()
            .and_then(|right| left.checked_shr(right)),
        "&" => Some(left & right),
        "|" => Some(left | right),
        "^" => Some(left ^ right),
        "<" => return ConstLattice::Constant(ConstValue::Bool(left < right)),
        "<=" => return ConstLattice::Constant(ConstValue::Bool(left <= right)),
        ">" => return ConstLattice::Constant(ConstValue::Bool(left > right)),
        ">=" => return ConstLattice::Constant(ConstValue::Bool(left >= right)),
        _ => return ConstLattice::Overdefined,
    };
    checked.map_or(ConstLattice::Overdefined, |value| {
        ConstLattice::Constant(ConstValue::Int(value))
    })
}

fn fold_floats(operator: &str, left: f64, right: f64) -> ConstLattice {
    let value = match operator {
        "+" => left + right,
        "-" => left - right,
        "*" => left * right,
        "/" => left / right,
        "%" => left % right,
        "**" => left.powf(right),
        "<" => return ConstLattice::Constant(ConstValue::Bool(left < right)),
        "<=" => return ConstLattice::Constant(ConstValue::Bool(left <= right)),
        ">" => return ConstLattice::Constant(ConstValue::Bool(left > right)),
        ">=" => return ConstLattice::Constant(ConstValue::Bool(left >= right)),
        _ => return ConstLattice::Overdefined,
    };
    ConstLattice::Constant(ConstValue::Float(value.to_bits()))
}

fn fold_ordering(operator: &str, ordering: std::cmp::Ordering) -> ConstLattice {
    let value = match operator {
        "<" => ordering.is_lt(),
        "<=" => ordering.is_le(),
        ">" => ordering.is_gt(),
        ">=" => ordering.is_ge(),
        _ => return ConstLattice::Overdefined,
    };
    ConstLattice::Constant(ConstValue::Bool(value))
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::{CpgNode, Language, MethodSignature, ScopeId, SourceRange, Visibility};

    fn node(id: u32, kind: CpgNodeKind) -> CpgNode {
        CpgNode::new(NodeId::new(id), kind, SourceRange::default())
    }

    fn function() -> CpgNodeKind {
        CpgNodeKind::Function {
            signature: Box::new(MethodSignature {
                name: "sccp_fixture".into(),
                params: Default::default(),
                return_type: None,
                is_static: false,
                is_async: false,
                visibility: Visibility::Private,
            }),
        }
    }

    fn block() -> CpgNodeKind {
        CpgNodeKind::Block {
            scope: ScopeId::GLOBAL,
        }
    }

    fn identifier(name: &str) -> CpgNodeKind {
        CpgNodeKind::Identifier {
            name: name.into(),
            definition: None,
        }
    }

    fn int(value: i64) -> CpgNodeKind {
        CpgNodeKind::Literal {
            kind: LiteralKind::Integer(value),
        }
    }

    fn connect_ast(cpg: &mut CodePropertyGraph, parent: NodeId, child: NodeId) {
        cpg.connect(parent, child, CpgEdgeKind::AstChild);
    }

    fn connect_cfg(cpg: &mut CodePropertyGraph, source: NodeId, target: NodeId, kind: CfgEdgeKind) {
        cpg.connect(source, target, CpgEdgeKind::ControlFlow(kind));
    }

    fn connect_def_use(cpg: &mut CodePropertyGraph, definition: NodeId, use_site: NodeId) {
        cpg.connect(
            definition,
            use_site,
            CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse),
        );
    }

    fn branch_fixture(value: bool, reverse: bool) -> (CodePropertyGraph, NodeId) {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let mut nodes = vec![
            node(0, function()),
            node(
                1,
                CpgNodeKind::Literal {
                    kind: LiteralKind::Bool(value),
                },
            ),
            node(2, block()),
            node(3, block()),
            node(4, block()),
        ];
        if reverse {
            nodes.reverse();
        }
        for node in nodes {
            cpg.add_node_with_id(node);
        }
        let function = NodeId::new(0);
        let guard = NodeId::new(1);
        let true_target = NodeId::new(2);
        let false_target = NodeId::new(3);
        let join = NodeId::new(4);
        for child in [guard, true_target, false_target, join] {
            connect_ast(&mut cpg, function, child);
        }
        let mut edges = vec![
            (function, guard, CfgEdgeKind::Sequential),
            (guard, true_target, CfgEdgeKind::ConditionalTrue),
            (guard, false_target, CfgEdgeKind::ConditionalFalse),
            (true_target, join, CfgEdgeKind::Sequential),
            (false_target, join, CfgEdgeKind::Sequential),
        ];
        if reverse {
            edges.reverse();
        }
        for (source, target, kind) in edges {
            connect_cfg(&mut cpg, source, target, kind);
        }
        (cpg, function)
    }

    fn phi_fixture(left: i64, right: i64) -> (CodePropertyGraph, NodeId, NodeId) {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let function = cpg.add_node_with_id(node(0, function()));
        let guard = cpg.add_node_with_id(node(1, identifier("external")));
        let left = cpg.add_node_with_id(node(2, int(left)));
        let right = cpg.add_node_with_id(node(3, int(right)));
        let use_site = cpg.add_node_with_id(node(4, identifier("x")));
        for child in [guard, left, right, use_site] {
            connect_ast(&mut cpg, function, child);
        }
        connect_cfg(&mut cpg, function, guard, CfgEdgeKind::Sequential);
        connect_cfg(&mut cpg, guard, left, CfgEdgeKind::ConditionalTrue);
        connect_cfg(&mut cpg, guard, right, CfgEdgeKind::ConditionalFalse);
        connect_cfg(&mut cpg, left, use_site, CfgEdgeKind::Sequential);
        connect_cfg(&mut cpg, right, use_site, CfgEdgeKind::Sequential);
        connect_def_use(&mut cpg, left, use_site);
        connect_def_use(&mut cpg, right, use_site);
        (cpg, function, use_site)
    }

    fn join(left: ConstLattice, right: &ConstLattice) -> ConstLattice {
        let mut result = left;
        result.join(right);
        result
    }

    fn arb_lattice() -> impl Strategy<Value = ConstLattice> {
        prop_oneof![
            Just(ConstLattice::Unknown),
            any::<i16>()
                .prop_map(|value| { ConstLattice::Constant(ConstValue::Int(i64::from(value))) }),
            any::<bool>().prop_map(|value| ConstLattice::Constant(ConstValue::Bool(value))),
            Just(ConstLattice::Overdefined),
        ]
    }

    #[test]
    fn constant_lattice_has_the_documented_height_two_order() {
        let one = ConstLattice::Constant(ConstValue::Int(1));
        let two = ConstLattice::Constant(ConstValue::Int(2));
        assert!(ConstLattice::Unknown.leq(&one));
        assert!(one.leq(&ConstLattice::Overdefined));
        assert!(!ConstLattice::Overdefined.leq(&one));
        assert_eq!(join(ConstLattice::Unknown, &one), one);
        assert_eq!(join(one, &two), ConstLattice::Overdefined);
    }

    #[test]
    fn constant_guard_prunes_one_edge_and_refines_reachability() {
        let (cpg, function) = branch_fixture(true, false);
        let before = (cpg.node_count(), cpg.edge_count());
        let result = sccp(&cpg, function).expect("valid SCCP fixture");
        let plain = crate::analysis::unreachable_code(&cpg, function).expect("plain reachability");
        let refined = plain
            .refine_with_sccp(&result)
            .expect("same-function refinement");

        assert_eq!(
            result.constants.get(&NodeId::new(1)),
            Some(&ConstValue::Bool(true))
        );
        assert!(result.executable_edges.contains(&(
            NodeId::new(1),
            NodeId::new(2),
            CfgEdgeKind::ConditionalTrue,
        )));
        assert_eq!(
            result.dead_edges,
            vec![(
                NodeId::new(1),
                NodeId::new(3),
                CfgEdgeKind::ConditionalFalse,
            )]
        );
        assert_eq!(result.unreachable_nodes, vec![NodeId::new(3)]);
        assert!(plain.unreachable.is_empty());
        assert_eq!(refined.unreachable, vec![NodeId::new(3)]);
        assert_eq!(refined.dead_branches.len(), 1);
        assert_eq!((cpg.node_count(), cpg.edge_count()), before);

        let conditional_executable: BTreeSet<_> = result
            .executable_edges
            .iter()
            .copied()
            .filter(|edge| edge.2.is_conditional())
            .map(|(source, target, kind)| {
                (
                    source,
                    target,
                    crate::analysis::cfg_scope::cfg_kind_rank(kind),
                )
            })
            .collect();
        let dead: BTreeSet<_> = result
            .dead_edges
            .iter()
            .map(|&(source, target, kind)| {
                (
                    source,
                    target,
                    crate::analysis::cfg_scope::cfg_kind_rank(kind),
                )
            })
            .collect();
        let all: BTreeSet<_> = cpg
            .edges()
            .filter_map(|edge| match edge.kind {
                CpgEdgeKind::ControlFlow(kind) if kind.is_conditional() => Some((
                    edge.source,
                    edge.target,
                    crate::analysis::cfg_scope::cfg_kind_rank(kind),
                )),
                _ => None,
            })
            .collect();
        assert!(conditional_executable.is_disjoint(&dead));
        assert_eq!(
            conditional_executable
                .union(&dead)
                .copied()
                .collect::<BTreeSet<_>>(),
            all
        );
    }

    #[test]
    fn overdefined_guard_executes_both_edges_and_phi_joins_definitions() {
        let (same, function, use_site) = phi_fixture(7, 7);
        let same_result = sccp(&same, function).expect("same constants");
        assert_eq!(
            same_result.constants.get(&use_site),
            Some(&ConstValue::Int(7))
        );
        assert!(same_result.dead_edges.is_empty());
        assert!(same_result.unreachable_nodes.is_empty());

        let (different, function, use_site) = phi_fixture(7, 9);
        let different_result = sccp(&different, function).expect("different constants");
        assert!(!different_result.constants.contains_key(&use_site));
        assert!(different_result.dead_edges.is_empty());
    }

    #[test]
    fn phi_ignores_a_definition_on_a_proven_dead_branch() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let function = cpg.add_node_with_id(node(0, function()));
        let guard = cpg.add_node_with_id(node(
            1,
            CpgNodeKind::Literal {
                kind: LiteralKind::Bool(true),
            },
        ));
        let live_definition = cpg.add_node_with_id(node(2, int(7)));
        let dead_definition = cpg.add_node_with_id(node(3, int(9)));
        let use_site = cpg.add_node_with_id(node(4, identifier("x")));
        for child in [guard, live_definition, dead_definition, use_site] {
            connect_ast(&mut cpg, function, child);
        }
        connect_cfg(&mut cpg, function, guard, CfgEdgeKind::Sequential);
        connect_cfg(
            &mut cpg,
            guard,
            live_definition,
            CfgEdgeKind::ConditionalTrue,
        );
        connect_cfg(
            &mut cpg,
            guard,
            dead_definition,
            CfgEdgeKind::ConditionalFalse,
        );
        connect_cfg(&mut cpg, live_definition, use_site, CfgEdgeKind::Sequential);
        connect_cfg(&mut cpg, dead_definition, use_site, CfgEdgeKind::Sequential);
        connect_def_use(&mut cpg, live_definition, use_site);
        connect_def_use(&mut cpg, dead_definition, use_site);

        let result = sccp(&cpg, function).expect("dead definition fixture");
        assert_eq!(result.constants.get(&use_site), Some(&ConstValue::Int(7)));
        assert!(!result.constants.contains_key(&dead_definition));
        assert!(result.unreachable_nodes.contains(&dead_definition));
    }

    #[test]
    fn scalar_literals_preserve_exact_values_and_aggregate_literals_do_not_fold() {
        assert_eq!(
            literal_value(&LiteralKind::Integer(-4)),
            ConstLattice::Constant(ConstValue::Int(-4))
        );
        assert_eq!(
            literal_value(&LiteralKind::Float(-0.0)),
            ConstLattice::Constant(ConstValue::Float((-0.0f64).to_bits()))
        );
        assert_eq!(
            literal_value(&LiteralKind::String(Arc::from("text"))),
            ConstLattice::Constant(ConstValue::Str(Arc::from("text")))
        );
        assert_eq!(
            literal_value(&LiteralKind::Char('λ')),
            ConstLattice::Constant(ConstValue::Char('λ'))
        );
        assert_eq!(
            literal_value(&LiteralKind::Bool(true)),
            ConstLattice::Constant(ConstValue::Bool(true))
        );
        assert_eq!(
            literal_value(&LiteralKind::Null),
            ConstLattice::Constant(ConstValue::Null)
        );
        for aggregate in [
            LiteralKind::Array,
            LiteralKind::Object,
            LiteralKind::Regex(Arc::from(".*")),
        ] {
            assert_eq!(literal_value(&aggregate), ConstLattice::Overdefined);
        }
    }

    #[test]
    fn calls_and_external_inputs_are_overdefined_not_false_constants() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let function = cpg.add_node_with_id(node(0, function()));
        let external = cpg.add_node_with_id(node(1, identifier("input")));
        let call = cpg.add_node_with_id(node(
            2,
            CpgNodeKind::Call {
                target: None,
                is_method: false,
            },
        ));
        connect_ast(&mut cpg, function, external);
        connect_ast(&mut cpg, function, call);
        connect_cfg(&mut cpg, function, external, CfgEdgeKind::Sequential);
        connect_cfg(&mut cpg, external, call, CfgEdgeKind::Sequential);

        let result = sccp(&cpg, function).expect("unknown values fixture");
        assert!(!result.constants.contains_key(&external));
        assert!(!result.constants.contains_key(&call));
        assert!(result.unreachable_nodes.is_empty());
    }

    #[test]
    fn checked_folding_never_turns_invalid_arithmetic_into_a_constant() {
        let max = ConstLattice::Constant(ConstValue::Int(i64::MAX));
        let one = ConstLattice::Constant(ConstValue::Int(1));
        let zero = ConstLattice::Constant(ConstValue::Int(0));
        let minus_one = ConstLattice::Constant(ConstValue::Int(-1));
        assert_eq!(fold_binary("+", &max, &one), ConstLattice::Overdefined);
        assert_eq!(fold_binary("/", &one, &zero), ConstLattice::Overdefined);
        assert_eq!(
            fold_binary(
                "/",
                &ConstLattice::Constant(ConstValue::Int(i64::MIN)),
                &minus_one,
            ),
            ConstLattice::Overdefined
        );
        assert_eq!(
            fold_binary("<<", &one, &minus_one),
            ConstLattice::Overdefined
        );
        assert_eq!(
            fold_unary("-", &ConstLattice::Constant(ConstValue::Int(i64::MIN)),),
            ConstLattice::Overdefined
        );
        assert_eq!(
            fold_unary("!", &ConstLattice::Constant(ConstValue::Int(1))),
            ConstLattice::Overdefined
        );
        assert_eq!(
            fold_unary("~", &ConstLattice::Constant(ConstValue::Int(1))),
            ConstLattice::Constant(ConstValue::Int(!1))
        );
    }

    #[test]
    fn boolean_absorption_and_float_equality_follow_runtime_semantics() {
        let unknown = ConstLattice::Unknown;
        let false_value = ConstLattice::Constant(ConstValue::Bool(false));
        let true_value = ConstLattice::Constant(ConstValue::Bool(true));
        assert_eq!(fold_binary("&&", &unknown, &false_value), false_value);
        assert_eq!(fold_binary("||", &unknown, &true_value), true_value);

        let negative_zero = ConstLattice::Constant(ConstValue::Float((-0.0f64).to_bits()));
        let positive_zero = ConstLattice::Constant(ConstValue::Float(0.0f64.to_bits()));
        assert_eq!(
            fold_binary("==", &negative_zero, &positive_zero),
            true_value
        );
        let nan = ConstLattice::Constant(ConstValue::Float(f64::NAN.to_bits()));
        assert_eq!(fold_binary("==", &nan, &nan), false_value);

        let integer_one = ConstLattice::Constant(ConstValue::Int(1));
        let float_one = ConstLattice::Constant(ConstValue::Float(1.0f64.to_bits()));
        assert_eq!(
            fold_binary("==", &integer_one, &float_one),
            ConstLattice::Overdefined
        );
        assert_eq!(
            fold_binary("!=", &integer_one, &float_one),
            ConstLattice::Overdefined
        );
    }

    #[test]
    fn scalar_operator_vocabulary_has_exact_conservative_transfers() {
        let int = |value| ConstLattice::Constant(ConstValue::Int(value));
        let float = |value: f64| ConstLattice::Constant(ConstValue::Float(value.to_bits()));
        let boolean = |value| ConstLattice::Constant(ConstValue::Bool(value));
        let string = |value: &str| ConstLattice::Constant(ConstValue::Str(Arc::from(value)));
        let character = |value| ConstLattice::Constant(ConstValue::Char(value));

        assert_eq!(fold_unary("+", &int(3)), int(3));
        assert_eq!(fold_unary("-", &int(3)), int(-3));
        assert_eq!(fold_unary("~", &int(3)), int(!3));
        assert_eq!(fold_unary("+", &float(2.5)), float(2.5));
        assert_eq!(fold_unary("-", &float(2.5)), float(-2.5));
        assert_eq!(fold_unary("!", &boolean(true)), boolean(false));
        assert_eq!(fold_unary("not", &boolean(false)), boolean(true));
        assert_eq!(fold_unary("-", &string("x")), ConstLattice::Overdefined);
        assert_eq!(fold_unary("?", &int(1)), ConstLattice::Overdefined);
        assert_eq!(fold_unary("?", &float(1.0)), ConstLattice::Overdefined);
        assert_eq!(fold_unary("?", &boolean(true)), ConstLattice::Overdefined);
        assert_eq!(
            fold_unary("-", &ConstLattice::Unknown),
            ConstLattice::Unknown
        );
        assert_eq!(
            fold_unary("-", &ConstLattice::Overdefined),
            ConstLattice::Overdefined
        );

        for (operator, expected) in [
            ("+", int(7)),
            ("-", int(3)),
            ("*", int(10)),
            ("/", int(2)),
            ("%", int(1)),
            ("**", int(25)),
            ("<<", int(20)),
            (">>", int(1)),
            ("&", int(0)),
            ("|", int(7)),
            ("^", int(7)),
            ("<", boolean(false)),
            ("<=", boolean(false)),
            (">", boolean(true)),
            (">=", boolean(true)),
        ] {
            assert_eq!(fold_binary(operator, &int(5), &int(2)), expected);
        }
        assert_eq!(
            fold_binary("?", &int(5), &int(2)),
            ConstLattice::Overdefined
        );

        for (operator, expected) in [
            ("+", float(6.0)),
            ("-", float(2.0)),
            ("*", float(8.0)),
            ("/", float(2.0)),
            ("%", float(0.0)),
            ("**", float(16.0)),
            ("<", boolean(false)),
            ("<=", boolean(false)),
            (">", boolean(true)),
            (">=", boolean(true)),
        ] {
            assert_eq!(fold_binary(operator, &float(4.0), &float(2.0)), expected);
        }
        assert_eq!(
            fold_binary("?", &float(4.0), &float(2.0)),
            ConstLattice::Overdefined
        );

        for (operator, expected) in [
            ("&&", boolean(false)),
            ("and", boolean(false)),
            ("||", boolean(true)),
            ("or", boolean(true)),
            ("&", boolean(false)),
            ("|", boolean(true)),
            ("^", boolean(true)),
        ] {
            assert_eq!(
                fold_binary(operator, &boolean(true), &boolean(false)),
                expected
            );
        }
        assert_eq!(
            fold_binary("+", &boolean(true), &boolean(false)),
            ConstLattice::Overdefined
        );

        assert_eq!(
            fold_binary("+", &string("ab"), &string("cd")),
            string("abcd")
        );
        assert_eq!(
            fold_binary("<", &string("ab"), &string("cd")),
            boolean(true)
        );
        assert_eq!(
            fold_binary(">=", &character('z'), &character('a')),
            boolean(true)
        );
        assert_eq!(
            fold_binary("+", &character('a'), &character('b')),
            ConstLattice::Overdefined
        );

        for value in [
            ConstValue::Int(1),
            ConstValue::Bool(true),
            ConstValue::Str(Arc::from("same")),
            ConstValue::Char('q'),
            ConstValue::Null,
        ] {
            let value = ConstLattice::Constant(value);
            assert_eq!(fold_binary("==", &value, &value), boolean(true));
            assert_eq!(fold_binary("!==", &value, &value), boolean(false));
        }

        assert_eq!(
            fold_binary("+", &ConstLattice::Overdefined, &int(1)),
            ConstLattice::Overdefined
        );
        assert_eq!(
            fold_binary("+", &int(1), &ConstLattice::Overdefined),
            ConstLattice::Overdefined
        );
        assert_eq!(
            fold_binary("+", &ConstLattice::Unknown, &int(1)),
            ConstLattice::Unknown
        );
        assert_eq!(
            fold_binary("+", &int(1), &ConstLattice::Unknown),
            ConstLattice::Unknown
        );
    }

    #[test]
    fn unary_compound_assignment_and_unsupported_cells_flow_end_to_end() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let function = cpg.add_node_with_id(node(0, function()));
        let variable = cpg.add_node_with_id(node(
            1,
            CpgNodeKind::Variable {
                name: Arc::from("x"),
                var_type: None,
                scope: ScopeId::GLOBAL,
                is_mutable: true,
            },
        ));
        let declared_name = cpg.add_node_with_id(node(2, identifier("x")));
        let initializer = cpg.add_node_with_id(node(3, int(5)));
        let unary = cpg.add_node_with_id(node(
            4,
            CpgNodeKind::UnaryOp {
                operator: Arc::from("-"),
            },
        ));
        let unary_operand = cpg.add_node_with_id(node(5, int(2)));
        let assignment = cpg.add_node_with_id(node(
            6,
            CpgNodeKind::Assignment {
                operator: Arc::from("+="),
            },
        ));
        let target = cpg.add_node_with_id(node(7, identifier("x")));
        let rhs = cpg.add_node_with_id(node(8, int(3)));
        let unsupported = [
            cpg.add_node_with_id(node(
                9,
                CpgNodeKind::MemberAccess {
                    member: Arc::from("field"),
                },
            )),
            cpg.add_node_with_id(node(10, CpgNodeKind::IndexAccess)),
            cpg.add_node_with_id(node(
                11,
                CpgNodeKind::Lambda {
                    captures: Default::default(),
                },
            )),
            cpg.add_node_with_id(node(12, CpgNodeKind::Await)),
            cpg.add_node_with_id(node(13, CpgNodeKind::Yield)),
        ];

        for child in [variable, unary, assignment].into_iter().chain(unsupported) {
            connect_ast(&mut cpg, function, child);
        }
        connect_ast(&mut cpg, variable, declared_name);
        connect_ast(&mut cpg, variable, initializer);
        connect_ast(&mut cpg, unary, unary_operand);
        connect_ast(&mut cpg, assignment, target);
        connect_ast(&mut cpg, assignment, rhs);
        connect_def_use(&mut cpg, variable, target);

        let mut flow = vec![variable, unary, assignment];
        flow.extend(unsupported);
        let mut source = function;
        for target_node in flow {
            connect_cfg(&mut cpg, source, target_node, CfgEdgeKind::Sequential);
            source = target_node;
        }

        let result = sccp(&cpg, function).expect("mixed expression fixture");
        assert_eq!(result.constants.get(&variable), Some(&ConstValue::Int(5)));
        assert_eq!(result.constants.get(&unary), Some(&ConstValue::Int(-2)));
        assert_eq!(result.constants.get(&target), Some(&ConstValue::Int(5)));
        assert_eq!(result.constants.get(&assignment), Some(&ConstValue::Int(8)));
        for node in unsupported {
            assert!(!result.constants.contains_key(&node));
        }
    }

    #[test]
    fn def_use_index_rejects_every_out_of_scope_or_non_value_endpoint() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let function = cpg.add_node_with_id(node(0, function()));
        let definition = cpg.add_node_with_id(node(1, int(7)));
        let use_site = cpg.add_node_with_id(node(2, identifier("x")));
        let non_value = cpg.add_node_with_id(node(3, block()));
        let outside_definition = cpg.add_node_with_id(node(4, int(9)));
        let outside_use = cpg.add_node_with_id(node(5, identifier("outside")));
        for child in [definition, use_site, non_value] {
            connect_ast(&mut cpg, function, child);
        }

        connect_def_use(&mut cpg, definition, use_site);
        connect_def_use(&mut cpg, definition, use_site);
        connect_def_use(&mut cpg, outside_definition, use_site);
        connect_def_use(&mut cpg, definition, outside_use);
        connect_def_use(&mut cpg, non_value, use_site);
        connect_def_use(&mut cpg, definition, non_value);

        let scope = function_scope_nodes(&cpg, function);
        let cells = BTreeSet::from([definition, use_site]);
        let (predecessors, successors, accepted_edge_count) = def_use_maps(&cpg, &scope, &cells);
        assert_eq!(predecessors, BTreeMap::from([(use_site, vec![definition])]));
        assert_eq!(successors, BTreeMap::from([(definition, vec![use_site])]));
        assert_eq!(accepted_edge_count, 2);
    }

    #[test]
    fn executable_node_scheduling_is_idempotent() {
        let owner = NodeId::new(7);
        let cell = NodeId::new(8);
        let mut executable_nodes = FxHashSet::default();
        let mut flow_work = BTreeSet::new();
        let mut value_work = BTreeSet::new();
        let cells_by_owner = BTreeMap::from([(owner, vec![cell])]);

        mark_node_executable(
            owner,
            &mut executable_nodes,
            &mut flow_work,
            &mut value_work,
            &cells_by_owner,
        );
        mark_node_executable(
            owner,
            &mut executable_nodes,
            &mut flow_work,
            &mut value_work,
            &cells_by_owner,
        );

        assert_eq!(executable_nodes, FxHashSet::from_iter([owner]));
        assert_eq!(flow_work, BTreeSet::from([owner]));
        assert_eq!(value_work, BTreeSet::from([cell]));
    }

    #[test]
    fn disconnected_cfg_islands_are_retained_as_unreachable_and_dead() {
        let (mut cpg, function) = branch_fixture(false, false);
        let island_guard = cpg.add_node_with_id(node(
            10,
            CpgNodeKind::Literal {
                kind: LiteralKind::Bool(true),
            },
        ));
        let island_target = cpg.add_node_with_id(node(11, block()));
        connect_ast(&mut cpg, function, island_guard);
        connect_ast(&mut cpg, function, island_target);
        connect_cfg(
            &mut cpg,
            island_guard,
            island_target,
            CfgEdgeKind::ConditionalTrue,
        );

        let result = sccp(&cpg, function).expect("scoped island");
        assert!(!result.constants.contains_key(&island_guard));
        assert!(result.unreachable_nodes.contains(&island_guard));
        assert!(result.unreachable_nodes.contains(&island_target));
        assert!(result.dead_edges.contains(&(
            island_guard,
            island_target,
            CfgEdgeKind::ConditionalTrue,
        )));
    }

    #[test]
    fn non_boolean_headers_and_case_edges_remain_conservatively_executable() {
        assert!(edge_is_executable(
            CfgEdgeKind::Case,
            Some(&ConstLattice::Unknown),
        ));
        assert!(edge_is_executable(
            CfgEdgeKind::DefaultCase,
            Some(&ConstLattice::Unknown),
        ));
        assert!(!edge_is_executable(
            CfgEdgeKind::ConditionalTrue,
            Some(&ConstLattice::Unknown),
        ));

        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let function_id = cpg.add_node_with_id(node(0, function()));
        let for_header = cpg.add_node_with_id(node(1, CpgNodeKind::For));
        let misleading_value = cpg.add_node_with_id(node(
            2,
            CpgNodeKind::Literal {
                kind: LiteralKind::Bool(false),
            },
        ));
        let body = cpg.add_node_with_id(node(3, block()));
        connect_ast(&mut cpg, function_id, for_header);
        connect_ast(&mut cpg, for_header, misleading_value);
        connect_ast(&mut cpg, for_header, body);
        connect_cfg(&mut cpg, function_id, for_header, CfgEdgeKind::Sequential);
        connect_cfg(&mut cpg, for_header, body, CfgEdgeKind::ConditionalTrue);

        let result = sccp(&cpg, function_id).expect("conservative for-header fixture");
        assert!(result.executable_edges.contains(&(
            for_header,
            body,
            CfgEdgeKind::ConditionalTrue,
        )));
        assert!(!result.unreachable_nodes.contains(&body));

        let mut wrapper_cpg = CodePropertyGraph::new(Language::Rust);
        let wrapper_function = wrapper_cpg.add_node_with_id(node(10, function()));
        let wrapper = wrapper_cpg.add_node_with_id(node(11, CpgNodeKind::If));
        let condition_container = wrapper_cpg.add_node_with_id(node(12, block()));
        let false_condition = wrapper_cpg.add_node_with_id(node(
            13,
            CpgNodeKind::Literal {
                kind: LiteralKind::Bool(false),
            },
        ));
        let true_target = wrapper_cpg.add_node_with_id(node(14, block()));
        let false_target = wrapper_cpg.add_node_with_id(node(15, block()));
        connect_ast(&mut wrapper_cpg, wrapper_function, wrapper);
        connect_ast(&mut wrapper_cpg, wrapper, condition_container);
        connect_ast(&mut wrapper_cpg, condition_container, false_condition);
        connect_ast(&mut wrapper_cpg, wrapper, true_target);
        connect_ast(&mut wrapper_cpg, wrapper, false_target);
        connect_cfg(
            &mut wrapper_cpg,
            wrapper_function,
            wrapper,
            CfgEdgeKind::Sequential,
        );
        connect_cfg(
            &mut wrapper_cpg,
            wrapper,
            true_target,
            CfgEdgeKind::ConditionalTrue,
        );
        connect_cfg(
            &mut wrapper_cpg,
            wrapper,
            false_target,
            CfgEdgeKind::ConditionalFalse,
        );

        let wrapper_result =
            sccp(&wrapper_cpg, wrapper_function).expect("wrapper-shaped guard fixture");
        assert!(wrapper_result.unreachable_nodes.contains(&true_target));
        assert!(!wrapper_result.unreachable_nodes.contains(&false_target));
    }

    #[test]
    fn invalid_selectors_and_cross_function_refinement_are_errors() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let first = cpg.add_node_with_id(node(0, function()));
        let second = cpg.add_node_with_id(node(1, function()));
        let block = cpg.add_node_with_id(node(2, block()));
        assert_eq!(
            sccp(&cpg, NodeId::new(99)),
            Err(AnalysisError::UnknownNode(NodeId::new(99)))
        );
        assert_eq!(sccp(&cpg, block), Err(AnalysisError::NotAFunction(block)));

        let report = crate::analysis::unreachable_code(&cpg, first).expect("first report");
        let second_result = sccp(&cpg, second).expect("second SCCP result");
        assert_eq!(
            report.refine_with_sccp(&second_result),
            Err(AnalysisError::FunctionMismatch {
                expected: first,
                actual: second,
            })
        );
    }

    #[test]
    fn malformed_ast_cycles_terminate_without_graph_mutation() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let function = cpg.add_node_with_id(node(0, function()));
        let guard = cpg.add_node_with_id(node(1, identifier("external")));
        let wrapper = cpg.add_node_with_id(node(2, block()));
        connect_ast(&mut cpg, function, wrapper);
        connect_ast(&mut cpg, wrapper, guard);
        connect_ast(&mut cpg, guard, wrapper);
        connect_cfg(&mut cpg, function, guard, CfgEdgeKind::Sequential);
        let before = (cpg.node_count(), cpg.edge_count());
        let result = sccp(&cpg, function).expect("cycle-safe SCCP");
        assert!(result.unreachable_nodes.is_empty());
        assert_eq!((cpg.node_count(), cpg.edge_count()), before);
    }

    #[derive(Debug, Clone)]
    struct StmtSpec {
        operator: &'static str,
        operand: i8,
    }

    fn straight_line_fixture(
        initial: i8,
        specs: &[StmtSpec],
    ) -> (CodePropertyGraph, NodeId, Vec<(NodeId, i64)>) {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let function = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            function(),
            SourceRange::default(),
        ));
        let definition = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Variable {
                name: "x".into(),
                var_type: None,
                scope: ScopeId::GLOBAL,
                is_mutable: true,
            },
            SourceRange::default(),
        ));
        let initial_literal = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            int(i64::from(initial)),
            SourceRange::default(),
        ));
        connect_ast(&mut cpg, function, definition);
        connect_ast(&mut cpg, definition, initial_literal);
        connect_cfg(&mut cpg, function, definition, CfgEdgeKind::Sequential);

        let mut current_definition = definition;
        let mut current_cfg = definition;
        let mut concrete = i64::from(initial);
        let mut expected = vec![(definition, concrete)];
        for spec in specs {
            let assignment = cpg.add_node(CpgNode::new(
                NodeId::new(0),
                CpgNodeKind::Assignment {
                    operator: "=".into(),
                },
                SourceRange::default(),
            ));
            let target = cpg.add_node(CpgNode::new(
                NodeId::new(0),
                identifier("x"),
                SourceRange::default(),
            ));
            let binary = cpg.add_node(CpgNode::new(
                NodeId::new(0),
                CpgNodeKind::BinaryOp {
                    operator: spec.operator.into(),
                },
                SourceRange::default(),
            ));
            let use_site = cpg.add_node(CpgNode::new(
                NodeId::new(0),
                identifier("x"),
                SourceRange::default(),
            ));
            let literal = cpg.add_node(CpgNode::new(
                NodeId::new(0),
                int(i64::from(spec.operand)),
                SourceRange::default(),
            ));
            connect_ast(&mut cpg, function, assignment);
            connect_ast(&mut cpg, assignment, target);
            connect_ast(&mut cpg, assignment, binary);
            connect_ast(&mut cpg, binary, use_site);
            connect_ast(&mut cpg, binary, literal);
            connect_cfg(&mut cpg, current_cfg, assignment, CfgEdgeKind::Sequential);
            connect_def_use(&mut cpg, current_definition, use_site);

            concrete = match spec.operator {
                "+" => concrete + i64::from(spec.operand),
                "-" => concrete - i64::from(spec.operand),
                "^" => concrete ^ i64::from(spec.operand),
                _ => unreachable!("generated operator vocabulary is closed"),
            };
            expected.push((assignment, concrete));
            current_definition = assignment;
            current_cfg = assignment;
        }
        (cpg, function, expected)
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(96))]

        #[test]
        fn lattice_join_is_commutative_associative_and_idempotent(
            left in arb_lattice(),
            middle in arb_lattice(),
            right in arb_lattice(),
        ) {
            prop_assert_eq!(join(left.clone(), &left), left.clone());
            prop_assert_eq!(join(left.clone(), &middle), join(middle.clone(), &left));
            prop_assert_eq!(
                join(join(left.clone(), &middle), &right),
                join(left, &join(middle, &right)),
            );
        }

        #[test]
        fn straight_line_literal_programs_match_the_concrete_interpreter(
            initial in any::<i8>(),
            raw in prop::collection::vec((0u8..3, any::<i8>()), 0..12),
        ) {
            let specs: Vec<_> = raw
                .into_iter()
                .map(|(operator, operand)| StmtSpec {
                    operator: match operator { 0 => "+", 1 => "-", _ => "^" },
                    operand,
                })
                .collect();
            let (cpg, function, expected) = straight_line_fixture(initial, &specs);
            let before = (cpg.node_count(), cpg.edge_count());
            let result = sccp(&cpg, function).expect("generated straight-line SCCP");
            for (definition, concrete) in expected {
                prop_assert_eq!(
                    result.constants.get(&definition),
                    Some(&ConstValue::Int(concrete)),
                );
            }
            prop_assert_eq!((cpg.node_count(), cpg.edge_count()), before);
        }

        #[test]
        fn result_is_invariant_under_node_and_non_ast_edge_insertion_order(
            value in any::<bool>(),
        ) {
            let (normal, function) = branch_fixture(value, false);
            let (reversed, reversed_function) = branch_fixture(value, true);
            let result = sccp(&normal, function).expect("normal insertion");
            let reversed_result =
                sccp(&reversed, reversed_function).expect("reversed insertion");
            prop_assert_eq!(&result, &reversed_result);

            let executable: BTreeSet<_> = result
                .executable_edges
                .iter()
                .copied()
                .filter(|edge| edge.2.is_conditional())
                .map(|(source, target, kind)| {
                    (
                        source,
                        target,
                        crate::analysis::cfg_scope::cfg_kind_rank(kind),
                    )
                })
                .collect();
            let dead: BTreeSet<_> = result
                .dead_edges
                .iter()
                .map(|&(source, target, kind)| {
                    (
                        source,
                        target,
                        crate::analysis::cfg_scope::cfg_kind_rank(kind),
                    )
                })
                .collect();
            let all: BTreeSet<_> = normal
                .edges()
                .filter_map(|edge| match edge.kind {
                    CpgEdgeKind::ControlFlow(kind) if kind.is_conditional() => {
                        Some((
                            edge.source,
                            edge.target,
                            crate::analysis::cfg_scope::cfg_kind_rank(kind),
                        ))
                    }
                    _ => None,
                })
                .collect();
            prop_assert!(executable.is_disjoint(&dead));
            prop_assert_eq!(
                executable.union(&dead).copied().collect::<BTreeSet<_>>(),
                all,
            );

            let structural = crate::analysis::unreachable_code(&normal, function)
                .expect("structural reachability");
            let refined = structural
                .refine_with_sccp(&result)
                .expect("same-function refinement");
            let structural_unreachable: BTreeSet<_> = structural.unreachable.into_iter().collect();
            let refined_unreachable: BTreeSet<_> = refined.unreachable.into_iter().collect();
            prop_assert!(structural_unreachable.is_subset(&refined_unreachable));
        }
    }

    #[cfg(feature = "serde")]
    #[test]
    fn public_values_and_result_round_trip_through_json() {
        let (cpg, function) = branch_fixture(true, false);
        let result = sccp(&cpg, function).expect("SCCP result");
        let encoded = serde_json::to_string(&result).expect("serialize SCCP result");
        let decoded: SccpResult = serde_json::from_str(&encoded).expect("deserialize SCCP result");
        assert_eq!(decoded, result);

        let lattice = ConstLattice::Constant(ConstValue::Str(Arc::from("stable")));
        let encoded = serde_json::to_string(&lattice).expect("serialize lattice");
        let decoded: ConstLattice = serde_json::from_str(&encoded).expect("deserialize lattice");
        assert_eq!(decoded, lattice);
    }
}
