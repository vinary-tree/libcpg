//! Bounded approximate evidence for exact Code Property Graph analyses.
//!
//! This module generates immutable, ranked candidates. It never mutates a Code
//! Property Graph (CPG), establishes program identity, creates graph edges, or
//! returns an exact-analysis verdict. Every consumer is paired with the exact
//! confirmation it must run before acting on a candidate.
//!
//! The implementation delegates edit distance to `liblevenshtein`. Its own
//! control flow is flat: input validation, deterministic candidate ordering,
//! and result ranking use loops and heap-backed collections rather than native
//! recursion. Every potentially input-shaped operation has an explicit work,
//! state, or memory limit, and incomplete work is reported as incomplete.

use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::mem::size_of;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::Arc;

use liblevenshtein::distance::{
    damerau_levenshtein_distance_bounded, standard_distance_bounded, transposition_distance_bounded,
};

use crate::NodeId;

/// Schema version of replayable approximate-evidence provenance.
pub const APPROXIMATE_EVIDENCE_SCHEMA_VERSION: u32 = 1;

/// Stable identity of an immutable approximate-matching candidate.
///
/// Graph and corpus namespaces prevent equal local identifiers from different
/// revisions or input collections from being mistaken for the same item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ApproximateId {
    /// A node in a namespaced CPG revision.
    CpgNode {
        /// Stable caller-assigned graph or revision namespace.
        graph: u64,
        /// Local CPG node identifier.
        node: NodeId,
    },
    /// An API-usage graph anchored in a namespaced CPG revision.
    ApiUsage {
        /// Stable caller-assigned graph or revision namespace.
        graph: u64,
        /// Anchor call of the API-usage graph.
        anchor: NodeId,
    },
    /// A caller-owned item outside a CPG.
    External {
        /// Stable caller-assigned corpus namespace.
        corpus: u64,
        /// Stable item identifier within the corpus.
        item: u64,
    },
}

/// Immutable tape compared by an approximate method.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ApproximateTape {
    /// Unicode scalar sequence, suitable for names and source-independent text.
    Text(Arc<str>),
    /// Typed token sequence, suitable for API sequences and structural traces.
    Tokens(Vec<Arc<str>>),
}

impl ApproximateTape {
    /// Creates a text tape.
    pub fn text(text: impl Into<Arc<str>>) -> Self {
        Self::Text(text.into())
    }

    /// Creates a typed token tape.
    pub fn tokens<I, S>(tokens: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<Arc<str>>,
    {
        Self::Tokens(tokens.into_iter().map(Into::into).collect())
    }

    fn units(&self) -> usize {
        match self {
            Self::Text(text) => text.chars().count(),
            Self::Tokens(tokens) => tokens.len(),
        }
    }
}

/// One immutable item admitted to approximate candidate generation.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ApproximateRecord {
    /// Stable identity retained by every emitted evidence record.
    pub id: ApproximateId,
    /// Immutable comparison tape.
    pub tape: Arc<ApproximateTape>,
}

impl ApproximateRecord {
    /// Creates an immutable record.
    pub fn new(id: ApproximateId, tape: ApproximateTape) -> Self {
        Self {
            id,
            tape: Arc::new(tape),
        }
    }
}

/// Supported edit-distance semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ApproximateMethod {
    /// Standard Levenshtein insertion, deletion, and substitution distance.
    StandardLevenshtein,
    /// Optimal string alignment with adjacent transposition.
    OptimalStringAlignment,
    /// Unrestricted Damerau-Levenshtein distance.
    DamerauLevenshtein,
}

impl ApproximateMethod {
    /// Mathematical contract of this method.
    pub const fn semantics(self) -> ApproximateScoreSemantics {
        match self {
            Self::StandardLevenshtein | Self::DamerauLevenshtein => {
                ApproximateScoreSemantics::MetricDistance
            }
            Self::OptimalStringAlignment => ApproximateScoreSemantics::SymmetricNonMetricDistance,
        }
    }

    const fn identity(self) -> &'static str {
        match self {
            Self::StandardLevenshtein => "liblevenshtein.standard.v1",
            Self::OptimalStringAlignment => "liblevenshtein.osa.v1",
            Self::DamerauLevenshtein => "liblevenshtein.damerau.v1",
        }
    }
}

