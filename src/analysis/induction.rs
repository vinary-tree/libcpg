//! Loop-invariant computations, induction variables, and counted-loop evidence.
//!
//! The analysis consumes the existing natural-loop forest, dominator tree, AST,
//! CFG, and DFG overlays. It never reparses source text and stores every result
//! outside the CPG. Loop invariance follows the Aho–Sethi–Ullman reaching-
//! definitions fixed point; induction recognition then matches semantic
//! assignment and range nodes over that invariant set.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use crate::{CodePropertyGraph, CpgNodeKind, DfgEdgeKind, LiteralKind, NodeId};

use super::{
    cfg_scope::function_cfg_view, dataflow::variable_facts, dominator_tree, function_scope_nodes,
    AnalysisError, DominatorTree, LoopForest, NaturalLoop, VarFacts,
};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Direction in which an induction variable changes on each iteration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum IvDirection {
    /// The variable has a statically positive step.
    Increasing,
    /// The variable has a statically negative step.
    Decreasing,
    /// The variable has a statically zero step.
    Stationary,
    /// The step's sign is loop-invariant but not statically known.
    Unknown,
}

/// Loop-invariant computations and the subset safe to consider for hoisting.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct LoopInvariants {
    /// Stable loop id from [`LoopForest::loops`].
    pub loop_id: usize,
    /// Sorted nodes whose DFG operands are defined outside the loop or by an
    /// already invariant node.
    pub invariant_nodes: Vec<NodeId>,
    /// Sorted, side-effect-free invariant computations that dominate every
    /// latch and loop-exit source. Definitions additionally satisfy the
    /// unique-definition and dominated-use safety checks.
    pub hoistable: Vec<NodeId>,
}

/// One basic or one-level-derived induction variable.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct InductionVariable {
    /// Stable loop id from [`LoopForest::loops`].
    pub loop_id: usize,
    /// Canonical bound variable name.
    pub variable: Arc<str>,
    /// Assignment node that changes the variable, or the semantic `For`
    /// header for an implicit range step.
    pub update_node: NodeId,
    /// Exact per-iteration step when all affine coefficients are integer
    /// literals and checked `i64` arithmetic succeeds.
    pub step: Option<i64>,
    /// Direction implied by [`Self::step`].
    pub direction: IvDirection,
    /// Sorted definitions that initialize the variable before the loop. A
    /// range-loop binder is its own initialization definition.
    pub init_defs: Vec<NodeId>,
}

/// Evidence that a loop guard compares an induction variable with an invariant.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum TripEvidence {
    /// A direction-compatible comparison supplies counted-loop evidence.
    Counted {
        /// Induction variable named by the guard.
        iv: Arc<str>,
        /// Comparison or semantic range-expression node.
        comparison: NodeId,
        /// Loop-invariant bound operand.
        bound_node: NodeId,
    },
    /// No supported counted-loop guard was proved.
    Unknown,
}

/// Trip evidence for one loop.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct LoopBoundEvidence {
    /// Stable loop id from [`LoopForest::loops`].
    pub loop_id: usize,
    /// Counted or unknown evidence.
    pub trip: TripEvidence,
}

/// Complete loop-induction report for one function.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct InductionAnalysis {
    /// Function whose loop forest was analyzed.
    pub function: NodeId,
    /// One invariant report per loop, in forest order.
    pub invariants: Vec<LoopInvariants>,
    /// Basic variables followed by one-level derived variables, canonically
    /// sorted by loop, name, and update node.
    pub ivs: Vec<InductionVariable>,
    /// One trip-evidence record per loop, in forest order.
    pub bounds: Vec<LoopBoundEvidence>,
}

#[derive(Debug)]
struct LoopContext {
    loop_id: usize,
    body: BTreeSet<NodeId>,
    region: BTreeSet<NodeId>,
    invariant: BTreeSet<NodeId>,
}

