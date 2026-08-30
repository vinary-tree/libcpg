//! Proof-producing, bounded expression equivalence over the Replete e-graph.
//!
//! This adapter consumes only live, complete, pure [`ExpressionIr`] values.
//! Exact equality is confined to one caller-declared binding context. A
//! separate alpha-normalized pass may report qualified Type-4 clone evidence
//! across binding contexts, but never relabels that evidence as same-state
//! semantic equality. All term construction, rewrite expansion, proof replay,
//! result ordering, cloning, serialization, and destruction use flat arenas.

use std::collections::{BTreeMap, BTreeSet};
use std::convert::Infallible;
use std::fmt;
use std::sync::{Arc, Mutex};

use replete::{
    apply_native_rewrite, write_framed, write_ordered_framed, AcBuildError, AcBuildLimits,
    AcOperatorCapability, AtomicEvidence, AtomicEvidenceVerifier, CanonicalSink, CanonicalValue,
    CanonicalValueError, ConditionId, ConditionOutcome, EClassId, EGraph, EGraphConfig,
    EGraphError, EvidenceLabel, ExplanationCompletion, MatchCompletion, MatchLimits, ModelConfig,
    NativeActionBudget, NativeActionDecision, NativeActionId, NativeActionLimits,
    NativeActionOutcome, NativeActionPlanBuilder, NativeActionPlanError, NativeActionResource,
    NativeMatchView, NativeRewriteAdapter, NativeRewriteRequest, PatternBuilder, PatternTape,
    PatternVariableId, ProofLimits, ProofTape, ProofTermId, ProvenanceStatus, RebuildCompletion,
    RewriteDirection, SaturationCompletion, SaturationLimits, SaturationProgram, SaturationRunner,
    SemanticHash, SerialScheduler, TapeLimits,
};
use thiserror::Error;

#[cfg(test)]
use replete::ProofBuilder;

#[cfg(feature = "serde")]
use serde::Serialize;

use super::expression_ir::{
    BinaryOperator, BindingIdentity, CallIdentity, ExpressionEvidenceMode, ExpressionFactKind,
    ExpressionIr, ExpressionIrCompletion, ExpressionIrId, ExpressionIrProvenance,
    ExpressionLiteral, ExpressionType, FloatMode, FloatType, FloatWidth, IntegerOverflow,
    IntegerType, MemoryAccess, RewriteLaw, SemanticKnowledge, SemanticProfile, ShiftBehavior,
    UnaryOperator,
};
use crate::NodeId;

/// Hard ceiling on candidates accepted by one request.
pub const MAX_EQUIVALENCE_CANDIDATES: usize = 100_000;
/// Hard ceiling on total input IR nodes accepted by one request.
pub const MAX_EQUIVALENCE_INPUT_NODES: usize = 4_000_000;
/// Hard ceiling on public positive pairs returned by one request.
pub const MAX_EQUIVALENCE_PAIRS: usize = 1_000_000;
/// Hard ceiling on equality-saturation matches per pass.
pub const MAX_EQUIVALENCE_MATCHES: usize = 16_000_000;
/// Hard ceiling on equality-saturation iterations per pass.
pub const MAX_EQUIVALENCE_ITERATIONS: usize = 1_000_000;

/// One live expression root offered to the equivalence adapter.
#[derive(Clone, Copy, Debug)]
pub struct ExpressionEquivalenceCandidate<'ir> {
    /// Stable CPG expression root.
    pub source: NodeId,
    /// Function or other exact binding namespace containing this expression.
    pub binding_context: NodeId,
    /// Live validated expression IR rooted at `source`.
    pub ir: &'ir ExpressionIr,
}

impl<'ir> ExpressionEquivalenceCandidate<'ir> {
    /// Creates one candidate without taking ownership of its flat IR.
    pub const fn new(source: NodeId, binding_context: NodeId, ir: &'ir ExpressionIr) -> Self {
        Self {
            source,
            binding_context,
            ir,
        }
    }
}

/// Inclusive resource limits for one exact and one alpha-normalized pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct ExpressionEquivalenceLimits {
    /// Maximum input candidates.
    pub max_candidates: usize,
    /// Maximum input IR nodes across all candidates.
    pub max_input_nodes: usize,
    /// Maximum exact e-nodes in either pass.
    pub max_egraph_nodes: usize,
    /// Maximum ordered e-node child references in either pass.
    pub max_egraph_children: usize,
    /// Maximum arity of one e-node.
    pub max_arity: usize,
    /// Maximum exact structural-key bytes for one e-node.
    pub max_key_bytes: usize,
    /// Maximum child-class-to-parent links.
    pub max_parent_links: usize,
    /// Maximum adapter and per-operation logical work.
    pub max_work: usize,
    /// Maximum distinct saturation matches per pass.
    pub max_matches: usize,
    /// Maximum complete or partial saturation iterations per pass.
    pub max_iterations: usize,
    /// Maximum public positive pairs.
    pub max_pairs: usize,
    /// Maximum exact terms retained in one explanation.
    pub max_proof_terms: usize,
    /// Maximum term-child references retained in one explanation.
    pub max_proof_term_edges: usize,
    /// Maximum proof nodes retained in one explanation.
    pub max_proof_nodes: usize,
    /// Maximum proof-to-proof references retained in one explanation.
    pub max_proof_edges: usize,
    /// Maximum semantic proof depth.
    pub max_proof_depth: usize,
    /// Maximum bytes for one operator and all evidence labels.
    pub max_proof_value_bytes: usize,
    /// Maximum canonical proof envelope bytes.
    pub max_proof_encoded_bytes: usize,
}

impl Default for ExpressionEquivalenceLimits {
    fn default() -> Self {
        Self {
            max_candidates: 10_000,
            max_input_nodes: 1_000_000,
            max_egraph_nodes: 1_000_000,
            max_egraph_children: 4_000_000,
            max_arity: 1_000_000,
            max_key_bytes: 64 * 1024 * 1024,
            max_parent_links: 4_000_000,
            max_work: 64 * 1024 * 1024,
            max_matches: 1_000_000,
            max_iterations: 1_024,
            max_pairs: 100_000,
            max_proof_terms: 1_000_000,
            max_proof_term_edges: 4_000_000,
            max_proof_nodes: 1_000_000,
            max_proof_edges: 4_000_000,
            max_proof_depth: 1_000_000,
            max_proof_value_bytes: 64 * 1024 * 1024,
            max_proof_encoded_bytes: 256 * 1024 * 1024,
        }
    }
}

impl ExpressionEquivalenceLimits {
    fn validate(self) -> Result<Self, ExpressionProofError> {
        if self.max_candidates > MAX_EQUIVALENCE_CANDIDATES {
            return Err(ExpressionProofError::InvalidLimit("max_candidates"));
        }
        if self.max_input_nodes > MAX_EQUIVALENCE_INPUT_NODES {
            return Err(ExpressionProofError::InvalidLimit("max_input_nodes"));
        }
        if self.max_pairs > MAX_EQUIVALENCE_PAIRS {
            return Err(ExpressionProofError::InvalidLimit("max_pairs"));
        }
        if self.max_matches > MAX_EQUIVALENCE_MATCHES {
            return Err(ExpressionProofError::InvalidLimit("max_matches"));
        }
        if self.max_iterations > MAX_EQUIVALENCE_ITERATIONS {
            return Err(ExpressionProofError::InvalidLimit("max_iterations"));
        }
        for (name, value) in [
            ("max_egraph_nodes", self.max_egraph_nodes),
            ("max_egraph_children", self.max_egraph_children),
            ("max_arity", self.max_arity),
            ("max_key_bytes", self.max_key_bytes),
            ("max_parent_links", self.max_parent_links),
            ("max_proof_value_bytes", self.max_proof_value_bytes),
            ("max_proof_encoded_bytes", self.max_proof_encoded_bytes),
        ] {
            if value == 0 {
                return Err(ExpressionProofError::InvalidLimit(name));
            }
        }
        Ok(self)
    }

    fn graph_config(self) -> EGraphConfig {
        EGraphConfig {
            model: ModelConfig {
                max_nodes: self.max_egraph_nodes,
                max_children: self.max_egraph_children,
                max_arity: self.max_arity,
                max_key_bytes: self.max_key_bytes,
                index_bits: 128,
            },
            max_parent_links: self.max_parent_links,
            max_work: self.max_work,
            max_order_cache_entries: self.max_egraph_nodes,
            max_value_bytes: self.max_proof_value_bytes,
            max_encoded_bytes: self.max_proof_encoded_bytes,
        }
    }

    fn proof_limits(self) -> ProofLimits {
        ProofLimits {
            max_terms: self.max_proof_terms,
            max_term_edges: self.max_proof_term_edges,
            max_proof_nodes: self.max_proof_nodes,
            max_proof_edges: self.max_proof_edges,
            max_depth: self.max_proof_depth,
            max_value_bytes: self.max_proof_value_bytes,
            max_encoded_bytes: self.max_proof_encoded_bytes,
            max_work: self.max_work,
        }
    }
}

/// Why a candidate did not enter either equality graph.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub enum UnsupportedExpressionReason {
    /// Another candidate already used the same stable source identity.
    DuplicateSource,
    /// Candidate profile differs from the request profile.
    ProfileMismatch,
    /// Lowering did not complete.
    IncompleteLowering(ExpressionIrCompletion),
    /// Persistence revoked live rewrite authority.
    PersistedArtifact,
    /// The declared source is not the IR root.
    RootMismatch,
    /// The root lacks complete pure evidence.
    NonPureEvidence,
}

/// One explicitly unsupported candidate.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct UnsupportedExpression {
    /// Stable source node.
    pub source: NodeId,
    /// Exact reason no equivalence claim may use this candidate.
    pub reason: UnsupportedExpressionReason,
}

/// Exact qualification attached to one positive pair.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub enum ExpressionEquivalenceKind {
    /// Same-binding-context equality in the configured equational model.
    Exact,
    /// Equality only after a witnessed bijective renaming of referenced bindings.
    AlphaRenamedType4Clone,
}

/// One slot in the explicit alpha-renaming bijection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct AlphaBindingCorrespondence {
    /// Canonical alpha slot.
    pub slot: u32,
    /// Left expression's resolved binding.
    pub left: BindingIdentity,
    /// Right expression's resolved binding.
    pub right: BindingIdentity,
}

/// Honest terminal cause for one pass or aggregate report.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub enum ExpressionEquivalenceCompletion {
    /// Every admitted candidate, rewrite, rebuild, proof, and pair completed.
    Complete,
    /// One or more candidates were explicitly unsupported.
    UnsupportedInputs,
    /// The candidate limit was exceeded before analysis.
    CandidateLimited,
    /// The total input-node limit was exceeded before analysis.
    InputNodeLimited,
    /// Replete refused the next e-node or child edge.
    NodeLimited,
    /// Adapter or Replete logical work was exhausted.
    WorkLimited,
    /// Replete proved that more rewrite matches exist than the configured cap.
    MatchLimited,
    /// The next fixed-point iteration could not begin within the configured cap.
    IterationLimited,
    /// The public pair limit truncated a deterministic positive prefix.
    PairLimited,
    /// Provenance recording or proof construction was incomplete.
    ProofLimited,
}

impl ExpressionEquivalenceCompletion {
    /// Returns whether absence from the corresponding result is meaningful.
    pub const fn is_complete(self) -> bool {
        matches!(self, Self::Complete)
    }
}

/// Deterministic accounting for one request.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct ExpressionEquivalenceMetrics {
    /// Candidates offered by the caller.
    pub candidates: usize,
    /// Candidates admitted to both passes.
    pub admitted: usize,
    /// Input flat IR nodes inspected.
    pub input_nodes: usize,
    /// Adapter logical work charged.
    pub work: usize,
    /// Local certified rewrite edges attempted.
    pub rewrites: usize,
    /// Distinct rewrite matches observed.
    pub matches: usize,
    /// Complete or partial fixed-point iterations begun.
    pub iterations: usize,
    /// Positive exact pairs emitted.
    pub exact_pairs: usize,
    /// Positive alpha-renamed pairs emitted.
    pub alpha_pairs: usize,
}

