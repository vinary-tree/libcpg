//! Owned Datalog EDB facts derived from proof-qualified semantic reports.
//!
//! This is a one-way evidence bridge, not a shared reasoning engine. It
//! replays exact proof edges, copies stable source identities into canonical
//! rows, and preserves completion. A caller-owned Datalog engine may consume
//! those rows, but no tuple can recreate equality-proof authority.

use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

#[cfg(feature = "serde")]
use serde::Serialize;

use crate::NodeId;

use super::datalog::{number, symbol};
use super::{
    ExpressionEquivalenceCompletion, ExpressionEquivalenceKind, ExpressionProofError,
    SemanticDataflowCompletion, SemanticExpressionFlowCompletion, SemanticExpressionFlowReport,
    SouffleFactFile,
};

/// Hard ceiling on owned rows in one semantic EDB export.
pub const MAX_SEMANTIC_DATALOG_FACTS: usize = 64 * 1024 * 1024;
/// Hard ceiling on encoded bytes across one semantic Souffle file set.
pub const MAX_SEMANTIC_DATALOG_BYTES: usize = 1024 * 1024 * 1024;

/// Resource policy for semantic EDB export and rendering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct SemanticDatalogLimits {
    /// Maximum rows copied across all relations.
    pub max_facts: usize,
    /// Maximum canonical bytes across all rendered fact files.
    pub max_rendered_bytes: usize,
}

impl Default for SemanticDatalogLimits {
    fn default() -> Self {
        Self {
            max_facts: 8 * 1024 * 1024,
            max_rendered_bytes: 256 * 1024 * 1024,
        }
    }
}

impl SemanticDatalogLimits {
    fn validate(self) -> Result<Self, SemanticDatalogError> {
        if self.max_facts == 0 || self.max_facts > MAX_SEMANTIC_DATALOG_FACTS {
            return Err(SemanticDatalogError::InvalidLimit("max_facts"));
        }
        if self.max_rendered_bytes == 0 || self.max_rendered_bytes > MAX_SEMANTIC_DATALOG_BYTES {
            return Err(SemanticDatalogError::InvalidLimit("max_rendered_bytes"));
        }
        Ok(self)
    }
}

/// One stable membership in a semantic expression class.
///
/// A representative's self-membership is reflexive identity. Every distinct
/// member is connected to the representative by replayed exact proof edges.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct SemanticClassFact {
    /// Function whose control-flow graph owns the occurrence.
    pub function: NodeId,
    /// Smallest stable source identity in the class.
    pub representative: NodeId,
    /// Stable source occurrence in the class.
    pub occurrence: NodeId,
}

/// One replayed exact proof edge, addressable in the source report.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct SemanticProofFact {
    /// Caller-selected equality-analysis domain.
    pub analysis_domain: NodeId,
    /// Smaller stable source identity.
    pub left: NodeId,
    /// Larger stable source identity.
    pub right: NodeId,
    /// Index into `SemanticExpressionFlowReport::equivalence.pairs`.
    pub proof_pair: u32,
}

/// One class fact at a stable CFG program point.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct SemanticPointFact {
    /// Selected function.
    pub function: NodeId,
    /// Stable program point.
    pub node: NodeId,
    /// Stable semantic-class representative.
    pub class: NodeId,
}

/// One proof-and-flow-qualified CSE candidate row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct SemanticCseFact {
    /// Selected function.
    pub function: NodeId,
    /// Stable semantic-class representative.
    pub class: NodeId,
    /// Dominating expression occurrence.
    pub producer: NodeId,
    /// Later equivalent occurrence.
    pub use_node: NodeId,
    /// Index of the replayed exact source proof.
    pub proof_pair: u32,
}

/// One occurrence governed by a PRE placement candidate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct SemanticPreFact {
    /// Selected function.
    pub function: NodeId,
    /// Stable semantic-class representative.
    pub class: NodeId,
    /// Common dominating placement.
    pub placement: NodeId,
    /// Occurrence dominated by the placement.
    pub occurrence: NodeId,
}

/// Independently completed semantic relation dimension.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub enum SemanticDatalogDimension {
    /// Exact replayed expression equality.
    ExactEquality,
    /// Forward must-availability.
    Availability,
    /// Backward must-anticipability.
    Anticipability,
    /// CSE candidate discovery.
    CseCandidates,
    /// PRE candidate discovery.
    PreCandidates,
}