/// Meaning of a raw approximate score.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ApproximateScoreSemantics {
    /// A non-negative distance satisfying the metric laws.
    MetricDistance,
    /// A symmetric non-negative distance without the triangle inequality.
    SymmetricNonMetricDistance,
}

/// Semiring used by distance and composition evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ApproximateSemiring {
    /// Tropical min-plus: alternative paths take minimum and sequential costs add.
    TropicalMinPlus,
}

/// Consumer requesting approximate candidate generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ApproximateConsumer {
    /// Typo-tolerant symbol navigation.
    SymbolNavigation,
    /// Cross-version or cross-language entity alignment.
    RevisionAlignment,
    /// Similar API-usage sequence discovery.
    ApiSequenceNeighbor,
    /// Candidate generation for exact structural clone detection.
    CloneCandidate,
    /// Candidate generation for exact semantic CPG differencing.
    SemanticDiffCandidate,
    /// Candidate generation for exact vulnerability extrapolation.
    VulnerabilityAnalogue,
}

impl ApproximateConsumer {
    /// Exact predicate that must confirm this consumer's candidates.
    pub const fn exact_confirmation(self) -> ExactConfirmation {
        match self {
            Self::SymbolNavigation => ExactConfirmation::UserSelectionOrExactLookup,
            Self::RevisionAlignment => ExactConfirmation::QualifiedTypedSignature,
            Self::ApiSequenceNeighbor => ExactConfirmation::ExactApiUsageTemporalProperty,
            Self::CloneCandidate => ExactConfirmation::ExactCloneStructure,
            Self::SemanticDiffCandidate => ExactConfirmation::ExactCpgDiff,
            Self::VulnerabilityAnalogue => ExactConfirmation::ExactVulnerabilityStructure,
        }
    }
}

/// Exact confirmation required before acting on approximate evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ExactConfirmation {
    /// A user selection or an exact symbol-table lookup.
    UserSelectionOrExactLookup,
    /// Exact qualified name and typed signature equivalence.
    QualifiedTypedSignature,
    /// Exact API-usage graph or temporal-property analysis.
    ExactApiUsageTemporalProperty,
    /// Exact structural clone predicate.
    ExactCloneStructure,
    /// Exact typed CPG differencing.
    ExactCpgDiff,
    /// Exact PDG and vulnerability-structure confirmation.
    ExactVulnerabilityStructure,
}

/// Explicit resource limits for one search.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ApproximateLimits {
    /// Maximum number of candidate records examined.
    pub max_candidates: usize,
    /// Maximum Unicode scalars or typed tokens in either tape.
    pub max_input_units: usize,
    /// Maximum dynamic-programming cells charged to one comparison.
    pub max_pair_cells: u64,
    /// Maximum dynamic-programming cells charged to the complete search.
    pub max_work_units: u64,
    /// Maximum algorithm states. Direct distance methods charge one state per cell.
    pub max_state_count: u64,
    /// Maximum estimated temporary and result memory in bytes.
    pub max_memory_bytes: u64,
    /// Maximum distinct typed tokens encoded in one comparison.
    pub max_unique_tokens: usize,
}

impl Default for ApproximateLimits {
    fn default() -> Self {
        Self {
            max_candidates: 100_000,
            max_input_units: 100_000,
            max_pair_cells: 10_000_000,
            max_work_units: 100_000_000,
            max_state_count: 10_000_000,
            max_memory_bytes: 256 * 1024 * 1024,
            max_unique_tokens: 100_000,
        }
    }
}

/// Complete configuration of one reproducible search.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ApproximateConfig {
    /// Distance method and its mathematical semantics.
    pub method: ApproximateMethod,
    /// Inclusive maximum raw edit distance.
    pub max_distance: usize,
    /// Number of lowest normalized-distance results retained.
    pub top_k: usize,
    /// Resource limits enforced before every delegated operation.
    pub limits: ApproximateLimits,
}

impl Default for ApproximateConfig {
    fn default() -> Self {
        Self {
            method: ApproximateMethod::StandardLevenshtein,
            max_distance: 2,
            top_k: 64,
            limits: ApproximateLimits::default(),
        }
    }
}

/// Cooperative cancellation checked between bounded comparisons.
#[derive(Debug, Clone, Default)]
pub struct ApproximateCancellation {
    cancelled: Arc<AtomicBool>,
}

impl ApproximateCancellation {
    /// Requests cancellation. A currently running bounded comparison completes
    /// before the search observes the request.
    pub fn cancel(&self) {
        self.cancelled.store(true, AtomicOrdering::Release);
    }