/// Proof validation failure.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum ExpressionProofError {
    /// A structural byte/graph limit is zero where Replete requires a positive
    /// bound, or a public hard safety ceiling was exceeded.
    #[error("invalid expression-equivalence limit: {0}")]
    InvalidLimit(&'static str),
    /// Replete rejected a graph operation or invariant.
    #[error("expression e-graph failed: {0}")]
    Graph(String),
    /// Replete rejected the proof structure or an atomic semantic step.
    #[error("expression proof replay failed: {0}")]
    Replay(String),
    /// A proof referenced an operator header not emitted by this adapter.
    #[error("expression proof references an unknown operator")]
    UnknownOperator,
}

/// Independently replayable equality proof.
#[derive(Clone)]
pub struct ExpressionEquivalenceProof {
    tape: ProofTape,
    profile: Arc<SemanticProfile>,
    operators: Arc<BTreeMap<Vec<u8>, EquivalenceOperator>>,
}

#[cfg(feature = "serde")]
impl Serialize for ExpressionEquivalenceProof {
    fn serialize<Serializer>(
        &self,
        serializer: Serializer,
    ) -> Result<Serializer::Ok, Serializer::Error>
    where
        Serializer: serde::Serializer,
    {
        use serde::ser::SerializeStruct as _;

        let mut state = serializer.serialize_struct("ExpressionEquivalenceProof", 3)?;
        state.serialize_field("tape", &self.tape)?;
        state.serialize_field("profile", &self.profile)?;
        state.serialize_field("operators", &OrderedOperatorDictionary(&self.operators))?;
        state.end()
    }
}

#[cfg(feature = "serde")]
struct OrderedOperatorDictionary<'dictionary>(&'dictionary BTreeMap<Vec<u8>, EquivalenceOperator>);

#[cfg(feature = "serde")]
impl Serialize for OrderedOperatorDictionary<'_> {
    fn serialize<Serializer>(
        &self,
        serializer: Serializer,
    ) -> Result<Serializer::Ok, Serializer::Error>
    where
        Serializer: serde::Serializer,
    {
        use serde::ser::SerializeSeq as _;

        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for entry in self.0 {
            sequence.serialize_element(&entry)?;
        }
        sequence.end()
    }
}

impl fmt::Debug for ExpressionEquivalenceProof {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExpressionEquivalenceProof")
            .field("terms", &self.tape.term_count())
            .field("proof_nodes", &self.tape.proof_node_count())
            .field("encoded_bytes", &self.tape.encoded_len())
            .finish_non_exhaustive()
    }
}

impl ExpressionEquivalenceProof {
    /// Independently validates structural proof rules and every named atomic law.
    pub fn replay(&self) -> Result<(), ExpressionProofError> {
        let mut verifier = ExpressionAtomicVerifier {
            profile: &self.profile,
            operators: &self.operators,
        };
        self.tape
            .replay(&mut verifier)
            .map_err(|error| ExpressionProofError::Replay(error.to_string()))
    }

    /// Returns the canonical encoded proof length without allocating.
    pub fn encoded_len(&self) -> usize {
        self.tape.encoded_len()
    }
}

/// One canonical positive relation with its full qualification.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct ExpressionEquivalencePair {
    /// Smaller stable source identifier.
    pub left: NodeId,
    /// Larger stable source identifier.
    pub right: NodeId,
    /// Caller-selected analysis domain.
    pub analysis_domain: NodeId,
    /// Left binding namespace.
    pub left_binding_context: NodeId,
    /// Right binding namespace.
    pub right_binding_context: NodeId,
    /// Exact or qualified alpha-renamed relation.
    pub kind: ExpressionEquivalenceKind,
    /// Completion of the pass that produced this still-valid positive.
    pub completion: ExpressionEquivalenceCompletion,
    /// Explicit alpha bijection; empty for exact evidence.
    pub alpha_bindings: Vec<AlphaBindingCorrespondence>,
    /// Independently replayable equality proof.
    pub proof: ExpressionEquivalenceProof,
}

/// Complete typed report for one analysis domain and semantic profile.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct ExpressionEquivalenceReport {
    /// Caller-selected comparison domain.
    pub analysis_domain: NodeId,
    /// Semantic profile under which all admitted rewrites were certified.
    pub profile: SemanticProfile,
    /// Aggregate terminal cause.
    pub completion: ExpressionEquivalenceCompletion,
    /// Exact-pass terminal cause.
    pub exact_completion: ExpressionEquivalenceCompletion,
    /// Alpha-normalized-pass terminal cause.
    pub alpha_completion: ExpressionEquivalenceCompletion,
    /// Canonical, duplicate-free, source-ordered positive pairs.
    pub pairs: Vec<ExpressionEquivalencePair>,
    /// Candidates excluded with explicit reasons.
    pub unsupported: Vec<UnsupportedExpression>,
    /// Exact deterministic accounting.
    pub metrics: ExpressionEquivalenceMetrics,
    /// Limits that governed the result.
    pub limits: ExpressionEquivalenceLimits,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize))]
enum EquivalenceBinding {
    Exact(BindingIdentity),
    Alpha(u32),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize))]
enum EquivalenceOperatorKind {
    Literal(ExpressionLiteral),
    Binding(EquivalenceBinding),
    Unary(UnaryOperator),
    Binary(BinaryOperator),
    Call(CallIdentity),
    Load {
        location: NodeId,
        access: MemoryAccess,
    },
    Opaque,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize))]
struct EquivalenceOperator {
    namespace: Option<NodeId>,
    kind: EquivalenceOperatorKind,
    value_type: ExpressionType,
}

impl SemanticHash for EquivalenceOperator {
    fn write_content(&self, output: &mut Vec<u8>) {
        match self.namespace {
            Some(namespace) => {
                output.push(1);
                output.extend_from_slice(&namespace.as_u32().to_le_bytes());
            }
            None => output.push(0),
        }
        write_operator_kind(&self.kind, output);
        write_expression_type(self.value_type, output);
    }
}

impl CanonicalValue for EquivalenceOperator {
    fn encode_canonical(&self, output: &mut dyn CanonicalSink) -> Result<(), CanonicalValueError> {
        let mut encoded = Vec::new();
        self.write_content(&mut encoded);
        output.write(&encoded)
    }

    fn decode_canonical(bytes: &[u8]) -> Result<Self, CanonicalValueError> {
        OperatorDecoder::new(bytes).decode()
    }
}

struct OperatorDecoder<'encoded> {
    encoded: &'encoded [u8],
    offset: usize,
}

impl<'encoded> OperatorDecoder<'encoded> {
    const fn new(encoded: &'encoded [u8]) -> Self {
        Self { encoded, offset: 0 }
    }

    fn take(&mut self, length: usize) -> Result<&'encoded [u8], CanonicalValueError> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or(CanonicalValueError::Capacity)?;
        let bytes = self
            .encoded
            .get(self.offset..end)
            .ok_or(CanonicalValueError::InvalidLength)?;
        self.offset = end;
        Ok(bytes)
    }

    fn byte(&mut self) -> Result<u8, CanonicalValueError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, CanonicalValueError> {
        Ok(u16::from_le_bytes(
            self.take(2)?
                .try_into()
                .map_err(|_| CanonicalValueError::InvalidLength)?,
        ))
    }

    fn u32(&mut self) -> Result<u32, CanonicalValueError> {
        Ok(u32::from_le_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| CanonicalValueError::InvalidLength)?,
        ))
    }

    fn u64(&mut self) -> Result<u64, CanonicalValueError> {
        Ok(u64::from_le_bytes(
            self.take(8)?
                .try_into()
                .map_err(|_| CanonicalValueError::InvalidLength)?,
        ))
    }

    fn u128(&mut self) -> Result<u128, CanonicalValueError> {
        Ok(u128::from_le_bytes(
            self.take(16)?
                .try_into()
                .map_err(|_| CanonicalValueError::InvalidLength)?,
        ))
    }

    fn framed_string(&mut self) -> Result<Arc<str>, CanonicalValueError> {
        let length = usize::try_from(self.u64()?).map_err(|_| CanonicalValueError::Capacity)?;
        let value = std::str::from_utf8(self.take(length)?)
            .map_err(|_| CanonicalValueError::InvalidUtf8)?;
        Ok(Arc::from(value))
    }

    fn literal(&mut self) -> Result<ExpressionLiteral, CanonicalValueError> {
        match self.byte()? {
            0 => Ok(ExpressionLiteral::Integer(self.u128()? as i128)),
            1 => Ok(ExpressionLiteral::Float32(self.u32()?)),
            2 => Ok(ExpressionLiteral::Float64(self.u64()?)),
            3 => match self.byte()? {
                0 => Ok(ExpressionLiteral::Bool(false)),
                1 => Ok(ExpressionLiteral::Bool(true)),
                _ => Err(CanonicalValueError::InvalidDiscriminant),
            },
            4 => Ok(ExpressionLiteral::String(self.framed_string()?)),
            5 => char::from_u32(self.u32()?)
                .map(ExpressionLiteral::Char)
                .ok_or(CanonicalValueError::InvalidDiscriminant),
            6 => Ok(ExpressionLiteral::Null),
            _ => Err(CanonicalValueError::InvalidDiscriminant),
        }
    }

    fn kind(&mut self) -> Result<EquivalenceOperatorKind, CanonicalValueError> {
        match self.byte()? {
            0 => Ok(EquivalenceOperatorKind::Literal(self.literal()?)),
            1 => Ok(EquivalenceOperatorKind::Binding(EquivalenceBinding::Exact(
                BindingIdentity {
                    definition: NodeId::new(self.u32()?),
                    scope: self.u32()?,
                },
            ))),
            2 => Ok(EquivalenceOperatorKind::Binding(EquivalenceBinding::Alpha(
                self.u32()?,
            ))),
            3 => Ok(EquivalenceOperatorKind::Unary(match self.byte()? {
                0 => UnaryOperator::Negate,
                1 => UnaryOperator::LogicalNot,
                2 => UnaryOperator::BitwiseNot,
                _ => return Err(CanonicalValueError::InvalidDiscriminant),
            })),
            4 => Ok(EquivalenceOperatorKind::Binary(decode_binary_operator(
                self.byte()?,
            )?)),
            5 => Ok(EquivalenceOperatorKind::Call(match self.byte()? {
                0 => CallIdentity::Resolved(NodeId::new(self.u32()?)),
                1 => CallIdentity::External(self.framed_string()?),
                2 => CallIdentity::Unresolved,
                _ => return Err(CanonicalValueError::InvalidDiscriminant),
            })),
            6 => Ok(EquivalenceOperatorKind::Load {
                location: NodeId::new(self.u32()?),
                access: match self.byte()? {
                    0 => MemoryAccess::Immutable,
                    1 => MemoryAccess::Shared,
                    2 => MemoryAccess::Mutable,
                    3 => MemoryAccess::Volatile,
                    4 => MemoryAccess::Atomic,
                    _ => return Err(CanonicalValueError::InvalidDiscriminant),
                },
            }),
            7 => Ok(EquivalenceOperatorKind::Opaque),
            _ => Err(CanonicalValueError::InvalidDiscriminant),
        }
    }

    fn value_type(&mut self) -> Result<ExpressionType, CanonicalValueError> {
        match self.byte()? {
            0 => Ok(ExpressionType::Bool),
            1 => {
                let width = match self.byte()? {
                    0 => None,
                    1 => Some(self.u16()?),
                    _ => return Err(CanonicalValueError::InvalidDiscriminant),
                };
                let signed = match self.byte()? {
                    0 => false,
                    1 => true,
                    _ => return Err(CanonicalValueError::InvalidDiscriminant),
                };
                let overflow = match self.byte()? {
                    0 => IntegerOverflow::Wrapping,
                    1 => IntegerOverflow::Checked,
                    2 => IntegerOverflow::Saturating,
                    3 => IntegerOverflow::Undefined,
                    4 => IntegerOverflow::ArbitraryPrecision,
                    _ => return Err(CanonicalValueError::InvalidDiscriminant),
                };
                let shift = match self.byte()? {
                    0 => ShiftBehavior::Masked,
                    1 => ShiftBehavior::Checked,
                    2 => ShiftBehavior::Undefined,
                    _ => return Err(CanonicalValueError::InvalidDiscriminant),
                };
                let integer = IntegerType {
                    width,
                    signed,
                    overflow,
                    shift,
                };
                if !integer.is_valid() {
                    return Err(CanonicalValueError::Custom("invalid integer operator type"));
                }
                Ok(ExpressionType::Integer(integer))
            }
            2 => Ok(ExpressionType::Float(FloatType {
                width: match self.byte()? {
                    0 => FloatWidth::F32,
                    1 => FloatWidth::F64,
                    _ => return Err(CanonicalValueError::InvalidDiscriminant),
                },
                mode: match self.byte()? {
                    0 => FloatMode::StrictIeee754,
                    1 => FloatMode::FiniteReassociation,
                    _ => return Err(CanonicalValueError::InvalidDiscriminant),
                },
            })),
            3 => Ok(ExpressionType::String),
            4 => Ok(ExpressionType::Char),
            5 => Ok(ExpressionType::Unit),
            6 => Ok(ExpressionType::Dynamic),
            _ => Err(CanonicalValueError::InvalidDiscriminant),
        }
    }

    fn decode(mut self) -> Result<EquivalenceOperator, CanonicalValueError> {
        let namespace = match self.byte()? {
            0 => None,
            1 => Some(NodeId::new(self.u32()?)),
            _ => return Err(CanonicalValueError::InvalidDiscriminant),
        };
        let kind = self.kind()?;
        let value_type = self.value_type()?;
        if self.offset != self.encoded.len() {
            return Err(CanonicalValueError::InvalidLength);
        }
        Ok(EquivalenceOperator {
            namespace,
            kind,
            value_type,
        })
    }
}

