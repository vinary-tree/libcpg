//! Forward interval abstract interpretation for integer value ranges.
//!
//! The analysis instantiates the crate's deterministic monotone data-flow
//! solver with a pointwise environment lattice. It interprets already parsed
//! CPG nodes; parsing remains the responsibility of the tree-sitter frontends.
//! Loop-header widening guarantees convergence over the infinite-height
//! interval domain, and deterministic descending sweeps recover precision.

use std::{
    cmp::Ordering,
    collections::{BTreeMap, HashSet},
    sync::Arc,
};

use smallvec::SmallVec;

use crate::{CfgEdgeKind, CodePropertyGraph, CpgNodeKind, LiteralKind, NodeId};

use super::{
    solve, variable_facts, Confluence, DataflowAnalysis, DataflowSolution, Direction, Lattice,
    SolverConfig, VarFacts,
};
use crate::analysis::{block_cfg, loop_forest, AnalysisError, BlockCfg, BlockEdge, BlockId};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// One endpoint in the extended signed-integer number line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Bound {
    /// Negative infinity.
    NegInf,
    /// A finite signed 128-bit endpoint.
    Finite(i128),
    /// Positive infinity.
    PosInf,
}

/// A non-empty closed interval over signed integers.
///
/// Values produced by the constructors always satisfy `lo <= hi`. The public
/// fields make reports straightforward to inspect and serialize; callers that
/// construct intervals should use [`Interval::new`] to preserve the invariant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Interval {
    /// Inclusive lower endpoint.
    pub lo: Bound,
    /// Inclusive upper endpoint.
    pub hi: Bound,
}

impl Interval {
    /// Constructs a non-empty interval when the endpoints are ordered.
    ///
    /// Positive infinity cannot be a lower endpoint and negative infinity
    /// cannot be an upper endpoint because neither infinity is a concrete
    /// signed integer.
    pub const fn new(lo: Bound, hi: Bound) -> Option<Self> {
        if !matches!(lo, Bound::PosInf) && !matches!(hi, Bound::NegInf) && bound_leq(lo, hi) {
            Some(Self { lo, hi })
        } else {
            None
        }
    }

    /// Returns the interval containing every signed integer.
    pub const fn top() -> Self {
        Self {
            lo: Bound::NegInf,
            hi: Bound::PosInf,
        }
    }

    /// Returns the interval containing exactly `value`.
    pub const fn singleton(value: i128) -> Self {
        Self {
            lo: Bound::Finite(value),
            hi: Bound::Finite(value),
        }
    }

    /// Returns the least interval containing `self` and `other`.
    pub fn hull(self, other: Self) -> Self {
        Self {
            lo: self.lo.min(other.lo),
            hi: self.hi.max(other.hi),
        }
    }

    /// Applies the standard interval widening operator.
    ///
    /// A lower endpoint that moves down becomes negative infinity; an upper
    /// endpoint that moves up becomes positive infinity.
    pub fn widen(self, other: Self) -> Self {
        Self {
            lo: if other.lo < self.lo {
                Bound::NegInf
            } else {
                self.lo
            },
            hi: if other.hi > self.hi {
                Bound::PosInf
            } else {
                self.hi
            },
        }
    }

    /// Descends toward `other` without ever enlarging `self`.
    ///
    /// Infinite endpoints are replaced by the corresponding endpoint from
    /// `other`; finite endpoints are retained. If the resulting bounds would
    /// be empty, `self` is returned because this type represents only
    /// non-empty intervals.
    pub fn narrow(self, other: Self) -> Self {
        let candidate = Self {
            lo: if self.lo == Bound::NegInf {
                other.lo
            } else {
                self.lo
            },
            hi: if self.hi == Bound::PosInf {
                other.hi
            } else {
                self.hi
            },
        };
        Self::new(candidate.lo, candidate.hi).unwrap_or(self)
    }

    /// Returns the non-empty intersection, if one exists.
    pub fn meet(self, other: Self) -> Option<Self> {
        Self::new(self.lo.max(other.lo), self.hi.min(other.hi))
    }

    /// Returns whether `value` belongs to this interval.
    pub fn contains(self, value: i128) -> bool {
        endpoint_contains_lower(self.lo, value) && endpoint_contains_upper(self.hi, value)
    }

    /// Returns whether every value in `self` also belongs to `other`.
    pub fn is_subset_of(self, other: Self) -> bool {
        other.lo <= self.lo && self.hi <= other.hi
    }

    fn shift(self, amount: i128) -> Self {
        let candidate = Self {
            lo: add_finite(self.lo, amount),
            hi: add_finite(self.hi, amount),
        };
        Self::new(candidate.lo, candidate.hi).unwrap_or_else(Self::top)
    }

    fn negate(self) -> Self {
        let candidate = Self {
            lo: negate_bound(self.hi),
            hi: negate_bound(self.lo),
        };
        Self::new(candidate.lo, candidate.hi).unwrap_or_else(Self::top)
    }
}

const fn bound_leq(left: Bound, right: Bound) -> bool {
    match (left, right) {
        (Bound::NegInf, _) | (_, Bound::PosInf) => true,
        (Bound::Finite(left), Bound::Finite(right)) => left <= right,
        (Bound::Finite(_) | Bound::PosInf, Bound::NegInf) | (Bound::PosInf, Bound::Finite(_)) => {
            false
        }
    }
}

fn endpoint_contains_lower(bound: Bound, value: i128) -> bool {
    match bound {
        Bound::NegInf => true,
        Bound::Finite(lower) => lower <= value,
        Bound::PosInf => false,
    }
}

fn endpoint_contains_upper(bound: Bound, value: i128) -> bool {
    match bound {
        Bound::PosInf => true,
        Bound::Finite(upper) => value <= upper,
        Bound::NegInf => false,
    }
}

fn overflow_bound(negative: bool) -> Bound {
    if negative {
        Bound::NegInf
    } else {
        Bound::PosInf
    }
}

fn add_finite(bound: Bound, amount: i128) -> Bound {
    match bound {
        Bound::NegInf => Bound::NegInf,
        Bound::PosInf => Bound::PosInf,
        Bound::Finite(value) => value
            .checked_add(amount)
            .map(Bound::Finite)
            .unwrap_or_else(|| overflow_bound(amount.is_negative())),
    }
}

fn negate_bound(bound: Bound) -> Bound {
    match bound {
        Bound::NegInf => Bound::PosInf,
        Bound::PosInf => Bound::NegInf,
        Bound::Finite(value) => value
            .checked_neg()
            .map(Bound::Finite)
            .unwrap_or(Bound::PosInf),
    }
}

/// A canonical variable-id-to-interval environment.
///
/// Entries are sorted by dense variable id and never duplicated. An empty
/// environment is lattice bottom (no executable state); absent individual
/// variables are also bottom until joined with a state that contains them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct IntervalEnv {
    /// Sorted `(variable id, interval)` pairs.
    pub vars: SmallVec<[(u32, Interval); 8]>,
}

impl IntervalEnv {
    /// Returns lattice bottom.
    pub fn bottom() -> Self {
        Self {
            vars: SmallVec::new(),
        }
    }

    /// Creates a canonical environment from arbitrary pairs.
    pub fn from_vars(vars: impl IntoIterator<Item = (u32, Interval)>) -> Self {
        let mut vars: SmallVec<[(u32, Interval); 8]> = vars.into_iter().collect();
        vars.sort_unstable_by_key(|(variable, _)| *variable);
        let mut result = Self::bottom();
        for (variable, interval) in vars {
            if let Some((last_variable, last_interval)) = result.vars.last_mut() {
                if *last_variable == variable {
                    *last_interval = last_interval.hull(interval);
                    continue;
                }
            }
            result.vars.push((variable, interval));
        }
        result
    }

    /// Returns whether the environment is lattice bottom.
    pub fn is_bottom(&self) -> bool {
        self.vars.is_empty()
    }

    /// Returns the interval for `variable`, if present.
    pub fn get(&self, variable: u32) -> Option<Interval> {
        self.vars
            .binary_search_by_key(&variable, |(candidate, _)| *candidate)
            .ok()
            .map(|index| self.vars[index].1)
    }

