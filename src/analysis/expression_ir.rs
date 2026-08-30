//! Typed, language-neutral, stack-safe expression intermediate representation.
//!
//! Frontends supply explicit [ExpressionFact] values while a
//! [SemanticProfile] states the language rules under which those facts may be
//! transformed. This module never parses source text and never infers an
//! operator from its spelling. Lowering uses a heap worklist and emits one flat
//! postorder arena whose child identifiers always precede their parent.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use thiserror::Error;

use crate::{CodePropertyGraph, CpgNodeKind, Language, LiteralKind, NodeId};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Languages with complete built-in expression semantic profiles.
///
/// Other [Language] values remain representable through
/// [SemanticProfile::unknown], but cannot receive rewrite certificates until
/// a frontend supplies a complete profile.
pub const BUILTIN_EXPRESSION_PROFILE_LANGUAGES: [Language; 9] = [
    Language::Rust,
    Language::C,
    Language::Cpp,
    Language::Go,
    Language::Java,
    Language::Python,
    Language::JavaScript,
    Language::TypeScript,
    Language::Ruby,
];

/// Explicit presence or absence of one semantic fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum SemanticKnowledge<T> {
    /// The frontend supplied this semantic dimension.
    Known(T),
    /// The dimension is intentionally unknown and must fail closed.
    Unknown,
}

impl<T> SemanticKnowledge<T> {
    /// Returns whether this dimension is explicitly known.
    pub const fn is_known(&self) -> bool {
        matches!(self, Self::Known(_))
    }

    /// Borrows the known value, if present.
    pub const fn as_ref(&self) -> Option<&T> {
        match self {
            Self::Known(value) => Some(value),
            Self::Unknown => None,
        }
    }
}

/// How expression effects and purity are established.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum EffectSemantics {
    /// Every fact carries explicit effect evidence.
    ExplicitPerFact,
}

/// Operand evaluation order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum EvaluationOrder {
    /// Operands evaluate from left to right.
    LeftToRight,
    /// The language leaves operand order unspecified.
    Unspecified,
}

/// How integer widths and overflow behavior are obtained.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum IntegerSemantics {
    /// Each typed fact supplies width, signedness, overflow, and shift behavior.
    ExplicitPerFact,
}

/// How floating-point behavior is obtained.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum FloatingPointSemantics {
    /// Each typed fact supplies its IEEE-754 or relaxed floating-point mode.
    ExplicitPerFact,
}

/// How exceptional completion is represented.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ExceptionSemantics {
    /// A fact's effect evidence explicitly states whether evaluation may fail.
    ExplicitPerFact,
}

/// Language-level Boolean evaluation behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ShortCircuitSemantics {
    /// Logical conjunction and disjunction short-circuit in source order.
    LogicalAndOr,
    /// Boolean operators evaluate all operands.
    Eager,
}

/// How aliasing and memory reads are classified.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum AliasingSemantics {
    /// Each memory fact states immutable, shared, mutable, volatile, or atomic access.
    ExplicitPerFact,
}

/// How function and method calls are classified.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum CallSemantics {
    /// Resolution identity and transitive effects are supplied per call fact.
    ExplicitPerFact,
}

/// How volatile and atomic access is classified.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum VolatileAtomicSemantics {
    /// Every such access is explicit and is never silently treated as pure.
    ExplicitPerFact,
}

/// How binding identity and lexical scope are established.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum BindingSemantics {
    /// Bindings use a resolved definition and frontend-assigned lexical scope.
    ResolvedDefinitionAndScope,
}

/// A named transformation family understood by the expression IR boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum RewriteLaw {
    /// Evaluate a closed unary or binary expression.
    ConstantFold,
    /// Exchange the operands of a commutative binary operator.
    Commutativity,
    /// Reassociate a binary operator.
    Associativity,
    /// Eliminate integer addition by zero.
    AdditiveIdentity,
    /// Eliminate integer multiplication by one.
    MultiplicativeIdentity,
    /// Eliminate conjunction with true or disjunction with false.
    BooleanIdentity,
}

impl RewriteLaw {
    const ALL: [Self; 6] = [
        Self::ConstantFold,
        Self::Commutativity,
        Self::Associativity,
        Self::AdditiveIdentity,
        Self::MultiplicativeIdentity,
        Self::BooleanIdentity,
    ];
}

/// Complete or explicitly incomplete frontend semantics.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct SemanticProfile {
    /// Language whose typed facts this profile interprets.
    pub language: Language,
    /// Purity and effect discipline.
    pub effects: SemanticKnowledge<EffectSemantics>,
    /// Operand evaluation order.
    pub evaluation_order: SemanticKnowledge<EvaluationOrder>,
    /// Integer widths, signedness, overflow, division, and shifts.
    pub integers: SemanticKnowledge<IntegerSemantics>,
    /// Floating-point representation and algebraic mode.
    pub floating_point: SemanticKnowledge<FloatingPointSemantics>,
    /// Exceptional-completion discipline.
    pub exceptions: SemanticKnowledge<ExceptionSemantics>,
    /// Boolean short-circuit discipline.
    pub short_circuit: SemanticKnowledge<ShortCircuitSemantics>,
    /// Aliasing and memory-read discipline.
    pub aliasing: SemanticKnowledge<AliasingSemantics>,
    /// Call resolution and effects discipline.
    pub calls: SemanticKnowledge<CallSemantics>,
    /// Volatile and atomic discipline.
    pub volatile_atomic: SemanticKnowledge<VolatileAtomicSemantics>,
    /// Binding and lexical-scope discipline.
    pub bindings: SemanticKnowledge<BindingSemantics>,
    rewrite_laws: BTreeSet<RewriteLaw>,
}

impl SemanticProfile {
    /// Returns the conservative built-in profile for a language.
    ///
    /// A complete built-in profile still requires explicit per-fact types and
    /// effects. Unsupported languages receive [Self::unknown] rather than an
    /// assumed C-like default.
    pub fn for_language(language: Language) -> Self {
        let evaluation_order = match language {
            Language::Rust
            | Language::Java
            | Language::Python
            | Language::JavaScript
            | Language::TypeScript
            | Language::Ruby => Some(EvaluationOrder::LeftToRight),
            Language::C | Language::Cpp | Language::Go => Some(EvaluationOrder::Unspecified),
            _ => None,
        };
        let Some(evaluation_order) = evaluation_order else {
            return Self::unknown(language);
        };
        Self {
            language,
            effects: SemanticKnowledge::Known(EffectSemantics::ExplicitPerFact),
            evaluation_order: SemanticKnowledge::Known(evaluation_order),
            integers: SemanticKnowledge::Known(IntegerSemantics::ExplicitPerFact),
            floating_point: SemanticKnowledge::Known(FloatingPointSemantics::ExplicitPerFact),
            exceptions: SemanticKnowledge::Known(ExceptionSemantics::ExplicitPerFact),
            short_circuit: SemanticKnowledge::Known(ShortCircuitSemantics::LogicalAndOr),
            aliasing: SemanticKnowledge::Known(AliasingSemantics::ExplicitPerFact),
            calls: SemanticKnowledge::Known(CallSemantics::ExplicitPerFact),
            volatile_atomic: SemanticKnowledge::Known(VolatileAtomicSemantics::ExplicitPerFact),
            bindings: SemanticKnowledge::Known(BindingSemantics::ResolvedDefinitionAndScope),
            rewrite_laws: RewriteLaw::ALL.into_iter().collect(),
        }
    }