fn decode_binary_operator(code: u8) -> Result<BinaryOperator, CanonicalValueError> {
    Ok(match code {
        0 => BinaryOperator::Add,
        1 => BinaryOperator::Subtract,
        2 => BinaryOperator::Multiply,
        3 => BinaryOperator::Divide,
        4 => BinaryOperator::Remainder,
        5 => BinaryOperator::ShiftLeft,
        6 => BinaryOperator::ShiftRight,
        7 => BinaryOperator::BitwiseAnd,
        8 => BinaryOperator::BitwiseOr,
        9 => BinaryOperator::BitwiseXor,
        10 => BinaryOperator::LogicalAnd,
        11 => BinaryOperator::LogicalOr,
        12 => BinaryOperator::Equal,
        13 => BinaryOperator::NotEqual,
        14 => BinaryOperator::Less,
        15 => BinaryOperator::LessEqual,
        16 => BinaryOperator::Greater,
        17 => BinaryOperator::GreaterEqual,
        _ => return Err(CanonicalValueError::InvalidDiscriminant),
    })
}

fn write_operator_kind(kind: &EquivalenceOperatorKind, output: &mut Vec<u8>) {
    match kind {
        EquivalenceOperatorKind::Literal(literal) => {
            output.push(0);
            write_literal(literal, output);
        }
        EquivalenceOperatorKind::Binding(EquivalenceBinding::Exact(binding)) => {
            output.push(1);
            output.extend_from_slice(&binding.definition.as_u32().to_le_bytes());
            output.extend_from_slice(&binding.scope.to_le_bytes());
        }
        EquivalenceOperatorKind::Binding(EquivalenceBinding::Alpha(slot)) => {
            output.push(2);
            output.extend_from_slice(&slot.to_le_bytes());
        }
        EquivalenceOperatorKind::Unary(operator) => {
            output.push(3);
            output.push(unary_code(*operator));
        }
        EquivalenceOperatorKind::Binary(operator) => {
            output.push(4);
            output.push(binary_code(*operator));
        }
        EquivalenceOperatorKind::Call(identity) => {
            output.push(5);
            match identity {
                CallIdentity::Resolved(node) => {
                    output.push(0);
                    output.extend_from_slice(&node.as_u32().to_le_bytes());
                }
                CallIdentity::External(symbol) => {
                    output.push(1);
                    write_framed(output, symbol.as_bytes());
                }
                CallIdentity::Unresolved => output.push(2),
            }
        }
        EquivalenceOperatorKind::Load { location, access } => {
            output.push(6);
            output.extend_from_slice(&location.as_u32().to_le_bytes());
            output.push(memory_access_code(*access));
        }
        EquivalenceOperatorKind::Opaque => output.push(7),
    }
}

fn write_literal(literal: &ExpressionLiteral, output: &mut Vec<u8>) {
    match literal {
        ExpressionLiteral::Integer(value) => {
            output.push(0);
            output.extend_from_slice(&value.to_le_bytes());
        }
        ExpressionLiteral::Float32(bits) => {
            output.push(1);
            output.extend_from_slice(&bits.to_le_bytes());
        }
        ExpressionLiteral::Float64(bits) => {
            output.push(2);
            output.extend_from_slice(&bits.to_le_bytes());
        }
        ExpressionLiteral::Bool(value) => {
            output.push(3);
            output.push(u8::from(*value));
        }
        ExpressionLiteral::String(value) => {
            output.push(4);
            write_framed(output, value.as_bytes());
        }
        ExpressionLiteral::Char(value) => {
            output.push(5);
            output.extend_from_slice(&u32::from(*value).to_le_bytes());
        }
        ExpressionLiteral::Null => output.push(6),
    }
}

fn write_expression_type(value_type: ExpressionType, output: &mut Vec<u8>) {
    match value_type {
        ExpressionType::Bool => output.push(0),
        ExpressionType::Integer(integer) => {
            output.push(1);
            match integer.width {
                Some(width) => {
                    output.push(1);
                    output.extend_from_slice(&width.to_le_bytes());
                }
                None => output.push(0),
            }
            output.push(u8::from(integer.signed));
            output.push(integer_overflow_code(integer.overflow));
            output.push(shift_code(integer.shift));
        }
        ExpressionType::Float(FloatType { width, mode }) => {
            output.push(2);
            output.push(match width {
                FloatWidth::F32 => 0,
                FloatWidth::F64 => 1,
            });
            output.push(match mode {
                FloatMode::StrictIeee754 => 0,
                FloatMode::FiniteReassociation => 1,
            });
        }
        ExpressionType::String => output.push(3),
        ExpressionType::Char => output.push(4),
        ExpressionType::Unit => output.push(5),
        ExpressionType::Dynamic => output.push(6),
    }
}

fn unary_code(operator: UnaryOperator) -> u8 {
    match operator {
        UnaryOperator::Negate => 0,
        UnaryOperator::LogicalNot => 1,
        UnaryOperator::BitwiseNot => 2,
    }
}

fn binary_code(operator: BinaryOperator) -> u8 {
    match operator {
        BinaryOperator::Add => 0,
        BinaryOperator::Subtract => 1,
        BinaryOperator::Multiply => 2,
        BinaryOperator::Divide => 3,
        BinaryOperator::Remainder => 4,
        BinaryOperator::ShiftLeft => 5,
        BinaryOperator::ShiftRight => 6,
        BinaryOperator::BitwiseAnd => 7,
        BinaryOperator::BitwiseOr => 8,
        BinaryOperator::BitwiseXor => 9,
        BinaryOperator::LogicalAnd => 10,
        BinaryOperator::LogicalOr => 11,
        BinaryOperator::Equal => 12,
        BinaryOperator::NotEqual => 13,
        BinaryOperator::Less => 14,
        BinaryOperator::LessEqual => 15,
        BinaryOperator::Greater => 16,
        BinaryOperator::GreaterEqual => 17,
    }
}

fn memory_access_code(access: MemoryAccess) -> u8 {
    match access {
        MemoryAccess::Immutable => 0,
        MemoryAccess::Shared => 1,
        MemoryAccess::Mutable => 2,
        MemoryAccess::Volatile => 3,
        MemoryAccess::Atomic => 4,
    }
}

fn integer_overflow_code(overflow: IntegerOverflow) -> u8 {
    match overflow {
        IntegerOverflow::Wrapping => 0,
        IntegerOverflow::Checked => 1,
        IntegerOverflow::Saturating => 2,
        IntegerOverflow::Undefined => 3,
        IntegerOverflow::ArbitraryPrecision => 4,
    }
}

fn shift_code(shift: ShiftBehavior) -> u8 {
    match shift {
        ShiftBehavior::Masked => 0,
        ShiftBehavior::Checked => 1,
        ShiftBehavior::Undefined => 2,
    }
}

fn operator_header(operator: &EquivalenceOperator, arity: usize) -> Vec<u8> {
    let mut value = Vec::new();
    operator.write_content(&mut value);
    let mut header = Vec::new();
    write_ordered_framed(&mut header, &value);
    header.extend_from_slice(&(arity as u64).to_be_bytes());
    header
}

#[derive(Clone, Debug)]
struct FlatExpressionNode {
    exact_operator: EquivalenceOperator,
    alpha_operator: EquivalenceOperator,
    children: Vec<usize>,
}

#[derive(Clone, Debug)]
struct PreparedCandidate {
    source: NodeId,
    binding_context: NodeId,
    nodes: Vec<FlatExpressionNode>,
    root: usize,
    alpha_bindings: Vec<BindingIdentity>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PassMode {
    Exact,
    Alpha,
}

impl PassMode {
    const fn kind(self) -> ExpressionEquivalenceKind {
        match self {
            Self::Exact => ExpressionEquivalenceKind::Exact,
            Self::Alpha => ExpressionEquivalenceKind::AlphaRenamedType4Clone,
        }
    }

    fn comparable(self, left: &PreparedCandidate, right: &PreparedCandidate) -> bool {
        match self {
            Self::Exact => left.binding_context == right.binding_context,
            Self::Alpha => left.binding_context != right.binding_context,
        }
    }
}

struct PassState {
    completion: ExpressionEquivalenceCompletion,
    pairs: Vec<ExpressionEquivalencePair>,
    work: usize,
    rewrites: usize,
    matches: usize,
    iterations: usize,
}

impl PassState {
    fn complete() -> Self {
        Self {
            completion: ExpressionEquivalenceCompletion::Complete,
            pairs: Vec::new(),
            work: 0,
            rewrites: 0,
            matches: 0,
            iterations: 0,
        }
    }