    /// Replaces the interval for `variable` while preserving canonical order.
    pub fn set(&mut self, variable: u32, interval: Interval) {
        match self
            .vars
            .binary_search_by_key(&variable, |(candidate, _)| *candidate)
        {
            Ok(index) => self.vars[index].1 = interval,
            Err(index) => self.vars.insert(index, (variable, interval)),
        }
    }

    fn meet_variable(&mut self, variable: u32, constraint: Interval) {
        if self.is_bottom() {
            return;
        }
        let current = self.get(variable).unwrap_or_else(Interval::top);
        if let Some(refined) = current.meet(constraint) {
            self.set(variable, refined);
        } else {
            self.vars.clear();
        }
    }

    fn widen_with(&mut self, other: &Self) -> bool {
        if self.is_bottom() {
            if other.is_bottom() {
                return false;
            }
            *self = other.clone();
            return true;
        }
        let before = self.clone();
        for &(variable, incoming) in &other.vars {
            match self.get(variable) {
                Some(current) => self.set(variable, current.widen(incoming)),
                None => self.set(variable, incoming),
            }
        }
        *self != before
    }
}

impl Lattice for IntervalEnv {
    fn join(&mut self, other: &Self) -> bool {
        if self.is_bottom() {
            if other.is_bottom() {
                return false;
            }
            *self = other.clone();
            return true;
        }
        let before = self.clone();
        for &(variable, incoming) in &other.vars {
            match self.get(variable) {
                Some(current) => self.set(variable, current.hull(incoming)),
                None => self.set(variable, incoming),
            }
        }
        *self != before
    }

    fn leq(&self, other: &Self) -> bool {
        self.vars.iter().all(|&(variable, interval)| {
            other
                .get(variable)
                .is_some_and(|candidate| interval.is_subset_of(candidate))
        })
    }
}

/// Integer intervals at every reachable basic-block entry in one function.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct IntervalReport {
    /// Function whose control-flow graph was analyzed.
    pub function: NodeId,
    /// Canonical variable-name intervals keyed by basic-block leader.
    pub at_block_entry: BTreeMap<NodeId, Vec<(Arc<str>, Interval)>>,
}

impl IntervalReport {
    /// Returns the interval for `variable` at the block led by `entry`.
    pub fn interval_at(&self, entry: NodeId, variable: &str) -> Option<Interval> {
        self.at_block_entry.get(&entry).and_then(|entries| {
            entries
                .binary_search_by(|(name, _)| name.as_ref().cmp(variable))
                .ok()
                .map(|index| entries[index].1)
        })
    }
}

struct IntervalAnalysis<'facts> {
    facts: &'facts VarFacts,
}

impl IntervalAnalysis<'_> {
    fn transfer_node(&self, cpg: &CodePropertyGraph, node: NodeId, state: &mut IntervalEnv) {
        if state.is_bottom() {
            return;
        }
        for definition in self.facts.definitions_at(node) {
            let interval = self.definition_value(cpg, node, definition.variable, state);
            state.set(definition.variable, interval);
        }
    }

    fn definition_value(
        &self,
        cpg: &CodePropertyGraph,
        node: NodeId,
        variable: u32,
        state: &IntervalEnv,
    ) -> Interval {
        match cpg.node(node).map(|node| &node.kind) {
            Some(CpgNodeKind::Variable { name, .. }) => declaration_initializer(cpg, node, name)
                .map_or_else(Interval::top, |initializer| {
                    self.eval_expr(cpg, initializer, state)
                }),
            Some(CpgNodeKind::Assignment { operator }) => {
                let children = cpg.ast_children(node);
                let rhs = children.get(1).copied();
                if &**operator == "=" {
                    rhs.map_or_else(Interval::top, |rhs| self.eval_expr(cpg, rhs, state))
                } else {
                    let current = state.get(variable).unwrap_or_else(Interval::top);
                    let operand =
                        rhs.map_or_else(Interval::top, |rhs| self.eval_expr(cpg, rhs, state));
                    match (&**operator, singleton_value(operand)) {
                        ("+=", Some(amount)) => current.shift(amount),
                        ("-=", Some(amount)) => current.shift(amount.saturating_neg()),
                        _ => Interval::top(),
                    }
                }
            }
            Some(CpgNodeKind::Parameter { .. }) => Interval::top(),
            _ => Interval::top(),
        }
    }

    fn eval_expr(&self, cpg: &CodePropertyGraph, root: NodeId, state: &IntervalEnv) -> Interval {
        let mut frames = Vec::new();
        let mut values = Vec::new();
        let mut on_path = HashSet::new();
        if !push_interval_frame(&mut frames, IntervalExprFrame::Enter(root)) {
            return Interval::top();
        }

        while let Some(frame) = frames.pop() {
            match frame {
                IntervalExprFrame::Enter(node) => {
                    if on_path.contains(&node) {
                        if !push_interval_value(&mut values, Interval::top()) {
                            return Interval::top();
                        }
                        continue;
                    }
                    if on_path.try_reserve(1).is_err() {
                        return Interval::top();
                    }
                    on_path.insert(node);
                    let next = match cpg.node(node).map(|node| &node.kind) {
                        Some(CpgNodeKind::Literal {
                            kind: LiteralKind::Integer(value),
                        }) => IntervalExprFrame::Exit {
                            node,
                            value: Interval::singleton(i128::from(*value)),
                        },
                        Some(CpgNodeKind::Identifier { name, .. }) => IntervalExprFrame::Exit {
                            node,
                            value: variable_id(self.facts, name)
                                .and_then(|variable| state.get(variable))
                                .unwrap_or_else(Interval::top),
                        },
                        Some(CpgNodeKind::UnaryOp { operator }) => {
                            let Some(operand) = cpg.ast_children(node).first().copied() else {
                                if !push_interval_frame(
                                    &mut frames,
                                    IntervalExprFrame::Exit {
                                        node,
                                        value: Interval::top(),
                                    },
                                ) {
                                    return Interval::top();
                                }
                                continue;
                            };
                            if !push_interval_frame(
                                &mut frames,
                                IntervalExprFrame::Unary {
                                    node,
                                    operator: operator.clone(),
                                },
                            ) || !push_interval_frame(
                                &mut frames,
                                IntervalExprFrame::Enter(operand),
                            ) {
                                return Interval::top();
                            }
                            continue;
                        }
                        Some(CpgNodeKind::BinaryOp { operator }) => {
                            let children = cpg.ast_children(node);
                            let [left, right, ..] = children.as_slice() else {
                                if !push_interval_frame(
                                    &mut frames,
                                    IntervalExprFrame::Exit {
                                        node,
                                        value: Interval::top(),
                                    },
                                ) {
                                    return Interval::top();
                                }
                                continue;
                            };
                            if !push_interval_frame(
                                &mut frames,
                                IntervalExprFrame::BinaryLeft {
                                    node,
                                    operator: operator.clone(),
                                    right: *right,
                                },
                            ) || !push_interval_frame(
                                &mut frames,
                                IntervalExprFrame::Enter(*left),
                            ) {
                                return Interval::top();
                            }
                            continue;
                        }
                        _ => IntervalExprFrame::Exit {
                            node,
                            value: Interval::top(),
                        },
                    };
                    if !push_interval_frame(&mut frames, next) {
                        return Interval::top();
                    }
                }
                IntervalExprFrame::Unary { node, operator } => {
                    let operand = values.pop().unwrap_or_else(Interval::top);
                    let value = match &*operator {
                        "+" => operand,
                        "-" => operand.negate(),
                        _ => Interval::top(),
                    };
                    if !push_interval_frame(&mut frames, IntervalExprFrame::Exit { node, value }) {
                        return Interval::top();
                    }
                }
                IntervalExprFrame::BinaryLeft {
                    node,
                    operator,
                    right,
                } => {
                    let left = values.pop().unwrap_or_else(Interval::top);
                    if !push_interval_frame(
                        &mut frames,
                        IntervalExprFrame::BinaryRight {
                            node,
                            operator,
                            left,
                        },
                    ) || !push_interval_frame(&mut frames, IntervalExprFrame::Enter(right))
                    {
                        return Interval::top();
                    }
                }
                IntervalExprFrame::BinaryRight {
                    node,
                    operator,
                    left,
                } => {
                    let right = values.pop().unwrap_or_else(Interval::top);
                    let value = match (&*operator, singleton_value(right), singleton_value(left)) {
                        ("+", Some(amount), _) => left.shift(amount),
                        ("+", _, Some(amount)) => right.shift(amount),
                        ("-", Some(amount), _) => left.shift(amount.saturating_neg()),
                        _ => Interval::top(),
                    };
                    if !push_interval_frame(&mut frames, IntervalExprFrame::Exit { node, value }) {
                        return Interval::top();
                    }
                }
                IntervalExprFrame::Exit { node, value } => {
                    on_path.remove(&node);
                    if !push_interval_value(&mut values, value) {
                        return Interval::top();
                    }
                }
            }
        }
        values.pop().unwrap_or_else(Interval::top)
    }

    fn edge_constraint(
        &self,
        cpg: &CodePropertyGraph,
        cfg: &BlockCfg,
        edge: &BlockEdge,
    ) -> Option<(u32, Interval)> {
        let truth = match edge.kind {
            CfgEdgeKind::ConditionalTrue => true,
            CfgEdgeKind::ConditionalFalse => false,
            _ => return None,
        };
        let source = cfg.block(edge.source)?.nodes.last().copied()?;
        let comparison = condition_expression(cpg, source)?;
        let CpgNodeKind::BinaryOp { operator } = &cpg.node(comparison)?.kind else {
            return None;
        };
        let children = cpg.ast_children(comparison);
        let [left, right, ..] = children.as_slice() else {
            return None;
        };
        let left_kind = &cpg.node(*left)?.kind;
        let right_kind = &cpg.node(*right)?.kind;
        let (name, constant, normalized_operator) = match (left_kind, right_kind) {
            (
                CpgNodeKind::Identifier { name, .. },
                CpgNodeKind::Literal {
                    kind: LiteralKind::Integer(constant),
                },
            ) => (name, i128::from(*constant), &**operator),
            (
                CpgNodeKind::Literal {
                    kind: LiteralKind::Integer(constant),
                },
                CpgNodeKind::Identifier { name, .. },
            ) => (name, i128::from(*constant), reverse_comparison(operator)?),
            _ => return None,
        };
        let variable = variable_id(self.facts, name)?;
        comparison_interval(normalized_operator, constant, truth)
            .map(|interval| (variable, interval))
    }
}