    /// Returns a fully fail-closed profile for an unsupported frontend.
    pub fn unknown(language: Language) -> Self {
        Self {
            language,
            effects: SemanticKnowledge::Unknown,
            evaluation_order: SemanticKnowledge::Unknown,
            integers: SemanticKnowledge::Unknown,
            floating_point: SemanticKnowledge::Unknown,
            exceptions: SemanticKnowledge::Unknown,
            short_circuit: SemanticKnowledge::Unknown,
            aliasing: SemanticKnowledge::Unknown,
            calls: SemanticKnowledge::Unknown,
            volatile_atomic: SemanticKnowledge::Unknown,
            bindings: SemanticKnowledge::Unknown,
            rewrite_laws: BTreeSet::new(),
        }
    }

    /// Returns whether all ten required semantic dimensions are known.
    pub fn is_complete(&self) -> bool {
        self.effects.is_known()
            && self.evaluation_order.is_known()
            && self.integers.is_known()
            && self.floating_point.is_known()
            && self.exceptions.is_known()
            && self.short_circuit.is_known()
            && self.aliasing.is_known()
            && self.calls.is_known()
            && self.volatile_atomic.is_known()
            && self.bindings.is_known()
    }

    /// Returns whether the profile admits the named law before node-local checks.
    pub fn allows(&self, law: RewriteLaw) -> bool {
        self.is_complete() && self.rewrite_laws.contains(&law)
    }

    /// Enables or disables one law explicitly.
    pub fn with_rewrite_law(mut self, law: RewriteLaw, enabled: bool) -> Self {
        if enabled {
            self.rewrite_laws.insert(law);
        } else {
            self.rewrite_laws.remove(&law);
        }
        self
    }
}

/// Integer overflow behavior attached to one typed fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum IntegerOverflow {
    /// Arithmetic reduces modulo the fixed width.
    Wrapping,
    /// Overflow produces an explicitly modeled failure.
    Checked,
    /// Arithmetic clamps to the representable range.
    Saturating,
    /// Overflow makes the source operation semantically undefined.
    Undefined,
    /// The integer has no fixed-width overflow.
    ArbitraryPrecision,
}

/// Shift-count behavior attached to one typed integer fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ShiftBehavior {
    /// Counts are reduced modulo the integer width.
    Masked,
    /// Out-of-range counts produce an explicitly modeled failure.
    Checked,
    /// Out-of-range counts make the operation semantically undefined.
    Undefined,
}

/// Exact integer type semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct IntegerType {
    /// Fixed width, or no width for arbitrary precision.
    pub width: Option<u16>,
    /// Whether values are signed.
    pub signed: bool,
    /// Arithmetic overflow behavior.
    pub overflow: IntegerOverflow,
    /// Shift-count behavior.
    pub shift: ShiftBehavior,
}

impl IntegerType {
    /// Constructs a fixed-width integer type.
    pub fn fixed(
        width: u16,
        signed: bool,
        overflow: IntegerOverflow,
        shift: ShiftBehavior,
    ) -> Result<Self, ExpressionFactError> {
        if width == 0 || width > 128 || overflow == IntegerOverflow::ArbitraryPrecision {
            return Err(ExpressionFactError::InvalidIntegerType);
        }
        Ok(Self {
            width: Some(width),
            signed,
            overflow,
            shift,
        })
    }

    /// Constructs an arbitrary-precision integer type.
    pub const fn arbitrary_precision(signed: bool) -> Self {
        Self {
            width: None,
            signed,
            overflow: IntegerOverflow::ArbitraryPrecision,
            shift: ShiftBehavior::Checked,
        }
    }

    /// Returns whether the public descriptor has a coherent representation.
    pub const fn is_valid(self) -> bool {
        match self.width {
            Some(width) => {
                width > 0
                    && width <= 128
                    && !matches!(self.overflow, IntegerOverflow::ArbitraryPrecision)
            }
            None => {
                matches!(self.overflow, IntegerOverflow::ArbitraryPrecision)
                    && !matches!(self.shift, ShiftBehavior::Masked)
            }
        }
    }
}

/// IEEE-754 binary format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum FloatWidth {
    /// IEEE-754 binary32.
    F32,
    /// IEEE-754 binary64.
    F64,
}

/// Floating-point algebraic contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum FloatMode {
    /// Preserve IEEE exceptional values, signed zero, and source grouping.
    StrictIeee754,
    /// The frontend proves NaNs and signed zero absent and permits reassociation.
    FiniteReassociation,
}

/// Exact floating-point type semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct FloatType {
    /// Binary representation width.
    pub width: FloatWidth,
    /// Algebraic contract selected by the frontend.
    pub mode: FloatMode,
}

/// Language-neutral value type required by rewrite preconditions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ExpressionType {
    /// Boolean value.
    Bool,
    /// Typed integer value.
    Integer(IntegerType),
    /// Typed floating-point value.
    Float(FloatType),
    /// Unicode or language string value.
    String,
    /// Unicode scalar or language character value.
    Char,
    /// Unit or void value.
    Unit,
    /// Dynamically typed value whose runtime class is not narrowed here.
    Dynamic,
}

/// Explicit effects for one expression fact.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ExpressionEffects {
    /// Reads mutable or aliasable memory.
    pub reads_memory: bool,
    /// Writes memory.
    pub writes_memory: bool,
    /// Performs a call with externally visible behavior.
    pub calls: bool,
    /// May throw, trap, panic, or otherwise complete exceptionally.
    pub may_fail: bool,
    /// Performs volatile access.
    pub volatile: bool,
    /// Performs atomic access or synchronization.
    pub atomic: bool,
    /// Depends on nondeterminism, time, randomness, or ambient state.
    pub nondeterministic: bool,
    /// Suspends or yields control.
    pub suspends: bool,
}

impl ExpressionEffects {
    /// The explicit effect-free evidence value.
    pub const PURE: Self = Self {
        reads_memory: false,
        writes_memory: false,
        calls: false,
        may_fail: false,
        volatile: false,
        atomic: false,
        nondeterministic: false,
        suspends: false,
    };

    /// Returns whether no effect dimension is present.
    pub const fn is_empty(self) -> bool {
        !self.reads_memory
            && !self.writes_memory
            && !self.calls
            && !self.may_fail
            && !self.volatile
            && !self.atomic
            && !self.nondeterministic
            && !self.suspends
    }
}

/// Exact literal identity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ExpressionLiteral {
    /// Signed integer representation used by current libcpg literal facts.
    Integer(i128),
    /// Exact binary32 bits.
    Float32(u32),
    /// Exact binary64 bits.
    Float64(u64),
    /// Boolean literal.
    Bool(bool),
    /// String literal.
    String(Arc<str>),
    /// Character literal.
    Char(char),
    /// Null or nil literal.
    Null,
}

/// Typed unary operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum UnaryOperator {
    /// Arithmetic negation.
    Negate,
    /// Boolean negation.
    LogicalNot,
    /// Integer bitwise complement.
    BitwiseNot,
}