    fn charge(&mut self, amount: usize, maximum: usize) -> bool {
        let Some(next) = self.work.checked_add(amount) else {
            self.completion = ExpressionEquivalenceCompletion::WorkLimited;
            return false;
        };
        if next > maximum {
            self.completion = ExpressionEquivalenceCompletion::WorkLimited;
            return false;
        }
        self.work = next;
        true
    }
}

enum PassAbort {
    Limited(ExpressionEquivalenceCompletion),
    Fatal(ExpressionProofError),
}

impl From<ExpressionProofError> for PassAbort {
    fn from(error: ExpressionProofError) -> Self {
        Self::Fatal(error)
    }
}

/// Builds exact and alpha-normalized e-graphs and returns proof-producing
/// positive evidence.
///
/// The function takes no mutable CPG reference, so graph topology is immutable
/// by construction. Absence from `pairs` is meaningful only when the relevant
/// pass completion is [`ExpressionEquivalenceCompletion::Complete`].
pub fn analyze_expression_equivalence(
    analysis_domain: NodeId,
    profile: &SemanticProfile,
    candidates: &[ExpressionEquivalenceCandidate<'_>],
    limits: ExpressionEquivalenceLimits,
) -> Result<ExpressionEquivalenceReport, ExpressionProofError> {
    let limits = limits.validate()?;
    let mut metrics = ExpressionEquivalenceMetrics {
        candidates: candidates.len(),
        ..ExpressionEquivalenceMetrics::default()
    };

    if candidates.len() > limits.max_candidates {
        return Ok(limited_report(
            analysis_domain,
            profile,
            limits,
            metrics,
            ExpressionEquivalenceCompletion::CandidateLimited,
        ));
    }

    let mut ordered = candidates.to_vec();
    ordered.sort_unstable_by_key(|candidate| (candidate.source, candidate.binding_context));
    let mut seen = BTreeSet::new();
    let mut prepared = Vec::new();
    prepared
        .try_reserve(ordered.len())
        .map_err(|_| ExpressionProofError::Graph("candidate allocation failed".to_owned()))?;
    let mut unsupported = Vec::new();

    for candidate in ordered {
        let reason = if !seen.insert(candidate.source) {
            Some(UnsupportedExpressionReason::DuplicateSource)
        } else if candidate.ir.profile() != profile {
            Some(UnsupportedExpressionReason::ProfileMismatch)
        } else if !matches!(candidate.ir.completion(), ExpressionIrCompletion::Complete) {
            Some(UnsupportedExpressionReason::IncompleteLowering(
                candidate.ir.completion().clone(),
            ))
        } else if candidate.ir.provenance() != ExpressionIrProvenance::LiveLowering {
            Some(UnsupportedExpressionReason::PersistedArtifact)
        } else if candidate.ir.root() != candidate.ir.source_id(candidate.source) {
            Some(UnsupportedExpressionReason::RootMismatch)
        } else if candidate
            .ir
            .root()
            .and_then(|root| candidate.ir.node(root))
            .is_none_or(|node| node.mode != ExpressionEvidenceMode::Pure)
        {
            Some(UnsupportedExpressionReason::NonPureEvidence)
        } else {
            None
        };

        if let Some(reason) = reason {
            unsupported.push(UnsupportedExpression {
                source: candidate.source,
                reason,
            });
            continue;
        }

        let nodes = candidate.ir.metrics().nodes;
        let Some(total_nodes) = metrics.input_nodes.checked_add(nodes) else {
            return Ok(limited_report(
                analysis_domain,
                profile,
                limits,
                metrics,
                ExpressionEquivalenceCompletion::InputNodeLimited,
            ));
        };
        if total_nodes > limits.max_input_nodes {
            return Ok(limited_report(
                analysis_domain,
                profile,
                limits,
                metrics,
                ExpressionEquivalenceCompletion::InputNodeLimited,
            ));
        }
        metrics.input_nodes = total_nodes;
        prepared.push(prepare_candidate(candidate)?);
    }
    metrics.admitted = prepared.len();

    let exact = run_pass(analysis_domain, profile, &prepared, limits, PassMode::Exact)?;
    let alpha = run_pass(analysis_domain, profile, &prepared, limits, PassMode::Alpha)?;

    metrics.work = exact.work.saturating_add(alpha.work);
    metrics.rewrites = exact.rewrites.saturating_add(alpha.rewrites);
    metrics.matches = exact.matches.saturating_add(alpha.matches);
    metrics.iterations = exact.iterations.saturating_add(alpha.iterations);
    metrics.exact_pairs = exact.pairs.len();
    metrics.alpha_pairs = alpha.pairs.len();

    let exact_completion = exact.completion;
    let alpha_completion = alpha.completion;
    let mut pairs = exact.pairs;
    pairs.extend(alpha.pairs);
    pairs.sort_unstable_by_key(|pair| (pair.left, pair.right, pair.kind));
    pairs.dedup_by(|left, right| {
        left.left == right.left && left.right == right.right && left.kind == right.kind
    });

    let completion = aggregate_completion(exact_completion, alpha_completion, &unsupported);
    Ok(ExpressionEquivalenceReport {
        analysis_domain,
        profile: profile.clone(),
        completion,
        exact_completion,
        alpha_completion,
        pairs,
        unsupported,
        metrics,
        limits,
    })
}

fn limited_report(
    analysis_domain: NodeId,
    profile: &SemanticProfile,
    limits: ExpressionEquivalenceLimits,
    metrics: ExpressionEquivalenceMetrics,
    completion: ExpressionEquivalenceCompletion,
) -> ExpressionEquivalenceReport {
    ExpressionEquivalenceReport {
        analysis_domain,
        profile: profile.clone(),
        completion,
        exact_completion: completion,
        alpha_completion: completion,
        pairs: Vec::new(),
        unsupported: Vec::new(),
        metrics,
        limits,
    }
}

fn aggregate_completion(
    exact: ExpressionEquivalenceCompletion,
    alpha: ExpressionEquivalenceCompletion,
    unsupported: &[UnsupportedExpression],
) -> ExpressionEquivalenceCompletion {
    if exact != ExpressionEquivalenceCompletion::Complete {
        exact
    } else if alpha != ExpressionEquivalenceCompletion::Complete {
        alpha
    } else if unsupported.is_empty() {
        ExpressionEquivalenceCompletion::Complete
    } else {
        ExpressionEquivalenceCompletion::UnsupportedInputs
    }
}

fn prepare_candidate(
    candidate: ExpressionEquivalenceCandidate<'_>,
) -> Result<PreparedCandidate, ExpressionProofError> {
    let count = candidate.ir.metrics().nodes;
    let mut nodes = Vec::new();
    nodes
        .try_reserve_exact(count)
        .map_err(|_| ExpressionProofError::Graph("flat term allocation failed".to_owned()))?;
    let mut alpha_slots = BTreeMap::<BindingIdentity, u32>::new();
    let mut alpha_bindings = Vec::new();

    for index in 0..count {
        let id = ExpressionIrId(
            u32::try_from(index)
                .map_err(|_| ExpressionProofError::Graph("IR identifier overflow".to_owned()))?,
        );
        let node = candidate
            .ir
            .node(id)
            .ok_or_else(|| ExpressionProofError::Graph("IR node is unavailable".to_owned()))?;
        let value_type = match node.value_type {
            SemanticKnowledge::Known(value_type) => value_type,
            SemanticKnowledge::Unknown => {
                return Err(ExpressionProofError::Graph(
                    "pure IR node has unknown value type".to_owned(),
                ));
            }
        };
        let exact_operator_kind = exact_kind(&node.kind);
        let alpha_kind = match &node.kind {
            ExpressionFactKind::Binding(binding) => {
                let slot = if let Some(slot) = alpha_slots.get(binding).copied() {
                    slot
                } else {
                    let slot = u32::try_from(alpha_bindings.len()).map_err(|_| {
                        ExpressionProofError::Graph("alpha binding domain overflow".to_owned())
                    })?;
                    alpha_slots.insert(*binding, slot);
                    alpha_bindings.push(*binding);
                    slot
                };
                EquivalenceOperatorKind::Binding(EquivalenceBinding::Alpha(slot))
            }
            other => exact_kind(other),
        };
        let children = candidate
            .ir
            .operands(id)
            .ok_or_else(|| ExpressionProofError::Graph("IR operands are unavailable".to_owned()))?
            .iter()
            .map(|child| child.0 as usize)
            .collect();
        nodes.push(FlatExpressionNode {
            exact_operator: EquivalenceOperator {
                namespace: Some(candidate.binding_context),
                kind: exact_operator_kind,
                value_type,
            },
            alpha_operator: EquivalenceOperator {
                namespace: None,
                kind: alpha_kind,
                value_type,
            },
            children,
        });
    }

    let root = candidate
        .ir
        .root()
        .ok_or_else(|| ExpressionProofError::Graph("complete IR has no root".to_owned()))?
        .0 as usize;
    Ok(PreparedCandidate {
        source: candidate.source,
        binding_context: candidate.binding_context,
        nodes,
        root,
        alpha_bindings,
    })
}

fn exact_kind(kind: &ExpressionFactKind) -> EquivalenceOperatorKind {
    match kind {
        ExpressionFactKind::Literal(literal) => EquivalenceOperatorKind::Literal(literal.clone()),
        ExpressionFactKind::Binding(binding) => {
            EquivalenceOperatorKind::Binding(EquivalenceBinding::Exact(*binding))
        }
        ExpressionFactKind::Unary(operator) => EquivalenceOperatorKind::Unary(*operator),
        ExpressionFactKind::Binary(operator) => EquivalenceOperatorKind::Binary(*operator),
        ExpressionFactKind::Call(identity) => EquivalenceOperatorKind::Call(identity.clone()),
        ExpressionFactKind::Load { location, access } => EquivalenceOperatorKind::Load {
            location: *location,
            access: *access,
        },
        ExpressionFactKind::Opaque => EquivalenceOperatorKind::Opaque,
    }
}

type EquivalencePattern = PatternTape<EquivalenceOperator, u8>;
const AC_NORMALIZE_LABEL: &str = "libcpg.ac-normalize";

struct NamedPatternRule {
    label: EvidenceLabel,
    source: EquivalencePattern,
    target: EquivalencePattern,
}

struct FoldPattern {
    operator: EquivalenceOperator,
    pattern: EquivalencePattern,
}

fn pattern_limits() -> TapeLimits {
    TapeLimits {
        max_nodes: 8,
        max_edges: 8,
        max_variables: 3,
        max_depth: 4,
        max_encoded_bytes: 64 * 1024,
        max_value_bytes: 16 * 1024,
        max_work: 64 * 1024,
    }
}

fn pattern_error(error: impl fmt::Display) -> ExpressionProofError {
    ExpressionProofError::Graph(format!("expression rewrite pattern failed: {error}"))
}

fn plain_binary_pattern(
    operator: &EquivalenceOperator,
    left: u8,
    right: u8,
) -> Result<EquivalencePattern, ExpressionProofError> {
    let mut builder = PatternBuilder::new(pattern_limits()).map_err(pattern_error)?;
    let variables = [
        builder.declare_variable(0).map_err(pattern_error)?,
        builder.declare_variable(1).map_err(pattern_error)?,
    ];
    let left = builder
        .variable(variables[usize::from(left)])
        .map_err(pattern_error)?;
    let right = builder
        .variable(variables[usize::from(right)])
        .map_err(pattern_error)?;
    let root = builder
        .app(operator.clone(), &[left, right])
        .map_err(pattern_error)?;
    builder.finish(root).map_err(pattern_error)
}

fn associative_pattern(
    operator: &EquivalenceOperator,
    left_associated: bool,
) -> Result<EquivalencePattern, ExpressionProofError> {
    let mut builder = PatternBuilder::new(pattern_limits()).map_err(pattern_error)?;
    let declarations = [
        builder.declare_variable(0).map_err(pattern_error)?,
        builder.declare_variable(1).map_err(pattern_error)?,
        builder.declare_variable(2).map_err(pattern_error)?,
    ];
    let variables = [
        builder.variable(declarations[0]).map_err(pattern_error)?,
        builder.variable(declarations[1]).map_err(pattern_error)?,
        builder.variable(declarations[2]).map_err(pattern_error)?,
    ];
    let root = if left_associated {
        let inner = builder
            .app(operator.clone(), &variables[..2])
            .map_err(pattern_error)?;
        builder
            .app(operator.clone(), &[inner, variables[2]])
            .map_err(pattern_error)?
    } else {
        let inner = builder
            .app(operator.clone(), &variables[1..])
            .map_err(pattern_error)?;
        builder
            .app(operator.clone(), &[variables[0], inner])
            .map_err(pattern_error)?
    };
    builder.finish(root).map_err(pattern_error)
}

fn identity_source_pattern(
    operator: &EquivalenceOperator,
    identity: ExpressionLiteral,
    identity_on_left: bool,
) -> Result<EquivalencePattern, ExpressionProofError> {
    let mut builder = PatternBuilder::new(pattern_limits()).map_err(pattern_error)?;
    let declaration = builder.declare_variable(0).map_err(pattern_error)?;
    let variable = builder.variable(declaration).map_err(pattern_error)?;
    let identity = builder
        .leaf(EquivalenceOperator {
            namespace: operator.namespace,
            kind: EquivalenceOperatorKind::Literal(identity),
            value_type: operator.value_type,
        })
        .map_err(pattern_error)?;
    let children = if identity_on_left {
        [identity, variable]
    } else {
        [variable, identity]
    };
    let root = builder
        .app(operator.clone(), &children)
        .map_err(pattern_error)?;
    builder.finish(root).map_err(pattern_error)
}

fn single_variable_pattern() -> Result<EquivalencePattern, ExpressionProofError> {
    let mut builder = PatternBuilder::new(pattern_limits()).map_err(pattern_error)?;
    let declaration = builder.declare_variable(0).map_err(pattern_error)?;
    let root = builder.variable(declaration).map_err(pattern_error)?;
    builder.finish(root).map_err(pattern_error)
}

fn root_operator_pattern(
    operator: &EquivalenceOperator,
    arity: usize,
) -> Result<EquivalencePattern, ExpressionProofError> {
    let mut builder = PatternBuilder::new(pattern_limits()).map_err(pattern_error)?;
    let mut children = Vec::new();
    children
        .try_reserve_exact(arity)
        .map_err(|_| ExpressionProofError::Graph("fold pattern allocation failed".to_owned()))?;
    for variable in 0..arity {
        let variable = u8::try_from(variable)
            .map_err(|_| ExpressionProofError::Graph("fold pattern arity overflow".to_owned()))?;
        let declaration = builder.declare_variable(variable).map_err(pattern_error)?;
        children.push(builder.variable(declaration).map_err(pattern_error)?);
    }
    let root = builder
        .app(operator.clone(), &children)
        .map_err(pattern_error)?;
    builder.finish(root).map_err(pattern_error)
}

fn named_label(
    law: RewriteLaw,
    ordinal: usize,
    direction: &str,
) -> Result<EvidenceLabel, ExpressionProofError> {
    EvidenceLabel::new(format!("{}.{}.{}", law_label(law), ordinal, direction))
        .map_err(pattern_error)
}

fn add_identity_rules(
    rules: &mut Vec<NamedPatternRule>,
    operator: &EquivalenceOperator,
    law: RewriteLaw,
    identity: ExpressionLiteral,
    ordinal: usize,
) -> Result<(), ExpressionProofError> {
    for (identity_on_left, direction) in [(true, "left"), (false, "right")] {
        rules.push(NamedPatternRule {
            label: named_label(law, ordinal, direction)?,
            source: identity_source_pattern(operator, identity.clone(), identity_on_left)?,
            target: single_variable_pattern()?,
        });
    }
    Ok(())
}

fn build_rewrite_programs(
    profile: &SemanticProfile,
    operators: &BTreeMap<Vec<u8>, EquivalenceOperator>,
) -> Result<(Vec<NamedPatternRule>, Vec<FoldPattern>), ExpressionProofError> {
    let mut rules = Vec::new();
    let mut folds = Vec::new();
    for (ordinal, operator) in operators.values().enumerate() {
        let arity = match operator.kind {
            EquivalenceOperatorKind::Unary(_) => 1,
            EquivalenceOperatorKind::Binary(_) => 2,
            _ => continue,
        };
        let has_compatible_literal = operators.values().any(|candidate| {
            candidate.namespace == operator.namespace
                && candidate.value_type == operator.value_type
                && matches!(candidate.kind, EquivalenceOperatorKind::Literal(_))
        });
        if has_compatible_literal
            && profile.allows(RewriteLaw::ConstantFold)
            && operator_admits_law(operator, RewriteLaw::ConstantFold)
        {
            folds.push(FoldPattern {
                operator: operator.clone(),
                pattern: root_operator_pattern(operator, arity)?,
            });
        }
        if arity != 2 {
            continue;
        }
        let ac_normalized = operator_is_ac(profile, operator);
        if !ac_normalized
            && profile.allows(RewriteLaw::Commutativity)
            && operator_admits_law(operator, RewriteLaw::Commutativity)
        {
            rules.push(NamedPatternRule {
                label: named_label(RewriteLaw::Commutativity, ordinal, "forward")?,
                source: plain_binary_pattern(operator, 0, 1)?,
                target: plain_binary_pattern(operator, 1, 0)?,
            });
        }
        if !ac_normalized
            && profile.allows(RewriteLaw::Associativity)
            && operator_admits_law(operator, RewriteLaw::Associativity)
        {
            rules.push(NamedPatternRule {
                label: named_label(RewriteLaw::Associativity, ordinal, "left")?,
                source: associative_pattern(operator, true)?,
                target: associative_pattern(operator, false)?,
            });
            rules.push(NamedPatternRule {
                label: named_label(RewriteLaw::Associativity, ordinal, "right")?,
                source: associative_pattern(operator, false)?,
                target: associative_pattern(operator, true)?,
            });
        }
        if profile.allows(RewriteLaw::AdditiveIdentity)
            && operator_admits_law(operator, RewriteLaw::AdditiveIdentity)
        {
            add_identity_rules(
                &mut rules,
                operator,
                RewriteLaw::AdditiveIdentity,
                ExpressionLiteral::Integer(0),
                ordinal,
            )?;
        }
        if profile.allows(RewriteLaw::MultiplicativeIdentity)
            && operator_admits_law(operator, RewriteLaw::MultiplicativeIdentity)
        {
            add_identity_rules(
                &mut rules,
                operator,
                RewriteLaw::MultiplicativeIdentity,
                ExpressionLiteral::Integer(1),
                ordinal,
            )?;
        }
        if profile.allows(RewriteLaw::BooleanIdentity)
            && operator_admits_law(operator, RewriteLaw::BooleanIdentity)
        {
            let EquivalenceOperatorKind::Binary(binary) = operator.kind else {
                continue;
            };
            let identity = match binary {
                BinaryOperator::LogicalAnd => true,
                BinaryOperator::LogicalOr => false,
                _ => continue,
            };
            add_identity_rules(
                &mut rules,
                operator,
                RewriteLaw::BooleanIdentity,
                ExpressionLiteral::Bool(identity),
                ordinal,
            )?;
        }
    }
    Ok((rules, folds))
}

fn operator_is_ac(profile: &SemanticProfile, operator: &EquivalenceOperator) -> bool {
    profile.allows(RewriteLaw::Associativity)
        && profile.allows(RewriteLaw::Commutativity)
        && operator_admits_law(operator, RewriteLaw::Associativity)
        && operator_admits_law(operator, RewriteLaw::Commutativity)
}

fn normalize_ac_node(
    graph: &mut EGraph<EquivalenceOperator>,
    operator: &EquivalenceOperator,
    children: &[EClassId],
    limits: ExpressionEquivalenceLimits,
    operators: &mut BTreeMap<Vec<u8>, EquivalenceOperator>,
    state: &mut PassState,
) -> Result<EClassId, PassAbort> {
    let remaining_work = limits.max_work.saturating_sub(state.work);
    let policy = |candidate: &EquivalenceOperator| {
        (candidate == operator).then_some(AcOperatorCapability::without_unit())
    };
    let report = graph
        .add_ac(
            operator.clone(),
            children,
            &policy,
            AcBuildLimits {
                max_work: remaining_work,
            },
        )
        .map_err(|error| match error {
            AcBuildError::WorkLimit { .. } => {
                PassAbort::Limited(ExpressionEquivalenceCompletion::WorkLimited)
            }
            AcBuildError::Graph(graph_error) => graph_limit_completion(&graph_error)
                .map(PassAbort::Limited)
                .unwrap_or_else(PassAbort::Fatal),
            preserved => PassAbort::Fatal(pattern_error(preserved)),
        })?;
    if !state.charge(report.work(), limits.max_work) {
        return Err(PassAbort::Limited(
            ExpressionEquivalenceCompletion::WorkLimited,
        ));
    }
    operators.insert(
        operator_header(operator, report.canonical_members()),
        operator.clone(),
    );
    Ok(report.root())
}

fn merge_ac_normalization(
    graph: &mut EGraph<EquivalenceOperator>,
    positional: EClassId,
    normalized: EClassId,
    limits: ExpressionEquivalenceLimits,
    state: &mut PassState,
) -> Result<(), PassAbort> {
    if graph
        .equivalent(positional, normalized)
        .map_err(|error| PassAbort::Fatal(graph_error(error)))?
    {
        return Ok(());
    }
    if !state.charge(1, limits.max_work) {
        return Err(PassAbort::Limited(
            ExpressionEquivalenceCompletion::WorkLimited,
        ));
    }
    let label = EvidenceLabel::new(AC_NORMALIZE_LABEL)
        .map_err(|error| PassAbort::Fatal(pattern_error(error)))?;
    graph
        .merge_rewritten(positional, normalized, label, RewriteDirection::Forward)
        .map_err(|error| {
            graph_limit_completion(&error)
                .map(PassAbort::Limited)
                .unwrap_or_else(PassAbort::Fatal)
        })?;
    let rebuilt = graph.rebuild().map_err(|error| {
        graph_limit_completion(&error)
            .map(PassAbort::Limited)
            .unwrap_or_else(PassAbort::Fatal)
    })?;
    if !state.charge(rebuilt.work(), limits.max_work)
        || rebuilt.completion() != RebuildCompletion::Complete
    {
        return Err(PassAbort::Limited(
            ExpressionEquivalenceCompletion::WorkLimited,
        ));
    }
    state.rewrites = state.rewrites.saturating_add(1);
    Ok(())
}

#[derive(Debug, Error)]
enum FoldAdapterError {
    #[error("native fold view is malformed")]
    MalformedView,
    #[error("one equality class contains conflicting typed literals")]
    ConflictingLiteral,
    #[error("native fold action plan failed: {0}")]
    Plan(String),
    #[error("native fold result handoff was poisoned")]
    Poisoned,
}

struct ExpressionFoldAdapter {
    operator: EquivalenceOperator,
    variables: Vec<PatternVariableId>,
    produced: Mutex<Option<EquivalenceOperator>>,
}

impl ExpressionFoldAdapter {
    fn new(operator: EquivalenceOperator, pattern: &EquivalencePattern) -> Self {
        Self {
            operator,
            variables: pattern.variables().map(|(id, _)| id).collect(),
            produced: Mutex::new(None),
        }
    }

    fn literal_child(
        &self,
        input: &NativeMatchView<EquivalenceOperator>,
        class: EClassId,
    ) -> Result<Option<EquivalenceOperator>, FoldAdapterError> {
        let class = input.class(class).ok_or(FoldAdapterError::MalformedView)?;
        let mut literal: Option<EquivalenceOperator> = None;
        for node in class.nodes() {
            if !node.children().is_empty()
                || node.op().namespace != self.operator.namespace
                || node.op().value_type != self.operator.value_type
                || !matches!(node.op().kind, EquivalenceOperatorKind::Literal(_))
            {
                continue;
            }
            if literal
                .as_ref()
                .is_some_and(|existing| existing != node.op())
            {
                return Err(FoldAdapterError::ConflictingLiteral);
            }
            literal = Some(node.op().clone());
        }
        Ok(literal)
    }

    fn plan_error(
        error: NativeActionPlanError,
    ) -> NativeActionDecision<EquivalenceOperator, FoldAdapterError> {
        match error {
            NativeActionPlanError::Limited(limit) => NativeActionDecision::Limited(limit),
            preserved => NativeActionDecision::Error(FoldAdapterError::Plan(preserved.to_string())),
        }
    }

    fn take_produced(&self) -> Result<Option<EquivalenceOperator>, FoldAdapterError> {
        self.produced
            .lock()
            .map_err(|_| FoldAdapterError::Poisoned)
            .map(|mut produced| produced.take())
    }
}

impl NativeRewriteAdapter<EquivalenceOperator> for ExpressionFoldAdapter {
    type Witness = EquivalenceOperator;
    type Error = FoldAdapterError;

    fn evaluate_condition(
        &self,
        _condition: ConditionId,
        input: &NativeMatchView<EquivalenceOperator>,
        budget: &mut NativeActionBudget,
    ) -> ConditionOutcome<Self::Witness, Self::Error> {
        if let Err(limit) = budget.charge(1) {
            return ConditionOutcome::Limited(limit);
        }
        let mut children = Vec::new();
        if children.try_reserve_exact(self.variables.len()).is_err() {
            return ConditionOutcome::Error(FoldAdapterError::MalformedView);
        }
        for &variable in &self.variables {
            let Some(class) = input.binding(variable) else {
                return ConditionOutcome::Error(FoldAdapterError::MalformedView);
            };
            match self.literal_child(input, class) {
                Ok(Some(child)) => children.push(child),
                Ok(None) => return ConditionOutcome::Refuted,
                Err(error) => return ConditionOutcome::Error(error),
            }
        }
        let child_refs: Vec<_> = children.iter().collect();
        let Some(literal) = fold_literal(&self.operator, &child_refs) else {
            return ConditionOutcome::Refuted;
        };
        ConditionOutcome::Satisfied(EquivalenceOperator {
            namespace: self.operator.namespace,
            kind: EquivalenceOperatorKind::Literal(literal),
            value_type: self.operator.value_type,
        })
    }

    fn apply_action(
        &self,
        _action: NativeActionId,
        _input: &NativeMatchView<EquivalenceOperator>,
        witness: Option<&Self::Witness>,
        budget: &mut NativeActionBudget,
        limits: NativeActionLimits,
    ) -> NativeActionDecision<EquivalenceOperator, Self::Error> {
        if let Err(limit) = budget.charge(1) {
            return NativeActionDecision::Limited(limit);
        }
        let Some(target) = witness.cloned() else {
            return NativeActionDecision::Error(FoldAdapterError::MalformedView);
        };
        let mut builder = match NativeActionPlanBuilder::new(limits) {
            Ok(builder) => builder,
            Err(error) => return Self::plan_error(error),
        };
        let root = match builder.add(target.clone(), []) {
            Ok(root) => root,
            Err(error) => return Self::plan_error(error),
        };
        let plan = match builder.finish(root) {
            Ok(plan) => plan,
            Err(error) => return Self::plan_error(error),
        };
        let Ok(mut produced) = self.produced.lock() else {
            return NativeActionDecision::Error(FoldAdapterError::Poisoned);
        };
        *produced = Some(target);
        NativeActionDecision::Apply(plan)
    }
}

fn saturation_completion(completion: SaturationCompletion) -> Result<(), PassAbort> {
    match completion {
        SaturationCompletion::Saturated => Ok(()),
        SaturationCompletion::IterationLimit => Err(PassAbort::Limited(
            ExpressionEquivalenceCompletion::IterationLimited,
        )),
        SaturationCompletion::NodeLimit => Err(PassAbort::Limited(
            ExpressionEquivalenceCompletion::NodeLimited,
        )),
        SaturationCompletion::MatchLimit => Err(PassAbort::Limited(
            ExpressionEquivalenceCompletion::MatchLimited,
        )),
        SaturationCompletion::WorkLimit => Err(PassAbort::Limited(
            ExpressionEquivalenceCompletion::WorkLimited,
        )),
        SaturationCompletion::Cancelled
        | SaturationCompletion::TimeLimit
        | SaturationCompletion::InvalidInput(_) => {
            Err(PassAbort::Fatal(ExpressionProofError::Graph(format!(
                "unexpected deterministic saturation completion: {completion:?}"
            ))))
        }
    }
}

fn charge_saturation_report(
    report: &replete::SaturationReport,
    limits: ExpressionEquivalenceLimits,
    state: &mut PassState,
) -> Result<(), PassAbort> {
    let stats = report.stats();
    if !state.charge(stats.work(), limits.max_work) {
        return Err(PassAbort::Limited(
            ExpressionEquivalenceCompletion::WorkLimited,
        ));
    }
    state.matches = state
        .matches
        .checked_add(stats.matches())
        .ok_or(PassAbort::Limited(
            ExpressionEquivalenceCompletion::MatchLimited,
        ))?;
    if state.matches > limits.max_matches {
        return Err(PassAbort::Limited(
            ExpressionEquivalenceCompletion::MatchLimited,
        ));
    }
    state.iterations =
        state
            .iterations
            .checked_add(stats.iterations())
            .ok_or(PassAbort::Limited(
                ExpressionEquivalenceCompletion::IterationLimited,
            ))?;
    if state.iterations > limits.max_iterations {
        return Err(PassAbort::Limited(
            ExpressionEquivalenceCompletion::IterationLimited,
        ));
    }
    state.rewrites = state.rewrites.saturating_add(stats.applications());
    saturation_completion(report.completion())
}

fn native_limits(limits: ExpressionEquivalenceLimits, remaining_work: usize) -> NativeActionLimits {
    NativeActionLimits {
        max_view_classes: limits.max_egraph_nodes,
        max_view_nodes: limits.max_egraph_nodes,
        max_view_edges: limits.max_egraph_children,
        max_plan_steps: 1,
        max_plan_edges: 0,
        max_nodes: limits.max_egraph_nodes,
        max_work: remaining_work,
    }
}

fn run_fold_sweep(
    graph: &mut EGraph<EquivalenceOperator>,
    folds: &[FoldPattern],
    limits: ExpressionEquivalenceLimits,
    operators: &mut BTreeMap<Vec<u8>, EquivalenceOperator>,
    state: &mut PassState,
) -> Result<bool, PassAbort> {
    if state.iterations >= limits.max_iterations {
        return Err(PassAbort::Limited(
            ExpressionEquivalenceCompletion::IterationLimited,
        ));
    }
    state.iterations += 1;
    let mut changed = false;
    for fold in folds {
        let remaining_matches = limits.max_matches.saturating_sub(state.matches);
        let remaining_work = limits.max_work.saturating_sub(state.work);
        let compiled = fold
            .pattern
            .compile_positional()
            .map_err(|error| PassAbort::Fatal(pattern_error(error)))?;
        let report = compiled
            .search(
                graph,
                MatchLimits {
                    max_results: remaining_matches,
                    max_work: remaining_work,
                },
            )
            .map_err(|error| PassAbort::Fatal(pattern_error(error)))?;
        if !state.charge(report.stats().work(), limits.max_work) {
            return Err(PassAbort::Limited(
                ExpressionEquivalenceCompletion::WorkLimited,
            ));
        }
        state.matches =
            state
                .matches
                .checked_add(report.matches().len())
                .ok_or(PassAbort::Limited(
                    ExpressionEquivalenceCompletion::MatchLimited,
                ))?;
        match report.completion() {
            MatchCompletion::Complete => {}
            MatchCompletion::WorkLimit => {
                return Err(PassAbort::Limited(
                    ExpressionEquivalenceCompletion::WorkLimited,
                ));
            }
            MatchCompletion::ResultLimit => {
                return Err(PassAbort::Limited(
                    ExpressionEquivalenceCompletion::MatchLimited,
                ));
            }
        }
        let matches = report.into_matches();
        let adapter = ExpressionFoldAdapter::new(fold.operator.clone(), &fold.pattern);
        for matched in matches {
            let remaining_work = limits.max_work.saturating_sub(state.work);
            if remaining_work == 0 {
                return Err(PassAbort::Limited(
                    ExpressionEquivalenceCompletion::WorkLimited,
                ));
            }
            let outcome = apply_native_rewrite(
                graph,
                &matched,
                EvidenceLabel::new(law_label(RewriteLaw::ConstantFold))
                    .map_err(|error| PassAbort::Fatal(pattern_error(error)))?,
                NativeRewriteRequest::conditional(ConditionId::new(1), NativeActionId::new(1)),
                native_limits(limits, remaining_work),
                &adapter,
            );
            match outcome {
                NativeActionOutcome::Added(report) | NativeActionOutcome::Merged(report) => {
                    if !state.charge(report.work(), limits.max_work) {
                        return Err(PassAbort::Limited(
                            ExpressionEquivalenceCompletion::WorkLimited,
                        ));
                    }
                    state.rewrites = state.rewrites.saturating_add(1);
                    changed |= report.changed();
                    if let Some(operator) = adapter.take_produced().map_err(|error| {
                        PassAbort::Fatal(ExpressionProofError::Graph(error.to_string()))
                    })? {
                        operators.insert(operator_header(&operator, 0), operator);
                    }
                }
                NativeActionOutcome::NotApplicable => {}
                NativeActionOutcome::Limited(limit) => {
                    let completion = if limit.resource() == NativeActionResource::Work {
                        ExpressionEquivalenceCompletion::WorkLimited
                    } else {
                        ExpressionEquivalenceCompletion::NodeLimited
                    };
                    return Err(PassAbort::Limited(completion));
                }
                NativeActionOutcome::Error(error) => {
                    return Err(PassAbort::Fatal(ExpressionProofError::Graph(
                        error.to_string(),
                    )));
                }
            }
        }
    }
    Ok(changed)
}

fn saturate_graph(
    graph: &mut EGraph<EquivalenceOperator>,
    rules: &[NamedPatternRule],
    folds: &[FoldPattern],
    limits: ExpressionEquivalenceLimits,
    operators: &mut BTreeMap<Vec<u8>, EquivalenceOperator>,
    state: &mut PassState,
) -> Result<(), PassAbort> {
    let program = SaturationProgram::compile_named(
        rules
            .iter()
            .map(|rule| (rule.label.clone(), &rule.source, &rule.target)),
    )
    .map_err(|error| PassAbort::Fatal(pattern_error(error)))?;
    loop {
        if !rules.is_empty() {
            let runner = SaturationRunner::new(
                SaturationLimits {
                    max_iterations: limits.max_iterations.saturating_sub(state.iterations),
                    max_nodes: limits.max_egraph_nodes,
                    max_matches: limits.max_matches.saturating_sub(state.matches),
                    max_work: limits.max_work.saturating_sub(state.work),
                    max_duration: None,
                },
                SerialScheduler::Canonical,
            );
            let report = runner
                .run_unobserved(graph, &program)
                .map_err(|error| PassAbort::Fatal(pattern_error(error)))?;
            charge_saturation_report(&report, limits, state)?;
        }
        if folds.is_empty() || !run_fold_sweep(graph, folds, limits, operators, state)? {
            return Ok(());
        }
    }
}

fn run_pass(
    analysis_domain: NodeId,
    profile: &SemanticProfile,
    candidates: &[PreparedCandidate],
    limits: ExpressionEquivalenceLimits,
    mode: PassMode,
) -> Result<PassState, ExpressionProofError> {
    let mut state = PassState::complete();
    let mut graph = EGraph::with_config(limits.graph_config()).map_err(graph_error)?;
    graph
        .enable_provenance(limits.proof_limits())
        .map_err(graph_error)?;
    let mut roots = Vec::new();
    roots
        .try_reserve_exact(candidates.len())
        .map_err(|_| ExpressionProofError::Graph("root allocation failed".to_owned()))?;
    let mut all_classes = Vec::new();
    all_classes
        .try_reserve_exact(candidates.len())
        .map_err(|_| ExpressionProofError::Graph("class-map allocation failed".to_owned()))?;
    let mut operators = BTreeMap::new();

    for candidate in candidates {
        let mut classes = Vec::new();
        classes
            .try_reserve_exact(candidate.nodes.len())
            .map_err(|_| ExpressionProofError::Graph("class tape allocation failed".to_owned()))?;
        for node in &candidate.nodes {
            if !state.charge(1usize.saturating_add(node.children.len()), limits.max_work) {
                return Ok(state);
            }
            let operator = match mode {
                PassMode::Exact => &node.exact_operator,
                PassMode::Alpha => &node.alpha_operator,
            };
            let mut children = Vec::new();
            children
                .try_reserve_exact(node.children.len())
                .map_err(|_| {
                    ExpressionProofError::Graph("child tape allocation failed".to_owned())
                })?;
            for &child in &node.children {
                let class = classes.get(child).copied().ok_or_else(|| {
                    ExpressionProofError::Graph("IR child is not backward-referenced".to_owned())
                })?;
                children.push(class);
            }
            operators.insert(operator_header(operator, children.len()), operator.clone());
            let class = match graph.add(operator.clone(), &children) {
                Ok(class) => class,
                Err(error) => {
                    state.completion = graph_limit_completion(&error)?;
                    return Ok(state);
                }
            };
            if operator_is_ac(profile, operator) {
                let normalized = match normalize_ac_node(
                    &mut graph,
                    operator,
                    &children,
                    limits,
                    &mut operators,
                    &mut state,
                ) {
                    Ok(normalized) => normalized,
                    Err(PassAbort::Limited(completion)) => {
                        state.completion = completion;
                        return Ok(state);
                    }
                    Err(PassAbort::Fatal(error)) => return Err(error),
                };
                match merge_ac_normalization(&mut graph, class, normalized, limits, &mut state) {
                    Ok(()) => {}
                    Err(PassAbort::Limited(completion)) => {
                        state.completion = completion;
                        return Ok(state);
                    }
                    Err(PassAbort::Fatal(error)) => return Err(error),
                }
            }
            classes.push(class);
        }
        let root = classes.get(candidate.root).copied().ok_or_else(|| {
            ExpressionProofError::Graph("candidate root class is unavailable".to_owned())
        })?;
        roots.push(root);
        all_classes.push(classes);
    }

    let (rules, folds) = build_rewrite_programs(profile, &operators)?;
    if let Err(abort) = saturate_graph(
        &mut graph,
        &rules,
        &folds,
        limits,
        &mut operators,
        &mut state,
    ) {
        match abort {
            PassAbort::Limited(completion) => {
                state.completion = completion;
                return Ok(state);
            }
            PassAbort::Fatal(error) => return Err(error),
        }
    }

    let rebuilt = graph.rebuild().map_err(graph_error)?;
    state.work = state.work.saturating_add(rebuilt.work());
    if rebuilt.completion() != RebuildCompletion::Complete || state.work > limits.max_work {
        state.completion = ExpressionEquivalenceCompletion::WorkLimited;
        return Ok(state);
    }
    if !matches!(graph.provenance_status(), ProvenanceStatus::Complete { .. }) {
        state.completion = ExpressionEquivalenceCompletion::ProofLimited;
        return Ok(state);
    }

    let shared_profile = Arc::new(profile.clone());
    let shared_operators = Arc::new(operators);
    'pairs: for left_index in 0..candidates.len() {
        for right_index in (left_index + 1)..candidates.len() {
            let left = &candidates[left_index];
            let right = &candidates[right_index];
            if !mode.comparable(left, right) {
                continue;
            }
            if state.pairs.len() >= limits.max_pairs {
                state.completion = ExpressionEquivalenceCompletion::PairLimited;
                break 'pairs;
            }
            if !graph
                .equivalent(roots[left_index], roots[right_index])
                .map_err(graph_error)?
            {
                continue;
            }
            let explanation = graph
                .explain(roots[left_index], roots[right_index])
                .map_err(graph_error)?;
            let tape = match explanation.completion() {
                ExplanationCompletion::Found => explanation.into_proof().ok_or_else(|| {
                    ExpressionProofError::Graph("found explanation has no proof".to_owned())
                })?,
                ExplanationCompletion::NotEquivalent => {
                    return Err(ExpressionProofError::Graph(
                        "equivalent roots produced a negative explanation".to_owned(),
                    ));
                }
                ExplanationCompletion::Exhausted => {
                    state.completion = ExpressionEquivalenceCompletion::ProofLimited;
                    break 'pairs;
                }
            };
            let (left_source, right_source, left_context, right_context, alpha_bindings) =
                ordered_pair_payload(left, right, mode)?;
            let proof = ExpressionEquivalenceProof {
                tape,
                profile: Arc::clone(&shared_profile),
                operators: Arc::clone(&shared_operators),
            };
            proof.replay()?;
            state.pairs.push(ExpressionEquivalencePair {
                left: left_source,
                right: right_source,
                analysis_domain,
                left_binding_context: left_context,
                right_binding_context: right_context,
                kind: mode.kind(),
                completion: state.completion,
                alpha_bindings,
                proof,
            });
        }
    }
    for pair in &mut state.pairs {
        pair.completion = state.completion;
    }
    Ok(state)
}

fn ordered_pair_payload(
    left: &PreparedCandidate,
    right: &PreparedCandidate,
    mode: PassMode,
) -> Result<
    (
        NodeId,
        NodeId,
        NodeId,
        NodeId,
        Vec<AlphaBindingCorrespondence>,
    ),
    ExpressionProofError,
> {
    let (left, right) = if left.source < right.source {
        (left, right)
    } else {
        (right, left)
    };
    let alpha_bindings = if mode == PassMode::Alpha {
        if left.alpha_bindings.len() != right.alpha_bindings.len() {
            return Err(ExpressionProofError::Graph(
                "alpha-equivalent terms have non-bijective binding domains".to_owned(),
            ));
        }
        left.alpha_bindings
            .iter()
            .copied()
            .zip(right.alpha_bindings.iter().copied())
            .enumerate()
            .map(|(slot, (left, right))| AlphaBindingCorrespondence {
                slot: slot as u32,
                left,
                right,
            })
            .collect()
    } else {
        Vec::new()
    };
    Ok((
        left.source,
        right.source,
        left.binding_context,
        right.binding_context,
        alpha_bindings,
    ))
}

fn graph_error(error: EGraphError) -> ExpressionProofError {
    ExpressionProofError::Graph(error.to_string())
}

fn graph_limit_completion(
    error: &EGraphError,
) -> Result<ExpressionEquivalenceCompletion, ExpressionProofError> {
    match error {
        EGraphError::Limit { resource, .. } => {
            if resource.contains("work") {
                Ok(ExpressionEquivalenceCompletion::WorkLimited)
            } else {
                Ok(ExpressionEquivalenceCompletion::NodeLimited)
            }
        }
        EGraphError::Model(replete::ModelError::Limit { resource, .. }) => {
            if resource.contains("work") {
                Ok(ExpressionEquivalenceCompletion::WorkLimited)
            } else {
                Ok(ExpressionEquivalenceCompletion::NodeLimited)
            }
        }
        _ => Err(graph_error(error.clone())),
    }
}

const REWRITE_LAWS: [RewriteLaw; 6] = [
    RewriteLaw::ConstantFold,
    RewriteLaw::Commutativity,
    RewriteLaw::Associativity,
    RewriteLaw::AdditiveIdentity,
    RewriteLaw::MultiplicativeIdentity,
    RewriteLaw::BooleanIdentity,
];

fn is_integer_literal(operator: &EquivalenceOperator, expected: i128) -> bool {
    matches!(
        operator.kind,
        EquivalenceOperatorKind::Literal(ExpressionLiteral::Integer(value)) if value == expected
    )
}

fn is_bool_literal(operator: &EquivalenceOperator, expected: bool) -> bool {
    matches!(
        operator.kind,
        EquivalenceOperatorKind::Literal(ExpressionLiteral::Bool(value)) if value == expected
    )
}

fn law_label(law: RewriteLaw) -> &'static str {
    match law {
        RewriteLaw::ConstantFold => "libcpg.constant-fold",
        RewriteLaw::Commutativity => "libcpg.commutativity",
        RewriteLaw::Associativity => "libcpg.associativity",
        RewriteLaw::AdditiveIdentity => "libcpg.additive-identity",
        RewriteLaw::MultiplicativeIdentity => "libcpg.multiplicative-identity",
        RewriteLaw::BooleanIdentity => "libcpg.boolean-identity",
    }
}