/// Exact completion or limiting cause exported beside semantic rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub enum SemanticDatalogCompletion {
    /// The semantic dimension reached its fixed point and completed output.
    Complete,
    /// One or more inputs were explicitly unsupported.
    UnsupportedInputs,
    /// Equality candidates exceeded their cap.
    CandidateLimited,
    /// Flat expression input nodes exceeded their cap.
    InputNodeLimited,
    /// Replete graph nodes or children exceeded their cap.
    NodeLimited,
    /// Equality logical work exceeded its cap.
    WorkLimited,
    /// Rewrite matches exceeded their cap.
    MatchLimited,
    /// Equality iterations exceeded their cap.
    IterationLimited,
    /// Public equality pairs exceeded their cap.
    PairLimited,
    /// Proof construction or provenance exceeded its cap.
    ProofLimited,
    /// Forward must-dataflow exceeded its visit cap.
    AvailableDataflowLimited,
    /// Backward must-dataflow exceeded its visit cap.
    VeryBusyDataflowLimited,
    /// Candidate path/report work exceeded its cap.
    CandidateWorkLimited,
}

impl SemanticDatalogCompletion {
    /// Returns whether absence in this semantic dimension is meaningful.
    pub const fn is_complete(self) -> bool {
        matches!(self, Self::Complete)
    }
}

/// One explicit semantic completion row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct SemanticCompletionFact {
    /// Selected function.
    pub function: NodeId,
    /// Relation dimension governed by this completion.
    pub dimension: SemanticDatalogDimension,
    /// Exact completion or limiting cause.
    pub completion: SemanticDatalogCompletion,
}

/// Completion reported by the caller-owned Datalog evaluator.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub enum DatalogEvaluationCompletion {
    /// The caller-owned relational fixed point completed.
    Complete,
    /// Logical work was exhausted.
    WorkLimited,
    /// Fixed-point iteration was exhausted.
    IterationLimited,
    /// Resident or engine memory was exhausted.
    MemoryLimited,
    /// A time budget was exhausted.
    TimeLimited,
    /// Evaluation was cancelled.
    Cancelled,
    /// The engine did not supply a precise completion cause.
    Unknown,
}

impl DatalogEvaluationCompletion {
    /// Returns whether the relational fixed point completed.
    pub const fn is_complete(self) -> bool {
        matches!(self, Self::Complete)
    }
}

/// Combined semantic/relational completion for one caller query.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct SemanticDatalogQueryCompletion {
    /// Semantic relation dimension queried.
    pub dimension: SemanticDatalogDimension,
    /// Semantic completion copied from the analyzed report.
    pub semantic: SemanticDatalogCompletion,
    /// Caller-owned Datalog evaluation completion.
    pub datalog: DatalogEvaluationCompletion,
}

impl SemanticDatalogQueryCompletion {
    /// Returns whether both knowledge-growth processes completed.
    pub const fn is_complete(self) -> bool {
        self.semantic.is_complete() && self.datalog.is_complete()
    }

    /// Returns whether absence from the combined query is conclusive.
    pub const fn can_conclude_absence(self) -> bool {
        self.is_complete()
    }
}

/// Canonical owned semantic EDB bundle.
///
/// The bundle retains no CPG, semantic-report, Replete, or Datalog-engine
/// reference. Serialization is audit/data transport only; it does not restore
/// equality-proof authority.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct SemanticDatalogFacts {
    class_members: Vec<SemanticClassFact>,
    proof_edges: Vec<SemanticProofFact>,
    available: Vec<SemanticPointFact>,
    anticipable: Vec<SemanticPointFact>,
    cse_candidates: Vec<SemanticCseFact>,
    pre_candidates: Vec<SemanticPreFact>,
    completion: Vec<SemanticCompletionFact>,
    limits: SemanticDatalogLimits,
}

impl SemanticDatalogFacts {
    /// Stable semantic class-membership rows.
    pub fn class_members(&self) -> &[SemanticClassFact] {
        &self.class_members
    }