/// Typed binary operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum BinaryOperator {
    /// Addition.
    Add,
    /// Subtraction.
    Subtract,
    /// Multiplication.
    Multiply,
    /// Division.
    Divide,
    /// Remainder.
    Remainder,
    /// Left shift.
    ShiftLeft,
    /// Right shift.
    ShiftRight,
    /// Bitwise conjunction.
    BitwiseAnd,
    /// Bitwise disjunction.
    BitwiseOr,
    /// Bitwise exclusive disjunction.
    BitwiseXor,
    /// Short-circuit Boolean conjunction.
    LogicalAnd,
    /// Short-circuit Boolean disjunction.
    LogicalOr,
    /// Equality comparison.
    Equal,
    /// Inequality comparison.
    NotEqual,
    /// Less-than comparison.
    Less,
    /// Less-than-or-equal comparison.
    LessEqual,
    /// Greater-than comparison.
    Greater,
    /// Greater-than-or-equal comparison.
    GreaterEqual,
}

/// Resolved or conservative call identity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum CallIdentity {
    /// A uniquely resolved CPG function.
    Resolved(NodeId),
    /// A stable external symbol identity.
    External(Arc<str>),
    /// Resolution was incomplete.
    Unresolved,
}

/// Memory-read classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum MemoryAccess {
    /// Read from frontend-proved immutable storage.
    Immutable,
    /// Read from aliasable shared storage.
    Shared,
    /// Read through mutable storage.
    Mutable,
    /// Volatile read.
    Volatile,
    /// Atomic read.
    Atomic,
}

/// Resolved binding identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct BindingIdentity {
    /// Definition node.
    pub definition: NodeId,
    /// Frontend-assigned lexical scope identifier.
    pub scope: u32,
}

/// Typed expression fact kind.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ExpressionFactKind {
    /// Exact literal.
    Literal(ExpressionLiteral),
    /// Resolved lexical binding.
    Binding(BindingIdentity),
    /// Typed unary operation.
    Unary(UnaryOperator),
    /// Typed binary operation.
    Binary(BinaryOperator),
    /// Call with explicit identity and effect evidence.
    Call(CallIdentity),
    /// Read from an explicit memory location.
    Load {
        /// Stable location identity supplied by the frontend.
        location: NodeId,
        /// Access discipline.
        access: MemoryAccess,
    },
    /// Unsupported expression retained only by source identity.
    Opaque,
}

/// Per-node semantic facts supplied by a language frontend.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ExpressionFact {
    /// Typed expression category.
    pub kind: ExpressionFactKind,
    /// Ordered source-node operands.
    pub operands: Vec<NodeId>,
    /// Result type evidence.
    pub value_type: SemanticKnowledge<ExpressionType>,
    /// Effect evidence.
    pub effects: SemanticKnowledge<ExpressionEffects>,
}

impl ExpressionFact {
    /// Constructs a fact with explicit known type and effects.
    pub fn known(
        kind: ExpressionFactKind,
        operands: Vec<NodeId>,
        value_type: ExpressionType,
        effects: ExpressionEffects,
    ) -> Self {
        Self {
            kind,
            operands,
            value_type: SemanticKnowledge::Known(value_type),
            effects: SemanticKnowledge::Known(effects),
        }
    }
}

/// Canonical frontend fact table for one CPG language.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ExpressionFactTable {
    language: Language,
    facts: BTreeMap<NodeId, ExpressionFact>,
}

impl ExpressionFactTable {
    /// Builds a fact table and rejects duplicate source identities.
    pub fn try_new(
        language: Language,
        facts: impl IntoIterator<Item = (NodeId, ExpressionFact)>,
    ) -> Result<Self, ExpressionFactError> {
        let mut table = BTreeMap::new();
        for (source, fact) in facts {
            if table.insert(source, fact).is_some() {
                return Err(ExpressionFactError::DuplicateSource(source));
            }
        }
        Ok(Self {
            language,
            facts: table,
        })
    }

    /// Returns the table language.
    pub const fn language(&self) -> Language {
        self.language
    }

    /// Returns a fact by source identity.
    pub fn get(&self, source: NodeId) -> Option<&ExpressionFact> {
        self.facts.get(&source)
    }

    /// Returns the number of typed facts.
    pub fn len(&self) -> usize {
        self.facts.len()
    }

    /// Returns whether the table is empty.
    pub fn is_empty(&self) -> bool {
        self.facts.is_empty()
    }
}

/// Flat expression node identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ExpressionIrId(pub u32);

/// Semantic admission mode for one lowered node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ExpressionEvidenceMode {
    /// Complete effect-free semantics permit node-local law checks.
    Pure,
    /// Identity is known but transformations are forbidden.
    IdentityOnly,
    /// One or more semantic dimensions are unknown.
    Unknown,
}

/// One node in the flat postorder arena.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ExpressionIrNode {
    /// Typed operation or identity.
    pub kind: ExpressionFactKind,
    /// Result type evidence.
    pub value_type: SemanticKnowledge<ExpressionType>,
    /// Explicit effect evidence.
    pub effects: SemanticKnowledge<ExpressionEffects>,
    /// First operand in the shared operand tape.
    first_operand: u32,
    /// Number of ordered operands.
    operand_count: u32,
    /// Transformation admission mode.
    pub mode: ExpressionEvidenceMode,
}

/// Exact resource bounds for one lowering request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ExpressionIrLimits {
    /// Maximum emitted IR nodes.
    pub max_nodes: usize,
    /// Maximum emitted operand identifiers.
    pub max_operands: usize,
    /// Maximum work-machine frames processed.
    pub max_work: usize,
}

impl Default for ExpressionIrLimits {
    fn default() -> Self {
        Self {
            max_nodes: 1_000_000,
            max_operands: 4_000_000,
            max_work: 5_000_000,
        }
    }
}

/// Exact lowering measurements.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ExpressionIrMetrics {
    /// Emitted nodes.
    pub nodes: usize,
    /// Emitted operand identifiers.
    pub operands: usize,
    /// Processed work frames.
    pub work: usize,
}

/// Allocation site whose fallible reserve operation failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ExpressionIrAllocation {
    /// Flat node arena.
    NodeArena,
    /// Explicit lowering worklist.
    Worklist,
    /// Shared operand tape.
    OperandTape,
}

impl std::fmt::Display for ExpressionIrAllocation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::NodeArena => "node arena",
            Self::Worklist => "lowering worklist",
            Self::OperandTape => "operand tape",
        })
    }
}

