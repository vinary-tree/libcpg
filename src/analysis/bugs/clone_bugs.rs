//! Inconsistent-clone detection over pairwise clone, GED, and AST evidence.
//!
//! The detector consumes the exact Type-2/3 pair relation produced by the
//! clone engine. It aligns normalized AST tapes with heap-resident dynamic
//! programming, then reports only typed semantic divergences supported by a
//! dominant identifier-renaming convention or by aligned guard payloads.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::analysis::function_scope_nodes;
use crate::pattern::{
    detect_clone_pairs, ged_upper_bound, CloneOptions, ClonePairRelation, CloneType, GedBound,
    GedOptions, NodeKindTag,
};
use crate::{AnalysisError, CodePropertyGraph, CpgEdgeKind, CpgNodeKind, LiteralKind, NodeId};

use super::evidence::{PopulationFraction, PopulationThreshold};

/// Hard ceiling on retained nodes in one clone-side AST tape.
pub const MAX_CLONE_BUG_AST_NODES: usize = 200_000;
/// Hard ceiling on cells in one non-trivial AST alignment matrix.
pub const MAX_CLONE_ALIGNMENT_CELLS: usize = 4_000_000;

/// Configuration for [`analyze_inconsistent_clones`].
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CloneBugConfig {
    /// Candidate taxonomy, similarity thresholds, and clone-engine budgets.
    pub clone_options: CloneOptions,
    /// Costs and size boundary for pair-local graph-edit-distance evidence.
    pub ged_options: GedOptions,
    /// Largest exact fraction of normalized AST edits admitted as a near miss.
    pub maximum_structural_edit_fraction: PopulationThreshold,
    /// Minimum exact support for the dominant mapping of one source symbol.
    pub minimum_rename_consistency: PopulationThreshold,
    /// Minimum aligned occurrences needed to infer a rename convention.
    pub minimum_rename_occurrences: usize,
    /// Maximum retained AST nodes on either side of one candidate pair.
    pub max_ast_nodes: usize,
    /// Maximum dynamic-programming cells for a non-isomorphic tape pair.
    pub max_alignment_cells: usize,
}

impl Default for CloneBugConfig {
    fn default() -> Self {
        Self {
            clone_options: CloneOptions::default(),
            ged_options: GedOptions::default(),
            maximum_structural_edit_fraction: PopulationThreshold::new(1, 5),
            minimum_rename_consistency: PopulationThreshold::new(4, 5),
            minimum_rename_occurrences: 3,
            max_ast_nodes: MAX_CLONE_BUG_AST_NODES,
            max_alignment_cells: 1_000_000,
        }
    }
}

/// Direction in which an identifier-renaming convention was inferred.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum RenameDirection {
    /// A symbol in the lower-id function normally maps to one right symbol.
    LeftToRight,
    /// A symbol in the higher-id function normally maps to one left symbol.
    RightToLeft,
}

/// Exact typed payload retained for an aligned guard difference.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum CloneGuardValue {
    /// Binary operator spelling supplied by the frontend.
    BinaryOperator(Arc<str>),
    /// Unary operator spelling supplied by the frontend.
    UnaryOperator(Arc<str>),
    /// Signed integer literal.
    Integer(i64),
    /// IEEE-754 `f64` represented by its exact bits.
    FloatBits(u64),
    /// String literal.
    String(Arc<str>),
    /// Character literal.
    Character(char),
    /// Boolean literal.
    Boolean(bool),
    /// Null or nil literal.
    Null,
    /// Array literal marker.
    Array,
    /// Object or map literal marker.
    Object,
    /// Regular-expression literal.
    Regex(Arc<str>),
}

/// Typed reason that a near-miss clone pair is suspicious.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum InconsistentCloneKind {
    /// One aligned symbol pair violates a well-supported rename convention.
    IdentifierRename {
        /// Direction in which the convention was inferred.
        direction: RenameDirection,
        /// Observed spelling in the lower-id function.
        left_name: Arc<str>,
        /// Observed spelling in the higher-id function.
        right_name: Arc<str>,
        /// Expected spelling on the destination side of `direction`.
        expected: Arc<str>,
        /// Exact support for the dominant mapping, including the anomaly.
        support: PopulationFraction,
    },
    /// Aligned nodes inside the two branch conditions carry different payloads.
    GuardDivergence {
        /// Payload in the lower-id function.
        left: CloneGuardValue,
        /// Payload in the higher-id function.
        right: CloneGuardValue,
    },
}