fn law_from_label(label: &str) -> Option<RewriteLaw> {
    REWRITE_LAWS.into_iter().find(|law| {
        let base = law_label(*law);
        label == base
            || label
                .strip_prefix(base)
                .is_some_and(|suffix| suffix.starts_with('.'))
    })
}

fn fold_literal(
    operator: &EquivalenceOperator,
    children: &[&EquivalenceOperator],
) -> Option<ExpressionLiteral> {
    match (&operator.kind, operator.value_type, children) {
        (EquivalenceOperatorKind::Unary(unary), ExpressionType::Integer(integer), [child]) => {
            let EquivalenceOperatorKind::Literal(ExpressionLiteral::Integer(value)) = child.kind
            else {
                return None;
            };
            fold_integer_unary(*unary, integer, value).map(ExpressionLiteral::Integer)
        }
        (
            EquivalenceOperatorKind::Unary(UnaryOperator::LogicalNot),
            ExpressionType::Bool,
            [child],
        ) => {
            let EquivalenceOperatorKind::Literal(ExpressionLiteral::Bool(value)) = child.kind
            else {
                return None;
            };
            Some(ExpressionLiteral::Bool(!value))
        }
        (
            EquivalenceOperatorKind::Binary(binary),
            ExpressionType::Integer(integer),
            [left, right],
        ) => {
            let (
                EquivalenceOperatorKind::Literal(ExpressionLiteral::Integer(left)),
                EquivalenceOperatorKind::Literal(ExpressionLiteral::Integer(right)),
            ) = (&left.kind, &right.kind)
            else {
                return None;
            };
            fold_integer_binary(*binary, integer, *left, *right).map(ExpressionLiteral::Integer)
        }
        (EquivalenceOperatorKind::Binary(binary), ExpressionType::Bool, [left, right]) => {
            let (
                EquivalenceOperatorKind::Literal(ExpressionLiteral::Bool(left)),
                EquivalenceOperatorKind::Literal(ExpressionLiteral::Bool(right)),
            ) = (&left.kind, &right.kind)
            else {
                return None;
            };
            let value = match binary {
                BinaryOperator::LogicalAnd => *left && *right,
                BinaryOperator::LogicalOr => *left || *right,
                BinaryOperator::Equal => left == right,
                BinaryOperator::NotEqual => left != right,
                _ => return None,
            };
            Some(ExpressionLiteral::Bool(value))
        }
        _ => None,
    }
}