    /// Returns whether cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(AtomicOrdering::Acquire)
    }
}

/// Reason why a search could not examine all admitted evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ApproximateLimit {
    /// Candidate count exceeded the configured cap.
    CandidateCount,
    /// At least one comparison tape exceeded its input-unit cap.
    InputUnits,
    /// At least one comparison exceeded its per-pair work cap.
    PairWork,
    /// Total charged work reached its cap.
    TotalWork,
    /// An algorithm-state estimate exceeded its cap.
    StateCount,
    /// A temporary or retained allocation estimate exceeded its cap.
    Memory,
    /// Typed-token encoding exceeded its distinct-symbol cap.
    UniqueTokens,
    /// The caller requested cancellation.
    Cancelled,
}

/// Whether all eligible candidates were examined.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ApproximateCompletion {
    /// Every candidate was examined and the returned top-k is complete.
    Complete,
    /// Some work was excluded. This is unknown/limited evidence, never no-match.
    Incomplete(Vec<ApproximateLimit>),
}

/// Exact non-negative rational used for raw and normalized costs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ApproximateRational {
    /// Non-negative numerator.
    pub numerator: u64,
    /// Positive denominator.
    pub denominator: u64,
}

impl ApproximateRational {
    fn new(numerator: u64, denominator: u64) -> Self {
        debug_assert_ne!(denominator, 0);
        Self {
            numerator,
            denominator,
        }
    }

    fn compare(self, other: Self) -> Ordering {
        (u128::from(self.numerator) * u128::from(other.denominator))
            .cmp(&(u128::from(other.numerator) * u128::from(self.denominator)))
    }
}

/// Replayable identity of a search and its admitted inputs.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ApproximateProvenance {
    /// Evidence schema version.
    pub schema_version: u32,
    /// Stable method/configuration identity.
    pub method_identity: &'static str,
    /// Deterministic digest of the complete configuration.
    pub config_digest: u64,
    /// Deterministic digest of the query identity and tape.
    pub query_digest: u64,
    /// Deterministic digest of the ordered, admitted corpus records.
    pub corpus_digest: u64,
}

/// Resource accounting for one search.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ApproximateCounters {
    /// Candidate records presented by the caller.
    pub candidate_count: usize,
    /// Candidate records admitted by the count cap.
    pub admitted_candidates: usize,
    /// Candidate records for which a distance was computed.
    pub examined_candidates: usize,
    /// Candidate records within the configured distance.
    pub matched_candidates: usize,
    /// Charged dynamic-programming work units.
    pub work_units: u64,
    /// Largest state estimate of any delegated comparison.
    pub peak_state_count: u64,
    /// Largest temporary-memory estimate of any delegated comparison.
    pub peak_memory_bytes: u64,
}

/// One ranked approximate candidate requiring exact confirmation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApproximateEvidence {
    /// Stable candidate identity.
    pub candidate: ApproximateId,
    /// Immutable candidate tape used by the distance method.
    pub candidate_tape: Arc<ApproximateTape>,
    /// Requesting consumer.
    pub consumer: ApproximateConsumer,
    /// Exact predicate required before acting on this evidence.
    pub exact_confirmation: ExactConfirmation,
    /// Method used to generate the evidence.
    pub method: ApproximateMethod,
    /// Score semantics of that method.
    pub score_semantics: ApproximateScoreSemantics,
    /// Semiring used to combine sequential edit costs.
    pub semiring: ApproximateSemiring,
    /// Exact raw edit cost.
    pub raw_cost: ApproximateRational,
    /// Exact raw cost divided by the longer tape length.
    pub normalized_cost: ApproximateRational,
    /// One-based deterministic rank.
    pub rank: usize,
    /// Inclusive raw-cost bound used by the search.
    pub distance_bound: usize,
    /// Maximum number of ranked results retained.
    pub top_k: usize,
    /// Resource limits under which this evidence was generated.
    pub limits: ApproximateLimits,
    /// Completion state of the complete search.
    pub completion: ApproximateCompletion,
    /// Replayable provenance shared by the complete report.
    pub provenance: ApproximateProvenance,
}

/// Immutable result of bounded approximate candidate generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApproximateReport {
    /// Ranked candidates. An empty incomplete report is not evidence of no match.
    pub evidence: Vec<ApproximateEvidence>,
    /// Whether every eligible candidate was examined.
    pub completion: ApproximateCompletion,
    /// Deterministic search provenance.
    pub provenance: ApproximateProvenance,
    /// Resource accounting.
    pub counters: ApproximateCounters,
}