/// Malformed typed-fact reason.
#[derive(Debug, Error, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ExpressionFactError {
    /// Fixed integer types require a width from 1 through 128 and a fixed mode.
    #[error("invalid integer type")]
    InvalidIntegerType,
    /// The table contained the same source twice.
    #[error("duplicate expression fact for source {0:?}")]
    DuplicateSource(NodeId),
    /// The root or an operand had no fact.
    #[error("missing expression fact for source {0:?}")]
    MissingFact(NodeId),
    /// The source node was absent from the CPG.
    #[error("expression source {0:?} is absent from the CPG")]
    MissingCpgNode(NodeId),
    /// The source node was not an expression.
    #[error("CPG source {0:?} is not an expression")]
    NonExpressionSource(NodeId),
    /// Typed operands differed from the CPG's ordered AST children.
    #[error("typed operands disagree with CPG children at source {0:?}")]
    OperandTopologyMismatch(NodeId),
    /// The typed kind did not match the CPG expression category.
    #[error("typed kind disagrees with CPG category at source {0:?}")]
    KindMismatch(NodeId),
    /// The typed operation had the wrong arity.
    #[error("typed operation has invalid arity at source {0:?}")]
    InvalidArity(NodeId),
    /// Known operand/result types violated the typed operator contract.
    #[error("typed operation has incompatible types at source {0:?}")]
    TypeMismatch(NodeId),
    /// A literal does not fit or agree with its known result type.
    #[error("literal does not agree with its type at source {0:?}")]
    LiteralTypeMismatch(NodeId),
    /// Typed literal identity differed from the normalized CPG literal.
    #[error("literal identity disagrees with the CPG at source {0:?}")]
    SourceLiteralMismatch(NodeId),
    /// The frontend's resolved binding differs from CPG resolution.
    #[error("binding identity disagrees with CPG resolution at source {0:?}")]
    BindingMismatch(NodeId),
    /// Typed call identity differed from CPG resolution.
    #[error("call identity disagrees with CPG resolution at source {0:?}")]
    CallMismatch(NodeId),
    /// CPG, fact table, and semantic profile languages differed.
    #[error("CPG, expression-fact, and semantic-profile languages differ")]
    LanguageMismatch {
        /// Language attached to the CPG.
        cpg: Language,
        /// Language attached to the expression facts.
        facts: Language,
        /// Language attached to the semantic profile.
        profile: Language,
    },
    /// The fact graph contains a reachable cycle.
    #[error("expression fact cycle reaches source {0:?}")]
    Cycle(NodeId),
    /// A flat identifier or count exceeded the u32 representation.
    #[error("expression IR identifier domain exhausted")]
    IdentifierOverflow,
    /// A required heap allocation failed.
    #[error("expression IR allocation failed while reserving {0}")]
    AllocationFailed(ExpressionIrAllocation),
}

/// Honest completion state for one lowering request.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ExpressionIrCompletion {
    /// Root and every reachable operand were emitted.
    Complete,
    /// The exact work bound was exhausted.
    WorkLimited {
        /// Processed frames.
        actual: usize,
        /// Configured maximum.
        maximum: usize,
    },
    /// The exact node bound was exhausted.
    NodeLimited {
        /// Emitted nodes.
        actual: usize,
        /// Configured maximum.
        maximum: usize,
    },
    /// The exact operand bound was exhausted.
    OperandLimited {
        /// Emitted operands plus the next node's operands.
        actual: usize,
        /// Configured maximum.
        maximum: usize,
    },
    /// Typed facts were malformed or inconsistent with the CPG.
    Malformed(ExpressionFactError),
}

/// Immutable flat IR plus explicit completion evidence.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ExpressionIr {
    nodes: Vec<ExpressionIrNode>,
    operands: Vec<ExpressionIrId>,
    sources: BTreeMap<NodeId, ExpressionIrId>,
    root: Option<ExpressionIrId>,
    profile: SemanticProfile,
    completion: ExpressionIrCompletion,
    metrics: ExpressionIrMetrics,
    #[cfg_attr(
        feature = "serde",
        serde(skip, default = "ExpressionIrProvenance::persisted")
    )]
    provenance: ExpressionIrProvenance,
}

/// Authority retained by an expression IR artifact.
///
/// Serialization intentionally omits this value. Every deserialized artifact
/// becomes [Self::PersistedIdentityOnly] because bytes cannot re-prove the CPG
/// and frontend facts checked by the live lowerer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExpressionIrProvenance {
    /// Produced by a validated lowering in the current process.
    LiveLowering,
    /// Loaded from persistence and therefore ineligible for rewrite authority.
    PersistedIdentityOnly,
}

impl ExpressionIrProvenance {
    #[cfg(feature = "serde")]
    const fn persisted() -> Self {
        Self::PersistedIdentityOnly
    }
}

impl ExpressionIr {
    /// Returns the explicit completion state.
    pub const fn completion(&self) -> &ExpressionIrCompletion {
        &self.completion
    }

    /// Returns exact lowering measurements.
    pub const fn metrics(&self) -> ExpressionIrMetrics {
        self.metrics
    }

    /// Returns the root only for a complete lowering.
    pub fn root(&self) -> Option<ExpressionIrId> {
        if matches!(self.completion, ExpressionIrCompletion::Complete) {
            self.root
        } else {
            None
        }
    }

    /// Returns a node by flat identifier.
    pub fn node(&self, id: ExpressionIrId) -> Option<&ExpressionIrNode> {
        self.nodes.get(id.0 as usize)
    }

    /// Returns a source node's emitted flat identifier.
    pub fn source_id(&self, source: NodeId) -> Option<ExpressionIrId> {
        self.sources.get(&source).copied()
    }

    /// Returns ordered operands for a node.
    pub fn operands(&self, id: ExpressionIrId) -> Option<&[ExpressionIrId]> {
        let node = self.node(id)?;
        let first = node.first_operand as usize;
        let end = first.checked_add(node.operand_count as usize)?;
        self.operands.get(first..end)
    }

    /// Returns the semantic profile retained by this artifact.
    pub const fn profile(&self) -> &SemanticProfile {
        &self.profile
    }

    /// Returns whether this artifact still carries live lowering authority.
    pub const fn provenance(&self) -> ExpressionIrProvenance {
        self.provenance
    }

    /// Checks every named precondition and returns an unforgeable certificate.
    pub fn certify(
        &self,
        node: ExpressionIrId,
        law: RewriteLaw,
    ) -> Result<RewriteCertificate, RewriteDenial> {
        if self.provenance != ExpressionIrProvenance::LiveLowering {
            return Err(RewriteDenial::PersistedArtifact);
        }
        if !matches!(self.completion, ExpressionIrCompletion::Complete) {
            return Err(RewriteDenial::IncompleteLowering);
        }
        let ir_node = self.node(node).ok_or(RewriteDenial::UnknownNode)?;
        if ir_node.mode != ExpressionEvidenceMode::Pure {
            return Err(RewriteDenial::NonPureEvidence);
        }
        if !self.profile.is_complete() {
            return Err(RewriteDenial::IncompleteProfile);
        }
        if !self.profile.allows(law) {
            return Err(RewriteDenial::LawDisabled);
        }
        if !law_holds(self, node, law) {
            return Err(RewriteDenial::PreconditionFailed);
        }
        Ok(RewriteCertificate {
            node,
            law,
            language: self.profile.language,
        })
    }
}

/// Successful proof that one named rewrite law is admissible at one IR node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct RewriteCertificate {
    node: ExpressionIrId,
    law: RewriteLaw,
    language: Language,
}

impl RewriteCertificate {
    /// Certified node.
    pub const fn node(self) -> ExpressionIrId {
        self.node
    }

    /// Certified law.
    pub const fn law(self) -> RewriteLaw {
        self.law
    }

    /// Profile language under which the law was checked.
    pub const fn language(self) -> Language {
        self.language
    }
}