/// One advisory inconsistent-clone finding.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct InconsistentCloneBug {
    /// Lower stable-id function in the pair.
    pub left_function: NodeId,
    /// Higher stable-id function in the pair.
    pub right_function: NodeId,
    /// Exact clone relation established by the clone detector.
    pub clone_type: CloneType,
    /// Clone-detector similarity for this exact pair.
    pub clone_similarity: f64,
    /// Concrete bipartite GED upper bound over retained AST graphs.
    pub ged: GedBound,
    /// Normalized sequence edits in the aligned typed AST tapes.
    pub structural_edits: usize,
    /// Larger retained tape size used as the edit-fraction denominator.
    pub structural_size: usize,
    /// Witness node in `left_function`.
    pub left_node: NodeId,
    /// Witness node in `right_function`.
    pub right_node: NodeId,
    /// Typed suspicious divergence.
    pub kind: InconsistentCloneKind,
}

/// Pair-local evidence evaluated even when it produced no finding.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ClonePairEvidence {
    /// Lower stable-id function in the pair.
    pub left_function: NodeId,
    /// Higher stable-id function in the pair.
    pub right_function: NodeId,
    /// Exact Type-2 or Type-3 relation.
    pub clone_type: CloneType,
    /// Clone-detector similarity for the exact pair.
    pub clone_similarity: f64,
    /// Concrete GED upper bound over the retained AST graphs.
    pub ged: GedBound,
    /// Normalized AST sequence edits.
    pub structural_edits: usize,
    /// Larger retained tape size.
    pub structural_size: usize,
    /// Whether the exact configured edit-fraction boundary admitted the pair.
    pub is_low_edit: bool,
}

/// Explicit evidence gaps that prevent a complete negative claim.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum CloneBugIssue {
    /// A retained AST edge referenced a node absent from the CPG.
    MissingAstNode {
        /// Function whose tape was being built.
        function: NodeId,
        /// Missing node id.
        node: NodeId,
    },
    /// A retained AST contains a directed cycle.
    CyclicAst {
        /// Function whose tape was being built.
        function: NodeId,
        /// Node encountered while it was already active.
        node: NodeId,
    },
    /// A retained node has more than one structural path from its function.
    SharedAstNode {
        /// Function whose tape was being built.
        function: NodeId,
        /// Repeated node.
        node: NodeId,
    },
    /// A function AST exceeded the configured retained-node budget.
    AstNodeCap {
        /// Capped function.
        function: NodeId,
        /// First required count beyond the cap.
        needed: usize,
        /// Configured cap.
        cap: usize,
    },
    /// Exact tape alignment exceeded the configured matrix-cell budget.
    AlignmentCellCap {
        /// Lower stable-id function.
        left_function: NodeId,
        /// Higher stable-id function.
        right_function: NodeId,
        /// Required matrix cells.
        needed: usize,
        /// Configured cap.
        cap: usize,
    },
    /// The alignment matrix could not be allocated.
    AlignmentAllocationFailed {
        /// Lower stable-id function.
        left_function: NodeId,
        /// Higher stable-id function.
        right_function: NodeId,
        /// Requested matrix cells.
        cells: usize,
    },
    /// GED used its size-capped surrogate, so the pair was not classified.
    GedCapped {
        /// Lower stable-id function.
        left_function: NodeId,
        /// Higher stable-id function.
        right_function: NodeId,
    },
}

/// Findings, evaluated pairs, and completeness evidence for clone-bug analysis.
#[derive(Debug, Clone, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CloneBugAnalysis {
    /// Canonically ordered advisory findings.
    pub findings: Vec<InconsistentCloneBug>,
    /// Canonically ordered exact pairs that received complete local analysis.
    pub analyzed_pairs: Vec<ClonePairEvidence>,
    /// Ordered evidence gaps excluded from classification.
    pub issues: BTreeSet<CloneBugIssue>,
}

impl CloneBugAnalysis {
    /// Returns whether every Type-2/3 candidate had complete bounded evidence.
    pub fn is_complete(&self) -> bool {
        self.issues.is_empty()
    }
}