enum IntervalExprFrame {
    Enter(NodeId),
    Unary {
        node: NodeId,
        operator: Arc<str>,
    },
    BinaryLeft {
        node: NodeId,
        operator: Arc<str>,
        right: NodeId,
    },
    BinaryRight {
        node: NodeId,
        operator: Arc<str>,
        left: Interval,
    },
    Exit {
        node: NodeId,
        value: Interval,
    },
}

fn push_interval_frame(frames: &mut Vec<IntervalExprFrame>, frame: IntervalExprFrame) -> bool {
    if frames.try_reserve(1).is_err() {
        return false;
    }
    frames.push(frame);
    true
}

fn push_interval_value(values: &mut Vec<Interval>, value: Interval) -> bool {
    if values.try_reserve(1).is_err() {
        return false;
    }
    values.push(value);
    true
}

impl DataflowAnalysis for IntervalAnalysis<'_> {
    type Domain = IntervalEnv;

    const NAME: &'static str = "interval-analysis";
    const DIRECTION: Direction = Direction::Forward;
    const CONFLUENCE: Confluence = Confluence::May;

    fn bottom(&self, _cfg: &BlockCfg) -> Self::Domain {
        IntervalEnv::bottom()
    }

    fn boundary(&self, _cfg: &BlockCfg) -> Self::Domain {
        IntervalEnv::from_vars(
            self.facts
                .universe
                .iter()
                .map(|(variable, _)| (variable, Interval::top())),
        )
    }

    fn transfer_block(
        &self,
        cpg: &CodePropertyGraph,
        cfg: &BlockCfg,
        block: BlockId,
        state: &mut Self::Domain,
    ) {
        for &node in &cfg
            .block(block)
            .expect("the solver transfers only blocks in its BlockCfg")
            .nodes
        {
            self.transfer_node(cpg, node, state);
        }
    }

    fn transfer_edge(
        &self,
        cpg: &CodePropertyGraph,
        cfg: &BlockCfg,
        edge: &BlockEdge,
        state: &Self::Domain,
    ) -> Option<Self::Domain> {
        let (variable, constraint) = self.edge_constraint(cpg, cfg, edge)?;
        let mut refined = state.clone();
        refined.meet_variable(variable, constraint);
        Some(refined)
    }

    fn widen(&self, into: &mut Self::Domain, from: &Self::Domain) -> bool {
        into.widen_with(from)
    }
}

/// Computes sound integer value ranges at each reachable block entry.
///
/// Integer literals become singletons; simple assignments evaluate identifier
/// reads and `+`/`-` shifts; compound `+=` and `-=` assignments shift the
/// current range. Unsupported or language-dependent right-hand sides become
/// [`Interval::top`]. True and false edges from integer comparisons intersect
/// the compared variable with the corresponding half-line.
///
/// The analysis uses a 64-visit hard cap, begins widening after the third
/// visit only at headers reported by [`loop_forest`], and then performs two
/// deterministic narrowing sweeps. It is feature-free, read-only, and stores
/// every result outside the CPG.
pub fn value_ranges(
    cpg: &CodePropertyGraph,
    function: NodeId,
) -> Result<IntervalReport, AnalysisError> {
    let (cfg, facts, solution) = solve_function(cpg, function)?;

    let mut at_block_entry = BTreeMap::new();
    for block in &cfg.blocks {
        let state = &solution.before[block.id.0 as usize];
        if state.is_bottom() {
            continue;
        }
        let values = state
            .vars
            .iter()
            .filter_map(|&(variable, interval)| {
                facts.name(variable).map(|name| (name.clone(), interval))
            })
            .collect();
        at_block_entry.insert(block.leader, values);
    }
    Ok(IntervalReport {
        function,
        at_block_entry,
    })
}

fn solve_function(
    cpg: &CodePropertyGraph,
    function: NodeId,
) -> Result<(BlockCfg, VarFacts, DataflowSolution<IntervalEnv>), AnalysisError> {
    let cfg = block_cfg(cpg, function)?;
    let facts = variable_facts(cpg, function);
    let loops = loop_forest(cpg, function)?;
    let analysis = IntervalAnalysis { facts: &facts };
    let solution = solve(
        cpg,
        &cfg,
        &analysis,
        &SolverConfig {
            max_visits_per_block: 64,
            widen_after: Some(3),
            narrow_passes: 2,
        },
        Some(&loops),
    )?;
    Ok((cfg, facts, solution))
}

fn singleton_value(interval: Interval) -> Option<i128> {
    match (interval.lo, interval.hi) {
        (Bound::Finite(left), Bound::Finite(right)) if left == right => Some(left),
        _ => None,
    }
}

fn variable_id(facts: &VarFacts, name: &str) -> Option<u32> {
    facts
        .universe
        .as_slice()
        .binary_search_by(|candidate| candidate.as_ref().cmp(name))
        .ok()
        .map(|index| index as u32)
}

fn declaration_initializer(
    cpg: &CodePropertyGraph,
    declaration: NodeId,
    name: &str,
) -> Option<NodeId> {
    cpg.ast_children(declaration)
        .into_iter()
        .rev()
        .find(|child| {
            !matches!(
                cpg.node(*child).map(|node| &node.kind),
                Some(CpgNodeKind::Identifier { name: child_name, .. }) if &**child_name == name
            )
        })
}

fn condition_expression(cpg: &CodePropertyGraph, source: NodeId) -> Option<NodeId> {
    if matches!(
        cpg.node(source).map(|node| &node.kind),
        Some(CpgNodeKind::BinaryOp { .. })
    ) {
        return Some(source);
    }
    let root = cpg.ast_children(source).first().copied()?;
    let mut pending = vec![root];
    let mut seen = Vec::new();
    while let Some(node) = pending.pop() {
        if seen.contains(&node) {
            continue;
        }
        seen.push(node);
        if matches!(
            cpg.node(node).map(|node| &node.kind),
            Some(CpgNodeKind::BinaryOp { operator })
                if matches!(&**operator, "<" | "<=" | ">" | ">=")
        ) {
            return Some(node);
        }
        let mut children = cpg.ast_children(node);
        children.reverse();
        pending.extend(children);
    }
    None
}

fn reverse_comparison(operator: &str) -> Option<&'static str> {
    match operator {
        "<" => Some(">"),
        "<=" => Some(">="),
        ">" => Some("<"),
        ">=" => Some("<="),
        _ => None,
    }
}