fn fold_integer_unary(operator: UnaryOperator, integer: IntegerType, value: i128) -> Option<i128> {
    match operator {
        UnaryOperator::Negate => match integer.overflow {
            IntegerOverflow::Wrapping => Some(normalize_wrapping(value.wrapping_neg(), integer)),
            IntegerOverflow::Saturating => {
                let (minimum, maximum) = integer_bounds(integer)?;
                Some(value.saturating_neg().clamp(minimum, maximum))
            }
            IntegerOverflow::Checked
            | IntegerOverflow::Undefined
            | IntegerOverflow::ArbitraryPrecision => {
                let result = value.checked_neg()?;
                integer_contains(integer, result).then_some(result)
            }
        },
        UnaryOperator::BitwiseNot => Some(normalize_wrapping(!value, integer)),
        UnaryOperator::LogicalNot => None,
    }
}

fn fold_integer_binary(
    operator: BinaryOperator,
    integer: IntegerType,
    left: i128,
    right: i128,
) -> Option<i128> {
    match operator {
        BinaryOperator::Add | BinaryOperator::Subtract | BinaryOperator::Multiply => {
            match integer.overflow {
                IntegerOverflow::Wrapping => {
                    let value = match operator {
                        BinaryOperator::Add => left.wrapping_add(right),
                        BinaryOperator::Subtract => left.wrapping_sub(right),
                        BinaryOperator::Multiply => left.wrapping_mul(right),
                        _ => unreachable!(),
                    };
                    Some(normalize_wrapping(value, integer))
                }
                IntegerOverflow::Saturating => {
                    let (minimum, maximum) = integer_bounds(integer)?;
                    let value = match operator {
                        BinaryOperator::Add => left.saturating_add(right),
                        BinaryOperator::Subtract => left.saturating_sub(right),
                        BinaryOperator::Multiply => left.saturating_mul(right),
                        _ => unreachable!(),
                    };
                    Some(value.clamp(minimum, maximum))
                }
                IntegerOverflow::Checked
                | IntegerOverflow::Undefined
                | IntegerOverflow::ArbitraryPrecision => {
                    let value = match operator {
                        BinaryOperator::Add => left.checked_add(right),
                        BinaryOperator::Subtract => left.checked_sub(right),
                        BinaryOperator::Multiply => left.checked_mul(right),
                        _ => unreachable!(),
                    }?;
                    integer_contains(integer, value).then_some(value)
                }
            }
        }
        BinaryOperator::Divide => {
            let value = left.checked_div(right)?;
            integer_contains(integer, value).then_some(value)
        }
        BinaryOperator::Remainder => {
            let value = left.checked_rem(right)?;
            integer_contains(integer, value).then_some(value)
        }
        BinaryOperator::ShiftLeft | BinaryOperator::ShiftRight => {
            let count = shift_count(integer, right)?;
            let value = match operator {
                BinaryOperator::ShiftLeft => left.wrapping_shl(count),
                BinaryOperator::ShiftRight if integer.signed => left.wrapping_shr(count),
                BinaryOperator::ShiftRight => ((left as u128).wrapping_shr(count)) as i128,
                _ => unreachable!(),
            };
            Some(normalize_wrapping(value, integer))
        }
        BinaryOperator::BitwiseAnd => Some(normalize_wrapping(left & right, integer)),
        BinaryOperator::BitwiseOr => Some(normalize_wrapping(left | right, integer)),
        BinaryOperator::BitwiseXor => Some(normalize_wrapping(left ^ right, integer)),
        _ => None,
    }
}