/// Reason a rewrite certificate was denied.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RewriteDenial {
    /// Persistence cannot retain the live evidence required for authorization.
    #[error("persisted expression IR is identity-only and cannot authorize rewrites")]
    PersistedArtifact,
    /// Lowering was limited or malformed.
    #[error("lowering is not complete")]
    IncompleteLowering,
    /// The requested flat node does not exist.
    #[error("IR node does not exist")]
    UnknownNode,
    /// The node or a descendant is effectful, identity-only, or unknown.
    #[error("node lacks complete pure evidence")]
    NonPureEvidence,
    /// At least one profile dimension is unknown.
    #[error("semantic profile is incomplete")]
    IncompleteProfile,
    /// The profile disables this named law.
    #[error("semantic profile disables this rewrite law")]
    LawDisabled,
    /// Node-local type, shape, literal, overflow, or floating-point checks failed.
    #[error("node does not satisfy the named rewrite preconditions")]
    PreconditionFailed,
}

#[derive(Debug, Clone, Copy)]
enum LowerFrame {
    Enter(NodeId),
    Exit(NodeId),
}

/// Lowers typed CPG expression facts into a flat, stack-safe IR.
///
/// The CPG and fact table are borrowed and never mutated. Every incomplete
/// outcome returns a typed completion state whose artifact cannot mint a
/// [RewriteCertificate].
pub fn lower_expression_ir(
    cpg: &CodePropertyGraph,
    root: NodeId,
    facts: &ExpressionFactTable,
    profile: &SemanticProfile,
    limits: ExpressionIrLimits,
) -> ExpressionIr {
    let mut ir = ExpressionIr {
        nodes: Vec::new(),
        operands: Vec::new(),
        sources: BTreeMap::new(),
        root: None,
        profile: profile.clone(),
        completion: ExpressionIrCompletion::Complete,
        metrics: ExpressionIrMetrics::default(),
        provenance: ExpressionIrProvenance::LiveLowering,
    };
    if cpg.language() != facts.language() || cpg.language() != profile.language {
        ir.completion = ExpressionIrCompletion::Malformed(ExpressionFactError::LanguageMismatch {
            cpg: cpg.language(),
            facts: facts.language(),
            profile: profile.language,
        });
        return ir;
    }

    let reserve_nodes = facts.len().min(limits.max_nodes);
    if ir.nodes.try_reserve(reserve_nodes).is_err() {
        ir.completion = allocation_failure(ExpressionIrAllocation::NodeArena);
        return ir;
    }
    let mut pending = Vec::new();
    if pending.try_reserve(1).is_err() {
        ir.completion = allocation_failure(ExpressionIrAllocation::Worklist);
        return ir;
    }
    pending.push(LowerFrame::Enter(root));
    let mut active = BTreeSet::new();

    while let Some(frame) = pending.pop() {
        if ir.metrics.work >= limits.max_work {
            ir.completion = ExpressionIrCompletion::WorkLimited {
                actual: ir.metrics.work,
                maximum: limits.max_work,
            };
            return ir;
        }
        ir.metrics.work += 1;

        match frame {
            LowerFrame::Enter(source) => {
                if active.contains(&source) {
                    ir.completion =
                        ExpressionIrCompletion::Malformed(ExpressionFactError::Cycle(source));
                    return ir;
                }
                if ir.sources.contains_key(&source) {
                    continue;
                }
                let Some(fact) = facts.get(source) else {
                    ir.completion =
                        ExpressionIrCompletion::Malformed(ExpressionFactError::MissingFact(source));
                    return ir;
                };
                if let Err(error) = validate_fact(cpg, source, fact) {
                    ir.completion = ExpressionIrCompletion::Malformed(error);
                    return ir;
                }
                if let Err(error) = validate_operand_types(source, fact, facts) {
                    ir.completion = ExpressionIrCompletion::Malformed(error);
                    return ir;
                }
                let required = match fact.operands.len().checked_add(1) {
                    Some(required) => required,
                    None => {
                        ir.completion = ExpressionIrCompletion::Malformed(
                            ExpressionFactError::IdentifierOverflow,
                        );
                        return ir;
                    }
                };
                if pending.try_reserve(required).is_err() {
                    ir.completion = allocation_failure(ExpressionIrAllocation::Worklist);
                    return ir;
                }
                active.insert(source);
                pending.push(LowerFrame::Exit(source));
                pending.extend(fact.operands.iter().rev().copied().map(LowerFrame::Enter));
            }
            LowerFrame::Exit(source) => {
                let fact = facts
                    .get(source)
                    .expect("an exit frame is created only from a validated fact");
                if ir.nodes.len() >= limits.max_nodes {
                    ir.completion = ExpressionIrCompletion::NodeLimited {
                        actual: ir.nodes.len(),
                        maximum: limits.max_nodes,
                    };
                    return ir;
                }
                let next_operands = match ir.operands.len().checked_add(fact.operands.len()) {
                    Some(value) => value,
                    None => {
                        ir.completion = ExpressionIrCompletion::Malformed(
                            ExpressionFactError::IdentifierOverflow,
                        );
                        return ir;
                    }
                };
                if next_operands > limits.max_operands {
                    ir.completion = ExpressionIrCompletion::OperandLimited {
                        actual: next_operands,
                        maximum: limits.max_operands,
                    };
                    return ir;
                }
                let first_operand = match u32::try_from(ir.operands.len()) {
                    Ok(value) => value,
                    Err(_) => {
                        ir.completion = ExpressionIrCompletion::Malformed(
                            ExpressionFactError::IdentifierOverflow,
                        );
                        return ir;
                    }
                };
                let operand_count = match u32::try_from(fact.operands.len()) {
                    Ok(value) => value,
                    Err(_) => {
                        ir.completion = ExpressionIrCompletion::Malformed(
                            ExpressionFactError::IdentifierOverflow,
                        );
                        return ir;
                    }
                };
                if ir.operands.try_reserve(fact.operands.len()).is_err() {
                    ir.completion = allocation_failure(ExpressionIrAllocation::OperandTape);
                    return ir;
                }
                let mut child_unknown = false;
                let mut child_identity_only = false;
                for operand in &fact.operands {
                    let Some(id) = ir.sources.get(operand).copied() else {
                        ir.completion = ExpressionIrCompletion::Malformed(
                            ExpressionFactError::OperandTopologyMismatch(source),
                        );
                        return ir;
                    };
                    ir.operands.push(id);
                    match ir
                        .node(id)
                        .expect("mapped operands identify prior arena nodes")
                        .mode
                    {
                        ExpressionEvidenceMode::Unknown => child_unknown = true,
                        ExpressionEvidenceMode::IdentityOnly => child_identity_only = true,
                        ExpressionEvidenceMode::Pure => {}
                    }
                }
                let id = match u32::try_from(ir.nodes.len()) {
                    Ok(value) => ExpressionIrId(value),
                    Err(_) => {
                        ir.completion = ExpressionIrCompletion::Malformed(
                            ExpressionFactError::IdentifierOverflow,
                        );
                        return ir;
                    }
                };
                let mode = evidence_mode(profile, fact, child_unknown, child_identity_only);
                ir.nodes.push(ExpressionIrNode {
                    kind: fact.kind.clone(),
                    value_type: fact.value_type,
                    effects: fact.effects,
                    first_operand,
                    operand_count,
                    mode,
                });
                ir.sources.insert(source, id);
                active.remove(&source);
                ir.metrics.nodes = ir.nodes.len();
                ir.metrics.operands = ir.operands.len();
            }
        }
    }

    ir.root = ir.sources.get(&root).copied();
    if ir.root.is_none() {
        ir.completion = ExpressionIrCompletion::Malformed(ExpressionFactError::MissingFact(root));
    }
    ir
}