/// Invalid input that prevents a deterministic search from starting.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ApproximateError {
    /// A required non-zero limit was zero.
    #[error("approximate-matching limit `{0}` must be non-zero")]
    ZeroLimit(&'static str),
    /// Two candidates used the same stable identifier.
    #[error("duplicate approximate candidate identifier: {0:?}")]
    DuplicateCandidate(ApproximateId),
    /// Query and candidate tape kinds differed.
    #[error("query and candidate {candidate:?} use different tape kinds")]
    TapeKindMismatch {
        /// Candidate with a tape kind incompatible with the query.
        candidate: ApproximateId,
    },
    /// A platform-sized input or cost could not be represented in evidence.
    #[error("approximate-matching value `{0}` exceeds the evidence integer domain")]
    IntegerDomain(&'static str),
}

/// Generates bounded approximate candidates without mutating graph state.
///
/// Candidates are ordered by stable identity before evaluation. Results use an
/// exact rational normalized distance and stable identity as their total-order
/// tie breaker. An incomplete empty report means only that the configured
/// limits prevented a conclusion.
pub fn approximate_candidates(
    query: &ApproximateRecord,
    candidates: &[ApproximateRecord],
    consumer: ApproximateConsumer,
    config: &ApproximateConfig,
    cancellation: &ApproximateCancellation,
) -> Result<ApproximateReport, ApproximateError> {
    validate_config(config)?;

    let pointer_bytes = candidates
        .len()
        .checked_mul(size_of::<&ApproximateRecord>())
        .and_then(|bytes| u64::try_from(bytes).ok())
        .ok_or(ApproximateError::IntegerDomain("candidate pointer storage"))?;

    let mut ordered: Vec<_> = candidates.iter().collect();
    ordered.sort_unstable_by_key(|record| record.id);
    for duplicate in ordered.windows(2) {
        if duplicate[0].id == duplicate[1].id {
            return Err(ApproximateError::DuplicateCandidate(duplicate[0].id));
        }
    }

    let candidate_count = ordered.len();
    let mut limitations = BTreeSet::new();
    if ordered.len() > config.limits.max_candidates {
        ordered.truncate(config.limits.max_candidates);
        limitations.insert(ApproximateLimit::CandidateCount);
    }
    if pointer_bytes > config.limits.max_memory_bytes {
        ordered.clear();
        limitations.insert(ApproximateLimit::Memory);
    }

    let retained_bytes = config
        .top_k
        .min(ordered.len())
        .checked_mul(size_of::<PendingEvidence>())
        .and_then(|bytes| u64::try_from(bytes).ok())
        .ok_or(ApproximateError::IntegerDomain("retained evidence storage"))?;
    let base_memory_bytes = pointer_bytes
        .checked_add(retained_bytes)
        .ok_or(ApproximateError::IntegerDomain("base search storage"))?;
    if base_memory_bytes > config.limits.max_memory_bytes {
        ordered.clear();
        limitations.insert(ApproximateLimit::Memory);
    }

    let provenance = provenance(query, &ordered, config);
    let mut counters = ApproximateCounters {
        candidate_count,
        admitted_candidates: ordered.len(),
        peak_memory_bytes: base_memory_bytes,
        ..ApproximateCounters::default()
    };
    let mut evidence = Vec::with_capacity(config.top_k.min(ordered.len()));

    if query.tape.units() > config.limits.max_input_units {
        limitations.insert(ApproximateLimit::InputUnits);
        return Ok(finish_report(
            evidence,
            limitations,
            provenance,
            counters,
            consumer,
            config,
        ));
    }

    for candidate in ordered {
        if cancellation.is_cancelled() {
            limitations.insert(ApproximateLimit::Cancelled);
            break;
        }

        let prepared = match prepare_pair(&query.tape, &candidate.tape, config) {
            Ok(prepared) => prepared,
            Err(PairPreparation::Limit(limit)) => {
                limitations.insert(limit);
                continue;
            }
            Err(PairPreparation::Mismatch) => {
                return Err(ApproximateError::TapeKindMismatch {
                    candidate: candidate.id,
                });
            }
            Err(PairPreparation::Domain) => {
                return Err(ApproximateError::IntegerDomain("pair dimensions"));
            }
        };

        counters.peak_state_count = counters.peak_state_count.max(prepared.cells);
        let Some(resident_memory) = base_memory_bytes.checked_add(prepared.memory_bytes) else {
            limitations.insert(ApproximateLimit::Memory);
            continue;
        };
        counters.peak_memory_bytes = counters.peak_memory_bytes.max(resident_memory);
        if prepared.cells > config.limits.max_pair_cells {
            limitations.insert(ApproximateLimit::PairWork);
            continue;
        }
        if prepared.cells > config.limits.max_state_count {
            limitations.insert(ApproximateLimit::StateCount);
            continue;
        }
        if resident_memory > config.limits.max_memory_bytes {
            limitations.insert(ApproximateLimit::Memory);
            continue;
        }
        let Some(next_work) = counters.work_units.checked_add(prepared.cells) else {
            limitations.insert(ApproximateLimit::TotalWork);
            break;
        };
        if next_work > config.limits.max_work_units {
            limitations.insert(ApproximateLimit::TotalWork);
            break;
        }
        counters.work_units = next_work;

        let distance = delegated_distance(
            prepared.query(),
            prepared.candidate(),
            config.method,
            config.max_distance,
        );
        counters.examined_candidates += 1;
        let Some(distance) = distance else {
            continue;
        };
        counters.matched_candidates += 1;
        let distance = u64::try_from(distance)
            .map_err(|_| ApproximateError::IntegerDomain("edit distance"))?;
        let normalization =
            u64::try_from(prepared.query_units.max(prepared.candidate_units).max(1))
                .map_err(|_| ApproximateError::IntegerDomain("normalization length"))?;
        evidence.push(PendingEvidence {
            candidate: candidate.id,
            candidate_tape: Arc::clone(&candidate.tape),
            raw_cost: ApproximateRational::new(distance, 1),
            normalized_cost: ApproximateRational::new(distance, normalization),
        });
        evidence.sort_unstable_by(compare_pending);
        if evidence.len() > config.top_k {
            evidence.pop();
        }
    }

    Ok(finish_report(
        evidence,
        limitations,
        provenance,
        counters,
        consumer,
        config,
    ))
}

fn validate_config(config: &ApproximateConfig) -> Result<(), ApproximateError> {
    for (name, value) in [
        ("top_k", config.top_k),
        ("max_candidates", config.limits.max_candidates),
        ("max_input_units", config.limits.max_input_units),
        ("max_unique_tokens", config.limits.max_unique_tokens),
    ] {
        if value == 0 {
            return Err(ApproximateError::ZeroLimit(name));
        }
    }
    for (name, value) in [
        ("max_pair_cells", config.limits.max_pair_cells),
        ("max_work_units", config.limits.max_work_units),
        ("max_state_count", config.limits.max_state_count),
        ("max_memory_bytes", config.limits.max_memory_bytes),
    ] {
        if value == 0 {
            return Err(ApproximateError::ZeroLimit(name));
        }
    }
    Ok(())
}

#[derive(Debug)]
struct PendingEvidence {
    candidate: ApproximateId,
    candidate_tape: Arc<ApproximateTape>,
    raw_cost: ApproximateRational,
    normalized_cost: ApproximateRational,
}

fn compare_pending(left: &PendingEvidence, right: &PendingEvidence) -> Ordering {
    left.normalized_cost
        .compare(right.normalized_cost)
        .then_with(|| left.raw_cost.compare(right.raw_cost))
        .then_with(|| left.candidate.cmp(&right.candidate))
}

fn finish_report(
    pending: Vec<PendingEvidence>,
    limitations: BTreeSet<ApproximateLimit>,
    provenance: ApproximateProvenance,
    counters: ApproximateCounters,
    consumer: ApproximateConsumer,
    config: &ApproximateConfig,
) -> ApproximateReport {
    let completion = if limitations.is_empty() {
        ApproximateCompletion::Complete
    } else {
        ApproximateCompletion::Incomplete(limitations.into_iter().collect())
    };
    let evidence = pending
        .into_iter()
        .enumerate()
        .map(|(index, item)| ApproximateEvidence {
            candidate: item.candidate,
            candidate_tape: item.candidate_tape,
            consumer,
            exact_confirmation: consumer.exact_confirmation(),
            method: config.method,
            score_semantics: config.method.semantics(),
            semiring: ApproximateSemiring::TropicalMinPlus,
            raw_cost: item.raw_cost,
            normalized_cost: item.normalized_cost,
            rank: index + 1,
            distance_bound: config.max_distance,
            top_k: config.top_k,
            limits: config.limits.clone(),
            completion: completion.clone(),
            provenance: provenance.clone(),
        })
        .collect();
    ApproximateReport {
        evidence,
        completion,
        provenance,
        counters,
    }
}

enum PairPreparation {
    Limit(ApproximateLimit),
    Mismatch,
    Domain,
}

enum PreparedStrings<'a> {
    Borrowed(&'a str, &'a str),
    Encoded(String, String),
}