fn shift_count(integer: IntegerType, count: i128) -> Option<u32> {
    let width = integer.width.map(u32::from);
    match (integer.shift, width) {
        (ShiftBehavior::Masked, Some(width)) => {
            let modulus = i128::from(width);
            u32::try_from(count.rem_euclid(modulus)).ok()
        }
        (ShiftBehavior::Checked | ShiftBehavior::Undefined, Some(width)) => {
            let count = u32::try_from(count).ok()?;
            (count < width).then_some(count)
        }
        (_, None) => u32::try_from(count).ok(),
    }
}

fn normalize_wrapping(value: i128, integer: IntegerType) -> i128 {
    let Some(width) = integer.width else {
        return value;
    };
    if width == 128 {
        return value;
    }
    let mask = (1u128 << width) - 1;
    let bits = (value as u128) & mask;
    if integer.signed && bits & (1u128 << (width - 1)) != 0 {
        (bits | !mask) as i128
    } else {
        bits as i128
    }
}

fn integer_bounds(integer: IntegerType) -> Option<(i128, i128)> {
    let Some(width) = integer.width else {
        return Some((i128::MIN, i128::MAX));
    };
    if integer.signed {
        if width == 128 {
            Some((i128::MIN, i128::MAX))
        } else {
            let magnitude = 1i128 << (width - 1);
            Some((-magnitude, magnitude - 1))
        }
    } else if width == 128 {
        Some((0, i128::MAX))
    } else {
        Some((0, (1i128 << width) - 1))
    }
}

