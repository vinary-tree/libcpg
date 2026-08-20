//! Evidence-bounded vulnerability extrapolation across function PDGs.
//!
//! The seed and each candidate are projected to program-dependence graphs
//! (PDGs). Their Weisfeiler--Lehman (WL) label multisets are refined in one
//! shared vocabulary and compared by cosine similarity. Equal-size candidates
//! above a second threshold receive bounded confirmation from the existing
//! stack-safe relaxed VF2 matcher. Results are advisory and never mutate the
//! input graph.

use std::collections::BTreeSet;
use std::sync::Arc;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::analysis::wl::wl_multiset_cosine;
use crate::analysis::{pdg_projection, GraphProjection, MAX_WL_ITERATIONS};
use crate::pattern::relaxed_pdg_equivalent;
use crate::{CodePropertyGraph, CpgNodeKind, NodeId};

/// Compile-time ceiling for non-seed functions considered by one request.
pub const MAX_EXTRAPOLATION_FUNCTIONS: usize = 100_000;
/// Compile-time ceiling for vertices in any compared PDG.
pub const MAX_EXTRAPOLATION_PDG_NODES: usize = 10_000;
/// Compile-time ceiling for relaxed VF2 confirmations in one request.
pub const MAX_EXTRAPOLATION_VF2_INVOCATIONS: usize = 100_000;
/// Compile-time ceiling for retained ranked candidates.
pub const MAX_EXTRAPOLATION_RESULTS: usize = 100_000;

/// Similarity thresholds and explicit resource limits for extrapolation.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ExtrapolationOptions {
    /// Minimum WL-multiset cosine required for a retained candidate.
    pub minimum_similarity: f64,
    /// Minimum WL similarity before equal-size relaxed VF2 confirmation.
    pub vf2_threshold: f64,
    /// Number of jointly canonicalized WL refinement rounds.
    pub wl_iterations: usize,
    /// Maximum non-seed functions examined after stable ordering.
    pub max_functions: usize,
    /// Maximum vertices allowed in the seed or one candidate PDG.
    pub max_pdg_nodes: usize,
    /// Maximum relaxed VF2 confirmations.
    pub max_vf2_invocations: usize,
    /// Maximum ranked candidates retained after complete scoring.
    pub max_results: usize,
}

impl Default for ExtrapolationOptions {
    fn default() -> Self {
        Self {
            minimum_similarity: 0.5,
            vf2_threshold: 0.8,
            wl_iterations: 3,
            max_functions: 10_000,
            max_pdg_nodes: 300,
            max_vf2_invocations: 10_000,
            max_results: 1_000,
        }
    }
}

/// One ranked, advisory function candidate.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ExtrapolationCandidate {
    /// Candidate function root.
    pub function: NodeId,
    /// Final ranking score in the closed interval `[0, 1]`.
    pub score: f64,
    /// Joint-vocabulary WL-label multiset cosine in `[0, 1]`.
    pub wl_similarity: f64,
    /// Whether bounded relaxed VF2 confirmed equal-size PDG equivalence.
    pub vf2_confirmed: bool,
    /// Candidate PDG vertex count.
    pub pdg_nodes: usize,
    /// Candidate PDG edge count after parallel-edge merging.
    pub pdg_edges: usize,
}

/// Invalid input or explicit evidence boundary qualifying a result.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ExtrapolationIssue {
    /// The seed node is absent from the CPG.
    UnknownSeed {
        /// Rejected seed identifier.
        seed: NodeId,
    },
    /// The seed exists but is not a function root.
    SeedNotFunction {
        /// Rejected seed identifier.
        seed: NodeId,
    },
    /// A threshold is non-finite or outside the closed unit interval.
    InvalidProbability {
        /// Stable option name.
        option: Arc<str>,
    },
    /// A requested resource or iteration limit exceeds its compile-time ceiling.
    LimitTooLarge {
        /// Stable option name.
        option: Arc<str>,
        /// Rejected requested value.
        requested: usize,
        /// Maximum accepted value.
        maximum: usize,
    },
    /// Stable function enumeration exceeded the configured request limit.
    FunctionsCapped {
        /// Non-seed functions represented by the CPG.
        observed: usize,
        /// Configured request limit.
        cap: usize,
    },
    /// One function's PDG exceeded the configured vertex limit.
    PdgNodesCapped {
        /// Affected function root.
        function: NodeId,
        /// Represented PDG vertices.
        observed: usize,
        /// Configured request limit.
        cap: usize,
    },
    /// More candidates required VF2 confirmation than the configured limit.
    Vf2InvocationsCapped {
        /// Number of candidates meeting the VF2 preconditions.
        needed: usize,
        /// Confirmations actually permitted.
        cap: usize,
    },
    /// More candidates qualified than the configured retained-result limit.
    ResultsCapped {
        /// Number of candidates meeting the similarity threshold.
        observed: usize,
        /// Ranked candidates retained.
        cap: usize,
    },
}