struct PreparedPair<'a> {
    strings: PreparedStrings<'a>,
    query_units: usize,
    candidate_units: usize,
    cells: u64,
    memory_bytes: u64,
}

impl PreparedPair<'_> {
    fn query(&self) -> &str {
        match &self.strings {
            PreparedStrings::Borrowed(query, _) => query,
            PreparedStrings::Encoded(query, _) => query,
        }
    }

    fn candidate(&self) -> &str {
        match &self.strings {
            PreparedStrings::Borrowed(_, candidate) => candidate,
            PreparedStrings::Encoded(_, candidate) => candidate,
        }
    }
}

fn prepare_pair<'a>(
    query: &'a ApproximateTape,
    candidate: &'a ApproximateTape,
    config: &ApproximateConfig,
) -> Result<PreparedPair<'a>, PairPreparation> {
    let query_units = query.units();
    let candidate_units = candidate.units();
    if query_units > config.limits.max_input_units
        || candidate_units > config.limits.max_input_units
    {
        return Err(PairPreparation::Limit(ApproximateLimit::InputUnits));
    }
    let strings = match (query, candidate) {
        (ApproximateTape::Text(query), ApproximateTape::Text(candidate)) => {
            PreparedStrings::Borrowed(query, candidate)
        }
        (ApproximateTape::Tokens(query), ApproximateTape::Tokens(candidate)) => {
            let (query, candidate) =
                encode_token_pair(query, candidate, config.limits.max_unique_tokens)?;
            PreparedStrings::Encoded(query, candidate)
        }
        _ => return Err(PairPreparation::Mismatch),
    };

    let rows = query_units.checked_add(1).ok_or(PairPreparation::Domain)?;
    let columns = candidate_units
        .checked_add(1)
        .ok_or(PairPreparation::Domain)?;
    let cells = rows.checked_mul(columns).ok_or(PairPreparation::Domain)?;
    let cells = u64::try_from(cells).map_err(|_| PairPreparation::Domain)?;
    let memory_bytes = estimated_pair_memory(config.method, query_units, candidate_units)?;
    Ok(PreparedPair {
        strings,
        query_units,
        candidate_units,
        cells,
        memory_bytes,
    })
}