    /// Replayed exact proof-edge rows.
    pub fn proof_edges(&self) -> &[SemanticProofFact] {
        &self.proof_edges
    }

    /// Must-available class rows.
    pub fn available(&self) -> &[SemanticPointFact] {
        &self.available
    }

    /// Must-anticipable class rows.
    pub fn anticipable(&self) -> &[SemanticPointFact] {
        &self.anticipable
    }

    /// Advisory CSE rows.
    pub fn cse_candidates(&self) -> &[SemanticCseFact] {
        &self.cse_candidates
    }

    /// Advisory PRE occurrence rows.
    pub fn pre_candidates(&self) -> &[SemanticPreFact] {
        &self.pre_candidates
    }

    /// Exact completion rows, one for every dimension.
    pub fn completions(&self) -> &[SemanticCompletionFact] {
        &self.completion
    }

    /// Policy that bounded construction and rendering.
    pub const fn limits(&self) -> SemanticDatalogLimits {
        self.limits
    }

    /// Total number of canonical rows across every relation.
    pub fn fact_count(&self) -> usize {
        self.class_members.len()
            + self.proof_edges.len()
            + self.available.len()
            + self.anticipable.len()
            + self.cse_candidates.len()
            + self.pre_candidates.len()
            + self.completion.len()
    }

    /// Combines one semantic dimension with caller-owned Datalog completion.
    pub fn query_completion(
        &self,
        dimension: SemanticDatalogDimension,
        datalog: DatalogEvaluationCompletion,
    ) -> SemanticDatalogQueryCompletion {
        let semantic = self
            .completion
            .iter()
            .find(|fact| fact.dimension == dimension)
            .map_or(SemanticDatalogCompletion::UnsupportedInputs, |fact| {
                fact.completion
            });
        SemanticDatalogQueryCompletion {
            dimension,
            semantic,
            datalog,
        }
    }

    /// Renders canonical RFC 4180 tab-separated Souffle fact buffers.
    ///
    /// The caller chooses filesystem, atomic-write, sandbox, and Datalog
    /// execution policy. No file is written by this method.
    pub fn souffle_files(&self) -> Result<Vec<SouffleFactFile>, SemanticDatalogError> {
        let encoded_bytes = self.encoded_len()?;
        if encoded_bytes > self.limits.max_rendered_bytes {
            return Err(SemanticDatalogError::RenderedByteLimit {
                required: encoded_bytes,
                limit: self.limits.max_rendered_bytes,
            });
        }
        Ok(vec![
            SouffleFactFile {
                relation: "semantic_class",
                contents: render_class_members(&self.class_members),
            },
            SouffleFactFile {
                relation: "semantic_proof",
                contents: render_proof_edges(&self.proof_edges),
            },
            SouffleFactFile {
                relation: "semantic_available",
                contents: render_points(&self.available),
            },
            SouffleFactFile {
                relation: "semantic_anticipable",
                contents: render_points(&self.anticipable),
            },
            SouffleFactFile {
                relation: "semantic_cse_candidate",
                contents: render_cse(&self.cse_candidates),
            },
            SouffleFactFile {
                relation: "semantic_pre_candidate",
                contents: render_pre(&self.pre_candidates),
            },
            SouffleFactFile {
                relation: "semantic_completion",
                contents: render_completion(&self.completion),
            },
        ])
    }