fn integer_contains(integer: IntegerType, value: i128) -> bool {
    integer_bounds(integer).is_some_and(|(minimum, maximum)| (minimum..=maximum).contains(&value))
}

struct ExpressionAtomicVerifier<'proof> {
    profile: &'proof SemanticProfile,
    operators: &'proof BTreeMap<Vec<u8>, EquivalenceOperator>,
}

impl AtomicEvidenceVerifier for ExpressionAtomicVerifier<'_> {
    type Error = Infallible;

    fn verify(
        &mut self,
        evidence: &AtomicEvidence,
        tape: &ProofTape,
        source: ProofTermId,
        target: ProofTermId,
    ) -> Result<bool, Self::Error> {
        let AtomicEvidence::Rewrite { rule, direction } = evidence else {
            return Ok(false);
        };
        if *direction != RewriteDirection::Forward {
            return Ok(false);
        }
        if rule.as_str() == AC_NORMALIZE_LABEL {
            return Ok(self.profile.allows(RewriteLaw::Associativity)
                && self.profile.allows(RewriteLaw::Commutativity)
                && verify_ac_normalization(tape, self.operators, source, target));
        }
        let Some(law) = law_from_label(rule.as_str()) else {
            return Ok(false);
        };
        if !self.profile.allows(law) {
            return Ok(false);
        }
        Ok(verify_proof_law(law, tape, self.operators, source, target))
    }
}

fn flattened_ac_leaves(
    tape: &ProofTape,
    operators: &BTreeMap<Vec<u8>, EquivalenceOperator>,
    root: ProofTermId,
    ac_operator: &EquivalenceOperator,
) -> Option<Vec<ProofTermId>> {
    let mut pending = vec![root];
    let mut leaves = Vec::new();
    while let Some(term_id) = pending.pop() {
        let (operator, term) = proof_term(tape, operators, term_id)?;
        if operator == ac_operator && !term.children().is_empty() {
            pending.extend(term.children().iter().rev().copied());
        } else {
            leaves.push(term_id);
        }
    }
    leaves.sort_unstable();
    Some(leaves)
}

fn verify_ac_normalization(
    tape: &ProofTape,
    operators: &BTreeMap<Vec<u8>, EquivalenceOperator>,
    source: ProofTermId,
    target: ProofTermId,
) -> bool {
    let Some((source_operator, _)) = proof_term(tape, operators, source) else {
        return false;
    };
    let Some((target_operator, _)) = proof_term(tape, operators, target) else {
        return false;
    };
    if source_operator != target_operator
        || !operator_admits_law(source_operator, RewriteLaw::Associativity)
        || !operator_admits_law(source_operator, RewriteLaw::Commutativity)
    {
        return false;
    }
    let Some(source_leaves) = flattened_ac_leaves(tape, operators, source, source_operator) else {
        return false;
    };
    let Some(target_leaves) = flattened_ac_leaves(tape, operators, target, source_operator) else {
        return false;
    };
    source_leaves == target_leaves
}

fn proof_term<'proof>(
    tape: &'proof ProofTape,
    operators: &'proof BTreeMap<Vec<u8>, EquivalenceOperator>,
    id: ProofTermId,
) -> Option<(&'proof EquivalenceOperator, replete::ProofTermView<'proof>)> {
    let term = tape.term(id)?;
    let operator = operators.get(term.operator_header())?;
    Some((operator, term))
}

#[derive(Clone, Copy)]
enum ClosedEvaluationFrame {
    Enter(ProofTermId),
    Exit(ProofTermId),
}

fn evaluate_closed_proof_term(
    tape: &ProofTape,
    operators: &BTreeMap<Vec<u8>, EquivalenceOperator>,
    root: ProofTermId,
) -> Option<EquivalenceOperator> {
    let mut values: Vec<Option<EquivalenceOperator>> = Vec::new();
    values.try_reserve_exact(tape.term_count()).ok()?;
    values.resize_with(tape.term_count(), || None);
    let mut pending = Vec::new();
    pending.try_reserve_exact(tape.term_count()).ok()?;
    pending.push(ClosedEvaluationFrame::Enter(root));

    while let Some(frame) = pending.pop() {
        match frame {
            ClosedEvaluationFrame::Enter(term_id) => {
                let (_, term) = proof_term(tape, operators, term_id)?;
                if values.get(term_id.index())?.is_some() {
                    continue;
                }
                pending.push(ClosedEvaluationFrame::Exit(term_id));
                for &child in term.children().iter().rev() {
                    pending.push(ClosedEvaluationFrame::Enter(child));
                }
            }
            ClosedEvaluationFrame::Exit(term_id) => {
                let (operator, term) = proof_term(tape, operators, term_id)?;
                let evaluated = if term.children().is_empty() {
                    matches!(operator.kind, EquivalenceOperatorKind::Literal(_))
                        .then(|| operator.clone())?
                } else {
                    let expected_arity = match operator.kind {
                        EquivalenceOperatorKind::Unary(_) => 1,
                        EquivalenceOperatorKind::Binary(_) => 2,
                        _ => return None,
                    };
                    if term.children().len() != expected_arity {
                        return None;
                    }
                    let mut children = Vec::new();
                    children.try_reserve_exact(expected_arity).ok()?;
                    for child in term.children() {
                        let child = values.get(child.index())?.as_ref()?;
                        if child.namespace != operator.namespace
                            || child.value_type != operator.value_type
                        {
                            return None;
                        }
                        children.push(child);
                    }
                    let literal = fold_literal(operator, &children)?;
                    EquivalenceOperator {
                        namespace: operator.namespace,
                        kind: EquivalenceOperatorKind::Literal(literal),
                        value_type: operator.value_type,
                    }
                };
                *values.get_mut(term_id.index())? = Some(evaluated);
            }
        }
    }

    values.get_mut(root.index())?.take()
}

fn verify_proof_law(
    law: RewriteLaw,
    tape: &ProofTape,
    operators: &BTreeMap<Vec<u8>, EquivalenceOperator>,
    source: ProofTermId,
    target: ProofTermId,
) -> bool {
    let Some((source_op, source_term)) = proof_term(tape, operators, source) else {
        return false;
    };
    let Some((target_op, target_term)) = proof_term(tape, operators, target) else {
        return false;
    };
    let source_children = source_term.children();
    let target_children = target_term.children();
    if !operator_admits_law(source_op, law) {
        return false;
    }
    match law {
        RewriteLaw::ConstantFold => {
            target_children.is_empty()
                && evaluate_closed_proof_term(tape, operators, source)
                    .is_some_and(|evaluated| &evaluated == target_op)
        }
        RewriteLaw::Commutativity => {
            source_op == target_op
                && source_children.len() == 2
                && target_children == [source_children[1], source_children[0]]
        }
        RewriteLaw::Associativity => verify_associativity(
            tape,
            operators,
            source_op,
            source_children,
            target_op,
            target_children,
        ),
        RewriteLaw::AdditiveIdentity => verify_identity(
            tape,
            operators,
            source_op,
            source_children,
            target,
            0,
            false,
        ),
        RewriteLaw::MultiplicativeIdentity => verify_identity(
            tape,
            operators,
            source_op,
            source_children,
            target,
            1,
            false,
        ),
        RewriteLaw::BooleanIdentity => {
            verify_identity(tape, operators, source_op, source_children, target, 0, true)
        }
    }
}

fn operator_admits_law(operator: &EquivalenceOperator, law: RewriteLaw) -> bool {
    let EquivalenceOperatorKind::Binary(binary) = operator.kind else {
        return law == RewriteLaw::ConstantFold
            && matches!(operator.kind, EquivalenceOperatorKind::Unary(_));
    };
    match law {
        RewriteLaw::ConstantFold => true,
        RewriteLaw::Commutativity => match operator.value_type {
            ExpressionType::Integer(_) => matches!(
                binary,
                BinaryOperator::Add
                    | BinaryOperator::Multiply
                    | BinaryOperator::BitwiseAnd
                    | BinaryOperator::BitwiseOr
                    | BinaryOperator::BitwiseXor
            ),
            ExpressionType::Bool => matches!(
                binary,
                BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr
            ),
            ExpressionType::Float(FloatType {
                mode: FloatMode::FiniteReassociation,
                ..
            }) => matches!(binary, BinaryOperator::Add | BinaryOperator::Multiply),
            _ => false,
        },
        RewriteLaw::Associativity => match operator.value_type {
            ExpressionType::Integer(integer) => match binary {
                BinaryOperator::Add | BinaryOperator::Multiply => matches!(
                    integer.overflow,
                    IntegerOverflow::Wrapping | IntegerOverflow::ArbitraryPrecision
                ),
                BinaryOperator::BitwiseAnd
                | BinaryOperator::BitwiseOr
                | BinaryOperator::BitwiseXor => true,
                _ => false,
            },
            ExpressionType::Bool => matches!(
                binary,
                BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr
            ),
            ExpressionType::Float(FloatType {
                mode: FloatMode::FiniteReassociation,
                ..
            }) => matches!(binary, BinaryOperator::Add | BinaryOperator::Multiply),
            _ => false,
        },
        RewriteLaw::AdditiveIdentity => {
            binary == BinaryOperator::Add
                && matches!(operator.value_type, ExpressionType::Integer(_))
        }
        RewriteLaw::MultiplicativeIdentity => {
            binary == BinaryOperator::Multiply
                && matches!(operator.value_type, ExpressionType::Integer(_))
        }
        RewriteLaw::BooleanIdentity => {
            matches!(
                binary,
                BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr
            ) && operator.value_type == ExpressionType::Bool
        }
    }
}

fn verify_associativity(
    tape: &ProofTape,
    operators: &BTreeMap<Vec<u8>, EquivalenceOperator>,
    source_op: &EquivalenceOperator,
    source_children: &[ProofTermId],
    target_op: &EquivalenceOperator,
    target_children: &[ProofTermId],
) -> bool {
    if source_op != target_op || source_children.len() != 2 || target_children.len() != 2 {
        return false;
    }
    if let Some((left_op, left_term)) = proof_term(tape, operators, source_children[0]) {
        if left_op == source_op && left_term.children().len() == 2 {
            if let Some((right_op, right_term)) = proof_term(tape, operators, target_children[1]) {
                if right_op == source_op
                    && right_term.children() == [left_term.children()[1], source_children[1]]
                    && target_children[0] == left_term.children()[0]
                {
                    return true;
                }
            }
        }
    }
    if let Some((right_op, right_term)) = proof_term(tape, operators, source_children[1]) {
        if right_op == source_op && right_term.children().len() == 2 {
            if let Some((left_op, left_term)) = proof_term(tape, operators, target_children[0]) {
                return left_op == source_op
                    && left_term.children() == [source_children[0], right_term.children()[0]]
                    && target_children[1] == right_term.children()[1];
            }
        }
    }
    false
}

fn verify_identity(
    tape: &ProofTape,
    operators: &BTreeMap<Vec<u8>, EquivalenceOperator>,
    source_op: &EquivalenceOperator,
    source_children: &[ProofTermId],
    target: ProofTermId,
    identity: i128,
    boolean: bool,
) -> bool {
    let EquivalenceOperatorKind::Binary(binary) = source_op.kind else {
        return false;
    };
    if source_children.len() != 2 {
        return false;
    }
    let valid_operator = if boolean {
        matches!(
            binary,
            BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr
        )
    } else if identity == 0 {
        binary == BinaryOperator::Add
    } else {
        binary == BinaryOperator::Multiply
    };
    if !valid_operator {
        return false;
    }
    for (candidate, other) in [
        (source_children[0], source_children[1]),
        (source_children[1], source_children[0]),
    ] {
        let Some((candidate_op, _)) = proof_term(tape, operators, candidate) else {
            continue;
        };
        let is_identity = if boolean {
            match binary {
                BinaryOperator::LogicalAnd => is_bool_literal(candidate_op, true),
                BinaryOperator::LogicalOr => is_bool_literal(candidate_op, false),
                _ => false,
            }
        } else {
            is_integer_literal(candidate_op, identity)
        };
        if is_identity && other == target {
            return true;
        }
    }
    false
}

#[cfg(test)]
#[path = "expression_equivalence_tests.rs"]
mod tests;
