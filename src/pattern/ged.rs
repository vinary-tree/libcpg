//! Deterministic bipartite graph-edit-distance upper bound.
//!
//! A Riesen–Bunke star-cost matrix selects a partial node correspondence via
//! Hungarian assignment. The public bound is then evaluated from the concrete
//! edit script induced by that correspondence, including edge kinds and
//! parallel multiplicity. Running both orientations and retaining the cheaper
//! script makes the reported bound symmetric.

use rustc_hash::FxHashMap;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use super::NodeKindTag;
use crate::{AnalysisError, CodePropertyGraph, CpgEdgeKind, NodeId};

/// Hard ceiling for the cubic bipartite assignment.
pub const MAX_GED_NODES: usize = 512;

/// Edit costs and the assignment-size budget.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct GedOptions {
    /// Cost of substituting two different [`NodeKindTag`] labels.
    pub node_sub: f64,
    /// Cost of inserting or deleting one node.
    pub node_ins_del: f64,
    /// Cost of inserting or deleting one typed edge.
    pub edge_ins_del: f64,
    /// Largest graph admitted to Hungarian assignment.
    ///
    /// Values above [`MAX_GED_NODES`] are clamped to the hard ceiling. If
    /// either graph exceeds the effective limit, [`ged_upper_bound`] returns
    /// the former size/label surrogate with [`GedBound::capped`] set.
    pub max_nodes: usize,
}

impl Default for GedOptions {
    fn default() -> Self {
        Self {
            node_sub: 1.0,
            node_ins_del: 1.0,
            edge_ins_del: 0.5,
            max_nodes: MAX_GED_NODES,
        }
    }
}

/// A normalized edit-distance upper bound, or an explicitly flagged fallback.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct GedBound {
    /// Edit cost of the retained concrete script.
    ///
    /// When [`Self::capped`] is true, this is the legacy surrogate distance
    /// scaled by the delete-all/insert-all cost and is not a proven GED bound.
    pub upper: f64,
    /// `upper` divided by the delete-all/insert-all cost, in `[0, 1]`.
    pub normalized: f64,
    /// Whether the node budget forced the explicitly documented surrogate.
    pub capped: bool,
}

/// One exact node correspondence induced by the bipartite GED script.
///
/// This remains crate-private because the public GED contract is the symmetric
/// upper bound. Analyses that need a concrete edit script reuse this assignment
/// instead of implementing a second aligner.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GedAlignment {
    /// Matched `(source, target)` nodes, sorted by source then target id.
    pub(crate) pairs: Vec<(NodeId, NodeId)>,
    /// Concrete cost of the retained script.
    pub(crate) cost: f64,
}

#[derive(Debug, Clone, PartialEq)]
struct AssignmentScript {
    cost: f64,
    pairs: Vec<(NodeId, NodeId)>,
}

/// Computes a deterministic bipartite upper bound on graph edit distance.
///
/// Nodes are ordered by stable [`NodeId`]. Assignment costs combine node-kind
/// substitution with total-degree difference, while deletion/insertion dummy
/// slots carry the corresponding node and incident-edge star cost. The chosen
/// mapping is evaluated as a concrete edit script: unmatched nodes are
/// inserted/deleted, different node tags are substituted, and typed directed
/// edge multisets are inserted/deleted. The cheaper of both assignment
/// orientations and the delete-all/insert-all script is returned.
///
/// All costs must be finite and nonnegative, and derived assignment arithmetic
/// must remain finite. Exceeding the effective node budget returns the legacy
/// size/label surrogate with [`GedBound::capped`] set rather than silently
/// claiming a computed bound.
pub fn ged_upper_bound(
    first: &CodePropertyGraph,
    second: &CodePropertyGraph,
    options: &GedOptions,
) -> Result<GedBound, AnalysisError> {
    validate_options(options)?;

    let normalization = delete_all_cost(first, second, options)?;
    let cap = options.max_nodes.min(MAX_GED_NODES);
    if first.node_count() > cap || second.node_count() > cap {
        if normalization == 0.0 {
            return Ok(GedBound {
                upper: 0.0,
                normalized: 0.0,
                capped: true,
            });
        }
        let normalized = 1.0 - legacy_graph_edit_similarity(first, second, 0.7, 0.3);
        return Ok(GedBound {
            upper: normalized * normalization,
            normalized,
            capped: true,
        });
    }

    if normalization == 0.0 {
        return Ok(GedBound {
            upper: 0.0,
            normalized: 0.0,
            capped: false,
        });
    }

    let forward = assignment_script(first, second, options, normalization)?;
    let reverse = assignment_script(second, first, options, normalization)?;
    let upper = forward.cost.min(reverse.cost).min(normalization);
    Ok(GedBound {
        upper,
        normalized: (upper / normalization).clamp(0.0, 1.0),
        capped: false,
    })
}