fn encode_token_pair(
    query: &[Arc<str>],
    candidate: &[Arc<str>],
    max_unique_tokens: usize,
) -> Result<(String, String), PairPreparation> {
    let mut vocabulary: Vec<&str> = query.iter().chain(candidate).map(AsRef::as_ref).collect();
    vocabulary.sort_unstable();
    vocabulary.dedup();
    if vocabulary.len() > max_unique_tokens || vocabulary.len() > VALID_TOKEN_SCALARS {
        return Err(PairPreparation::Limit(ApproximateLimit::UniqueTokens));
    }

    let encode = |tokens: &[Arc<str>]| -> Result<String, PairPreparation> {
        let mut encoded = String::with_capacity(tokens.len().saturating_mul(3));
        for token in tokens {
            let index = vocabulary
                .binary_search_by(|item| item.cmp(&token.as_ref()))
                .map_err(|_| PairPreparation::Domain)?;
            let scalar = token_scalar(index).ok_or(PairPreparation::Domain)?;
            encoded.push(scalar);
        }
        Ok(encoded)
    };
    Ok((encode(query)?, encode(candidate)?))
}

// Exclude the surrogate range and NUL, which is reserved as an unambiguous
// sentinel by several external tape encodings.
const VALID_TOKEN_SCALARS: usize = 0x11_0000 - 0x800 - 1;

fn token_scalar(index: usize) -> Option<char> {
    let mut scalar = u32::try_from(index).ok()?.checked_add(1)?;
    if scalar >= 0xD800 {
        scalar = scalar.checked_add(0x800)?;
    }
    char::from_u32(scalar)
}