    fn encoded_len(&self) -> Result<usize, SemanticDatalogError> {
        let mut total = 0usize;
        for fact in &self.class_members {
            charge_len(
                &mut total,
                numbers_len(&[
                    fact.function.as_u32(),
                    fact.representative.as_u32(),
                    fact.occurrence.as_u32(),
                ]),
            )?;
        }
        for fact in &self.proof_edges {
            charge_len(
                &mut total,
                numbers_len(&[
                    fact.analysis_domain.as_u32(),
                    fact.left.as_u32(),
                    fact.right.as_u32(),
                    fact.proof_pair,
                ]),
            )?;
        }
        for fact in self.available.iter().chain(&self.anticipable) {
            charge_len(
                &mut total,
                numbers_len(&[
                    fact.function.as_u32(),
                    fact.node.as_u32(),
                    fact.class.as_u32(),
                ]),
            )?;
        }
        for fact in &self.cse_candidates {
            charge_len(
                &mut total,
                numbers_len(&[
                    fact.function.as_u32(),
                    fact.class.as_u32(),
                    fact.producer.as_u32(),
                    fact.use_node.as_u32(),
                    fact.proof_pair,
                ]),
            )?;
        }
        for fact in &self.pre_candidates {
            charge_len(
                &mut total,
                numbers_len(&[
                    fact.function.as_u32(),
                    fact.class.as_u32(),
                    fact.placement.as_u32(),
                    fact.occurrence.as_u32(),
                ]),
            )?;
        }
        for fact in &self.completion {
            let row = decimal_len(fact.function.as_u32())
                .checked_add(1 + quoted_len(dimension_name(fact.dimension)))
                .and_then(|value| {
                    value.checked_add(1 + quoted_len(completion_name(fact.completion)))
                })
                .and_then(|value| value.checked_add(1))
                .ok_or(SemanticDatalogError::SizeOverflow)?;
            charge_len(&mut total, row)?;
        }
        Ok(total)
    }
}