fn allocation_failure(context: ExpressionIrAllocation) -> ExpressionIrCompletion {
    ExpressionIrCompletion::Malformed(ExpressionFactError::AllocationFailed(context))
}

fn validate_fact(
    cpg: &CodePropertyGraph,
    source: NodeId,
    fact: &ExpressionFact,
) -> Result<(), ExpressionFactError> {
    let node = cpg
        .node(source)
        .ok_or(ExpressionFactError::MissingCpgNode(source))?;
    if !node.is_expression() {
        return Err(ExpressionFactError::NonExpressionSource(source));
    }
    if cpg.ast_children(source) != fact.operands.as_slice() {
        return Err(ExpressionFactError::OperandTopologyMismatch(source));
    }
    if !kind_matches(&node.kind, &fact.kind) {
        return Err(ExpressionFactError::KindMismatch(source));
    }
    if !valid_arity(&fact.kind, fact.operands.len()) {
        return Err(ExpressionFactError::InvalidArity(source));
    }
    if let ExpressionFactKind::Binding(identity) = &fact.kind {
        let CpgNodeKind::Identifier { definition, .. } = &node.kind else {
            return Err(ExpressionFactError::BindingMismatch(source));
        };
        if *definition != Some(identity.definition) {
            return Err(ExpressionFactError::BindingMismatch(source));
        }
    }
    if let ExpressionFactKind::Literal(literal) = &fact.kind {
        let CpgNodeKind::Literal { kind } = &node.kind else {
            return Err(ExpressionFactError::SourceLiteralMismatch(source));
        };
        if !source_literal_matches(kind, literal) {
            return Err(ExpressionFactError::SourceLiteralMismatch(source));
        }
    }
    if let ExpressionFactKind::Call(identity) = &fact.kind {
        let CpgNodeKind::Call { target, .. } = &node.kind else {
            return Err(ExpressionFactError::CallMismatch(source));
        };
        let matches = match identity {
            CallIdentity::Resolved(resolved) => *target == Some(*resolved),
            CallIdentity::External(_) | CallIdentity::Unresolved => target.is_none(),
        };
        if !matches {
            return Err(ExpressionFactError::CallMismatch(source));
        }
    }
    if let SemanticKnowledge::Known(value_type) = fact.value_type {
        if !known_fact_type_is_valid(&fact.kind, value_type) {
            return Err(match fact.kind {
                ExpressionFactKind::Literal(_) => ExpressionFactError::LiteralTypeMismatch(source),
                _ => ExpressionFactError::TypeMismatch(source),
            });
        }
    }
    Ok(())
}

fn source_literal_matches(source: &LiteralKind, fact: &ExpressionLiteral) -> bool {
    match (source, fact) {
        (LiteralKind::Integer(source), ExpressionLiteral::Integer(fact)) => {
            i128::from(*source) == *fact
        }
        (LiteralKind::Float(source), ExpressionLiteral::Float64(fact)) => source.to_bits() == *fact,
        (LiteralKind::Float(source), ExpressionLiteral::Float32(fact)) => {
            (*source as f32).to_bits() == *fact
        }
        (LiteralKind::Integer(source), ExpressionLiteral::Float64(fact)) => {
            (*source as f64).to_bits() == *fact
        }
        (LiteralKind::Integer(source), ExpressionLiteral::Float32(fact)) => {
            (*source as f32).to_bits() == *fact
        }
        (LiteralKind::String(source), ExpressionLiteral::String(fact)) => source == fact,
        (LiteralKind::Char(source), ExpressionLiteral::Char(fact)) => source == fact,
        (LiteralKind::Bool(source), ExpressionLiteral::Bool(fact)) => source == fact,
        (LiteralKind::Null, ExpressionLiteral::Null) => true,
        _ => false,
    }
}

fn kind_matches(cpg: &CpgNodeKind, fact: &ExpressionFactKind) -> bool {
    matches!(
        (cpg, fact),
        (CpgNodeKind::Literal { .. }, ExpressionFactKind::Literal(_))
            | (
                CpgNodeKind::Identifier { .. },
                ExpressionFactKind::Binding(_)
            )
            | (CpgNodeKind::UnaryOp { .. }, ExpressionFactKind::Unary(_))
            | (CpgNodeKind::BinaryOp { .. }, ExpressionFactKind::Binary(_))
            | (CpgNodeKind::Call { .. }, ExpressionFactKind::Call(_))
            | (
                CpgNodeKind::MemberAccess { .. } | CpgNodeKind::IndexAccess,
                ExpressionFactKind::Load { .. }
            )
            | (_, ExpressionFactKind::Opaque)
    )
}

fn valid_arity(kind: &ExpressionFactKind, arity: usize) -> bool {
    match kind {
        ExpressionFactKind::Literal(_) | ExpressionFactKind::Binding(_) => arity == 0,
        ExpressionFactKind::Unary(_) => arity == 1,
        ExpressionFactKind::Binary(_) => arity == 2,
        ExpressionFactKind::Call(_) | ExpressionFactKind::Opaque => true,
        ExpressionFactKind::Load { .. } => arity > 0,
    }
}

fn known_fact_type_is_valid(kind: &ExpressionFactKind, value_type: ExpressionType) -> bool {
    if !expression_type_is_valid(value_type) {
        return false;
    }
    match kind {
        ExpressionFactKind::Literal(literal) => literal_matches_type(literal, value_type),
        ExpressionFactKind::Unary(operator) => match operator {
            UnaryOperator::LogicalNot => value_type == ExpressionType::Bool,
            UnaryOperator::Negate => matches!(
                value_type,
                ExpressionType::Integer(IntegerType { signed: true, .. })
                    | ExpressionType::Float(_)
            ),
            UnaryOperator::BitwiseNot => matches!(value_type, ExpressionType::Integer(_)),
        },
        ExpressionFactKind::Binary(operator) => match operator {
            BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr => {
                value_type == ExpressionType::Bool
            }
            BinaryOperator::Equal
            | BinaryOperator::NotEqual
            | BinaryOperator::Less
            | BinaryOperator::LessEqual
            | BinaryOperator::Greater
            | BinaryOperator::GreaterEqual => value_type == ExpressionType::Bool,
            BinaryOperator::BitwiseAnd
            | BinaryOperator::BitwiseOr
            | BinaryOperator::BitwiseXor
            | BinaryOperator::ShiftLeft
            | BinaryOperator::ShiftRight => {
                matches!(value_type, ExpressionType::Integer(_))
            }
            BinaryOperator::Add
            | BinaryOperator::Subtract
            | BinaryOperator::Multiply
            | BinaryOperator::Divide
            | BinaryOperator::Remainder => matches!(
                value_type,
                ExpressionType::Integer(_) | ExpressionType::Float(_)
            ),
        },
        ExpressionFactKind::Binding(_)
        | ExpressionFactKind::Call(_)
        | ExpressionFactKind::Load { .. }
        | ExpressionFactKind::Opaque => true,
    }
}