fn comparison_interval(operator: &str, constant: i128, truth: bool) -> Option<Interval> {
    let successor = constant
        .checked_add(1)
        .map(Bound::Finite)
        .unwrap_or(Bound::PosInf);
    let predecessor = constant
        .checked_sub(1)
        .map(Bound::Finite)
        .unwrap_or(Bound::NegInf);
    let interval = match (operator, truth) {
        ("<", true) | (">=", false) => Interval {
            lo: Bound::NegInf,
            hi: predecessor,
        },
        ("<", false) | (">=", true) => Interval {
            lo: Bound::Finite(constant),
            hi: Bound::PosInf,
        },
        ("<=", true) | (">", false) => Interval {
            lo: Bound::NegInf,
            hi: Bound::Finite(constant),
        },
        ("<=", false) | (">", true) => Interval {
            lo: successor,
            hi: Bound::PosInf,
        },
        _ => return None,
    };
    Interval::new(interval.lo, interval.hi)
}

impl PartialOrd for Interval {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        if *self == *other {
            Some(Ordering::Equal)
        } else if self.is_subset_of(*other) {
            Some(Ordering::Less)
        } else if other.is_subset_of(*self) {
            Some(Ordering::Greater)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        sccp, ConstValue, CpgEdgeKind, CpgNode, DfgEdgeKind, Language, MethodSignature, ScopeId,
        SourceRange, Visibility,
    };
    use proptest::prelude::*;

    fn node(id: u32, kind: CpgNodeKind) -> CpgNode {
        CpgNode::new(NodeId::new(id), kind, SourceRange::default())
    }

    fn function() -> CpgNodeKind {
        CpgNodeKind::Function {
            signature: MethodSignature {
                name: "interval_fixture".into(),
                params: Default::default(),
                return_type: None,
                is_static: false,
                is_async: false,
                visibility: Visibility::Private,
            },
        }
    }

    fn variable(name: &str) -> CpgNodeKind {
        CpgNodeKind::Variable {
            name: name.into(),
            var_type: None,
            scope: ScopeId::GLOBAL,
            is_mutable: true,
        }
    }

    fn identifier(name: &str) -> CpgNodeKind {
        CpgNodeKind::Identifier {
            name: name.into(),
            definition: None,
        }
    }