/// Semantic EDB construction failure.
#[derive(Debug, Error)]
pub enum SemanticDatalogError {
    /// A limit is zero or exceeds its public hard ceiling.
    #[error("invalid semantic-Datalog limit: {0}")]
    InvalidLimit(&'static str),
    /// The request would exceed its fact-row limit.
    #[error("semantic-Datalog fact limit exceeded: required {required}, limit {limit}")]
    FactLimit {
        /// Canonical rows required by the report.
        required: usize,
        /// Configured maximum canonical rows.
        limit: usize,
    },
    /// Canonical rendering would exceed its byte limit.
    #[error("semantic-Datalog byte limit exceeded: required {required}, limit {limit}")]
    RenderedByteLimit {
        /// Bytes required by canonical rendering.
        required: usize,
        /// Configured maximum canonical bytes.
        limit: usize,
    },
    /// Exact proof replay rejected a source edge.
    #[error(transparent)]
    Proof(#[from] ExpressionProofError),
    /// A checked report invariant was malformed.
    #[error("semantic-Datalog report invariant failed: {0}")]
    Invariant(&'static str),
    /// Checked size arithmetic overflowed.
    #[error("semantic-Datalog size arithmetic overflowed")]
    SizeOverflow,
}

/// Copies a semantic-flow report into an owned canonical Datalog EDB bundle.
///
/// Every distinct class membership and candidate proof reference is validated
/// against independently replayed exact proof edges. The function borrows its
/// report and retains no reference to it.
pub fn export_semantic_datalog(
    report: &SemanticExpressionFlowReport,
    limits: SemanticDatalogLimits,
) -> Result<SemanticDatalogFacts, SemanticDatalogError> {
    let limits = limits.validate()?;
    let minimum_required = report
        .classes
        .iter()
        .map(|class| class.occurrences.len())
        .try_fold(5usize, |total, value| total.checked_add(value))
        .ok_or(SemanticDatalogError::SizeOverflow)?;
    if minimum_required > limits.max_facts {
        return Err(SemanticDatalogError::FactLimit {
            required: minimum_required,
            limit: limits.max_facts,
        });
    }

    let mut source_class = BTreeMap::<NodeId, NodeId>::new();
    let mut class_members = BTreeSet::new();
    let mut previous_representative = None;
    for class in &report.classes {
        if class.occurrences.is_empty()
            || class.occurrences.first().copied() != Some(class.representative)
            || !strictly_sorted(&class.occurrences)
            || previous_representative.is_some_and(|previous| previous >= class.representative)
        {
            return Err(SemanticDatalogError::Invariant(
                "semantic classes are not canonical",
            ));
        }
        previous_representative = Some(class.representative);
        for &occurrence in &class.occurrences {
            if source_class
                .insert(occurrence, class.representative)
                .is_some()
            {
                return Err(SemanticDatalogError::Invariant(
                    "one occurrence belongs to multiple semantic classes",
                ));
            }
            class_members.insert(SemanticClassFact {
                function: report.function,
                representative: class.representative,
                occurrence,
            });
        }
    }

    let required = estimated_fact_count(report, &source_class)?;
    if required > limits.max_facts {
        return Err(SemanticDatalogError::FactLimit {
            required,
            limit: limits.max_facts,
        });
    }

    let mut proof_edges = BTreeSet::new();
    let mut proof_by_index = BTreeMap::new();
    let mut adjacency = BTreeMap::<NodeId, Vec<NodeId>>::new();
    for (index, pair) in report.equivalence.pairs.iter().enumerate() {
        if pair.kind != ExpressionEquivalenceKind::Exact {
            continue;
        }
        if pair.left >= pair.right || pair.analysis_domain != report.equivalence.analysis_domain {
            return Err(SemanticDatalogError::Invariant(
                "exact proof pair is not canonically qualified",
            ));
        }
        let (Some(&left_class), Some(&right_class)) =
            (source_class.get(&pair.left), source_class.get(&pair.right))
        else {
            continue;
        };
        if left_class != right_class {
            return Err(SemanticDatalogError::Invariant(
                "exact proof crosses semantic classes",
            ));
        }
        pair.proof.replay()?;
        let proof_pair = u32::try_from(index)
            .map_err(|_| SemanticDatalogError::Invariant("proof index overflow"))?;
        proof_edges.insert(SemanticProofFact {
            analysis_domain: pair.analysis_domain,
            left: pair.left,
            right: pair.right,
            proof_pair,
        });
        proof_by_index.insert(index, (pair.left, pair.right));
        adjacency.entry(pair.left).or_default().push(pair.right);
        adjacency.entry(pair.right).or_default().push(pair.left);
    }
    for neighbours in adjacency.values_mut() {
        neighbours.sort_unstable();
        neighbours.dedup();
    }
    for class in &report.classes {
        if class.occurrences.len() > 1
            && !class_is_connected(class.representative, &class.occurrences, &adjacency)
        {
            return Err(SemanticDatalogError::Invariant(
                "nontrivial semantic class lacks spanning exact proofs",
            ));
        }
    }

    let available = copy_points(
        report.function,
        &report.available_in,
        &source_class,
        "available fact references an unknown class",
    )?;
    let anticipable = copy_points(
        report.function,
        &report.busy_out,
        &source_class,
        "anticipable fact references an unknown class",
    )?;

    let mut cse_candidates = BTreeSet::new();
    for candidate in &report.cse_candidates {
        let &(left, right) =
            proof_by_index
                .get(&candidate.proof_pair)
                .ok_or(SemanticDatalogError::Invariant(
                    "CSE candidate references a missing exact proof",
                ))?;
        let endpoints = if candidate.producer < candidate.use_node {
            (candidate.producer, candidate.use_node)
        } else {
            (candidate.use_node, candidate.producer)
        };
        if (left, right) != endpoints
            || source_class.get(&candidate.producer) != Some(&candidate.class)
            || source_class.get(&candidate.use_node) != Some(&candidate.class)
        {
            return Err(SemanticDatalogError::Invariant(
                "CSE candidate proof or class does not match its endpoints",
            ));
        }
        cse_candidates.insert(SemanticCseFact {
            function: report.function,
            class: candidate.class,
            producer: candidate.producer,
            use_node: candidate.use_node,
            proof_pair: u32::try_from(candidate.proof_pair)
                .map_err(|_| SemanticDatalogError::Invariant("proof index overflow"))?,
        });
    }

    let mut pre_candidates = BTreeSet::new();
    for candidate in &report.pre_candidates {
        if candidate.occurrences.is_empty() || !strictly_sorted(&candidate.occurrences) {
            return Err(SemanticDatalogError::Invariant(
                "PRE occurrences are not canonical",
            ));
        }
        let class_occurrences = report
            .classes
            .iter()
            .find(|class| class.representative == candidate.class)
            .map(|class| class.occurrences.as_slice())
            .ok_or(SemanticDatalogError::Invariant(
                "PRE candidate references an unknown class",
            ))?;
        if class_occurrences != candidate.occurrences.as_slice() {
            return Err(SemanticDatalogError::Invariant(
                "PRE candidate does not cover its semantic class",
            ));
        }
        let occurrence_set = candidate
            .occurrences
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        let mut pre_adjacency = BTreeMap::<NodeId, Vec<NodeId>>::new();
        for &index in &candidate.proof_pairs {
            let &(left, right) =
                proof_by_index
                    .get(&index)
                    .ok_or(SemanticDatalogError::Invariant(
                        "PRE candidate references a missing exact proof",
                    ))?;
            if !occurrence_set.contains(&left) || !occurrence_set.contains(&right) {
                return Err(SemanticDatalogError::Invariant(
                    "PRE proof leaves its semantic class",
                ));
            }
            pre_adjacency.entry(left).or_default().push(right);
            pre_adjacency.entry(right).or_default().push(left);
        }
        if candidate.occurrences.len() > 1
            && !class_is_connected(candidate.class, &candidate.occurrences, &pre_adjacency)
        {
            return Err(SemanticDatalogError::Invariant(
                "PRE candidate proofs do not span its class",
            ));
        }
        for &occurrence in &candidate.occurrences {
            pre_candidates.insert(SemanticPreFact {
                function: report.function,
                class: candidate.class,
                placement: candidate.placement,
                occurrence,
            });
        }
    }

    let completion = completion_facts(report);
    let facts = SemanticDatalogFacts {
        class_members: class_members.into_iter().collect(),
        proof_edges: proof_edges.into_iter().collect(),
        available: available.into_iter().collect(),
        anticipable: anticipable.into_iter().collect(),
        cse_candidates: cse_candidates.into_iter().collect(),
        pre_candidates: pre_candidates.into_iter().collect(),
        completion,
        limits,
    };
    if facts.fact_count() > limits.max_facts {
        return Err(SemanticDatalogError::FactLimit {
            required: facts.fact_count(),
            limit: limits.max_facts,
        });
    }
    Ok(facts)
}

fn estimated_fact_count(
    report: &SemanticExpressionFlowReport,
    source_class: &BTreeMap<NodeId, NodeId>,
) -> Result<usize, SemanticDatalogError> {
    let counts = [
        report
            .classes
            .iter()
            .map(|class| class.occurrences.len())
            .try_fold(0usize, |total, value| total.checked_add(value))
            .ok_or(SemanticDatalogError::SizeOverflow)?,
        report
            .equivalence
            .pairs
            .iter()
            .filter(|pair| {
                pair.kind == ExpressionEquivalenceKind::Exact
                    && source_class.contains_key(&pair.left)
                    && source_class.contains_key(&pair.right)
            })
            .count(),
        report
            .available_in
            .values()
            .map(Vec::len)
            .try_fold(0usize, |total, value| total.checked_add(value))
            .ok_or(SemanticDatalogError::SizeOverflow)?,
        report
            .busy_out
            .values()
            .map(Vec::len)
            .try_fold(0usize, |total, value| total.checked_add(value))
            .ok_or(SemanticDatalogError::SizeOverflow)?,
        report.cse_candidates.len(),
        report
            .pre_candidates
            .iter()
            .map(|candidate| candidate.occurrences.len())
            .try_fold(0usize, |total, value| total.checked_add(value))
            .ok_or(SemanticDatalogError::SizeOverflow)?,
        5,
    ];
    counts
        .into_iter()
        .try_fold(0usize, |total, count| total.checked_add(count))
        .ok_or(SemanticDatalogError::SizeOverflow)
}

fn strictly_sorted(values: &[NodeId]) -> bool {
    values.windows(2).all(|pair| pair[0] < pair[1])
}

fn class_is_connected(
    representative: NodeId,
    occurrences: &[NodeId],
    adjacency: &BTreeMap<NodeId, Vec<NodeId>>,
) -> bool {
    let expected = occurrences.iter().copied().collect::<BTreeSet<_>>();
    let mut seen = BTreeSet::from([representative]);
    let mut pending = vec![representative];
    while let Some(current) = pending.pop() {
        if let Some(neighbours) = adjacency.get(&current) {
            for &neighbour in neighbours.iter().rev() {
                if expected.contains(&neighbour) && seen.insert(neighbour) {
                    pending.push(neighbour);
                }
            }
        }
    }
    seen == expected
}

fn copy_points(
    function: NodeId,
    points: &BTreeMap<NodeId, Vec<NodeId>>,
    source_class: &BTreeMap<NodeId, NodeId>,
    error: &'static str,
) -> Result<BTreeSet<SemanticPointFact>, SemanticDatalogError> {
    let representatives = source_class.values().copied().collect::<BTreeSet<_>>();
    let mut output = BTreeSet::new();
    for (&node, classes) in points {
        if !classes.windows(2).all(|pair| pair[0] < pair[1]) {
            return Err(SemanticDatalogError::Invariant(
                "semantic point facts are not canonical",
            ));
        }
        for &class in classes {
            if !representatives.contains(&class) {
                return Err(SemanticDatalogError::Invariant(error));
            }
            output.insert(SemanticPointFact {
                function,
                node,
                class,
            });
        }
    }
    Ok(output)
}

fn completion_facts(report: &SemanticExpressionFlowReport) -> Vec<SemanticCompletionFact> {
    let equality = equality_completion(report.equivalence.exact_completion);
    let availability = if equality.is_complete() {
        match report.available_completion {
            SemanticDataflowCompletion::Complete => SemanticDatalogCompletion::Complete,
            SemanticDataflowCompletion::IterationLimited => {
                SemanticDatalogCompletion::AvailableDataflowLimited
            }
        }
    } else {
        equality
    };
    let anticipability = if equality.is_complete() {
        match report.very_busy_completion {
            SemanticDataflowCompletion::Complete => SemanticDatalogCompletion::Complete,
            SemanticDataflowCompletion::IterationLimited => {
                SemanticDatalogCompletion::VeryBusyDataflowLimited
            }
        }
    } else {
        equality
    };
    let candidate_limited = matches!(
        report.completion,
        SemanticExpressionFlowCompletion::CandidateWorkLimited
    );
    let cse = if !equality.is_complete() {
        equality
    } else if !availability.is_complete() {
        availability
    } else if candidate_limited {
        SemanticDatalogCompletion::CandidateWorkLimited
    } else {
        SemanticDatalogCompletion::Complete
    };
    let pre = if !equality.is_complete() {
        equality
    } else if !anticipability.is_complete() {
        anticipability
    } else if candidate_limited {
        SemanticDatalogCompletion::CandidateWorkLimited
    } else {
        SemanticDatalogCompletion::Complete
    };
    [
        (SemanticDatalogDimension::ExactEquality, equality),
        (SemanticDatalogDimension::Availability, availability),
        (SemanticDatalogDimension::Anticipability, anticipability),
        (SemanticDatalogDimension::CseCandidates, cse),
        (SemanticDatalogDimension::PreCandidates, pre),
    ]
    .into_iter()
    .map(|(dimension, completion)| SemanticCompletionFact {
        function: report.function,
        dimension,
        completion,
    })
    .collect()
}

fn equality_completion(value: ExpressionEquivalenceCompletion) -> SemanticDatalogCompletion {
    match value {
        ExpressionEquivalenceCompletion::Complete => SemanticDatalogCompletion::Complete,
        ExpressionEquivalenceCompletion::UnsupportedInputs => {
            SemanticDatalogCompletion::UnsupportedInputs
        }
        ExpressionEquivalenceCompletion::CandidateLimited => {
            SemanticDatalogCompletion::CandidateLimited
        }
        ExpressionEquivalenceCompletion::InputNodeLimited => {
            SemanticDatalogCompletion::InputNodeLimited
        }
        ExpressionEquivalenceCompletion::NodeLimited => SemanticDatalogCompletion::NodeLimited,
        ExpressionEquivalenceCompletion::WorkLimited => SemanticDatalogCompletion::WorkLimited,
        ExpressionEquivalenceCompletion::MatchLimited => SemanticDatalogCompletion::MatchLimited,
        ExpressionEquivalenceCompletion::IterationLimited => {
            SemanticDatalogCompletion::IterationLimited
        }
        ExpressionEquivalenceCompletion::PairLimited => SemanticDatalogCompletion::PairLimited,
        ExpressionEquivalenceCompletion::ProofLimited => SemanticDatalogCompletion::ProofLimited,
    }
}

fn render_class_members(facts: &[SemanticClassFact]) -> String {
    render_numbers(facts.iter().map(|fact| {
        [
            fact.function.as_u32(),
            fact.representative.as_u32(),
            fact.occurrence.as_u32(),
        ]
    }))
}

fn render_proof_edges(facts: &[SemanticProofFact]) -> String {
    render_numbers(facts.iter().map(|fact| {
        [
            fact.analysis_domain.as_u32(),
            fact.left.as_u32(),
            fact.right.as_u32(),
            fact.proof_pair,
        ]
    }))
}

fn render_points(facts: &[SemanticPointFact]) -> String {
    render_numbers(facts.iter().map(|fact| {
        [
            fact.function.as_u32(),
            fact.node.as_u32(),
            fact.class.as_u32(),
        ]
    }))
}

fn render_cse(facts: &[SemanticCseFact]) -> String {
    render_numbers(facts.iter().map(|fact| {
        [
            fact.function.as_u32(),
            fact.class.as_u32(),
            fact.producer.as_u32(),
            fact.use_node.as_u32(),
            fact.proof_pair,
        ]
    }))
}

fn render_pre(facts: &[SemanticPreFact]) -> String {
    render_numbers(facts.iter().map(|fact| {
        [
            fact.function.as_u32(),
            fact.class.as_u32(),
            fact.placement.as_u32(),
            fact.occurrence.as_u32(),
        ]
    }))
}

fn render_numbers<const N: usize>(rows: impl Iterator<Item = [u32; N]>) -> String {
    let mut output = String::new();
    for row in rows {
        for (index, value) in row.into_iter().enumerate() {
            if index != 0 {
                output.push('\t');
            }
            number(&mut output, value);
        }
        output.push('\n');
    }
    output
}

fn render_completion(facts: &[SemanticCompletionFact]) -> String {
    let mut output = String::new();
    for fact in facts {
        number(&mut output, fact.function.as_u32());
        output.push('\t');
        symbol(&mut output, dimension_name(fact.dimension));
        output.push('\t');
        symbol(&mut output, completion_name(fact.completion));
        output.push('\n');
    }
    output
}

fn dimension_name(value: SemanticDatalogDimension) -> &'static str {
    match value {
        SemanticDatalogDimension::ExactEquality => "exact_equality",
        SemanticDatalogDimension::Availability => "availability",
        SemanticDatalogDimension::Anticipability => "anticipability",
        SemanticDatalogDimension::CseCandidates => "cse_candidates",
        SemanticDatalogDimension::PreCandidates => "pre_candidates",
    }
}

fn completion_name(value: SemanticDatalogCompletion) -> &'static str {
    match value {
        SemanticDatalogCompletion::Complete => "complete",
        SemanticDatalogCompletion::UnsupportedInputs => "unsupported_inputs",
        SemanticDatalogCompletion::CandidateLimited => "candidate_limited",
        SemanticDatalogCompletion::InputNodeLimited => "input_node_limited",
        SemanticDatalogCompletion::NodeLimited => "node_limited",
        SemanticDatalogCompletion::WorkLimited => "work_limited",
        SemanticDatalogCompletion::MatchLimited => "match_limited",
        SemanticDatalogCompletion::IterationLimited => "iteration_limited",
        SemanticDatalogCompletion::PairLimited => "pair_limited",
        SemanticDatalogCompletion::ProofLimited => "proof_limited",
        SemanticDatalogCompletion::AvailableDataflowLimited => "available_dataflow_limited",
        SemanticDatalogCompletion::VeryBusyDataflowLimited => "very_busy_dataflow_limited",
        SemanticDatalogCompletion::CandidateWorkLimited => "candidate_work_limited",
    }
}

fn numbers_len(values: &[u32]) -> usize {
    values
        .iter()
        .map(|&value| decimal_len(value))
        .sum::<usize>()
        + values.len()
}

fn decimal_len(mut value: u32) -> usize {
    let mut length = 1;
    while value >= 10 {
        value /= 10;
        length += 1;
    }
    length
}

fn quoted_len(value: &str) -> usize {
    value.len() + value.bytes().filter(|&byte| byte == b'"').count() + 2
}

fn charge_len(total: &mut usize, amount: usize) -> Result<(), SemanticDatalogError> {
    *total = total
        .checked_add(amount)
        .ok_or(SemanticDatalogError::SizeOverflow)?;
    Ok(())
}

#[cfg(test)]
#[path = "semantic_datalog_tests.rs"]
mod tests;