fn estimated_pair_memory(
    method: ApproximateMethod,
    query_units: usize,
    candidate_units: usize,
) -> Result<u64, PairPreparation> {
    let shorter = query_units.min(candidate_units);
    let row_cells = shorter.checked_add(1).ok_or(PairPreparation::Domain)?;
    let bytes_per_cell = size_of::<usize>();
    let dynamic_program_bytes = match method {
        ApproximateMethod::StandardLevenshtein => row_cells
            .checked_mul(2)
            .and_then(|cells| cells.checked_mul(bytes_per_cell)),
        ApproximateMethod::OptimalStringAlignment => row_cells
            .checked_mul(3)
            .and_then(|cells| cells.checked_mul(bytes_per_cell)),
        ApproximateMethod::DamerauLevenshtein => query_units
            .checked_add(2)
            .and_then(|rows| {
                candidate_units
                    .checked_add(2)
                    .and_then(|cols| rows.checked_mul(cols))
            })
            .and_then(|cells| cells.checked_mul(bytes_per_cell)),
    }
    .ok_or(PairPreparation::Domain)?;

    // The upstream routines materialize Unicode scalar vectors. Token tapes
    // additionally materialize encoded UTF-8 strings and a temporary sorted
    // vocabulary. Charge all of these concurrently; overestimation is safer
    // than admitting a comparison whose allocator peak exceeds the contract.
    let total_units = query_units
        .checked_add(candidate_units)
        .ok_or(PairPreparation::Domain)?;
    let scalar_bytes = total_units
        .checked_mul(size_of::<char>())
        .ok_or(PairPreparation::Domain)?;
    let token_encoding_bytes = total_units
        .checked_mul(4usize.saturating_add(size_of::<&str>()))
        .ok_or(PairPreparation::Domain)?;
    let matrix_row_headers = if method == ApproximateMethod::DamerauLevenshtein {
        query_units
            .checked_add(2)
            .and_then(|rows| rows.checked_mul(size_of::<Vec<usize>>()))
            .ok_or(PairPreparation::Domain)?
    } else {
        0
    };
    let alphabet_table_bytes = if method == ApproximateMethod::DamerauLevenshtein {
        total_units.checked_mul(64).ok_or(PairPreparation::Domain)?
    } else {
        0
    };
    let bytes = dynamic_program_bytes
        .checked_add(scalar_bytes.max(token_encoding_bytes))
        .and_then(|bytes| bytes.checked_add(matrix_row_headers))
        .and_then(|bytes| bytes.checked_add(alphabet_table_bytes))
        .ok_or(PairPreparation::Domain)?;
    u64::try_from(bytes).map_err(|_| PairPreparation::Domain)
}

fn delegated_distance(
    query: &str,
    candidate: &str,
    method: ApproximateMethod,
    max_distance: usize,
) -> Option<usize> {
    match method {
        ApproximateMethod::StandardLevenshtein => {
            standard_distance_bounded(query, candidate, max_distance)
        }
        ApproximateMethod::OptimalStringAlignment => {
            transposition_distance_bounded(query, candidate, max_distance)
        }
        ApproximateMethod::DamerauLevenshtein => {
            damerau_levenshtein_distance_bounded(query, candidate, max_distance)
        }
    }
}

fn provenance(
    query: &ApproximateRecord,
    candidates: &[&ApproximateRecord],
    config: &ApproximateConfig,
) -> ApproximateProvenance {
    let mut config_hash = StableHasher::new();
    config_hash.write_debug(config);
    let mut query_hash = StableHasher::new();
    query_hash.write_debug(query);
    let mut corpus_hash = StableHasher::new();
    for candidate in candidates {
        corpus_hash.write_debug(candidate);
    }
    ApproximateProvenance {
        schema_version: APPROXIMATE_EVIDENCE_SCHEMA_VERSION,
        method_identity: config.method.identity(),
        config_digest: config_hash.finish(),
        query_digest: query_hash.finish(),
        corpus_digest: corpus_hash.finish(),
    }
}

struct StableHasher(u64);