/// Computes loop invariants, induction variables, and loop-bound evidence.
///
/// The supplied forest remains authoritative for loop identity and nesting.
/// Every represented CFG body node must belong to `function`; malformed or
/// foreign node ids return [`AnalysisError::UnknownNode`]. The computation is
/// feature-free, deterministic, bounded by each finite loop region, read-only,
/// and emits only [`InductionAnalysis`].
pub fn loop_induction(
    cpg: &CodePropertyGraph,
    function: NodeId,
    forest: &LoopForest,
) -> Result<InductionAnalysis, AnalysisError> {
    let view = function_cfg_view(cpg, function)?;
    let root = view.projection.node_id(view.entry);
    let dominators = dominator_tree(&view.projection, root)?;
    let facts = variable_facts(cpg, function);
    let scope = function_scope_nodes(cpg, function);

    let mut contexts = Vec::with_capacity(forest.loops.len());
    let mut invariants = Vec::with_capacity(forest.loops.len());
    let mut basic_ivs = Vec::new();

    for natural_loop in &forest.loops {
        let body: BTreeSet<_> = natural_loop.body.iter().copied().collect();
        let referenced_nodes = std::iter::once(natural_loop.header)
            .chain(natural_loop.entries.iter().copied())
            .chain(natural_loop.latches.iter().copied())
            .chain(natural_loop.body.iter().copied())
            .chain(
                natural_loop
                    .exits
                    .iter()
                    .flat_map(|exit| [exit.source, exit.target]),
            );
        for node in referenced_nodes {
            if !scope.contains(&node) || view.projection.dense_index(node).is_none() {
                return Err(AnalysisError::UnknownNode(node));
            }
        }
        let region = loop_region_nodes(cpg, &body, &scope);
        let invariant = invariant_fixed_point(cpg, &region, &facts);
        let hoistable = hoistable_nodes(
            cpg,
            natural_loop,
            &body,
            &region,
            &invariant,
            &facts,
            &dominators,
        );
        invariants.push(LoopInvariants {
            loop_id: natural_loop.id,
            invariant_nodes: invariant.iter().copied().collect(),
            hoistable,
        });

        basic_ivs.extend(basic_induction_variables(
            cpg,
            natural_loop,
            &body,
            &region,
            &invariant,
            &facts,
            &dominators,
        ));
        contexts.push(LoopContext {
            loop_id: natural_loop.id,
            body,
            region,
            invariant,
        });
    }

    canonicalize_ivs(&mut basic_ivs);
    let mut ivs = basic_ivs.clone();
    for (natural_loop, context) in forest.loops.iter().zip(&contexts) {
        ivs.extend(derived_induction_variables(
            cpg,
            natural_loop,
            context,
            &facts,
            &dominators,
            &basic_ivs,
        ));
    }
    canonicalize_ivs(&mut ivs);

    let bounds = forest
        .loops
        .iter()
        .zip(&contexts)
        .map(|(natural_loop, context)| LoopBoundEvidence {
            loop_id: natural_loop.id,
            trip: counted_trip_evidence(cpg, natural_loop, context, &ivs),
        })
        .collect();

    Ok(InductionAnalysis {
        function,
        invariants,
        ivs,
        bounds,
    })
}

fn loop_region_nodes(
    cpg: &CodePropertyGraph,
    body: &BTreeSet<NodeId>,
    scope: &rustc_hash::FxHashSet<NodeId>,
) -> BTreeSet<NodeId> {
    let mut region = BTreeSet::new();
    let mut pending: Vec<_> = body.iter().rev().copied().collect();
    while let Some(node) = pending.pop() {
        if !scope.contains(&node) || !region.insert(node) {
            continue;
        }
        if !body.contains(&node)
            && cpg
                .node(node)
                .is_some_and(|node| matches!(node.kind, CpgNodeKind::Function { .. }))
        {
            continue;
        }
        let mut children = cpg.ast_children(node).to_vec();
        children.reverse();
        pending.extend(children);
    }
    region
}

fn invariant_fixed_point(
    cpg: &CodePropertyGraph,
    region: &BTreeSet<NodeId>,
    facts: &VarFacts,
) -> BTreeSet<NodeId> {
    let candidates: Vec<_> = region
        .iter()
        .copied()
        .filter(|&node| invariant_candidate(cpg, node, facts))
        .collect();
    let mut invariant = BTreeSet::new();
    let mut use_cache: BTreeMap<NodeId, Arc<[NodeId]>> = BTreeMap::new();
    let mut uses_by_candidate = BTreeMap::new();
    for &candidate in &candidates {
        uses_by_candidate.insert(
            candidate,
            candidate_uses(cpg, candidate, facts, &mut use_cache),
        );
    }

    for _ in 0..=candidates.len() {
        let mut changed = false;
        for &candidate in &candidates {
            if invariant.contains(&candidate) {
                continue;
            }
            let uses = &uses_by_candidate[&candidate];
            let operands_are_invariant = uses.iter().all(|&use_site| {
                let predecessors: Vec<_> = cpg
                    .dfg_predecessors(use_site)
                    .into_iter()
                    .filter_map(|(source, kind)| (kind == DfgEdgeKind::DefUse).then_some(source))
                    .collect();
                !predecessors.is_empty()
                    && predecessors
                        .iter()
                        .all(|source| !region.contains(source) || invariant.contains(source))
            });
            if operands_are_invariant {
                changed |= invariant.insert(candidate);
            }
        }
        if !changed {
            break;
        }
    }
    invariant
}