/// Returns the deterministic concrete correspondence used by the symmetric
/// bipartite GED upper bound.
///
/// Unlike [`ged_upper_bound`], this operation cannot substitute the legacy
/// capped surrogate because a scalar similarity contains no correspondence.
/// The cheaper assignment orientation is normalized to `(source, target)`
/// pairs; equal-cost scripts use lexicographic pair order. A strictly cheaper
/// delete-all/insert-all script has no pairs.
pub(crate) fn ged_alignment(
    source: &CodePropertyGraph,
    target: &CodePropertyGraph,
    options: &GedOptions,
) -> Result<GedAlignment, AnalysisError> {
    validate_options(options)?;
    let cap = options.max_nodes.min(MAX_GED_NODES);
    let needed = source.node_count().max(target.node_count());
    if needed > cap {
        return Err(AnalysisError::BudgetExceeded {
            analysis: "cpg_diff",
            unit: "nodes per graph",
            needed: u64::try_from(needed).unwrap_or(u64::MAX),
            cap: u64::try_from(cap).unwrap_or(u64::MAX),
        });
    }

    let delete_all = delete_all_cost(source, target, options)?;
    if source.node_count() + target.node_count() == 0 {
        return Ok(GedAlignment {
            pairs: Vec::new(),
            cost: 0.0,
        });
    }

    let forward = assignment_script(source, target, options, delete_all)?;
    let reverse = assignment_script(target, source, options, delete_all)?;
    let mut reverse_pairs = reverse
        .pairs
        .into_iter()
        .map(|(target_node, source_node)| (source_node, target_node))
        .collect::<Vec<_>>();
    reverse_pairs.sort_unstable();
    let reverse = AssignmentScript {
        cost: reverse.cost,
        pairs: reverse_pairs,
    };
    let retained = if forward.cost < reverse.cost
        || (forward.cost == reverse.cost && forward.pairs <= reverse.pairs)
    {
        forward
    } else {
        reverse
    };

    if delete_all < retained.cost {
        Ok(GedAlignment {
            pairs: Vec::new(),
            cost: delete_all,
        })
    } else {
        Ok(GedAlignment {
            pairs: retained.pairs,
            cost: retained.cost,
        })
    }
}

pub(super) fn legacy_graph_edit_similarity(
    first: &CodePropertyGraph,
    second: &CodePropertyGraph,
    structural_weight: f64,
    label_weight: f64,
) -> f64 {
    let first_nodes = first.node_count();
    let second_nodes = second.node_count();
    let first_edges = first.edge_count();
    let second_edges = second.edge_count();

    let node_difference = first_nodes.abs_diff(second_nodes) as f64;
    let edge_difference = first_edges.abs_diff(second_edges) as f64;
    let node_similarity = 1.0 - node_difference / first_nodes.max(second_nodes).max(1) as f64;
    let edge_similarity = 1.0 - edge_difference / first_edges.max(second_edges).max(1) as f64;
    let label_similarity = node_kind_jaccard(first, second);

    (structural_weight * (0.6 * node_similarity + 0.4 * edge_similarity)
        + label_weight * label_similarity)
        .clamp(0.0, 1.0)
}

fn validate_options(options: &GedOptions) -> Result<(), AnalysisError> {
    if [options.node_sub, options.node_ins_del, options.edge_ins_del]
        .into_iter()
        .all(|cost| cost.is_finite() && cost >= 0.0)
    {
        Ok(())
    } else {
        Err(AnalysisError::InvalidInput {
            analysis: "ged_upper_bound",
            input: "options",
            reason: "edit costs must be finite and nonnegative",
        })
    }
}