/// Ranked candidates plus explicit completeness evidence.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ExtrapolationAnalysis {
    /// Labeled vulnerable function supplied by the caller.
    pub seed: NodeId,
    /// Candidates ordered by descending score, confirmation, WL score, then id.
    pub candidates: Vec<ExtrapolationCandidate>,
    /// Functions for which a complete WL score was computed, in stable-id order.
    pub analyzed_functions: Vec<NodeId>,
    /// Number of relaxed VF2 confirmations actually executed.
    pub vf2_invocations: usize,
    /// Seed PDG vertex count when the seed could be projected.
    pub seed_pdg_nodes: Option<usize>,
    /// Seed PDG edge count when the seed could be projected.
    pub seed_pdg_edges: Option<usize>,
    /// Ordered invalid-input and resource-bound evidence.
    pub issues: BTreeSet<ExtrapolationIssue>,
}

impl ExtrapolationAnalysis {
    /// Returns whether all represented functions and qualifying evidence were retained.
    pub fn is_complete(&self) -> bool {
        self.issues.is_empty()
    }

    fn empty(seed: NodeId, issues: BTreeSet<ExtrapolationIssue>) -> Self {
        Self {
            seed,
            candidates: Vec::new(),
            analyzed_functions: Vec::new(),
            vf2_invocations: 0,
            seed_pdg_nodes: None,
            seed_pdg_edges: None,
            issues,
        }
    }
}

/// Returns only the ranked `(function, score)` pairs for positive discovery.
///
/// Call [`analyze_vulnerability_extrapolation`] when a consumer must inspect
/// completeness or distinguish invalid input from an empty ranking.
pub fn extrapolate(
    cpg: &CodePropertyGraph,
    seed: NodeId,
    options: &ExtrapolationOptions,
) -> Vec<(NodeId, f64)> {
    analyze_vulnerability_extrapolation(cpg, seed, options)
        .candidates
        .into_iter()
        .map(|candidate| (candidate.function, candidate.score))
        .collect()
}