fn invariant_candidate(cpg: &CodePropertyGraph, node: NodeId, facts: &VarFacts) -> bool {
    match cpg.node(node).map(|node| &node.kind) {
        Some(CpgNodeKind::Literal { .. }) => true,
        Some(CpgNodeKind::Identifier { .. }) => !facts.uses_at(node).is_empty(),
        Some(CpgNodeKind::UnaryOp { .. }) => !cpg.ast_children(node).is_empty(),
        Some(CpgNodeKind::BinaryOp { .. }) => cpg.ast_children(node).len() >= 2,
        Some(CpgNodeKind::Variable { .. }) => {
            !is_for_pattern_binding(cpg, node) && definition_rhs(cpg, node).is_some()
        }
        Some(CpgNodeKind::Assignment { .. }) => definition_rhs(cpg, node).is_some(),
        _ => false,
    }
}

/// One continuation in postorder candidate-use summarization.
#[derive(Debug)]
enum CandidateUseFrame {
    Enter(NodeId),
    Exit { node: NodeId, children: Vec<NodeId> },
}

fn candidate_uses(
    cpg: &CodePropertyGraph,
    root: NodeId,
    facts: &VarFacts,
    cache: &mut BTreeMap<NodeId, Arc<[NodeId]>>,
) -> Arc<[NodeId]> {
    if let Some(cached) = cache.get(&root) {
        return cached.clone();
    }

    // Build summaries in postorder so an outer expression computes every
    // acyclic descendant once even when parents have smaller NodeIds. Keep the
    // new summaries local until the whole traversal proves acyclic: a summary
    // assembled across an active-path back edge would not be transitive.
    let mut frames = vec![CandidateUseFrame::Enter(root)];
    let mut active = BTreeSet::new();
    let mut local: BTreeMap<NodeId, Arc<[NodeId]>> = BTreeMap::new();
    let mut cyclic = false;
    while let Some(frame) = frames.pop() {
        match frame {
            CandidateUseFrame::Enter(node) => {
                if cache.contains_key(&node) || local.contains_key(&node) {
                    continue;
                }
                if !active.insert(node) {
                    cyclic = true;
                    break;
                }
                let children = if node != root
                    && cpg
                        .node(node)
                        .is_some_and(|node| matches!(node.kind, CpgNodeKind::Function { .. }))
                {
                    Vec::new()
                } else {
                    cpg.ast_children(node).to_vec()
                };
                frames.push(CandidateUseFrame::Exit {
                    node,
                    children: children.clone(),
                });
                frames.extend(children.into_iter().rev().map(CandidateUseFrame::Enter));
            }
            CandidateUseFrame::Exit { node, children } => {
                let mut uses = BTreeSet::new();
                if !facts.uses_at(node).is_empty() {
                    uses.insert(node);
                }
                for child in children {
                    let Some(child_uses) = local.get(&child).or_else(|| cache.get(&child)) else {
                        cyclic = true;
                        break;
                    };
                    uses.extend(child_uses.iter().copied());
                }
                if cyclic {
                    break;
                }
                active.remove(&node);
                local.insert(node, uses.into_iter().collect::<Vec<_>>().into());
            }
        }
    }

    if !cyclic {
        cache.extend(local);
        return cache
            .get(&root)
            .expect("postorder traversal summarizes its root")
            .clone();
    }

    // Malformed AST cycles still have a precise finite reachability meaning.
    // A global visited set computes it without recursive calls; caching the
    // completed root summary makes later candidates that reach this cycle
    // safe to reuse it.
    let mut uses = BTreeSet::new();
    let mut seen = BTreeSet::new();
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        if !seen.insert(node) {
            continue;
        }
        if !facts.uses_at(node).is_empty() {
            uses.insert(node);
        }
        if node != root
            && cpg
                .node(node)
                .is_some_and(|node| matches!(node.kind, CpgNodeKind::Function { .. }))
        {
            continue;
        }
        let mut children = cpg.ast_children(node).to_vec();
        children.reverse();
        pending.extend(children);
    }
    let uses: Arc<[NodeId]> = uses.into_iter().collect::<Vec<_>>().into();
    cache.insert(root, uses.clone());
    uses
}