    fn integer(value: i64) -> CpgNodeKind {
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

    fn add(cpg: &mut CodePropertyGraph, kind: CpgNodeKind) -> NodeId {
        cpg.add_node(CpgNode::new(NodeId::new(0), kind, SourceRange::default()))
    }

    fn join(left: IntervalEnv, right: &IntervalEnv) -> IntervalEnv {
        let mut result = left;
        result.join(right);
        result
    }

    fn branch_fixture(reverse: bool) -> (CodePropertyGraph, NodeId, NodeId, NodeId) {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let mut nodes = vec![
            node(0, function()),
            node(
                1,
                CpgNodeKind::Parameter {
                    name: "x".into(),
                    param_type: None,
                    is_variadic: false,
                },
            ),
            node(
                2,
                CpgNodeKind::BinaryOp {
                    operator: "<".into(),
                },
            ),
            node(3, identifier("x")),
            node(4, integer(5)),
            node(
                5,
                CpgNodeKind::Block {
                    scope: ScopeId::GLOBAL,
                },
            ),
            node(
                6,
                CpgNodeKind::Block {
                    scope: ScopeId::GLOBAL,
                },
            ),
            node(7, CpgNodeKind::Return),
        ];
        if reverse {
            nodes.reverse();
        }
        for node in nodes {
            cpg.add_node_with_id(node);
        }
        let function = NodeId::new(0);
        let parameter = NodeId::new(1);
        let comparison = NodeId::new(2);
        let true_target = NodeId::new(5);
        let false_target = NodeId::new(6);
        let join = NodeId::new(7);
        for child in [parameter, comparison, true_target, false_target, join] {
            connect_ast(&mut cpg, function, child);
        }
        connect_ast(&mut cpg, comparison, NodeId::new(3));
        connect_ast(&mut cpg, comparison, NodeId::new(4));
        let mut cfg_edges = vec![
            (function, parameter, CfgEdgeKind::Sequential),
            (parameter, comparison, CfgEdgeKind::Sequential),
            (comparison, true_target, CfgEdgeKind::ConditionalTrue),
            (comparison, false_target, CfgEdgeKind::ConditionalFalse),
            (true_target, join, CfgEdgeKind::Sequential),
            (false_target, join, CfgEdgeKind::Sequential),
        ];
        if reverse {
            cfg_edges.reverse();
        }
        for (source, target, kind) in cfg_edges {
            connect_cfg(&mut cpg, source, target, kind);
        }
        (cpg, function, true_target, false_target)
    }

    #[derive(Debug, Clone, Copy)]
    struct Shift {
        add: bool,
        amount: i8,
    }

    fn linear_fixture(
        initial: i8,
        shifts: &[Shift],
    ) -> (CodePropertyGraph, NodeId, Vec<(NodeId, i128)>) {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let function = add(&mut cpg, function());
        let declaration = add(&mut cpg, variable("x"));
        let binder = add(&mut cpg, identifier("x"));
        let initializer = add(&mut cpg, integer(i64::from(initial)));
        connect_ast(&mut cpg, function, declaration);
        connect_ast(&mut cpg, declaration, binder);
        connect_ast(&mut cpg, declaration, initializer);
        connect_cfg(&mut cpg, function, declaration, CfgEdgeKind::Sequential);

        let mut concrete = i128::from(initial);
        let mut expected = vec![(declaration, concrete)];
        let mut previous_cfg = declaration;
        let mut previous_definition = declaration;
        for shift in shifts {
            let assignment = add(
                &mut cpg,
                CpgNodeKind::Assignment {
                    operator: "=".into(),
                },
            );
            let target = add(&mut cpg, identifier("x"));
            let binary = add(
                &mut cpg,
                CpgNodeKind::BinaryOp {
                    operator: if shift.add { "+" } else { "-" }.into(),
                },
            );
            let use_site = add(&mut cpg, identifier("x"));
            let literal = add(&mut cpg, integer(i64::from(shift.amount)));
            connect_ast(&mut cpg, function, assignment);
            connect_ast(&mut cpg, assignment, target);
            connect_ast(&mut cpg, assignment, binary);
            connect_ast(&mut cpg, binary, use_site);
            connect_ast(&mut cpg, binary, literal);
            connect_cfg(&mut cpg, previous_cfg, assignment, CfgEdgeKind::Sequential);
            connect_def_use(&mut cpg, previous_definition, use_site);
            if shift.add {
                concrete += i128::from(shift.amount);
            } else {
                concrete -= i128::from(shift.amount);
            }
            expected.push((assignment, concrete));
            previous_cfg = assignment;
            previous_definition = assignment;
        }
        let exit = add(&mut cpg, CpgNodeKind::Return);
        connect_ast(&mut cpg, function, exit);
        connect_cfg(&mut cpg, previous_cfg, exit, CfgEdgeKind::Sequential);
        (cpg, function, expected)
    }

    fn loop_fixture(initial: i8, limit: i16, step: u8) -> (CodePropertyGraph, NodeId, NodeId) {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let function = add(&mut cpg, function());
        let declaration = add(&mut cpg, variable("x"));
        let binder = add(&mut cpg, identifier("x"));
        let initializer = add(&mut cpg, integer(i64::from(initial)));
        let header = add(
            &mut cpg,
            CpgNodeKind::BinaryOp {
                operator: "<".into(),
            },
        );
        let header_use = add(&mut cpg, identifier("x"));
        let limit_literal = add(&mut cpg, integer(i64::from(limit)));
        let body = add(
            &mut cpg,
            CpgNodeKind::Assignment {
                operator: "=".into(),
            },
        );
        let target = add(&mut cpg, identifier("x"));
        let addition = add(
            &mut cpg,
            CpgNodeKind::BinaryOp {
                operator: "+".into(),
            },
        );
        let body_use = add(&mut cpg, identifier("x"));
        let step_literal = add(&mut cpg, integer(i64::from(step)));
        let exit = add(&mut cpg, CpgNodeKind::Return);

        for child in [declaration, header, body, exit] {
            connect_ast(&mut cpg, function, child);
        }
        connect_ast(&mut cpg, declaration, binder);
        connect_ast(&mut cpg, declaration, initializer);
        connect_ast(&mut cpg, header, header_use);
        connect_ast(&mut cpg, header, limit_literal);
        connect_ast(&mut cpg, body, target);
        connect_ast(&mut cpg, body, addition);
        connect_ast(&mut cpg, addition, body_use);
        connect_ast(&mut cpg, addition, step_literal);

        connect_cfg(&mut cpg, function, declaration, CfgEdgeKind::Sequential);
        connect_cfg(&mut cpg, declaration, header, CfgEdgeKind::Sequential);
        connect_cfg(&mut cpg, header, body, CfgEdgeKind::ConditionalTrue);
        connect_cfg(&mut cpg, body, header, CfgEdgeKind::LoopBack);
        connect_cfg(&mut cpg, header, exit, CfgEdgeKind::ConditionalFalse);
        connect_def_use(&mut cpg, declaration, header_use);
        connect_def_use(&mut cpg, declaration, body_use);
        connect_def_use(&mut cpg, body, header_use);
        connect_def_use(&mut cpg, body, body_use);
        (cpg, function, header)
    }

    fn states_after_definitions(
        cpg: &CodePropertyGraph,
        function: NodeId,
    ) -> (VarFacts, BTreeMap<NodeId, IntervalEnv>) {
        let (cfg, facts, solution) = solve_function(cpg, function).expect("interval solution");
        let analysis = IntervalAnalysis { facts: &facts };
        let mut states = BTreeMap::new();
        for block in &cfg.blocks {
            let mut state = solution.before[block.id.0 as usize].clone();
            for &member in &block.nodes {
                analysis.transfer_node(cpg, member, &mut state);
                if !facts.definitions_at(member).is_empty() {
                    states.insert(member, state.clone());
                }
            }
        }
        (facts, states)
    }

    fn arb_interval() -> impl Strategy<Value = Interval> {
        prop_oneof![
            (any::<i16>(), any::<i16>()).prop_map(|(left, right)| {
                let lo = i128::from(left.min(right));
                let hi = i128::from(left.max(right));
                Interval {
                    lo: Bound::Finite(lo),
                    hi: Bound::Finite(hi),
                }
            }),
            any::<i16>().prop_map(|hi| Interval {
                lo: Bound::NegInf,
                hi: Bound::Finite(i128::from(hi)),
            }),
            any::<i16>().prop_map(|lo| Interval {
                lo: Bound::Finite(i128::from(lo)),
                hi: Bound::PosInf,
            }),
            Just(Interval::top()),
        ]
    }

    fn shallow_expression_fixture(steps: &[(u8, i8)]) -> (CodePropertyGraph, NodeId, NodeId) {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let function = add(&mut cpg, function());
        let declaration = add(&mut cpg, variable("x"));
        let binder = add(&mut cpg, identifier("x"));
        connect_ast(&mut cpg, function, declaration);
        connect_ast(&mut cpg, declaration, binder);

        let mut root = add(&mut cpg, identifier("x"));
        for &(shape, amount) in steps {
            if shape % 5 < 2 {
                let parent = add(
                    &mut cpg,
                    CpgNodeKind::UnaryOp {
                        operator: if shape % 5 == 0 { "+" } else { "-" }.into(),
                    },
                );
                connect_ast(&mut cpg, parent, root);
                root = parent;
                continue;
            }

            let parent = add(
                &mut cpg,
                CpgNodeKind::BinaryOp {
                    operator: if shape % 5 == 4 { "-" } else { "+" }.into(),
                },
            );
            let scalar = add(&mut cpg, integer(i64::from(amount)));
            if shape % 5 == 3 {
                connect_ast(&mut cpg, parent, scalar);
                connect_ast(&mut cpg, parent, root);
            } else {
                connect_ast(&mut cpg, parent, root);
                connect_ast(&mut cpg, parent, scalar);
            }
            root = parent;
        }
        let returned = add(&mut cpg, CpgNodeKind::Return);
        connect_ast(&mut cpg, function, returned);
        connect_ast(&mut cpg, returned, root);
        (cpg, function, root)
    }

    fn recursive_eval_expr(
        analysis: &IntervalAnalysis<'_>,
        cpg: &CodePropertyGraph,
        node: NodeId,
        state: &IntervalEnv,
        on_path: &mut Vec<NodeId>,
    ) -> Interval {
        if on_path.contains(&node) {
            return Interval::top();
        }
        on_path.push(node);
        let result = match cpg.node(node).map(|node| &node.kind) {
            Some(CpgNodeKind::Literal {
                kind: LiteralKind::Integer(value),
            }) => Interval::singleton(i128::from(*value)),
            Some(CpgNodeKind::Identifier { name, .. }) => variable_id(analysis.facts, name)
                .and_then(|variable| state.get(variable))
                .unwrap_or_else(Interval::top),
            Some(CpgNodeKind::UnaryOp { operator }) => cpg
                .ast_children(node)
                .first()
                .copied()
                .map_or_else(Interval::top, |operand| {
                    let operand = recursive_eval_expr(analysis, cpg, operand, state, on_path);
                    match &**operator {
                        "+" => operand,
                        "-" => operand.negate(),
                        _ => Interval::top(),
                    }
                }),
            Some(CpgNodeKind::BinaryOp { operator }) => {
                let children = cpg.ast_children(node);
                match children.as_slice() {
                    [left, right, ..] => {
                        let left = recursive_eval_expr(analysis, cpg, *left, state, on_path);
                        let right = recursive_eval_expr(analysis, cpg, *right, state, on_path);
                        match (&**operator, singleton_value(right), singleton_value(left)) {
                            ("+", Some(amount), _) => left.shift(amount),
                            ("+", _, Some(amount)) => right.shift(amount),
                            ("-", Some(amount), _) => left.shift(amount.saturating_neg()),
                            _ => Interval::top(),
                        }
                    }
                    _ => Interval::top(),
                }
            }
            _ => Interval::top(),
        };
        on_path.pop();
        result
    }

    #[test]
    fn interval_constructors_and_order_cover_extended_endpoints() {
        assert_eq!(
            Interval::new(Bound::Finite(-2), Bound::Finite(4)),
            Some(Interval {
                lo: Bound::Finite(-2),
                hi: Bound::Finite(4),
            })
        );
        assert_eq!(Interval::new(Bound::Finite(4), Bound::Finite(-2)), None);
        assert!(Interval::top().contains(i128::MIN));
        assert!(Interval::top().contains(i128::MAX));
        assert!(Interval::singleton(7).contains(7));
        assert!(!Interval::singleton(7).contains(8));
        assert!(Interval::singleton(7) < Interval::top());
    }

    #[test]
    fn interval_endpoint_arithmetic_never_constructs_an_empty_encoding() {
        assert_eq!(Interval::new(Bound::PosInf, Bound::PosInf), None);
        assert_eq!(Interval::new(Bound::NegInf, Bound::NegInf), None);
        assert_eq!(
            Interval::new(Bound::NegInf, Bound::Finite(0)),
            Some(Interval {
                lo: Bound::NegInf,
                hi: Bound::Finite(0),
            })
        );
        assert_eq!(
            Interval::new(Bound::Finite(0), Bound::PosInf),
            Some(Interval {
                lo: Bound::Finite(0),
                hi: Bound::PosInf,
            })
        );

        assert_eq!(Interval::singleton(i128::MAX).shift(1), Interval::top());
        assert_eq!(Interval::singleton(i128::MIN).shift(-1), Interval::top());
        assert_eq!(
            Interval {
                lo: Bound::Finite(0),
                hi: Bound::PosInf,
            }
            .shift(2),
            Interval {
                lo: Bound::Finite(2),
                hi: Bound::PosInf,
            }
        );
        assert_eq!(
            Interval {
                lo: Bound::NegInf,
                hi: Bound::Finite(0),
            }
            .shift(-2),
            Interval {
                lo: Bound::NegInf,
                hi: Bound::Finite(-2),
            }
        );
        assert_eq!(Interval::singleton(-7).negate(), Interval::singleton(7));
        assert_eq!(Interval::singleton(i128::MIN).negate(), Interval::top());
        assert_eq!(Interval::top().negate(), Interval::top());

        let finite = Interval::new(Bound::Finite(-2), Bound::Finite(4)).unwrap();
        assert_eq!(Interval::top().narrow(finite), finite);
        assert_eq!(finite.narrow(Interval::singleton(20)), finite);
        assert_eq!(finite.meet(Interval::singleton(20)), None);
        assert_eq!(finite.partial_cmp(&finite), Some(Ordering::Equal));
        assert_eq!(
            Interval::top().partial_cmp(&finite),
            Some(Ordering::Greater)
        );
        assert_eq!(
            Interval::singleton(0).partial_cmp(&Interval::singleton(1)),
            None
        );

        let invalid_lower = Interval {
            lo: Bound::PosInf,
            hi: Bound::PosInf,
        };
        let invalid_upper = Interval {
            lo: Bound::NegInf,
            hi: Bound::NegInf,
        };
        assert!(!invalid_lower.contains(0));
        assert!(!invalid_upper.contains(0));
    }

    #[test]
    fn environment_is_sorted_deduplicated_and_pointwise() {
        let env = IntervalEnv::from_vars([
            (2, Interval::singleton(8)),
            (0, Interval::singleton(1)),
            (2, Interval::singleton(10)),
        ]);
        assert_eq!(
            env.vars.as_slice(),
            &[
                (0, Interval::singleton(1)),
                (
                    2,
                    Interval {
                        lo: Bound::Finite(8),
                        hi: Bound::Finite(10),
                    }
                ),
            ]
        );
        assert!(IntervalEnv::bottom().leq(&env));
    }

    #[test]
    fn environment_operations_cover_bottom_insert_replace_join_and_widen() {
        let mut bottom = IntervalEnv::bottom();
        assert!(!bottom.join(&IntervalEnv::bottom()));
        assert!(!bottom.widen_with(&IntervalEnv::bottom()));
        bottom.meet_variable(0, Interval::singleton(1));
        assert!(bottom.is_bottom());

        let seed = IntervalEnv::from_vars([(1, Interval::singleton(4))]);
        assert!(bottom.join(&seed));
        assert_eq!(bottom, seed);
        assert!(!bottom.join(&seed));

        bottom.set(0, Interval::singleton(2));
        bottom.set(1, Interval::singleton(5));
        assert_eq!(bottom.get(0), Some(Interval::singleton(2)));
        assert_eq!(bottom.get(1), Some(Interval::singleton(5)));
        assert_eq!(bottom.get(99), None);

        let incoming =
            IntervalEnv::from_vars([(0, Interval::singleton(-1)), (2, Interval::singleton(8))]);
        assert!(bottom.join(&incoming));
        assert_eq!(
            bottom.get(0),
            Some(Interval::new(Bound::Finite(-1), Bound::Finite(2)).unwrap())
        );
        assert_eq!(bottom.get(2), Some(Interval::singleton(8)));

        let mut widened = IntervalEnv::bottom();
        assert!(widened.widen_with(&seed));
        assert_eq!(widened, seed);
        let expanding = IntervalEnv::from_vars([
            (
                1,
                Interval::new(Bound::Finite(3), Bound::Finite(6)).unwrap(),
            ),
            (3, Interval::singleton(9)),
        ]);
        assert!(widened.widen_with(&expanding));
        assert_eq!(widened.get(1), Some(Interval::top()));
        assert_eq!(widened.get(3), Some(Interval::singleton(9)));
        assert!(!widened.widen_with(&expanding));

        let mut refinement = IntervalEnv::from_vars([(0, Interval::singleton(3))]);
        refinement.meet_variable(4, Interval::new(Bound::NegInf, Bound::Finite(10)).unwrap());
        assert_eq!(
            refinement.get(4),
            Some(Interval::new(Bound::NegInf, Bound::Finite(10)).unwrap())
        );
        refinement.meet_variable(0, Interval::singleton(99));
        assert!(refinement.is_bottom());
    }

    #[test]
    fn conditional_edges_refine_both_halves_and_join_restores_the_hull() {
        let (cpg, function, true_target, false_target) = branch_fixture(false);
        let before = (cpg.node_count(), cpg.edge_count());
        let report = value_ranges(&cpg, function).expect("branch interval report");
        assert_eq!(
            report.interval_at(true_target, "x"),
            Some(Interval {
                lo: Bound::NegInf,
                hi: Bound::Finite(4),
            })
        );
        assert_eq!(
            report.interval_at(false_target, "x"),
            Some(Interval {
                lo: Bound::Finite(5),
                hi: Bound::PosInf,
            })
        );
        assert_eq!((cpg.node_count(), cpg.edge_count()), before);
    }

    #[test]
    fn reversed_comparison_operands_are_normalized_before_refinement() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let function = add(&mut cpg, function());
        let parameter = add(
            &mut cpg,
            CpgNodeKind::Parameter {
                name: "x".into(),
                param_type: None,
                is_variadic: false,
            },
        );
        let comparison = add(
            &mut cpg,
            CpgNodeKind::BinaryOp {
                operator: "<".into(),
            },
        );
        let five = add(&mut cpg, integer(5));
        let x_use = add(&mut cpg, identifier("x"));
        let true_target = add(
            &mut cpg,
            CpgNodeKind::Block {
                scope: ScopeId::GLOBAL,
            },
        );
        let false_target = add(
            &mut cpg,
            CpgNodeKind::Block {
                scope: ScopeId::GLOBAL,
            },
        );
        for child in [parameter, comparison, true_target, false_target] {
            connect_ast(&mut cpg, function, child);
        }
        connect_ast(&mut cpg, comparison, five);
        connect_ast(&mut cpg, comparison, x_use);
        connect_cfg(&mut cpg, function, parameter, CfgEdgeKind::Sequential);
        connect_cfg(&mut cpg, parameter, comparison, CfgEdgeKind::Sequential);
        connect_cfg(
            &mut cpg,
            comparison,
            true_target,
            CfgEdgeKind::ConditionalTrue,
        );
        connect_cfg(
            &mut cpg,
            comparison,
            false_target,
            CfgEdgeKind::ConditionalFalse,
        );

        let report = value_ranges(&cpg, function).expect("reversed comparison report");
        assert_eq!(
            report.interval_at(true_target, "x"),
            Some(Interval::new(Bound::Finite(6), Bound::PosInf).unwrap())
        );
        assert_eq!(
            report.interval_at(false_target, "x"),
            Some(Interval::new(Bound::NegInf, Bound::Finite(5)).unwrap())
        );
        assert_eq!(report.interval_at(true_target, "missing"), None);
        assert_eq!(report.interval_at(NodeId::new(u32::MAX), "x"), None);
    }