/// Ranks functions by joint-vocabulary PDG WL cosine and bounded VF2 evidence.
pub fn analyze_vulnerability_extrapolation(
    cpg: &CodePropertyGraph,
    seed: NodeId,
    options: &ExtrapolationOptions,
) -> ExtrapolationAnalysis {
    let mut issues = validate_options(options);
    let Some(seed_node) = cpg.node(seed) else {
        issues.insert(ExtrapolationIssue::UnknownSeed { seed });
        return ExtrapolationAnalysis::empty(seed, issues);
    };
    if !matches!(seed_node.kind, CpgNodeKind::Function { .. }) {
        issues.insert(ExtrapolationIssue::SeedNotFunction { seed });
        return ExtrapolationAnalysis::empty(seed, issues);
    }
    if !issues.is_empty() {
        return ExtrapolationAnalysis::empty(seed, issues);
    }

    let seed_pdg = pdg_projection(cpg, seed).expect("validated function seed has a PDG projection");
    let mut analysis = ExtrapolationAnalysis::empty(seed, issues);
    analysis.seed_pdg_nodes = Some(seed_pdg.node_count());
    analysis.seed_pdg_edges = Some(seed_pdg.edge_count());
    if seed_pdg.node_count() > options.max_pdg_nodes {
        analysis.issues.insert(ExtrapolationIssue::PdgNodesCapped {
            function: seed,
            observed: seed_pdg.node_count(),
            cap: options.max_pdg_nodes,
        });
        return analysis;
    }

    let mut functions = cpg
        .functions()
        .map(|function| function.id)
        .filter(|&function| function != seed)
        .collect::<Vec<_>>();
    functions.sort_unstable();
    if functions.len() > options.max_functions {
        analysis.issues.insert(ExtrapolationIssue::FunctionsCapped {
            observed: functions.len(),
            cap: options.max_functions,
        });
        functions.truncate(options.max_functions);
    }

    let mut vf2_needed = 0usize;
    for function in functions {
        let candidate_pdg = pdg_projection(cpg, function)
            .expect("enumerated function candidate has a PDG projection");
        if candidate_pdg.node_count() > options.max_pdg_nodes {
            analysis.issues.insert(ExtrapolationIssue::PdgNodesCapped {
                function,
                observed: candidate_pdg.node_count(),
                cap: options.max_pdg_nodes,
            });
            continue;
        }

        let wl_similarity = wl_multiset_cosine(&seed_pdg, &candidate_pdg, options.wl_iterations)
            .expect("CPG-derived PDG projections are well formed");
        analysis.analyzed_functions.push(function);

        let vf2_eligible = wl_similarity >= options.vf2_threshold
            && same_projection_size(&seed_pdg, &candidate_pdg);
        let vf2_confirmed = if vf2_eligible {
            vf2_needed = vf2_needed.saturating_add(1);
            if analysis.vf2_invocations < options.max_vf2_invocations {
                analysis.vf2_invocations += 1;
                relaxed_pdg_equivalent(cpg, &seed_pdg, &candidate_pdg)
            } else {
                false
            }
        } else {
            false
        };

        if wl_similarity >= options.minimum_similarity || vf2_confirmed {
            analysis.candidates.push(ExtrapolationCandidate {
                function,
                score: if vf2_confirmed { 1.0 } else { wl_similarity },
                wl_similarity,
                vf2_confirmed,
                pdg_nodes: candidate_pdg.node_count(),
                pdg_edges: candidate_pdg.edge_count(),
            });
        }
    }

    if vf2_needed > options.max_vf2_invocations {
        analysis
            .issues
            .insert(ExtrapolationIssue::Vf2InvocationsCapped {
                needed: vf2_needed,
                cap: options.max_vf2_invocations,
            });
    }

    analysis.candidates.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| right.vf2_confirmed.cmp(&left.vf2_confirmed))
            .then_with(|| right.wl_similarity.total_cmp(&left.wl_similarity))
            .then_with(|| left.function.cmp(&right.function))
    });
    if analysis.candidates.len() > options.max_results {
        analysis.issues.insert(ExtrapolationIssue::ResultsCapped {
            observed: analysis.candidates.len(),
            cap: options.max_results,
        });
        analysis.candidates.truncate(options.max_results);
    }
    analysis
}

fn same_projection_size(left: &GraphProjection, right: &GraphProjection) -> bool {
    left.node_count() == right.node_count() && left.edge_count() == right.edge_count()
}

fn validate_options(options: &ExtrapolationOptions) -> BTreeSet<ExtrapolationIssue> {
    let mut issues = BTreeSet::new();
    for (name, value) in [
        ("minimum_similarity", options.minimum_similarity),
        ("vf2_threshold", options.vf2_threshold),
    ] {
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            issues.insert(ExtrapolationIssue::InvalidProbability {
                option: Arc::from(name),
            });
        }
    }
    for (name, requested, maximum) in [
        ("wl_iterations", options.wl_iterations, MAX_WL_ITERATIONS),
        (
            "max_functions",
            options.max_functions,
            MAX_EXTRAPOLATION_FUNCTIONS,
        ),
        (
            "max_pdg_nodes",
            options.max_pdg_nodes,
            MAX_EXTRAPOLATION_PDG_NODES,
        ),
        (
            "max_vf2_invocations",
            options.max_vf2_invocations,
            MAX_EXTRAPOLATION_VF2_INVOCATIONS,
        ),
        (
            "max_results",
            options.max_results,
            MAX_EXTRAPOLATION_RESULTS,
        ),
    ] {
        if requested > maximum {
            issues.insert(ExtrapolationIssue::LimitTooLarge {
                option: Arc::from(name),
                requested,
                maximum,
            });
        }
    }
    issues
}