fn hoistable_nodes(
    cpg: &CodePropertyGraph,
    natural_loop: &NaturalLoop,
    body: &BTreeSet<NodeId>,
    region: &BTreeSet<NodeId>,
    invariant: &BTreeSet<NodeId>,
    facts: &VarFacts,
    dominators: &DominatorTree,
) -> Vec<NodeId> {
    invariant
        .iter()
        .copied()
        .filter(|&candidate| {
            matches!(
                cpg.node(candidate).map(|node| &node.kind),
                Some(
                    CpgNodeKind::UnaryOp { .. }
                        | CpgNodeKind::BinaryOp { .. }
                        | CpgNodeKind::Variable { .. }
                        | CpgNodeKind::Assignment { .. }
                )
            )
        })
        .filter(|&candidate| {
            let Some(anchor) = cfg_anchor(cpg, candidate, body) else {
                return false;
            };
            natural_loop
                .latches
                .iter()
                .chain(natural_loop.exits.iter().map(|exit| &exit.source))
                .all(|&observation| dominators.dominates(anchor, observation))
        })
        .filter(|&candidate| {
            facts.definitions_at(candidate).iter().all(|definition| {
                let unique = facts
                    .definitions
                    .iter()
                    .filter(|(node, definitions)| {
                        region.contains(node)
                            && definitions
                                .iter()
                                .any(|other| other.variable == definition.variable)
                    })
                    .count()
                    == 1;
                let Some(definition_anchor) = cfg_anchor(cpg, candidate, body) else {
                    return false;
                };
                unique
                    && cpg
                        .dfg_successors(candidate)
                        .into_iter()
                        .filter(|(target, kind)| {
                            *kind == DfgEdgeKind::DefUse && region.contains(target)
                        })
                        .all(|(target, _)| {
                            cfg_anchor(cpg, target, body).is_some_and(|use_anchor| {
                                dominators.dominates(definition_anchor, use_anchor)
                            })
                        })
            })
        })
        .collect()
}

fn cfg_anchor(cpg: &CodePropertyGraph, node: NodeId, body: &BTreeSet<NodeId>) -> Option<NodeId> {
    if body.contains(&node) {
        return Some(node);
    }
    cpg.ast_ancestors(node)
        .into_iter()
        .find(|ancestor| body.contains(ancestor))
}

fn basic_induction_variables(
    cpg: &CodePropertyGraph,
    natural_loop: &NaturalLoop,
    body: &BTreeSet<NodeId>,
    region: &BTreeSet<NodeId>,
    invariant: &BTreeSet<NodeId>,
    facts: &VarFacts,
    dominators: &DominatorTree,
) -> Vec<InductionVariable> {
    let mut ivs = Vec::new();
    for &node in body {
        let Some(variable) = definition_name(cpg, node) else {
            continue;
        };
        let Some((operand, additive)) = basic_update_shape(cpg, node, &variable) else {
            continue;
        };
        if !operand_is_invariant(cpg, operand, invariant) {
            continue;
        }
        if !unique_loop_definition(facts, &variable, region, node)
            || !dominates_all_latches(node, natural_loop, dominators)
        {
            continue;
        }
        let step = integer_constant(cpg, operand).and_then(|amount| {
            if additive {
                Some(amount)
            } else {
                amount.checked_neg()
            }
        });
        ivs.push(InductionVariable {
            loop_id: natural_loop.id,
            variable: variable.clone(),
            update_node: node,
            step,
            direction: direction_from_step(step),
            init_defs: initial_definitions(cpg, node, &variable, region),
        });
    }

    if let Some((binder, _range, start, bound)) = for_range_parts(cpg, natural_loop.header) {
        if operand_is_invariant(cpg, start, invariant)
            && operand_is_invariant(cpg, bound, invariant)
        {
            if let Some(variable) =
                definition_name(cpg, binder).or_else(|| identifier_name(cpg, binder))
            {
                if !ivs.iter().any(|iv| iv.variable == variable) {
                    ivs.push(InductionVariable {
                        loop_id: natural_loop.id,
                        variable,
                        update_node: natural_loop.header,
                        step: Some(1),
                        direction: IvDirection::Increasing,
                        init_defs: vec![binder],
                    });
                }
            }
        }
    }

    canonicalize_ivs(&mut ivs);
    ivs
}

fn basic_update_shape(
    cpg: &CodePropertyGraph,
    assignment: NodeId,
    variable: &str,
) -> Option<(NodeId, bool)> {
    let CpgNodeKind::Assignment { operator } = &cpg.node(assignment)?.kind else {
        return None;
    };
    let children = cpg.ast_children(assignment);
    match &**operator {
        "+=" => children.get(1).copied().map(|operand| (operand, true)),
        "-=" => children.get(1).copied().map(|operand| (operand, false)),
        "=" => {
            let rhs = children.get(1).copied()?;
            let CpgNodeKind::BinaryOp { operator } = &cpg.node(rhs)?.kind else {
                return None;
            };
            let operands = cpg.ast_children(rhs);
            let [left, right, ..] = operands else {
                return None;
            };
            match &**operator {
                "+" if identifier_is(cpg, *left, variable) => Some((*right, true)),
                "+" if identifier_is(cpg, *right, variable) => Some((*left, true)),
                "-" if identifier_is(cpg, *left, variable) => Some((*right, false)),
                _ => None,
            }
        }
        _ => None,
    }
}