fn delete_all_cost(
    first: &CodePropertyGraph,
    second: &CodePropertyGraph,
    options: &GedOptions,
) -> Result<f64, AnalysisError> {
    let nodes = (first.node_count() + second.node_count()) as f64 * options.node_ins_del;
    let edges = (first.edge_count() + second.edge_count()) as f64 * options.edge_ins_del;
    let cost = nodes + edges;
    if cost.is_finite() {
        Ok(cost)
    } else {
        Err(AnalysisError::InvalidInput {
            analysis: "ged_upper_bound",
            input: "options",
            reason: "edit costs overflow for the supplied graphs",
        })
    }
}

#[cfg(test)]
fn assignment_script_cost(
    source: &CodePropertyGraph,
    target: &CodePropertyGraph,
    options: &GedOptions,
    deletion_upper: f64,
) -> Result<f64, AnalysisError> {
    Ok(assignment_script(source, target, options, deletion_upper)?.cost)
}

fn assignment_script(
    source: &CodePropertyGraph,
    target: &CodePropertyGraph,
    options: &GedOptions,
    deletion_upper: f64,
) -> Result<AssignmentScript, AnalysisError> {
    let mut source_nodes: Vec<_> = source.nodes().collect();
    let mut target_nodes: Vec<_> = target.nodes().collect();
    source_nodes.sort_by_key(|node| node.id);
    target_nodes.sort_by_key(|node| node.id);

    let source_degrees: Vec<_> = source_nodes
        .iter()
        .map(|node| total_degree(source, node.id))
        .collect();
    let target_degrees: Vec<_> = target_nodes
        .iter()
        .map(|node| total_degree(target, node.id))
        .collect();

    let dimension = source_nodes.len() + target_nodes.len();
    if dimension == 0 {
        return Ok(AssignmentScript {
            cost: 0.0,
            pairs: Vec::new(),
        });
    }
    let max_degree = source_degrees
        .iter()
        .chain(&target_degrees)
        .copied()
        .max()
        .unwrap_or(0) as f64;
    let largest_entry = options
        .node_sub
        .max(options.node_ins_del)
        .mul_add(1.0, 0.5 * options.edge_ins_del * max_degree);
    let forbidden = deletion_upper + largest_entry + 1.0;
    if !forbidden.is_finite() || !(forbidden * dimension as f64).is_finite() {
        return Err(AnalysisError::InvalidInput {
            analysis: "ged_upper_bound",
            input: "options",
            reason: "assignment costs exceed finite numerical range",
        });
    }

    let source_count = source_nodes.len();
    let target_count = target_nodes.len();
    let mut costs = vec![vec![forbidden; dimension]; dimension];

    for source_index in 0..source_count {
        for target_index in 0..target_count {
            let label_cost = if NodeKindTag::from_kind(&source_nodes[source_index].kind)
                == NodeKindTag::from_kind(&target_nodes[target_index].kind)
            {
                0.0
            } else {
                options.node_sub
            };
            let degree_cost = source_degrees[source_index].abs_diff(target_degrees[target_index])
                as f64
                * options.edge_ins_del
                * 0.5;
            costs[source_index][target_index] = label_cost + degree_cost;
        }
        costs[source_index][target_count + source_index] =
            options.node_ins_del + 0.5 * options.edge_ins_del * source_degrees[source_index] as f64;
    }

    for target_index in 0..target_count {
        let insertion_row = source_count + target_index;
        costs[insertion_row][target_index] =
            options.node_ins_del + 0.5 * options.edge_ins_del * target_degrees[target_index] as f64;
        costs[insertion_row][target_count..].fill(0.0);
    }

    let assignment = hungarian(&costs);
    let mapping = (0..source_count)
        .map(|source_index| {
            let target_index = assignment[source_index];
            (target_index < target_count).then_some(target_index)
        })
        .collect::<Vec<_>>();
    let cost = concrete_script_cost(
        source,
        target,
        &source_nodes,
        &target_nodes,
        &mapping,
        options,
    );
    let pairs = mapping
        .iter()
        .enumerate()
        .filter_map(|(source_index, target_index)| {
            target_index
                .map(|target_index| (source_nodes[source_index].id, target_nodes[target_index].id))
        })
        .collect();
    Ok(AssignmentScript { cost, pairs })
}

fn total_degree(graph: &CodePropertyGraph, node: NodeId) -> usize {
    graph.outgoing_edges(node).count() + graph.incoming_edges(node).count()
}