    #[test]
    fn comparison_helpers_cover_all_polarities_limits_and_wrappers() {
        let lower = Interval::new(Bound::NegInf, Bound::Finite(4)).unwrap();
        let lower_inclusive = Interval::new(Bound::NegInf, Bound::Finite(5)).unwrap();
        let upper = Interval::new(Bound::Finite(6), Bound::PosInf).unwrap();
        let upper_inclusive = Interval::new(Bound::Finite(5), Bound::PosInf).unwrap();
        for operator in ["<", ">="] {
            assert_eq!(
                comparison_interval(operator, 5, operator == "<"),
                Some(lower)
            );
            assert_eq!(
                comparison_interval(operator, 5, operator != "<"),
                Some(upper_inclusive)
            );
        }
        for operator in ["<=", ">"] {
            assert_eq!(
                comparison_interval(operator, 5, operator == "<="),
                Some(lower_inclusive)
            );
            assert_eq!(
                comparison_interval(operator, 5, operator != "<="),
                Some(upper)
            );
        }
        assert_eq!(comparison_interval("==", 5, true), None);
        assert_eq!(comparison_interval("<", i128::MIN, true), None);
        assert_eq!(comparison_interval(">", i128::MAX, true), None);
        assert_eq!(reverse_comparison("<"), Some(">"));
        assert_eq!(reverse_comparison("<="), Some(">="));
        assert_eq!(reverse_comparison(">"), Some("<"));
        assert_eq!(reverse_comparison(">="), Some("<="));
        assert_eq!(reverse_comparison("=="), None);
        assert_eq!(singleton_value(Interval::top()), None);

        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let comparison = add(
            &mut cpg,
            CpgNodeKind::BinaryOp {
                operator: "<".into(),
            },
        );
        assert_eq!(condition_expression(&cpg, comparison), Some(comparison));

        let wrapper = add(
            &mut cpg,
            CpgNodeKind::UnaryOp {
                operator: "+".into(),
            },
        );
        let nested = add(
            &mut cpg,
            CpgNodeKind::UnaryOp {
                operator: "+".into(),
            },
        );
        connect_ast(&mut cpg, wrapper, nested);
        connect_ast(&mut cpg, nested, comparison);
        assert_eq!(condition_expression(&cpg, wrapper), Some(comparison));

        let cycle = add(
            &mut cpg,
            CpgNodeKind::UnaryOp {
                operator: "+".into(),
            },
        );
        connect_ast(&mut cpg, cycle, cycle);
        assert_eq!(condition_expression(&cpg, cycle), None);
        let leaf = add(&mut cpg, CpgNodeKind::Return);
        assert_eq!(condition_expression(&cpg, leaf), None);
    }