fn derived_induction_variables(
    cpg: &CodePropertyGraph,
    natural_loop: &NaturalLoop,
    context: &LoopContext,
    facts: &VarFacts,
    dominators: &DominatorTree,
    basic_ivs: &[InductionVariable],
) -> Vec<InductionVariable> {
    let basics: BTreeMap<Arc<str>, &InductionVariable> = basic_ivs
        .iter()
        .filter(|iv| iv.loop_id == context.loop_id)
        .map(|iv| (iv.variable.clone(), iv))
        .collect();
    if basics.is_empty() {
        return Vec::new();
    }

    let mut derived = Vec::new();
    for &node in &context.body {
        let Some(variable) = definition_name(cpg, node) else {
            continue;
        };
        if basics.contains_key(&variable) {
            continue;
        }
        if !unique_loop_definition(facts, &variable, &context.region, node)
            || !dominates_all_latches(node, natural_loop, dominators)
        {
            continue;
        }
        let Some(rhs) = definition_rhs(cpg, node) else {
            continue;
        };
        let mut seen = BTreeSet::new();
        let Some(affine) = affine_in_basic(cpg, rhs, &basics, &context.invariant, 3, &mut seen)
        else {
            continue;
        };
        let base = basics[&affine.base];
        let step = match (base.step, affine.coefficient) {
            (Some(step), Some(coefficient)) => step.checked_mul(coefficient),
            _ => None,
        };
        derived.push(InductionVariable {
            loop_id: context.loop_id,
            variable: variable.clone(),
            update_node: node,
            step,
            direction: direction_from_step(step),
            init_defs: external_definitions(facts, &variable, &context.region),
        });
    }
    canonicalize_ivs(&mut derived);
    derived
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AffineBase {
    base: Arc<str>,
    coefficient: Option<i64>,
}

/// One continuation in the bounded affine-expression evaluator.
#[derive(Debug)]
enum AffineFrame {
    Enter {
        node: NodeId,
        depth: usize,
    },
    ApplyUnary {
        node: NodeId,
        operator: Arc<str>,
    },
    AfterLeft {
        node: NodeId,
        operator: Arc<str>,
        left: NodeId,
        right: NodeId,
        child_depth: usize,
    },
    ApplyBinary {
        node: NodeId,
        operator: Arc<str>,
        left: NodeId,
        right: NodeId,
        left_affine: Option<AffineBase>,
    },
}

fn affine_in_basic(
    cpg: &CodePropertyGraph,
    node: NodeId,
    basics: &BTreeMap<Arc<str>, &InductionVariable>,
    invariant: &BTreeSet<NodeId>,
    depth: usize,
    seen: &mut BTreeSet<NodeId>,
) -> Option<AffineBase> {
    let mut frames = vec![AffineFrame::Enter { node, depth }];
    let mut values: Vec<Option<AffineBase>> = Vec::new();

    while let Some(frame) = frames.pop() {
        match frame {
            AffineFrame::Enter { node, depth } => {
                if !seen.insert(node) {
                    values.push(None);
                    continue;
                }
                if let Some(name) = identifier_name(cpg, node) {
                    let affine = basics.contains_key(&name).then_some(AffineBase {
                        base: name,
                        coefficient: Some(1),
                    });
                    seen.remove(&node);
                    values.push(affine);
                    continue;
                }
                if depth == 0 {
                    seen.remove(&node);
                    values.push(None);
                    continue;
                }

                match cpg.node(node).map(|node| &node.kind) {
                    Some(CpgNodeKind::UnaryOp { operator }) => {
                        let Some(operand) = cpg.ast_children(node).first().copied() else {
                            seen.remove(&node);
                            values.push(None);
                            continue;
                        };
                        frames.push(AffineFrame::ApplyUnary {
                            node,
                            operator: operator.clone(),
                        });
                        frames.push(AffineFrame::Enter {
                            node: operand,
                            depth: depth - 1,
                        });
                    }
                    Some(CpgNodeKind::BinaryOp { operator }) => {
                        let children = cpg.ast_children(node);
                        let [left, right, ..] = children else {
                            seen.remove(&node);
                            values.push(None);
                            continue;
                        };
                        frames.push(AffineFrame::AfterLeft {
                            node,
                            operator: operator.clone(),
                            left: *left,
                            right: *right,
                            child_depth: depth - 1,
                        });
                        frames.push(AffineFrame::Enter {
                            node: *left,
                            depth: depth - 1,
                        });
                    }
                    _ => {
                        seen.remove(&node);
                        values.push(None);
                    }
                }
            }
            AffineFrame::ApplyUnary { node, operator } => {
                let affine = values.pop().expect("unary child produces one value");
                let result = match &*operator {
                    "+" => affine,
                    "-" => affine.map(|mut affine| {
                        affine.coefficient = affine.coefficient.and_then(i64::checked_neg);
                        affine
                    }),
                    _ => None,
                };
                seen.remove(&node);
                values.push(result);
            }
            AffineFrame::AfterLeft {
                node,
                operator,
                left,
                right,
                child_depth,
            } => {
                let left_affine = values.pop().expect("left child produces one value");
                frames.push(AffineFrame::ApplyBinary {
                    node,
                    operator,
                    left,
                    right,
                    left_affine,
                });
                frames.push(AffineFrame::Enter {
                    node: right,
                    depth: child_depth,
                });
            }
            AffineFrame::ApplyBinary {
                node,
                operator,
                left,
                right,
                left_affine,
            } => {
                let right_affine = values.pop().expect("right child produces one value");
                let result = affine_binary(
                    cpg,
                    &operator,
                    left,
                    right,
                    invariant,
                    left_affine,
                    right_affine,
                );
                seen.remove(&node);
                values.push(result);
            }
        }
    }

    values.pop().flatten()
}

fn affine_binary(
    cpg: &CodePropertyGraph,
    operator: &str,
    left: NodeId,
    right: NodeId,
    invariant: &BTreeSet<NodeId>,
    left_affine: Option<AffineBase>,
    right_affine: Option<AffineBase>,
) -> Option<AffineBase> {
    match operator {
        "+" | "-" => match (left_affine, right_affine) {
            (Some(affine), None) if operand_is_invariant(cpg, right, invariant) => Some(affine),
            (None, Some(mut affine)) if operand_is_invariant(cpg, left, invariant) => {
                if operator == "-" {
                    affine.coefficient = affine.coefficient.and_then(i64::checked_neg);
                }
                Some(affine)
            }
            (Some(mut left), Some(right)) if left.base == right.base => {
                left.coefficient = match (left.coefficient, right.coefficient) {
                    (Some(left), Some(right)) if operator == "+" => left.checked_add(right),
                    (Some(left), Some(right)) => left.checked_sub(right),
                    _ => None,
                };
                Some(left)
            }
            _ => None,
        },
        "*" => match (left_affine, right_affine) {
            (Some(mut affine), None) if operand_is_invariant(cpg, right, invariant) => {
                affine.coefficient = multiply_coefficient(cpg, affine.coefficient, right);
                Some(affine)
            }
            (None, Some(mut affine)) if operand_is_invariant(cpg, left, invariant) => {
                affine.coefficient = multiply_coefficient(cpg, affine.coefficient, left);
                Some(affine)
            }
            _ => None,
        },
        _ => None,
    }
}

fn multiply_coefficient(
    cpg: &CodePropertyGraph,
    coefficient: Option<i64>,
    operand: NodeId,
) -> Option<i64> {
    match (coefficient, integer_constant(cpg, operand)) {
        (Some(coefficient), Some(multiplier)) => coefficient.checked_mul(multiplier),
        _ => None,
    }
}

fn counted_trip_evidence(
    cpg: &CodePropertyGraph,
    natural_loop: &NaturalLoop,
    context: &LoopContext,
    ivs: &[InductionVariable],
) -> TripEvidence {
    if natural_loop.irreducible {
        return TripEvidence::Unknown;
    }
    let loop_ivs: Vec<_> = ivs
        .iter()
        .filter(|iv| iv.loop_id == natural_loop.id)
        .collect();

    if let Some((_binder, range, _start, bound)) = for_range_parts(cpg, natural_loop.header) {
        if operand_is_invariant(cpg, bound, &context.invariant) {
            if let Some(iv) = loop_ivs
                .iter()
                .find(|iv| iv.update_node == natural_loop.header)
            {
                return TripEvidence::Counted {
                    iv: iv.variable.clone(),
                    comparison: range,
                    bound_node: bound,
                };
            }
        }
    }

    for exit in &natural_loop.exits {
        let Some(comparison) = comparison_expression(cpg, exit.source) else {
            continue;
        };
        let CpgNodeKind::BinaryOp { operator } = &cpg
            .node(comparison)
            .expect("comparison expression resolves to a CPG node")
            .kind
        else {
            continue;
        };
        let operands = cpg.ast_children(comparison);
        let [left, right, ..] = operands else {
            continue;
        };
        for iv in &loop_ivs {
            let (normalized, bound) = if identifier_is(cpg, *left, &iv.variable) {
                (&**operator, *right)
            } else if identifier_is(cpg, *right, &iv.variable) {
                let Some(reversed) = reverse_comparison(operator) else {
                    continue;
                };
                (reversed, *left)
            } else {
                continue;
            };
            if comparison_direction_matches(normalized, iv.direction)
                && operand_is_invariant(cpg, bound, &context.invariant)
            {
                return TripEvidence::Counted {
                    iv: iv.variable.clone(),
                    comparison,
                    bound_node: bound,
                };
            }
        }
    }
    TripEvidence::Unknown
}

fn comparison_expression(cpg: &CodePropertyGraph, source: NodeId) -> Option<NodeId> {
    let mut pending = vec![source];
    let mut seen = BTreeSet::new();
    while let Some(node) = pending.pop() {
        if !seen.insert(node) {
            continue;
        }
        if matches!(
            cpg.node(node).map(|node| &node.kind),
            Some(CpgNodeKind::BinaryOp { operator })
                if matches!(&**operator, "<" | "<=" | ">" | ">=" | "!=")
        ) {
            return Some(node);
        }
        let mut children = cpg.ast_children(node).to_vec();
        children.reverse();
        pending.extend(children);
    }
    None
}

fn comparison_direction_matches(operator: &str, direction: IvDirection) -> bool {
    match direction {
        IvDirection::Increasing => matches!(operator, "<" | "<=" | "!="),
        IvDirection::Decreasing => matches!(operator, ">" | ">=" | "!="),
        IvDirection::Stationary | IvDirection::Unknown => false,
    }
}

fn reverse_comparison(operator: &str) -> Option<&'static str> {
    match operator {
        "<" => Some(">"),
        "<=" => Some(">="),
        ">" => Some("<"),
        ">=" => Some("<="),
        "!=" => Some("!="),
        _ => None,
    }
}