#[derive(Debug, Clone)]
struct AstToken {
    node: NodeId,
    tag: NodeKindTag,
    child_count: usize,
}

#[derive(Debug, Clone)]
struct AstTape {
    tokens: Vec<AstToken>,
}

#[derive(Debug, Clone, Copy)]
enum TapeError {
    Missing(NodeId),
    Cycle(NodeId),
    Shared(NodeId),
    Cap { needed: usize },
}

#[derive(Debug)]
struct Alignment {
    pairs: Vec<(NodeId, NodeId)>,
    edits: usize,
    size: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SymbolKey {
    anchor: Option<NodeId>,
    name: Arc<str>,
}

/// Detects suspicious low-edit Type-2/3 clone divergences without mutating the CPG.
///
/// Exact pair membership comes from [`crate::pattern::detect_clones`]'s
/// internal pair relation rather than from transitive clone classes. A
/// same-shape pair is aligned in linear space; otherwise a bounded Wagner–
/// Fischer matrix aligns typed preorder tapes. Both paths use heap-resident
/// state, so native call depth is independent of AST depth.
pub fn analyze_inconsistent_clones(
    cpg: &CodePropertyGraph,
    config: &CloneBugConfig,
) -> Result<CloneBugAnalysis, AnalysisError> {
    validate_config(config)?;
    let candidates: Vec<_> = detect_clone_pairs(cpg, &config.clone_options)?
        .into_iter()
        .filter(|pair| matches!(pair.clone_type, CloneType::Type2 | CloneType::Type3))
        .collect();
    let mut analysis = CloneBugAnalysis::default();
    let mut functions = BTreeSet::new();
    for pair in &candidates {
        functions.insert(pair.left);
        functions.insert(pair.right);
    }

    let mut tapes = BTreeMap::<NodeId, Option<AstTape>>::new();
    for function in functions {
        match ast_tape(cpg, function, config.max_ast_nodes) {
            Ok(tape) => {
                tapes.insert(function, Some(tape));
            }
            Err(error) => {
                record_tape_issue(&mut analysis.issues, function, error, config.max_ast_nodes);
                tapes.insert(function, None);
            }
        }
    }

    for pair in candidates {
        let (Some(left), Some(right)) = (
            tapes.get(&pair.left).and_then(Option::as_ref),
            tapes.get(&pair.right).and_then(Option::as_ref),
        ) else {
            continue;
        };
        let alignment = match align_tapes(left, right, config.max_alignment_cells) {
            Ok(alignment) => alignment,
            Err(AlignmentError::Cap { needed }) => {
                analysis.issues.insert(CloneBugIssue::AlignmentCellCap {
                    left_function: pair.left,
                    right_function: pair.right,
                    needed,
                    cap: config.max_alignment_cells,
                });
                continue;
            }
            Err(AlignmentError::Allocation { cells }) => {
                analysis
                    .issues
                    .insert(CloneBugIssue::AlignmentAllocationFailed {
                        left_function: pair.left,
                        right_function: pair.right,
                        cells,
                    });
                continue;
            }
        };
        let left_graph = ast_graph(cpg, left);
        let right_graph = ast_graph(cpg, right);
        let ged = ged_upper_bound(&left_graph, &right_graph, &config.ged_options)?;
        if ged.capped {
            analysis.issues.insert(CloneBugIssue::GedCapped {
                left_function: pair.left,
                right_function: pair.right,
            });
            continue;
        }
        let is_low_edit = fraction_at_most(
            alignment.edits,
            alignment.size,
            config.maximum_structural_edit_fraction,
        );
        analysis.analyzed_pairs.push(ClonePairEvidence {
            left_function: pair.left,
            right_function: pair.right,
            clone_type: pair.clone_type,
            clone_similarity: pair.similarity,
            ged,
            structural_edits: alignment.edits,
            structural_size: alignment.size,
            is_low_edit,
        });
        if !is_low_edit {
            continue;
        }

        let common = FindingEvidence {
            pair,
            ged,
            structural_edits: alignment.edits,
            structural_size: alignment.size,
        };
        infer_rename_anomalies(
            cpg,
            &alignment.pairs,
            config,
            &common,
            &mut analysis.findings,
        );
        infer_guard_divergences(cpg, &alignment.pairs, &common, &mut analysis.findings);
    }

    analysis.analyzed_pairs.sort_by(|left, right| {
        (left.left_function, left.right_function).cmp(&(right.left_function, right.right_function))
    });
    analysis.findings.sort_by(|left, right| {
        (
            left.left_function,
            left.right_function,
            left.left_node,
            left.right_node,
            &left.kind,
        )
            .cmp(&(
                right.left_function,
                right.right_function,
                right.left_node,
                right.right_node,
                &right.kind,
            ))
    });
    analysis.findings.dedup_by(|left, right| {
        left.left_function == right.left_function
            && left.right_function == right.right_function
            && left.left_node == right.left_node
            && left.right_node == right.right_node
            && left.kind == right.kind
    });
    Ok(analysis)
}

/// Returns findings while discarding completeness evidence.
///
/// Consumers making negative claims must use [`analyze_inconsistent_clones`]
/// and require [`CloneBugAnalysis::is_complete`].
pub fn inconsistent_clone_bugs(
    cpg: &CodePropertyGraph,
    config: &CloneBugConfig,
) -> Result<Vec<InconsistentCloneBug>, AnalysisError> {
    Ok(analyze_inconsistent_clones(cpg, config)?.findings)
}

fn validate_config(config: &CloneBugConfig) -> Result<(), AnalysisError> {
    for (input, threshold) in [
        (
            "maximum_structural_edit_fraction",
            config.maximum_structural_edit_fraction,
        ),
        (
            "minimum_rename_consistency",
            config.minimum_rename_consistency,
        ),
    ] {
        if !threshold.is_valid() {
            return Err(AnalysisError::InvalidInput {
                analysis: "inconsistent-clone detection",
                input,
                reason:
                    "must have a positive denominator and numerator no greater than denominator",
            });
        }
    }
    if config.minimum_rename_occurrences < 2 {
        return Err(AnalysisError::InvalidInput {
            analysis: "inconsistent-clone detection",
            input: "minimum_rename_occurrences",
            reason: "must be at least two",
        });
    }
    if ![
        config.ged_options.node_sub,
        config.ged_options.node_ins_del,
        config.ged_options.edge_ins_del,
    ]
    .into_iter()
    .all(|cost| cost.is_finite() && cost >= 0.0)
    {
        return Err(AnalysisError::InvalidInput {
            analysis: "inconsistent-clone detection",
            input: "ged_options",
            reason: "edit costs must be finite and nonnegative",
        });
    }
    for (input, value, hard_cap) in [
        (
            "max_ast_nodes",
            config.max_ast_nodes,
            MAX_CLONE_BUG_AST_NODES,
        ),
        (
            "max_alignment_cells",
            config.max_alignment_cells,
            MAX_CLONE_ALIGNMENT_CELLS,
        ),
    ] {
        if value == 0 || value > hard_cap {
            return Err(AnalysisError::InvalidInput {
                analysis: "inconsistent-clone detection",
                input,
                reason: "must be positive and no greater than its documented hard ceiling",
            });
        }
    }
    Ok(())
}

fn fraction_at_most(count: usize, population: usize, maximum: PopulationThreshold) -> bool {
    population != 0
        && (count as u128) * (maximum.denominator as u128)
            <= (maximum.numerator as u128) * (population as u128)
}

enum TapeFrame {
    Enter(NodeId),
    Exit(NodeId),
}

fn ast_tape(cpg: &CodePropertyGraph, function: NodeId, cap: usize) -> Result<AstTape, TapeError> {
    let mut tokens = Vec::new();
    let mut active = BTreeSet::new();
    let mut seen = BTreeSet::new();
    let mut frames = vec![TapeFrame::Enter(function)];
    while let Some(frame) = frames.pop() {
        match frame {
            TapeFrame::Exit(node) => {
                active.remove(&node);
            }
            TapeFrame::Enter(node) => {
                if active.contains(&node) {
                    return Err(TapeError::Cycle(node));
                }
                if !seen.insert(node) {
                    return Err(TapeError::Shared(node));
                }
                let Some(payload) = cpg.node(node) else {
                    return Err(TapeError::Missing(node));
                };
                active.insert(node);
                let mut children = Vec::new();
                for &child in cpg.ast_children(node) {
                    let retained = match cpg.node(child) {
                        Some(child_node) => {
                            !matches!(child_node.kind, CpgNodeKind::Comment { .. })
                                && !(child != function
                                    && matches!(child_node.kind, CpgNodeKind::Function { .. }))
                        }
                        None => true,
                    };
                    if retained {
                        children.push(child);
                    }
                }
                let needed = tokens.len().saturating_add(1);
                if needed > cap {
                    return Err(TapeError::Cap { needed });
                }
                tokens.push(AstToken {
                    node,
                    tag: NodeKindTag::from_kind(&payload.kind),
                    child_count: children.len(),
                });
                frames.push(TapeFrame::Exit(node));
                for child in children.into_iter().rev() {
                    frames.push(TapeFrame::Enter(child));
                }
            }
        }
    }
    Ok(AstTape { tokens })
}

fn record_tape_issue(
    issues: &mut BTreeSet<CloneBugIssue>,
    function: NodeId,
    error: TapeError,
    cap: usize,
) {
    issues.insert(match error {
        TapeError::Missing(node) => CloneBugIssue::MissingAstNode { function, node },
        TapeError::Cycle(node) => CloneBugIssue::CyclicAst { function, node },
        TapeError::Shared(node) => CloneBugIssue::SharedAstNode { function, node },
        TapeError::Cap { needed } => CloneBugIssue::AstNodeCap {
            function,
            needed,
            cap,
        },
    });
}

fn same_shape(left: &AstToken, right: &AstToken) -> bool {
    left.tag == right.tag && left.child_count == right.child_count
}

fn same_tag(left: &AstToken, right: &AstToken) -> bool {
    left.tag == right.tag
}

#[derive(Debug)]
enum AlignmentError {
    Cap { needed: usize },
    Allocation { cells: usize },
}

fn align_tapes(
    left: &AstTape,
    right: &AstTape,
    max_cells: usize,
) -> Result<Alignment, AlignmentError> {
    if left.tokens.len() == right.tokens.len()
        && left
            .tokens
            .iter()
            .zip(&right.tokens)
            .all(|(left, right)| same_shape(left, right))
    {
        return Ok(Alignment {
            pairs: left
                .tokens
                .iter()
                .zip(&right.tokens)
                .map(|(left, right)| (left.node, right.node))
                .collect(),
            edits: 0,
            size: left.tokens.len(),
        });
    }

    let rows = left.tokens.len().saturating_add(1);
    let columns = right.tokens.len().saturating_add(1);
    let Some(cells) = rows.checked_mul(columns) else {
        return Err(AlignmentError::Cap { needed: usize::MAX });
    };
    if cells > max_cells {
        return Err(AlignmentError::Cap { needed: cells });
    }
    let mut distance = Vec::new();
    distance
        .try_reserve_exact(cells)
        .map_err(|_| AlignmentError::Allocation { cells })?;
    distance.resize(cells, 0u32);
    for row in 0..rows {
        distance[row * columns] = row as u32;
    }
    for (column, cell) in distance.iter_mut().take(columns).enumerate() {
        *cell = column as u32;
    }
    for row in 1..rows {
        for column in 1..columns {
            let substitution = distance[(row - 1) * columns + column - 1]
                + u32::from(!same_tag(&left.tokens[row - 1], &right.tokens[column - 1]));
            let deletion = distance[(row - 1) * columns + column] + 1;
            let insertion = distance[row * columns + column - 1] + 1;
            distance[row * columns + column] = substitution.min(deletion).min(insertion);
        }
    }

    let mut row = left.tokens.len();
    let mut column = right.tokens.len();
    let mut pairs = Vec::with_capacity(row.min(column));
    while row != 0 || column != 0 {
        let current = distance[row * columns + column];
        if row != 0 && column != 0 {
            let substitution = distance[(row - 1) * columns + column - 1]
                + u32::from(!same_tag(&left.tokens[row - 1], &right.tokens[column - 1]));
            if current == substitution {
                pairs.push((left.tokens[row - 1].node, right.tokens[column - 1].node));
                row -= 1;
                column -= 1;
                continue;
            }
        }
        if row != 0 && current == distance[(row - 1) * columns + column] + 1 {
            row -= 1;
        } else {
            debug_assert!(column != 0);
            column -= 1;
        }
    }
    pairs.reverse();
    Ok(Alignment {
        pairs,
        edits: distance[left.tokens.len() * columns + right.tokens.len()] as usize,
        size: left.tokens.len().max(right.tokens.len()),
    })
}

fn ast_graph(cpg: &CodePropertyGraph, tape: &AstTape) -> CodePropertyGraph {
    let retained: BTreeSet<_> = tape.tokens.iter().map(|token| token.node).collect();
    let mut graph = CodePropertyGraph::new(cpg.language());
    for &node in &retained {
        if let Some(node) = cpg.node(node) {
            graph.add_node_with_id(node.clone());
        }
    }
    let mut edges = BTreeMap::new();
    for edge in cpg.edges() {
        if matches!(edge.kind, CpgEdgeKind::AstChild)
            && retained.contains(&edge.source)
            && retained.contains(&edge.target)
        {
            edges.insert(edge.id.as_u32(), (edge.source, edge.target));
        }
    }
    for (_, (source, target)) in edges {
        graph.connect(source, target, CpgEdgeKind::AstChild);
    }
    graph
}

struct FindingEvidence {
    pair: ClonePairRelation,
    ged: GedBound,
    structural_edits: usize,
    structural_size: usize,
}

fn finding(
    evidence: &FindingEvidence,
    left_node: NodeId,
    right_node: NodeId,
    kind: InconsistentCloneKind,
) -> InconsistentCloneBug {
    InconsistentCloneBug {
        left_function: evidence.pair.left,
        right_function: evidence.pair.right,
        clone_type: evidence.pair.clone_type,
        clone_similarity: evidence.pair.similarity,
        ged: evidence.ged,
        structural_edits: evidence.structural_edits,
        structural_size: evidence.structural_size,
        left_node,
        right_node,
        kind,
    }
}

fn symbol_key(cpg: &CodePropertyGraph, node: NodeId) -> Option<SymbolKey> {
    match &cpg.node(node)?.kind {
        CpgNodeKind::Identifier { name, definition } => Some(SymbolKey {
            anchor: *definition,
            name: name.clone(),
        }),
        CpgNodeKind::Parameter { name, .. } | CpgNodeKind::Variable { name, .. } => {
            Some(SymbolKey {
                anchor: Some(node),
                name: name.clone(),
            })
        }
        _ => None,
    }
}

type RenamePopulation = BTreeMap<SymbolKey, BTreeMap<SymbolKey, Vec<(NodeId, NodeId)>>>;

fn infer_rename_anomalies(
    cpg: &CodePropertyGraph,
    aligned: &[(NodeId, NodeId)],
    config: &CloneBugConfig,
    evidence: &FindingEvidence,
    findings: &mut Vec<InconsistentCloneBug>,
) {
    let mut forward = RenamePopulation::new();
    let mut reverse = RenamePopulation::new();
    for &(left_node, right_node) in aligned {
        let (Some(left), Some(right)) = (symbol_key(cpg, left_node), symbol_key(cpg, right_node))
        else {
            continue;
        };
        forward
            .entry(left.clone())
            .or_default()
            .entry(right.clone())
            .or_default()
            .push((left_node, right_node));
        reverse
            .entry(right)
            .or_default()
            .entry(left)
            .or_default()
            .push((left_node, right_node));
    }
    emit_rename_direction(
        cpg,
        forward,
        RenameDirection::LeftToRight,
        config,
        evidence,
        findings,
    );
    emit_rename_direction(
        cpg,
        reverse,
        RenameDirection::RightToLeft,
        config,
        evidence,
        findings,
    );
}

fn emit_rename_direction(
    cpg: &CodePropertyGraph,
    populations: RenamePopulation,
    direction: RenameDirection,
    config: &CloneBugConfig,
    evidence: &FindingEvidence,
    findings: &mut Vec<InconsistentCloneBug>,
) {
    for mappings in populations.into_values() {
        let population: usize = mappings.values().map(Vec::len).sum();
        if population < config.minimum_rename_occurrences || mappings.len() < 2 {
            continue;
        }
        let Some((dominant, supporting)) = mappings
            .iter()
            .max_by(|left, right| {
                left.1
                    .len()
                    .cmp(&right.1.len())
                    .then_with(|| right.0.cmp(left.0))
            })
            .map(|(symbol, witnesses)| (symbol.clone(), witnesses.len()))
        else {
            continue;
        };
        let support = PopulationFraction::new(supporting, population);
        if !config.minimum_rename_consistency.accepts(support) {
            continue;
        }
        for (observed, occurrences) in mappings {
            if observed == dominant {
                continue;
            }
            for (left_node, right_node) in occurrences {
                let (Some(left), Some(right)) = (
                    symbol_key(cpg, left_node).map(|symbol| symbol.name),
                    symbol_key(cpg, right_node).map(|symbol| symbol.name),
                ) else {
                    continue;
                };
                findings.push(finding(
                    evidence,
                    left_node,
                    right_node,
                    InconsistentCloneKind::IdentifierRename {
                        direction,
                        left_name: left,
                        right_name: right,
                        expected: dominant.name.clone(),
                        support,
                    },
                ));
            }
        }
    }
}

fn infer_guard_divergences(
    cpg: &CodePropertyGraph,
    aligned: &[(NodeId, NodeId)],
    evidence: &FindingEvidence,
    findings: &mut Vec<InconsistentCloneBug>,
) {
    let left_guards = guard_nodes(cpg, evidence.pair.left);
    let right_guards = guard_nodes(cpg, evidence.pair.right);
    for &(left_node, right_node) in aligned {
        if !left_guards.contains(&left_node) || !right_guards.contains(&right_node) {
            continue;
        }
        let (Some(left), Some(right)) = (guard_value(cpg, left_node), guard_value(cpg, right_node))
        else {
            continue;
        };
        if left != right {
            findings.push(finding(
                evidence,
                left_node,
                right_node,
                InconsistentCloneKind::GuardDivergence { left, right },
            ));
        }
    }
}

fn guard_nodes(cpg: &CodePropertyGraph, function: NodeId) -> BTreeSet<NodeId> {
    let scope = function_scope_nodes(cpg, function);
    let mut guards = BTreeSet::new();
    let mut sources = BTreeSet::new();
    for &node in &scope {
        if cpg.outgoing_edges(node).any(
            |edge| matches!(edge.kind, CpgEdgeKind::ControlFlow(kind) if kind.is_conditional()),
        ) {
            sources.insert(node);
        }
    }
    for source in sources {
        let root = match cpg.node(source).map(|node| &node.kind) {
            Some(
                CpgNodeKind::BinaryOp { .. }
                | CpgNodeKind::UnaryOp { .. }
                | CpgNodeKind::Literal { .. }
                | CpgNodeKind::Identifier { .. },
            ) => Some(source),
            _ => cpg.ast_children(source).first().copied(),
        };
        let Some(root) = root else {
            continue;
        };
        let mut pending = vec![root];
        while let Some(node) = pending.pop() {
            if !scope.contains(&node) || !guards.insert(node) {
                continue;
            }
            let mut children = cpg.ast_children(node).to_vec();
            children.reverse();
            pending.extend(children);
        }
    }
    guards
}

fn guard_value(cpg: &CodePropertyGraph, node: NodeId) -> Option<CloneGuardValue> {
    match &cpg.node(node)?.kind {
        CpgNodeKind::BinaryOp { operator } => {
            Some(CloneGuardValue::BinaryOperator(operator.clone()))
        }
        CpgNodeKind::UnaryOp { operator } => Some(CloneGuardValue::UnaryOperator(operator.clone())),
        CpgNodeKind::Literal { kind } => Some(match kind {
            LiteralKind::Integer(value) => CloneGuardValue::Integer(*value),
            LiteralKind::Float(value) => CloneGuardValue::FloatBits(value.to_bits()),
            LiteralKind::String(value) => CloneGuardValue::String(value.clone()),
            LiteralKind::Char(value) => CloneGuardValue::Character(*value),
            LiteralKind::Bool(value) => CloneGuardValue::Boolean(*value),
            LiteralKind::Null => CloneGuardValue::Null,
            LiteralKind::Array => CloneGuardValue::Array,
            LiteralKind::Object => CloneGuardValue::Object,
            LiteralKind::Regex(value) => CloneGuardValue::Regex(value.clone()),
        }),
        _ => None,
    }
}

#[cfg(test)]
#[path = "clone_bugs/tests.rs"]
mod tests;