    #[test]
    fn straight_line_transfers_are_exact_and_unknown_rhs_becomes_top() {
        let shifts = [
            Shift {
                add: true,
                amount: 4,
            },
            Shift {
                add: false,
                amount: 7,
            },
        ];
        let (cpg, linear_function, expected) = linear_fixture(3, &shifts);
        let (facts, states) = states_after_definitions(&cpg, linear_function);
        let x = variable_id(&facts, "x").expect("x variable");
        for (definition, concrete) in expected {
            assert_eq!(
                states[&definition].get(x),
                Some(Interval::singleton(concrete))
            );
        }

        let mut unknown = CodePropertyGraph::new(Language::Rust);
        let function = add(&mut unknown, function());
        let declaration = add(&mut unknown, variable("x"));
        let binder = add(&mut unknown, identifier("x"));
        let initial = add(&mut unknown, integer(1));
        let assignment = add(
            &mut unknown,
            CpgNodeKind::Assignment {
                operator: "=".into(),
            },
        );
        let target = add(&mut unknown, identifier("x"));
        let call = add(
            &mut unknown,
            CpgNodeKind::Call {
                target: None,
                is_method: false,
            },
        );
        for child in [declaration, assignment] {
            connect_ast(&mut unknown, function, child);
        }
        connect_ast(&mut unknown, declaration, binder);
        connect_ast(&mut unknown, declaration, initial);
        connect_ast(&mut unknown, assignment, target);
        connect_ast(&mut unknown, assignment, call);
        connect_cfg(&mut unknown, function, declaration, CfgEdgeKind::Sequential);
        connect_cfg(
            &mut unknown,
            declaration,
            assignment,
            CfgEdgeKind::Sequential,
        );
        let (facts, states) = states_after_definitions(&unknown, function);
        let x = variable_id(&facts, "x").expect("x variable");
        assert_eq!(states[&assignment].get(x), Some(Interval::top()));
    }

    #[test]
    fn expression_and_definition_transfers_cover_every_supported_shape() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let function = add(&mut cpg, function());
        let declaration = add(&mut cpg, variable("x"));
        let binder = add(&mut cpg, identifier("x"));
        connect_ast(&mut cpg, function, declaration);
        connect_ast(&mut cpg, declaration, binder);

        let x_use = add(&mut cpg, identifier("x"));
        let unknown_use = add(&mut cpg, identifier("missing"));
        let three = add(&mut cpg, integer(3));
        let plus = add(
            &mut cpg,
            CpgNodeKind::BinaryOp {
                operator: "+".into(),
            },
        );
        connect_ast(&mut cpg, plus, x_use);
        connect_ast(&mut cpg, plus, three);

        let reverse_plus = add(
            &mut cpg,
            CpgNodeKind::BinaryOp {
                operator: "+".into(),
            },
        );
        connect_ast(&mut cpg, reverse_plus, three);
        connect_ast(&mut cpg, reverse_plus, x_use);

        let subtract = add(
            &mut cpg,
            CpgNodeKind::BinaryOp {
                operator: "-".into(),
            },
        );
        connect_ast(&mut cpg, subtract, x_use);
        connect_ast(&mut cpg, subtract, three);

        let multiply = add(
            &mut cpg,
            CpgNodeKind::BinaryOp {
                operator: "*".into(),
            },
        );
        connect_ast(&mut cpg, multiply, x_use);
        connect_ast(&mut cpg, multiply, three);
        let incomplete_binary = add(
            &mut cpg,
            CpgNodeKind::BinaryOp {
                operator: "+".into(),
            },
        );
        connect_ast(&mut cpg, incomplete_binary, x_use);

        let unary_plus = add(
            &mut cpg,
            CpgNodeKind::UnaryOp {
                operator: "+".into(),
            },
        );
        connect_ast(&mut cpg, unary_plus, x_use);
        let unary_minus = add(
            &mut cpg,
            CpgNodeKind::UnaryOp {
                operator: "-".into(),
            },
        );
        connect_ast(&mut cpg, unary_minus, x_use);
        let unary_unknown = add(
            &mut cpg,
            CpgNodeKind::UnaryOp {
                operator: "!".into(),
            },
        );
        connect_ast(&mut cpg, unary_unknown, x_use);
        let unary_empty = add(
            &mut cpg,
            CpgNodeKind::UnaryOp {
                operator: "+".into(),
            },
        );
        let unary_cycle = add(
            &mut cpg,
            CpgNodeKind::UnaryOp {
                operator: "+".into(),
            },
        );
        connect_ast(&mut cpg, unary_cycle, unary_cycle);

        let facts = variable_facts(&cpg, function);
        let x = variable_id(&facts, "x").expect("x variable");
        let analysis = IntervalAnalysis { facts: &facts };
        let state = IntervalEnv::from_vars([(x, Interval::singleton(10))]);
        let evaluate = |expression| analysis.eval_expr(&cpg, expression, &state);
        assert_eq!(evaluate(plus), Interval::singleton(13));
        assert_eq!(evaluate(reverse_plus), Interval::singleton(13));
        assert_eq!(evaluate(subtract), Interval::singleton(7));
        assert_eq!(evaluate(unary_plus), Interval::singleton(10));
        assert_eq!(evaluate(unary_minus), Interval::singleton(-10));
        for expression in [
            unknown_use,
            multiply,
            incomplete_binary,
            unary_unknown,
            unary_empty,
            unary_cycle,
            function,
        ] {
            assert_eq!(evaluate(expression), Interval::top());
        }

        for (operator, expected) in [
            ("+=", Interval::singleton(13)),
            ("-=", Interval::singleton(7)),
            ("*=", Interval::top()),
        ] {
            let assignment = add(
                &mut cpg,
                CpgNodeKind::Assignment {
                    operator: operator.into(),
                },
            );
            let target = add(&mut cpg, identifier("x"));
            connect_ast(&mut cpg, assignment, target);
            connect_ast(&mut cpg, assignment, three);
            assert_eq!(
                analysis.definition_value(&cpg, assignment, x, &state),
                expected
            );
        }