fn for_range_parts(
    cpg: &CodePropertyGraph,
    header: NodeId,
) -> Option<(NodeId, NodeId, NodeId, NodeId)> {
    if !matches!(cpg.node(header)?.kind, CpgNodeKind::For) {
        return None;
    }
    let children = cpg.ast_children(header);
    let binder = children.iter().copied().find(|&child| {
        matches!(
            cpg.node(child).map(|node| &node.kind),
            Some(CpgNodeKind::Variable { .. } | CpgNodeKind::Identifier { .. })
        )
    })?;
    let range = children.iter().copied().find(|&child| {
        matches!(
            cpg.node(child).map(|node| &node.kind),
            Some(CpgNodeKind::BinaryOp { operator }) if matches!(&**operator, ".." | "..=")
        )
    })?;
    let operands = cpg.ast_children(range);
    let [start, bound, ..] = operands else {
        return None;
    };
    Some((binder, range, *start, *bound))
}

fn is_for_pattern_binding(cpg: &CodePropertyGraph, node: NodeId) -> bool {
    let mut current = node;
    for parent in cpg.ast_ancestors(node) {
        if matches!(
            cpg.node(parent).map(|node| &node.kind),
            Some(CpgNodeKind::For)
        ) {
            return cpg.ast_children(parent).first().copied() == Some(current);
        }
        current = parent;
    }
    false
}