impl StableHasher {
    const fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    fn write_debug(&mut self, value: &impl std::fmt::Debug) {
        use std::fmt::Write as _;

        let mut encoded = String::new();
        write!(&mut encoded, "{value:?}").expect("writing to String is infallible");
        for byte in encoded.bytes() {
            self.0 ^= u64::from(byte);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
        self.0 ^= 0xff;
        self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
    }

    const fn finish(self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn external(item: u64, text: &str) -> ApproximateRecord {
        ApproximateRecord::new(
            ApproximateId::External { corpus: 7, item },
            ApproximateTape::text(text),
        )
    }

    #[test]
    fn ranks_example_by_exact_normalized_cost_and_stable_id() {
        let query = external(0, "parser");
        let candidates = vec![
            external(3, "parse"),
            external(2, "parcel"),
            external(1, "parser"),
        ];
        let report = approximate_candidates(
            &query,
            &candidates,
            ApproximateConsumer::SymbolNavigation,
            &ApproximateConfig {
                max_distance: 2,
                ..ApproximateConfig::default()
            },
            &ApproximateCancellation::default(),
        )
        .unwrap();

        assert_eq!(report.completion, ApproximateCompletion::Complete);
        assert_eq!(report.evidence.len(), 3);
        assert_eq!(report.evidence[0].candidate, candidates[2].id);
        assert_eq!(report.evidence[0].raw_cost.numerator, 0);
        assert_eq!(
            report.evidence[0].exact_confirmation,
            ExactConfirmation::UserSelectionOrExactLookup
        );
    }

    #[test]
    fn typed_tokens_preserve_token_equality_without_delimiters() {
        let query = ApproximateRecord::new(
            ApproximateId::External { corpus: 1, item: 0 },
            ApproximateTape::tokens(["ab", "c"]),
        );
        let candidates = [ApproximateRecord::new(
            ApproximateId::External { corpus: 1, item: 1 },
            ApproximateTape::tokens(["a", "bc"]),
        )];
        let report = approximate_candidates(
            &query,
            &candidates,
            ApproximateConsumer::ApiSequenceNeighbor,
            &ApproximateConfig::default(),
            &ApproximateCancellation::default(),
        )
        .unwrap();

        assert_eq!(report.evidence[0].raw_cost.numerator, 2);
    }

    #[test]
    fn cancellation_and_limits_are_unknown_not_no_match() {
        let query = external(0, "needle");
        let candidates = [external(1, "needle")];
        let cancellation = ApproximateCancellation::default();
        cancellation.cancel();
        let report = approximate_candidates(
            &query,
            &candidates,
            ApproximateConsumer::CloneCandidate,
            &ApproximateConfig::default(),
            &cancellation,
        )
        .unwrap();

        assert!(report.evidence.is_empty());
        assert_eq!(
            report.completion,
            ApproximateCompletion::Incomplete(vec![ApproximateLimit::Cancelled])
        );
    }

    proptest! {
        #[test]
        fn candidate_permutation_does_not_change_report(
            query in "[a-z]{0,12}",
            values in prop::collection::vec("[a-z]{0,12}", 0..32),
        ) {
            let mut candidates: Vec<_> = values
                .iter()
                .enumerate()
                .map(|(index, value)| external(index as u64 + 1, value))
                .collect();
            let forward = approximate_candidates(
                &external(0, &query),
                &candidates,
                ApproximateConsumer::RevisionAlignment,
                &ApproximateConfig::default(),
                &ApproximateCancellation::default(),
            ).unwrap();
            candidates.reverse();
            let reverse = approximate_candidates(
                &external(0, &query),
                &candidates,
                ApproximateConsumer::RevisionAlignment,
                &ApproximateConfig::default(),
                &ApproximateCancellation::default(),
            ).unwrap();
            prop_assert_eq!(forward, reverse);
        }

        #[test]
        fn standard_distance_is_symmetric_in_evidence(
            left in "[a-z]{0,16}",
            right in "[a-z]{0,16}",
        ) {
            let config = ApproximateConfig {
                max_distance: 32,
                ..ApproximateConfig::default()
            };
            let left_to_right = approximate_candidates(
                &external(0, &left),
                &[external(1, &right)],
                ApproximateConsumer::RevisionAlignment,
                &config,
                &ApproximateCancellation::default(),
            ).unwrap();
            let right_to_left = approximate_candidates(
                &external(0, &right),
                &[external(1, &left)],
                ApproximateConsumer::RevisionAlignment,
                &config,
                &ApproximateCancellation::default(),
            ).unwrap();
            prop_assert_eq!(
                left_to_right.evidence[0].raw_cost,
                right_to_left.evidence[0].raw_cost
            );
        }
    }
}