fn concrete_script_cost(
    source: &CodePropertyGraph,
    target: &CodePropertyGraph,
    source_nodes: &[&crate::CpgNode],
    target_nodes: &[&crate::CpgNode],
    mapping: &[Option<usize>],
    options: &GedOptions,
) -> f64 {
    let mut matched_targets = vec![false; target_nodes.len()];
    let mut node_cost = 0.0;
    for (source_index, mapped) in mapping.iter().copied().enumerate() {
        if let Some(target_index) = mapped {
            matched_targets[target_index] = true;
            if NodeKindTag::from_kind(&source_nodes[source_index].kind)
                != NodeKindTag::from_kind(&target_nodes[target_index].kind)
            {
                node_cost += options.node_sub;
            }
        } else {
            node_cost += options.node_ins_del;
        }
    }
    node_cost +=
        matched_targets.iter().filter(|&&matched| !matched).count() as f64 * options.node_ins_del;

    let source_index: FxHashMap<_, _> = source_nodes
        .iter()
        .enumerate()
        .map(|(dense, node)| (node.id, dense))
        .collect();
    let target_index: FxHashMap<_, _> = target_nodes
        .iter()
        .enumerate()
        .map(|(dense, node)| (node.id, dense))
        .collect();
    let mut remaining_target_edges: FxHashMap<(usize, usize, CpgEdgeKind), usize> =
        FxHashMap::default();
    for edge in target.edges() {
        let key = (
            target_index[&edge.source],
            target_index[&edge.target],
            edge.kind.clone(),
        );
        *remaining_target_edges.entry(key).or_insert(0) += 1;
    }

    let mut deleted_edges = 0usize;
    for edge in source.edges() {
        let mapped_source = mapping[source_index[&edge.source]];
        let mapped_target = mapping[source_index[&edge.target]];
        let Some(key) = mapped_source
            .zip(mapped_target)
            .map(|(from, to)| (from, to, edge.kind.clone()))
        else {
            deleted_edges += 1;
            continue;
        };
        match remaining_target_edges.get_mut(&key) {
            Some(count) if *count > 0 => *count -= 1,
            _ => deleted_edges += 1,
        }
    }
    let inserted_edges: usize = remaining_target_edges.values().sum();
    node_cost + (deleted_edges + inserted_edges) as f64 * options.edge_ins_del
}

fn node_kind_jaccard(first: &CodePropertyGraph, second: &CodePropertyGraph) -> f64 {
    let mut first_counts: FxHashMap<NodeKindTag, usize> = FxHashMap::default();
    let mut second_counts: FxHashMap<NodeKindTag, usize> = FxHashMap::default();
    for node in first.nodes() {
        *first_counts
            .entry(NodeKindTag::from_kind(&node.kind))
            .or_insert(0) += 1;
    }
    for node in second.nodes() {
        *second_counts
            .entry(NodeKindTag::from_kind(&node.kind))
            .or_insert(0) += 1;
    }
    let intersection: usize = first_counts
        .iter()
        .map(|(kind, &count)| count.min(second_counts.get(kind).copied().unwrap_or(0)))
        .sum();
    let union = first.node_count() + second.node_count() - intersection;
    if union == 0 {
        1.0
    } else {
        intersection as f64 / union as f64
    }
}