fn operand_is_invariant(
    cpg: &CodePropertyGraph,
    operand: NodeId,
    invariant: &BTreeSet<NodeId>,
) -> bool {
    invariant.contains(&operand)
        || matches!(
            cpg.node(operand).map(|node| &node.kind),
            Some(CpgNodeKind::Literal { .. })
        )
}

fn integer_constant(cpg: &CodePropertyGraph, node: NodeId) -> Option<i64> {
    #[derive(Debug, Clone, Copy)]
    enum SignFrame {
        Positive,
        Negative,
    }

    let mut current = node;
    let mut seen = BTreeSet::new();
    let mut continuations = Vec::new();
    loop {
        if !seen.insert(current) {
            return None;
        }
        match cpg.node(current).map(|node| &node.kind) {
            Some(CpgNodeKind::Literal {
                kind: LiteralKind::Integer(value),
            }) => {
                let mut value = *value;
                while let Some(frame) = continuations.pop() {
                    value = match frame {
                        SignFrame::Positive => value,
                        SignFrame::Negative => value.checked_neg()?,
                    };
                }
                return Some(value);
            }
            Some(CpgNodeKind::UnaryOp { operator }) => {
                let operand = cpg.ast_children(current).first().copied()?;
                continuations.push(match &**operator {
                    "+" => SignFrame::Positive,
                    "-" => SignFrame::Negative,
                    _ => return None,
                });
                current = operand;
            }
            _ => return None,
        }
    }
}