        for operator in ["=", "+="] {
            let assignment = add(
                &mut cpg,
                CpgNodeKind::Assignment {
                    operator: operator.into(),
                },
            );
            let target = add(&mut cpg, identifier("x"));
            connect_ast(&mut cpg, assignment, target);
            assert_eq!(
                analysis.definition_value(&cpg, assignment, x, &state),
                Interval::top()
            );
        }
        let parameter = add(
            &mut cpg,
            CpgNodeKind::Parameter {
                name: "x".into(),
                param_type: None,
                is_variadic: false,
            },
        );
        assert_eq!(
            analysis.definition_value(&cpg, declaration, x, &state),
            Interval::top()
        );
        assert_eq!(
            analysis.definition_value(&cpg, parameter, x, &state),
            Interval::top()
        );
        assert_eq!(
            analysis.definition_value(&cpg, function, x, &state),
            Interval::top()
        );
    }

    #[test]
    fn loop_headers_widen_then_narrow_without_reaching_the_cap() {
        let (cpg, function, header) = loop_fixture(0, 10, 1);
        let cfg = block_cfg(&cpg, function).expect("loop block CFG");
        let facts = variable_facts(&cpg, function);
        let loops = loop_forest(&cpg, function).expect("loop forest");
        assert_eq!(loops.loops.len(), 1);
        assert_eq!(loops.loops[0].header, header);
        let analysis = IntervalAnalysis { facts: &facts };
        let widened = solve(
            &cpg,
            &cfg,
            &analysis,
            &SolverConfig {
                max_visits_per_block: 64,
                widen_after: Some(3),
                narrow_passes: 0,
            },
            Some(&loops),
        )
        .expect("widened loop solution");
        let narrowed = solve(
            &cpg,
            &cfg,
            &analysis,
            &SolverConfig {
                max_visits_per_block: 64,
                widen_after: Some(3),
                narrow_passes: 2,
            },
            Some(&loops),
        )
        .expect("narrowed loop solution");
        assert!(widened.visits.iter().all(|visits| *visits < 64));
        for (narrowed, widened) in narrowed.before.iter().zip(&widened.before) {
            assert!(narrowed.leq(widened));
        }
        for (narrowed, widened) in narrowed.after.iter().zip(&widened.after) {
            assert!(narrowed.leq(widened));
        }
        let header_block = cfg.block_of_node[&header];
        let x = variable_id(&facts, "x").expect("x variable");
        let interval = narrowed.before[header_block.0 as usize]
            .get(x)
            .expect("header x interval");
        assert!(interval.contains(0));
        assert!(interval.contains(10));
    }

    #[test]
    fn malformed_ast_cycles_and_invalid_selectors_are_bounded() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let function = cpg.add_node_with_id(node(0, function()));
        let declaration = cpg.add_node_with_id(node(1, variable("x")));
        let wrapper = cpg.add_node_with_id(node(
            2,
            CpgNodeKind::UnaryOp {
                operator: "+".into(),
            },
        ));
        connect_ast(&mut cpg, function, declaration);
        connect_ast(&mut cpg, declaration, wrapper);
        connect_ast(&mut cpg, wrapper, declaration);
        connect_cfg(&mut cpg, function, declaration, CfgEdgeKind::Sequential);
        let report = value_ranges(&cpg, function).expect("cycle-safe interval report");
        assert_eq!(report.function, function);
        assert_eq!(
            value_ranges(&cpg, NodeId::new(99)),
            Err(AnalysisError::UnknownNode(NodeId::new(99)))
        );
        assert_eq!(
            value_ranges(&cpg, declaration),
            Err(AnalysisError::NotAFunction(declaration))
        );
    }

    #[test]
    fn sccp_constants_are_contained_by_corresponding_interval_states() {
        let shifts = [
            Shift {
                add: true,
                amount: 2,
            },
            Shift {
                add: false,
                amount: -3,
            },
        ];
        let (cpg, function, expected) = linear_fixture(5, &shifts);
        let constants = sccp(&cpg, function).expect("SCCP cross-analysis result");
        let (facts, states) = states_after_definitions(&cpg, function);
        let x = variable_id(&facts, "x").expect("x variable");
        for (definition, _) in expected {
            let Some(ConstValue::Int(constant)) = constants.constants.get(&definition) else {
                panic!("SCCP must prove the generated definition constant");
            };
            assert!(states[&definition]
                .get(x)
                .expect("interval definition state")
                .contains(i128::from(*constant)));
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(96))]

        #[test]
        fn flat_expression_machine_matches_the_bounded_recursive_oracle(
            steps in prop::collection::vec((any::<u8>(), any::<i8>()), 0..32),
            initial in arb_interval(),
        ) {
            let (cpg, function, root) = shallow_expression_fixture(&steps);
            let facts = variable_facts(&cpg, function);
            let variable = variable_id(&facts, "x").expect("x variable");
            let state = IntervalEnv::from_vars([(variable, initial)]);
            let analysis = IntervalAnalysis { facts: &facts };
            let actual = analysis.eval_expr(&cpg, root, &state);
            let expected = recursive_eval_expr(
                &analysis,
                &cpg,
                root,
                &state,
                &mut Vec::new(),
            );
            prop_assert_eq!(actual, expected);
        }

        #[test]
        fn interval_hull_widen_and_narrow_obey_their_algebra(
            left in arb_interval(),
            middle in arb_interval(),
            right in arb_interval(),
        ) {
            prop_assert_eq!(left.hull(left), left);
            prop_assert_eq!(left.hull(middle), middle.hull(left));
            prop_assert_eq!(
                left.hull(middle).hull(right),
                left.hull(middle.hull(right)),
            );
            let widened = left.widen(middle);
            prop_assert!(left.is_subset_of(widened));
            prop_assert!(middle.is_subset_of(widened));
            prop_assert!(left.narrow(middle).is_subset_of(left));
        }

        #[test]
        fn environment_join_is_commutative_associative_and_idempotent(
            left in arb_interval(),
            middle in arb_interval(),
            right in arb_interval(),
        ) {
            let left_env = IntervalEnv::from_vars([(0, left)]);
            let middle_env = IntervalEnv::from_vars([(0, middle)]);
            let right_env = IntervalEnv::from_vars([(0, right)]);
            prop_assert_eq!(join(left_env.clone(), &left_env), left_env.clone());
            prop_assert_eq!(
                join(left_env.clone(), &middle_env),
                join(middle_env.clone(), &left_env),
            );
            prop_assert_eq!(
                join(join(left_env.clone(), &middle_env), &right_env),
                join(left_env, &join(middle_env, &right_env)),
            );
        }

        #[test]
        fn generated_straight_line_programs_contain_every_concrete_value(
            initial in any::<i8>(),
            raw in prop::collection::vec((any::<bool>(), any::<i8>()), 0..14),
        ) {
            let shifts: Vec<_> = raw
                .into_iter()
                .map(|(add, amount)| Shift { add, amount })
                .collect();
            let (cpg, function, expected) = linear_fixture(initial, &shifts);
            let graph_size = (cpg.node_count(), cpg.edge_count());
            let (facts, states) = states_after_definitions(&cpg, function);
            let x = variable_id(&facts, "x").expect("x variable");
            for (definition, concrete) in expected {
                prop_assert!(states[&definition]
                    .get(x)
                    .expect("definition interval")
                    .contains(concrete));
            }
            prop_assert_eq!((cpg.node_count(), cpg.edge_count()), graph_size);
        }

        #[test]
        fn arbitrary_bounded_counting_loops_converge_and_cover_observations(
            initial in any::<i8>(),
            distance in 1u8..32,
            step in 1u8..9,
        ) {
            let limit = i16::from(initial) + i16::from(distance);
            let (cpg, function, header) = loop_fixture(initial, limit, step);
            let (cfg, facts, solution) = solve_function(&cpg, function)
                .expect("generated loop interval solution");
            prop_assert!(solution.visits.iter().all(|visits| *visits < 64));
            let x = variable_id(&facts, "x").expect("x variable");
            let header_block = cfg.block_of_node[&header];
            let abstract_header = solution.before[header_block.0 as usize]
                .get(x)
                .expect("header interval");
            let mut concrete = i128::from(initial);
            let concrete_limit = i128::from(limit);
            let concrete_step = i128::from(step);
            let mut iterations = 0u32;
            loop {
                prop_assert!(abstract_header.contains(concrete));
                if concrete >= concrete_limit {
                    break;
                }
                concrete += concrete_step;
                iterations += 1;
                prop_assert!(iterations <= 32);
            }
        }

        #[test]
        fn reports_are_invariant_under_node_and_cfg_edge_insertion_order(
            _seed in any::<u8>(),
        ) {
            let (normal, function, _, _) = branch_fixture(false);
            let (reversed, reversed_function, _, _) = branch_fixture(true);
            prop_assert_eq!(
                value_ranges(&normal, function).expect("normal report"),
                value_ranges(&reversed, reversed_function).expect("reversed report"),
            );
        }
    }

    #[cfg(feature = "serde")]
    #[test]
    fn public_interval_values_round_trip_through_json() {
        let (cpg, function, _, _) = branch_fixture(false);
        let report = value_ranges(&cpg, function).expect("interval report");
        let encoded = serde_json::to_string(&report).expect("serialize interval report");
        let decoded: IntervalReport =
            serde_json::from_str(&encoded).expect("deserialize interval report");
        assert_eq!(decoded, report);

        let env = IntervalEnv::from_vars([(3, Interval::singleton(9))]);
        let encoded = serde_json::to_string(&env).expect("serialize interval environment");
        let decoded: IntervalEnv =
            serde_json::from_str(&encoded).expect("deserialize interval environment");
        assert_eq!(decoded, env);
    }
}