/// Deterministic minimum-cost perfect assignment for a square finite matrix.
///
/// The implementation is the primal-dual Hungarian algorithm. Columns are
/// scanned in ascending order and equal reduced costs retain the smaller
/// predecessor/column, fixing the augmenting-path tie rule.
fn hungarian(costs: &[Vec<f64>]) -> Vec<usize> {
    let size = costs.len();
    if size == 0 {
        return Vec::new();
    }
    debug_assert!(costs.iter().all(|row| row.len() == size));

    let mut row_potential = vec![0.0; size + 1];
    let mut column_potential = vec![0.0; size + 1];
    let mut column_row = vec![0usize; size + 1];
    let mut predecessor = vec![0usize; size + 1];

    for row in 1..=size {
        column_row[0] = row;
        let mut available = vec![f64::INFINITY; size + 1];
        let mut used = vec![false; size + 1];
        let mut column = 0;
        loop {
            used[column] = true;
            let active_row = column_row[column];
            let mut delta = f64::INFINITY;
            let mut next_column = 0;
            for candidate in 1..=size {
                if used[candidate] {
                    continue;
                }
                let reduced = costs[active_row - 1][candidate - 1]
                    - row_potential[active_row]
                    - column_potential[candidate];
                if reduced < available[candidate] {
                    available[candidate] = reduced;
                    predecessor[candidate] = column;
                }
                if available[candidate] < delta {
                    delta = available[candidate];
                    next_column = candidate;
                }
            }
            for candidate in 0..=size {
                if used[candidate] {
                    row_potential[column_row[candidate]] += delta;
                    column_potential[candidate] -= delta;
                } else {
                    available[candidate] -= delta;
                }
            }
            column = next_column;
            if column_row[column] == 0 {
                break;
            }
        }

        loop {
            let previous = predecessor[column];
            column_row[column] = column_row[previous];
            column = previous;
            if column == 0 {
                break;
            }
        }
    }

    let mut assignment = vec![0; size];
    for column in 1..=size {
        assignment[column_row[column] - 1] = column - 1;
    }
    assignment
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::{CpgEdge, CpgNode, CpgNodeKind, EdgeId, Language, SourceRange};

    fn graph(
        labels: &[bool],
        edges: impl IntoIterator<Item = (usize, usize, CpgEdgeKind)>,
        reverse_nodes: bool,
    ) -> CodePropertyGraph {
        let mut graph = CodePropertyGraph::new(Language::Rust);
        let node_order: Vec<_> = if reverse_nodes {
            (0..labels.len()).rev().collect()
        } else {
            (0..labels.len()).collect()
        };
        for dense in node_order {
            let kind = if labels[dense] {
                CpgNodeKind::If
            } else {
                CpgNodeKind::While
            };
            graph.add_node_with_id(CpgNode::new(
                NodeId::new((dense * 11 + 7) as u32),
                kind,
                SourceRange::default(),
            ));
        }
        for (edge_id, (source, target, kind)) in edges.into_iter().enumerate() {
            graph
                .add_edge(CpgEdge::new(
                    EdgeId::new(edge_id as u32),
                    NodeId::new((source * 11 + 7) as u32),
                    NodeId::new((target * 11 + 7) as u32),
                    kind,
                ))
                .expect("fixture endpoints exist");
        }
        graph
    }

    fn graph_from_bits(node_count: usize, labels: u8, edges: u64) -> CodePropertyGraph {
        graph(
            &(0..node_count)
                .map(|node| labels & (1 << node) != 0)
                .collect::<Vec<_>>(),
            (0..node_count).flat_map(|source| {
                (0..node_count).filter_map(move |target| {
                    (edges & (1 << (source * 6 + target)) != 0).then_some((
                        source,
                        target,
                        CpgEdgeKind::AstChild,
                    ))
                })
            }),
            false,
        )
    }

    fn assignment_cost(costs: &[Vec<f64>], assignment: &[usize]) -> f64 {
        assignment
            .iter()
            .enumerate()
            .map(|(row, &column)| costs[row][column])
            .sum()
    }

    fn brute_assignment(costs: &[Vec<f64>]) -> f64 {
        fn visit(row: usize, costs: &[Vec<f64>], used: &mut [bool], cost: f64, best: &mut f64) {
            if row == costs.len() {
                *best = best.min(cost);
                return;
            }
            for column in 0..costs.len() {
                if !used[column] {
                    used[column] = true;
                    visit(row + 1, costs, used, cost + costs[row][column], best);
                    used[column] = false;
                }
            }
        }

        if costs.is_empty() {
            return 0.0;
        }
        let mut best = f64::INFINITY;
        visit(0, costs, &mut vec![false; costs.len()], 0.0, &mut best);
        best
    }

    fn exact_edit_distance(
        source: &CodePropertyGraph,
        target: &CodePropertyGraph,
        options: &GedOptions,
    ) -> f64 {
        struct ExactSearch<'a> {
            source: &'a CodePropertyGraph,
            target: &'a CodePropertyGraph,
            source_nodes: Vec<&'a CpgNode>,
            target_nodes: Vec<&'a CpgNode>,
            options: &'a GedOptions,
            best: f64,
        }

        impl ExactSearch<'_> {
            fn visit(
                &mut self,
                source_index: usize,
                mapping: &mut [Option<usize>],
                used: &mut [bool],
            ) {
                if source_index == self.source_nodes.len() {
                    self.best = self.best.min(concrete_script_cost(
                        self.source,
                        self.target,
                        &self.source_nodes,
                        &self.target_nodes,
                        mapping,
                        self.options,
                    ));
                    return;
                }

                mapping[source_index] = None;
                self.visit(source_index + 1, mapping, used);
                for target_index in 0..self.target_nodes.len() {
                    if !used[target_index] {
                        used[target_index] = true;
                        mapping[source_index] = Some(target_index);
                        self.visit(source_index + 1, mapping, used);
                        used[target_index] = false;
                    }
                }
            }
        }

        let mut source_nodes: Vec<_> = source.nodes().collect();
        let mut target_nodes: Vec<_> = target.nodes().collect();
        source_nodes.sort_by_key(|node| node.id);
        target_nodes.sort_by_key(|node| node.id);
        let mut search = ExactSearch {
            source,
            target,
            source_nodes,
            target_nodes,
            options,
            best: f64::INFINITY,
        };
        search.visit(
            0,
            &mut vec![None; search.source_nodes.len()],
            &mut vec![false; search.target_nodes.len()],
        );
        search.best
    }

    #[test]
    fn defaults_empty_and_identical_graphs_have_exact_semantics() {
        assert_eq!(
            GedOptions::default(),
            GedOptions {
                node_sub: 1.0,
                node_ins_del: 1.0,
                edge_ins_del: 0.5,
                max_nodes: 512,
            }
        );
        let empty = CodePropertyGraph::new(Language::Rust);
        assert_eq!(
            assignment_script_cost(&empty, &empty, &GedOptions::default(), 0.0)
                .expect("empty internal assignment"),
            0.0
        );
        assert_eq!(legacy_graph_edit_similarity(&empty, &empty, 0.7, 0.3), 1.0);
        assert_eq!(
            ged_upper_bound(&empty, &empty, &GedOptions::default()).expect("empty"),
            GedBound {
                upper: 0.0,
                normalized: 0.0,
                capped: false,
            }
        );

        let fixture = graph(
            &[true, false, true],
            [
                (0, 1, CpgEdgeKind::AstChild),
                (1, 2, CpgEdgeKind::Reference),
                (1, 2, CpgEdgeKind::Reference),
            ],
            false,
        );
        let result = ged_upper_bound(&fixture, &fixture.clone(), &GedOptions::default())
            .expect("identical graph");
        assert_eq!(result.upper, 0.0);
        assert_eq!(result.normalized, 0.0);
        assert!(!result.capped);
    }

    #[test]
    fn concrete_edits_count_node_labels_edge_kinds_and_parallel_edges() {
        let first = graph(
            &[true, false],
            [(0, 1, CpgEdgeKind::AstChild), (0, 1, CpgEdgeKind::AstChild)],
            false,
        );
        let second = graph(&[true, true], [(0, 1, CpgEdgeKind::Reference)], true);
        let result = ged_upper_bound(&first, &second, &GedOptions::default()).expect("small");
        assert_eq!(result.upper, 2.5, "one relabel + three edge edits");
        assert_eq!(result.normalized, 2.5 / 5.5);
        assert!(!result.capped);
        assert_eq!(
            result,
            ged_upper_bound(&second, &first, &GedOptions::default()).expect("reverse")
        );
    }

    #[test]
    fn configured_and_hard_caps_return_the_flagged_legacy_fallback() {
        let first = graph(&[true, false], [], false);
        let second = graph(&[true, true, false], [], false);
        let options = GedOptions {
            max_nodes: 1,
            ..GedOptions::default()
        };
        let result = ged_upper_bound(&first, &second, &options).expect("fallback");
        let expected_similarity = legacy_graph_edit_similarity(&first, &second, 0.7, 0.3);
        assert!(result.capped);
        assert_eq!(result.normalized, 1.0 - expected_similarity);
        assert_eq!(result.upper, result.normalized * 5.0);

        let zero_costs = GedOptions {
            node_sub: 0.0,
            node_ins_del: 0.0,
            edge_ins_del: 0.0,
            max_nodes: 0,
        };
        assert_eq!(
            ged_upper_bound(&first, &second, &zero_costs).expect("zero-cost fallback"),
            GedBound {
                upper: 0.0,
                normalized: 0.0,
                capped: true,
            }
        );

        let above_hard_cap = graph(&vec![false; MAX_GED_NODES + 1], [], false);
        assert!(
            ged_upper_bound(
                &above_hard_cap,
                &CodePropertyGraph::new(Language::Rust),
                &GedOptions {
                    max_nodes: usize::MAX,
                    ..GedOptions::default()
                },
            )
            .expect("hard-cap fallback")
            .capped
        );
    }

    #[test]
    fn invalid_or_overflowing_costs_are_rejected() {
        let graph = graph(&[true], [], false);
        for invalid in [f64::NAN, f64::INFINITY, -1.0] {
            for options in [
                GedOptions {
                    node_sub: invalid,
                    ..GedOptions::default()
                },
                GedOptions {
                    node_ins_del: invalid,
                    ..GedOptions::default()
                },
                GedOptions {
                    edge_ins_del: invalid,
                    ..GedOptions::default()
                },
            ] {
                assert!(matches!(
                    ged_upper_bound(&graph, &graph, &options),
                    Err(AnalysisError::InvalidInput {
                        analysis: "ged_upper_bound",
                        input: "options",
                        ..
                    })
                ));
            }
        }

        assert!(matches!(
            ged_upper_bound(
                &graph,
                &graph,
                &GedOptions {
                    node_ins_del: f64::MAX,
                    ..GedOptions::default()
                },
            ),
            Err(AnalysisError::InvalidInput { .. })
        ));
        assert!(matches!(
            ged_upper_bound(
                &graph,
                &graph,
                &GedOptions {
                    node_sub: f64::MAX,
                    node_ins_del: 1.0,
                    edge_ins_del: 0.0,
                    ..GedOptions::default()
                },
            ),
            Err(AnalysisError::InvalidInput { .. })
        ));
    }

    #[test]
    fn hungarian_ties_choose_smallest_columns() {
        assert!(hungarian(&[]).is_empty());
        assert_eq!(hungarian(&vec![vec![0.0; 4]; 4]), vec![0, 1, 2, 3]);
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]

        #[test]
        fn hungarian_matches_brute_force_permutations_through_seven(
            size in 0usize..=7,
            raw in prop::collection::vec(0u16..=1000, 49),
        ) {
            let costs: Vec<Vec<f64>> = (0..size)
                .map(|row| {
                    (0..size)
                        .map(|column| f64::from(raw[row * 7 + column]) / 10.0)
                        .collect()
                })
                .collect();
            let assignment = hungarian(&costs);
            prop_assert_eq!(assignment.len(), size);
            let mut columns = assignment.clone();
            columns.sort_unstable();
            prop_assert_eq!(columns, (0..size).collect::<Vec<_>>());
            prop_assert!((assignment_cost(&costs, &assignment) - brute_assignment(&costs)).abs() < 1e-8);
        }

        #[test]
        fn generated_bound_is_symmetric_deterministic_and_above_exact_ged(
            first_nodes in 0usize..=6,
            second_nodes in 0usize..=6,
            first_labels in any::<u8>(),
            second_labels in any::<u8>(),
            first_edges in any::<u64>(),
            second_edges in any::<u64>(),
        ) {
            let first = graph_from_bits(first_nodes, first_labels, first_edges);
            let second = graph_from_bits(second_nodes, second_labels, second_edges);
            let options = GedOptions::default();
            let result = ged_upper_bound(&first, &second, &options).expect("valid generated costs");
            let reverse = ged_upper_bound(&second, &first, &options).expect("reverse");
            let repeat = ged_upper_bound(&first, &second, &options).expect("repeat");
            let exact = exact_edit_distance(&first, &second, &options);

            prop_assert_eq!(result, reverse);
            prop_assert_eq!(result, repeat);
            prop_assert!(!result.capped);
            prop_assert!(result.upper + 1e-9 >= exact, "bound {} below exact {}", result.upper, exact);
            prop_assert!((0.0..=1.0).contains(&result.normalized));
            if first_nodes == second_nodes && first_labels == second_labels && first_edges == second_edges {
                prop_assert_eq!(result.upper, 0.0);
            }
        }
    }

    #[cfg(feature = "serde")]
    #[test]
    fn options_and_bound_round_trip_with_serde() {
        let options = GedOptions::default();
        let bound = ged_upper_bound(
            &graph(&[true], [], false),
            &graph(&[false], [], false),
            &options,
        )
        .expect("one substitution");
        assert_eq!(
            serde_json::from_str::<GedOptions>(
                &serde_json::to_string(&options).expect("serialize options")
            )
            .expect("deserialize options"),
            options
        );
        assert_eq!(
            serde_json::from_str::<GedBound>(
                &serde_json::to_string(&bound).expect("serialize bound")
            )
            .expect("deserialize bound"),
            bound
        );
    }
}