fn direction_from_step(step: Option<i64>) -> IvDirection {
    match step {
        Some(step) if step > 0 => IvDirection::Increasing,
        Some(step) if step < 0 => IvDirection::Decreasing,
        Some(_) => IvDirection::Stationary,
        None => IvDirection::Unknown,
    }
}

fn definition_name(cpg: &CodePropertyGraph, node: NodeId) -> Option<Arc<str>> {
    match cpg.node(node).map(|node| &node.kind) {
        Some(CpgNodeKind::Variable { name, .. }) => Some(name.clone()),
        Some(CpgNodeKind::Assignment { .. }) => cpg
            .ast_children(node)
            .first()
            .copied()
            .and_then(|target| identifier_name(cpg, target)),
        _ => None,
    }
}

fn definition_rhs(cpg: &CodePropertyGraph, node: NodeId) -> Option<NodeId> {
    match cpg.node(node).map(|node| &node.kind) {
        Some(CpgNodeKind::Assignment { .. }) => cpg.ast_children(node).get(1).copied(),
        Some(CpgNodeKind::Variable { name, .. }) => {
            let children = cpg.ast_children(node);
            let binder = children
                .iter()
                .copied()
                .find(|&child| identifier_is(cpg, child, name));
            children
                .iter()
                .copied()
                .rev()
                .find(|child| Some(*child) != binder)
        }
        _ => None,
    }
}

fn identifier_name(cpg: &CodePropertyGraph, node: NodeId) -> Option<Arc<str>> {
    match cpg.node(node).map(|node| &node.kind) {
        Some(CpgNodeKind::Identifier { name, .. }) => Some(name.clone()),
        _ => None,
    }
}

fn identifier_is(cpg: &CodePropertyGraph, node: NodeId, expected: &str) -> bool {
    identifier_name(cpg, node).is_some_and(|name| &*name == expected)
}

fn initial_definitions(
    cpg: &CodePropertyGraph,
    update: NodeId,
    variable: &str,
    region: &BTreeSet<NodeId>,
) -> Vec<NodeId> {
    let mut initial = BTreeSet::new();
    let mut seen = BTreeSet::new();
    let mut pending = vec![update];
    while let Some(node) = pending.pop() {
        if !seen.insert(node) {
            continue;
        }
        if identifier_is(cpg, node, variable) {
            for (definition, kind) in cpg.dfg_predecessors(node) {
                if kind == DfgEdgeKind::DefUse && !region.contains(&definition) {
                    initial.insert(definition);
                }
            }
        }
        let mut children = cpg.ast_children(node).to_vec();
        children.reverse();
        pending.extend(children);
    }
    initial.into_iter().collect()
}

fn external_definitions(
    facts: &VarFacts,
    variable: &Arc<str>,
    region: &BTreeSet<NodeId>,
) -> Vec<NodeId> {
    let Some(variable_id) = facts.universe.id_of(variable) else {
        return Vec::new();
    };
    facts
        .definitions
        .iter()
        .filter_map(|(&node, definitions)| {
            (!region.contains(&node)
                && definitions
                    .iter()
                    .any(|definition| definition.variable == variable_id))
            .then_some(node)
        })
        .collect()
}

fn unique_loop_definition(
    facts: &VarFacts,
    variable: &Arc<str>,
    region: &BTreeSet<NodeId>,
    expected: NodeId,
) -> bool {
    let Some(variable_id) = facts.universe.id_of(variable) else {
        return false;
    };
    let mut definitions = facts.definitions.iter().filter_map(|(&node, defined)| {
        (region.contains(&node)
            && defined
                .iter()
                .any(|definition| definition.variable == variable_id))
        .then_some(node)
    });
    definitions.next() == Some(expected) && definitions.next().is_none()
}

fn dominates_all_latches(
    update: NodeId,
    natural_loop: &NaturalLoop,
    dominators: &DominatorTree,
) -> bool {
    natural_loop
        .latches
        .iter()
        .all(|&latch| dominators.dominates(update, latch))
}

fn canonicalize_ivs(ivs: &mut Vec<InductionVariable>) {
    ivs.sort_by(|left, right| {
        (left.loop_id, &left.variable, left.update_node).cmp(&(
            right.loop_id,
            &right.variable,
            right.update_node,
        ))
    });
    ivs.dedup_by(|left, right| {
        left.loop_id == right.loop_id
            && left.variable == right.variable
            && left.update_node == right.update_node
    });
}

#[cfg(test)]
mod tests;