fn expression_type_is_valid(value_type: ExpressionType) -> bool {
    match value_type {
        ExpressionType::Integer(integer) => integer.is_valid(),
        ExpressionType::Bool
        | ExpressionType::Float(_)
        | ExpressionType::String
        | ExpressionType::Char
        | ExpressionType::Unit
        | ExpressionType::Dynamic => true,
    }
}

fn validate_operand_types(
    source: NodeId,
    fact: &ExpressionFact,
    facts: &ExpressionFactTable,
) -> Result<(), ExpressionFactError> {
    let operand_types: Option<Vec<_>> = fact
        .operands
        .iter()
        .map(|operand| {
            facts
                .get(*operand)
                .and_then(|child| child.value_type.as_ref().copied())
        })
        .collect();
    let (Some(result_type), Some(operand_types)) =
        (fact.value_type.as_ref().copied(), operand_types)
    else {
        return Ok(());
    };
    let compatible = match fact.kind {
        ExpressionFactKind::Unary(UnaryOperator::LogicalNot) => {
            operand_types.as_slice() == [ExpressionType::Bool]
                && result_type == ExpressionType::Bool
        }
        ExpressionFactKind::Unary(UnaryOperator::Negate | UnaryOperator::BitwiseNot) => {
            operand_types.as_slice() == [result_type]
        }
        ExpressionFactKind::Binary(BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr) => {
            operand_types.as_slice() == [ExpressionType::Bool, ExpressionType::Bool]
                && result_type == ExpressionType::Bool
        }
        ExpressionFactKind::Binary(
            BinaryOperator::Equal
            | BinaryOperator::NotEqual
            | BinaryOperator::Less
            | BinaryOperator::LessEqual
            | BinaryOperator::Greater
            | BinaryOperator::GreaterEqual,
        ) => {
            operand_types.len() == 2
                && operand_types[0] == operand_types[1]
                && result_type == ExpressionType::Bool
        }
        ExpressionFactKind::Binary(_) => operand_types.as_slice() == [result_type, result_type],
        ExpressionFactKind::Literal(_)
        | ExpressionFactKind::Binding(_)
        | ExpressionFactKind::Call(_)
        | ExpressionFactKind::Load { .. }
        | ExpressionFactKind::Opaque => true,
    };
    compatible
        .then_some(())
        .ok_or(ExpressionFactError::TypeMismatch(source))
}

fn literal_matches_type(literal: &ExpressionLiteral, value_type: ExpressionType) -> bool {
    if !expression_type_is_valid(value_type) {
        return false;
    }
    match (literal, value_type) {
        (ExpressionLiteral::Integer(value), ExpressionType::Integer(integer)) => {
            integer_contains(integer, *value)
        }
        (ExpressionLiteral::Float32(bits), ExpressionType::Float(float_type))
            if float_type.width == FloatWidth::F32 =>
        {
            float_literal_is_valid(
                f32::from_bits(*bits) as f64,
                *bits == (-0.0_f32).to_bits(),
                float_type,
            )
        }
        (ExpressionLiteral::Float64(bits), ExpressionType::Float(float_type))
            if float_type.width == FloatWidth::F64 =>
        {
            float_literal_is_valid(
                f64::from_bits(*bits),
                *bits == (-0.0_f64).to_bits(),
                float_type,
            )
        }
        (ExpressionLiteral::Bool(_), ExpressionType::Bool)
        | (ExpressionLiteral::String(_), ExpressionType::String)
        | (ExpressionLiteral::Char(_), ExpressionType::Char)
        | (ExpressionLiteral::Null, ExpressionType::Dynamic) => true,
        _ => false,
    }
}

fn float_literal_is_valid(value: f64, is_negative_zero: bool, float_type: FloatType) -> bool {
    match float_type.mode {
        FloatMode::StrictIeee754 => true,
        FloatMode::FiniteReassociation => value.is_finite() && !is_negative_zero,
    }
}

fn integer_contains(integer: IntegerType, value: i128) -> bool {
    if !integer.is_valid() {
        return false;
    }
    let Some(width) = integer.width else {
        return integer.signed || value >= 0;
    };
    if integer.signed {
        if width == 128 {
            true
        } else {
            let shift = u32::from(width - 1);
            let bound = 1_i128 << shift;
            (-bound..bound).contains(&value)
        }
    } else if value < 0 {
        false
    } else if width >= 127 {
        true
    } else {
        value < (1_i128 << u32::from(width))
    }
}

fn evidence_mode(
    profile: &SemanticProfile,
    fact: &ExpressionFact,
    child_unknown: bool,
    child_identity_only: bool,
) -> ExpressionEvidenceMode {
    if !profile.is_complete()
        || !fact.value_type.is_known()
        || !fact.effects.is_known()
        || child_unknown
    {
        return ExpressionEvidenceMode::Unknown;
    }
    let effects = match fact.effects {
        SemanticKnowledge::Known(effects) => effects,
        SemanticKnowledge::Unknown => return ExpressionEvidenceMode::Unknown,
    };
    if !effects.is_empty() || child_identity_only || !kind_can_be_pure(&fact.kind) {
        ExpressionEvidenceMode::IdentityOnly
    } else {
        ExpressionEvidenceMode::Pure
    }
}

fn kind_can_be_pure(kind: &ExpressionFactKind) -> bool {
    match kind {
        ExpressionFactKind::Literal(_)
        | ExpressionFactKind::Binding(_)
        | ExpressionFactKind::Unary(_)
        | ExpressionFactKind::Binary(_) => true,
        ExpressionFactKind::Call(CallIdentity::Resolved(_)) => true,
        ExpressionFactKind::Load {
            access: MemoryAccess::Immutable,
            ..
        } => true,
        ExpressionFactKind::Call(CallIdentity::External(_) | CallIdentity::Unresolved)
        | ExpressionFactKind::Load { .. }
        | ExpressionFactKind::Opaque => false,
    }
}

fn law_holds(ir: &ExpressionIr, node: ExpressionIrId, law: RewriteLaw) -> bool {
    let Some(ir_node) = ir.node(node) else {
        return false;
    };
    let Some(operands) = ir.operands(node) else {
        return false;
    };
    match law {
        RewriteLaw::ConstantFold => constant_fold_is_defined(ir, ir_node, operands),
        RewriteLaw::Commutativity => {
            let ExpressionFactKind::Binary(operator) = &ir_node.kind else {
                return false;
            };
            operands.len() == 2 && commutative_for_type(*operator, ir_node.value_type)
        }
        RewriteLaw::Associativity => {
            let ExpressionFactKind::Binary(operator) = &ir_node.kind else {
                return false;
            };
            operands.len() == 2 && associative_for_type(*operator, ir_node.value_type)
        }
        RewriteLaw::AdditiveIdentity => {
            matches!(
                &ir_node.kind,
                ExpressionFactKind::Binary(BinaryOperator::Add)
            ) && matches!(
                ir_node.value_type,
                SemanticKnowledge::Known(ExpressionType::Integer(_))
            ) && operands
                .iter()
                .any(|operand| literal_integer(ir, *operand) == Some(0))
        }
        RewriteLaw::MultiplicativeIdentity => {
            matches!(
                &ir_node.kind,
                ExpressionFactKind::Binary(BinaryOperator::Multiply)
            ) && matches!(
                ir_node.value_type,
                SemanticKnowledge::Known(ExpressionType::Integer(_))
            ) && operands
                .iter()
                .any(|operand| literal_integer(ir, *operand) == Some(1))
        }
        RewriteLaw::BooleanIdentity => {
            let identity = match &ir_node.kind {
                ExpressionFactKind::Binary(BinaryOperator::LogicalAnd) => true,
                ExpressionFactKind::Binary(BinaryOperator::LogicalOr) => false,
                _ => return false,
            };
            ir_node.value_type == SemanticKnowledge::Known(ExpressionType::Bool)
                && operands
                    .iter()
                    .any(|operand| literal_bool(ir, *operand) == Some(identity))
        }
    }
}

fn commutative_for_type(
    operator: BinaryOperator,
    value_type: SemanticKnowledge<ExpressionType>,
) -> bool {
    match value_type {
        SemanticKnowledge::Known(ExpressionType::Integer(_)) => matches!(
            operator,
            BinaryOperator::Add
                | BinaryOperator::Multiply
                | BinaryOperator::BitwiseAnd
                | BinaryOperator::BitwiseOr
                | BinaryOperator::BitwiseXor
        ),
        SemanticKnowledge::Known(ExpressionType::Bool) => matches!(
            operator,
            BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr
        ),
        SemanticKnowledge::Known(ExpressionType::Float(FloatType {
            mode: FloatMode::FiniteReassociation,
            ..
        })) => matches!(operator, BinaryOperator::Add | BinaryOperator::Multiply),
        _ => false,
    }
}

fn associative_for_type(
    operator: BinaryOperator,
    value_type: SemanticKnowledge<ExpressionType>,
) -> bool {
    match value_type {
        SemanticKnowledge::Known(ExpressionType::Integer(integer)) => match operator {
            BinaryOperator::Add | BinaryOperator::Multiply => matches!(
                integer.overflow,
                IntegerOverflow::Wrapping | IntegerOverflow::ArbitraryPrecision
            ),
            BinaryOperator::BitwiseAnd | BinaryOperator::BitwiseOr | BinaryOperator::BitwiseXor => {
                true
            }
            _ => false,
        },
        SemanticKnowledge::Known(ExpressionType::Bool) => matches!(
            operator,
            BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr
        ),
        SemanticKnowledge::Known(ExpressionType::Float(FloatType {
            mode: FloatMode::FiniteReassociation,
            ..
        })) => matches!(operator, BinaryOperator::Add | BinaryOperator::Multiply),
        _ => false,
    }
}

fn constant_fold_is_defined(
    ir: &ExpressionIr,
    node: &ExpressionIrNode,
    operands: &[ExpressionIrId],
) -> bool {
    match (&node.kind, node.value_type) {
        (
            ExpressionFactKind::Unary(operator),
            SemanticKnowledge::Known(ExpressionType::Integer(integer)),
        ) if operands.len() == 1 => {
            let Some(value) = literal_integer(ir, operands[0]) else {
                return false;
            };
            match operator {
                UnaryOperator::Negate => match integer.overflow {
                    IntegerOverflow::Wrapping | IntegerOverflow::Saturating => true,
                    IntegerOverflow::Checked
                    | IntegerOverflow::Undefined
                    | IntegerOverflow::ArbitraryPrecision => value
                        .checked_neg()
                        .is_some_and(|result| integer_contains(integer, result)),
                },
                UnaryOperator::BitwiseNot => true,
                UnaryOperator::LogicalNot => false,
            }
        }
        (
            ExpressionFactKind::Unary(UnaryOperator::LogicalNot),
            SemanticKnowledge::Known(ExpressionType::Bool),
        ) if operands.len() == 1 => literal_bool(ir, operands[0]).is_some(),
        (
            ExpressionFactKind::Binary(operator),
            SemanticKnowledge::Known(ExpressionType::Integer(integer)),
        ) if operands.len() == 2 => {
            let (Some(left), Some(right)) = (
                literal_integer(ir, operands[0]),
                literal_integer(ir, operands[1]),
            ) else {
                return false;
            };
            integer_binary_is_defined(*operator, integer, left, right)
        }
        (ExpressionFactKind::Binary(operator), SemanticKnowledge::Known(ExpressionType::Bool))
            if operands.len() == 2 =>
        {
            let both_boolean =
                literal_bool(ir, operands[0]).is_some() && literal_bool(ir, operands[1]).is_some();
            both_boolean
                && matches!(
                    operator,
                    BinaryOperator::LogicalAnd
                        | BinaryOperator::LogicalOr
                        | BinaryOperator::Equal
                        | BinaryOperator::NotEqual
                )
        }
        _ => false,
    }
}

fn integer_binary_is_defined(
    operator: BinaryOperator,
    integer: IntegerType,
    left: i128,
    right: i128,
) -> bool {
    match operator {
        BinaryOperator::Add | BinaryOperator::Subtract | BinaryOperator::Multiply => {
            if matches!(
                integer.overflow,
                IntegerOverflow::Wrapping | IntegerOverflow::Saturating
            ) {
                return true;
            }
            let result = match operator {
                BinaryOperator::Add => left.checked_add(right),
                BinaryOperator::Subtract => left.checked_sub(right),
                BinaryOperator::Multiply => left.checked_mul(right),
                _ => unreachable!(),
            };
            result.is_some_and(|result| integer_contains(integer, result))
        }
        BinaryOperator::Divide | BinaryOperator::Remainder => {
            if right == 0 {
                return false;
            }
            left.checked_div(right)
                .is_some_and(|result| integer_contains(integer, result))
        }
        BinaryOperator::ShiftLeft | BinaryOperator::ShiftRight => {
            let Some(width) = integer.width else {
                return right >= 0 && u32::try_from(right).is_ok();
            };
            match integer.shift {
                ShiftBehavior::Masked => true,
                ShiftBehavior::Checked | ShiftBehavior::Undefined => {
                    right >= 0 && right < i128::from(width)
                }
            }
        }
        BinaryOperator::BitwiseAnd
        | BinaryOperator::BitwiseOr
        | BinaryOperator::BitwiseXor
        | BinaryOperator::Equal
        | BinaryOperator::NotEqual
        | BinaryOperator::Less
        | BinaryOperator::LessEqual
        | BinaryOperator::Greater
        | BinaryOperator::GreaterEqual => true,
        BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr => false,
    }
}

fn literal_integer(ir: &ExpressionIr, node: ExpressionIrId) -> Option<i128> {
    match &ir.node(node)?.kind {
        ExpressionFactKind::Literal(ExpressionLiteral::Integer(value)) => Some(*value),
        _ => None,
    }
}

fn literal_bool(ir: &ExpressionIr, node: ExpressionIrId) -> Option<bool> {
    match &ir.node(node)?.kind {
        ExpressionFactKind::Literal(ExpressionLiteral::Bool(value)) => Some(*value),
        _ => None,
    }
}

#[cfg(test)]
#[path = "expression_ir_tests.rs"]
mod tests;
