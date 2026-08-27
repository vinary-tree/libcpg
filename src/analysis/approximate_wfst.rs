//! Marker-typed finite weighted relations for approximate normalization.
//!
//! Weighted finite-state transducer (WFST) composition is delegated to
//! `lling-llang`. The marker types in this module only prevent incompatible
//! tape domains from composing; they do not reimplement category machinery.
//! Relations are finite acyclic unions of explicit paths, use the same tropical
//! min-plus semiring as approximate distance, and are observed extensionally.
//!
//! Only a [`CompleteFiniteObservation`] can be materialized into another
//! relation. A bounded or cancelled observation therefore cannot be
//! reassociated and accidentally presented as evidence for identity or
//! associativity.

use std::collections::{BTreeMap, BTreeSet};
use std::marker::PhantomData;
use std::mem::size_of;
use std::sync::Arc;

use lling_llang::composition::{compose, LazyComposition};
use lling_llang::semiring::{Semiring, TropicalWeight};
use lling_llang::wfst::{MutableWfst, VectorWfst};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use super::{ApproximateCancellation, ApproximateCompletion, ApproximateLimit, ApproximateLimits};

/// Largest integer exactly represented by the tropical `f64` carrier.
pub const MAX_EXACT_TROPICAL_INTEGER: u64 = 1_u64 << 53;

/// Marker for symbol and identifier tapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SymbolTapeDomain {}

/// Marker for API-usage sequence tapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ApiTapeDomain {}

/// Marker for structural function-trace tapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StructuralTapeDomain {}

/// Marker for normalized intermediate tapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NormalizedTapeDomain {}

/// One explicit weighted relation path.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct FiniteRewriteRule {
    /// Complete input tape.
    pub input: Arc<str>,
    /// Complete output tape.
    pub output: Arc<str>,
    /// Exact non-negative tropical path cost.
    pub cost: u64,
}

impl FiniteRewriteRule {
    /// Creates a finite rewrite path with an exact integer cost.
    pub fn new(input: impl Into<Arc<str>>, output: impl Into<Arc<str>>, cost: u64) -> Self {
        Self {
            input: input.into(),
            output: output.into(),
            cost,
        }
    }
}

/// Failure to construct or observe a finite weighted relation.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FiniteRelationError {
    /// A required resource limit was zero.
    #[error("finite-relation limit `{0}` must be non-zero")]
    ZeroLimit(&'static str),
    /// Construction exceeded a declared bound.
    #[error("finite-relation construction exceeded {0:?}")]
    Limit(ApproximateLimit),
    /// A rule cost was outside the exactly represented tropical integer domain.
    #[error("finite-relation rule {rule} has inexact tropical cost {cost}")]
    InexactCost {
        /// Zero-based rule index.
        rule: usize,
        /// Rejected integer cost.
        cost: u64,
    },
    /// A duallity rule used a fractional, negative, or non-finite cost.
    #[error("duallity rule {rule} has non-exact cost bits {cost_bits:#018x}")]
    InexactDuallityCost {
        /// Zero-based rule index.
        rule: usize,
        /// Rejected `f64` bit pattern.
        cost_bits: u64,
    },
    /// A state identifier could not be represented by lling-llang.
    #[error("finite relation needs {0} states, exceeding the WFST state domain")]
    StateDomain(u64),
    /// An observed upstream tropical weight was not an exact non-negative integer.
    #[error("composition produced non-exact tropical weight bits {0:#018x}")]
    InexactObservedCost(u64),
}

/// Finite acyclic relation from tape domain `From` to tape domain `To`.
///
/// Domain markers have no runtime representation. Rust accepts composition
/// only when the first relation's output marker equals the second relation's
/// input marker.
#[derive(Clone, Debug)]
pub struct TypedFiniteRelation<From, To> {
    wfst: VectorWfst<char, TropicalWeight>,
    rules: Vec<FiniteRewriteRule>,
    state_count: u64,
    max_path_units: usize,
    marker: PhantomData<fn(From) -> To>,
}

impl<From, To> TypedFiniteRelation<From, To> {
    /// Builds a finite acyclic union of complete rewrite paths.
    pub fn from_rules(
        rules: &[FiniteRewriteRule],
        limits: &ApproximateLimits,
    ) -> Result<Self, FiniteRelationError> {
        validate_relation_limits(limits)?;
        if rules.len() > limits.max_candidates {
            return Err(FiniteRelationError::Limit(ApproximateLimit::CandidateCount));
        }

        let mut state_count = 1_u64;
        let mut transition_count = 0_u64;
        let mut max_path_units = 0usize;
        let mut string_bytes = 0_u64;
        for (index, rule) in rules.iter().enumerate() {
            if rule.cost > MAX_EXACT_TROPICAL_INTEGER {
                return Err(FiniteRelationError::InexactCost {
                    rule: index,
                    cost: rule.cost,
                });
            }
            let input_units = rule.input.chars().count();
            let output_units = rule.output.chars().count();
            let path_units = input_units.max(output_units);
            if path_units > limits.max_input_units {
                return Err(FiniteRelationError::Limit(ApproximateLimit::InputUnits));
            }
            let steps = path_units.max(1);
            let steps_u64 =
                u64::try_from(steps).map_err(|_| FiniteRelationError::StateDomain(u64::MAX))?;
            state_count = state_count
                .checked_add(steps_u64)
                .ok_or(FiniteRelationError::StateDomain(u64::MAX))?;
            transition_count = transition_count
                .checked_add(steps_u64)
                .ok_or(FiniteRelationError::Limit(ApproximateLimit::TotalWork))?;
            max_path_units = max_path_units.max(path_units);
            let rule_bytes = rule
                .input
                .len()
                .checked_add(rule.output.len())
                .and_then(|bytes| bytes.checked_add(4 * size_of::<usize>()))
                .and_then(|bytes| u64::try_from(bytes).ok())
                .ok_or(FiniteRelationError::Limit(ApproximateLimit::Memory))?;
            string_bytes = string_bytes
                .checked_add(rule_bytes)
                .ok_or(FiniteRelationError::Limit(ApproximateLimit::Memory))?;
        }

        if state_count > limits.max_state_count {
            return Err(FiniteRelationError::Limit(ApproximateLimit::StateCount));
        }
        if transition_count > limits.max_work_units {
            return Err(FiniteRelationError::Limit(ApproximateLimit::TotalWork));
        }
        if state_count >= u64::from(u32::MAX) {
            return Err(FiniteRelationError::StateDomain(state_count));
        }
        let memory_bytes = state_count
            .checked_mul(256)
            .and_then(|bytes| {
                transition_count
                    .checked_mul(128)
                    .and_then(|arcs| bytes.checked_add(arcs))
            })
            .and_then(|bytes| bytes.checked_add(string_bytes))
            .ok_or(FiniteRelationError::Limit(ApproximateLimit::Memory))?;
        if memory_bytes > limits.max_memory_bytes {
            return Err(FiniteRelationError::Limit(ApproximateLimit::Memory));
        }

        let capacity = usize::try_from(state_count)
            .map_err(|_| FiniteRelationError::StateDomain(state_count))?;
        let mut wfst = VectorWfst::with_capacity(capacity);
        let start = wfst.add_state();
        wfst.set_start(start);
        for rule in rules {
            let inputs: Vec<_> = rule.input.chars().collect();
            let outputs: Vec<_> = rule.output.chars().collect();
            let steps = inputs.len().max(outputs.len()).max(1);
            let mut from = start;
            for step in 0..steps {
                let to = wfst.add_state();
                let weight = if step == 0 {
                    TropicalWeight::new(rule.cost as f64)
                } else {
                    TropicalWeight::one()
                };
                wfst.add_arc(
                    from,
                    inputs.get(step).copied(),
                    outputs.get(step).copied(),
                    to,
                    weight,
                );
                from = to;
            }
            wfst.set_final(from, TropicalWeight::one());
        }

        Ok(Self {
            wfst,
            rules: rules.to_vec(),
            state_count,
            max_path_units,
            marker: PhantomData,
        })
    }

    /// Bridges duallity rewrite rules into the marker-typed finite relation.
    ///
    /// Rule priority affects streaming enumeration, not the extensional
    /// tropical relation, so this conversion retains paths and costs while
    /// deliberately discarding priority. Costs must be exact non-negative
    /// integers to keep law checks independent of floating-point reassociation.
    pub fn from_duallity_rules(
        rules: &[duallity::RewriteRule],
        limits: &ApproximateLimits,
    ) -> Result<Self, FiniteRelationError> {
        let mut finite = Vec::with_capacity(rules.len());
        for (index, rule) in rules.iter().enumerate() {
            if !rule.cost.is_finite()
                || rule.cost.is_sign_negative()
                || rule.cost.fract() != 0.0
                || rule.cost > MAX_EXACT_TROPICAL_INTEGER as f64
            {
                return Err(FiniteRelationError::InexactDuallityCost {
                    rule: index,
                    cost_bits: rule.cost.to_bits(),
                });
            }
            finite.push(FiniteRewriteRule::new(
                rule.input.as_str(),
                rule.output.as_str(),
                rule.cost as u64,
            ));
        }
        Self::from_rules(&finite, limits)
    }

    /// Number of explicit paths in the relation presentation.
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }

    /// Number of eagerly materialized WFST states.
    pub fn state_count(&self) -> u64 {
        self.state_count
    }

    /// Composes two compatible finite relations through lling-llang.
    pub fn compose<Next>(
        &self,
        next: &TypedFiniteRelation<To, Next>,
        limits: &ApproximateLimits,
    ) -> Result<TypedFiniteComposition<From, To, Next>, FiniteRelationError> {
        validate_relation_limits(limits)?;
        let path_upper_bound = u64::try_from(self.rule_count())
            .ok()
            .and_then(|left| {
                u64::try_from(next.rule_count())
                    .ok()
                    .and_then(|right| left.checked_mul(right))
            })
            .ok_or(FiniteRelationError::Limit(ApproximateLimit::TotalWork))?;
        let state_upper_bound = self
            .state_count
            .checked_mul(next.state_count)
            .and_then(|states| states.checked_mul(3))
            .ok_or(FiniteRelationError::Limit(ApproximateLimit::StateCount))?;
        let max_path_units = self.max_path_units.max(next.max_path_units);
        let path_payload_bytes = u64::try_from(max_path_units)
            .ok()
            .and_then(|units| units.checked_mul(8))
            .and_then(|bytes| bytes.checked_add(256))
            .and_then(|bytes| path_upper_bound.checked_mul(bytes))
            .ok_or(FiniteRelationError::Limit(ApproximateLimit::Memory))?;
        let memory_upper_bound = state_upper_bound
            .checked_mul(512)
            .and_then(|bytes| bytes.checked_add(path_payload_bytes))
            .ok_or(FiniteRelationError::Limit(ApproximateLimit::Memory))?;

        let mut limitations = BTreeSet::new();
        if state_upper_bound > limits.max_state_count {
            limitations.insert(ApproximateLimit::StateCount);
        }
        if path_upper_bound > limits.max_work_units {
            limitations.insert(ApproximateLimit::TotalWork);
        }
        if path_upper_bound > u64::try_from(limits.max_candidates).unwrap_or(u64::MAX) {
            limitations.insert(ApproximateLimit::CandidateCount);
        }
        if memory_upper_bound > limits.max_memory_bytes {
            limitations.insert(ApproximateLimit::Memory);
        }
        if max_path_units > limits.max_input_units {
            limitations.insert(ApproximateLimit::InputUnits);
        }

        Ok(TypedFiniteComposition {
            inner: compose(self.wfst.clone(), next.wfst.clone()),
            path_upper_bound,
            state_upper_bound,
            memory_upper_bound,
            max_path_units,
            preflight: completion(limitations),
            marker: PhantomData,
        })
    }
}

impl<Domain> TypedFiniteRelation<Domain, Domain> {
    /// Builds the identity relation over a finite set of complete tapes.
    pub fn identity<I, S>(tapes: I, limits: &ApproximateLimits) -> Result<Self, FiniteRelationError>
    where
        I: IntoIterator<Item = S>,
        S: Into<Arc<str>>,
    {
        let unique: BTreeSet<Arc<str>> = tapes.into_iter().map(Into::into).collect();
        let rules: Vec<_> = unique
            .into_iter()
            .map(|tape| FiniteRewriteRule::new(Arc::clone(&tape), tape, 0))
            .collect();
        Self::from_rules(&rules, limits)
    }
}

/// Resource counters for one finite composition observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct FiniteCompositionCounters {
    /// Conservative reachable product-state upper bound.
    pub state_upper_bound: u64,
    /// Conservative accepting-path upper bound.
    pub path_upper_bound: u64,
    /// Conservative memory upper bound in bytes.
    pub memory_upper_bound: u64,
    /// Longest admitted component path in Unicode scalars.
    pub max_path_units: usize,
    /// Raw accepting paths actually observed.
    pub observed_paths: u64,
}

/// One extensional input/output pair with its minimum tropical cost.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct FiniteRelationPath {
    /// Complete input tape.
    pub input: Arc<str>,
    /// Complete output tape.
    pub output: Arc<str>,
    /// Minimum exact tropical cost for this pair.
    pub cost: u64,
}

/// Bounded extensional observation of a composed relation.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct FiniteRelationObservation<From, To> {
    /// Canonically ordered, minimum-cost extensional paths.
    pub paths: Vec<FiniteRelationPath>,
    /// Whether the complete relation was observed.
    pub completion: ApproximateCompletion,
    /// Composition resource counters.
    pub counters: FiniteCompositionCounters,
    #[cfg_attr(feature = "serde", serde(skip))]
    marker: PhantomData<fn(From) -> To>,
}

impl<From, To> FiniteRelationObservation<From, To> {
    /// Converts this value into law-checkable evidence only when complete.
    pub fn into_complete(self) -> Result<CompleteFiniteObservation<From, To>, Self> {
        if self.completion == ApproximateCompletion::Complete {
            Ok(CompleteFiniteObservation(self))
        } else {
            Err(self)
        }
    }
}

/// Complete extensional observation eligible for category-law comparison.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CompleteFiniteObservation<From, To>(FiniteRelationObservation<From, To>);

impl<From, To> CompleteFiniteObservation<From, To> {
    /// Canonically ordered extensional paths.
    pub fn paths(&self) -> &[FiniteRelationPath] {
        &self.0.paths
    }

    /// Materializes the complete observation as another finite relation.
    pub fn materialize(
        &self,
        limits: &ApproximateLimits,
    ) -> Result<TypedFiniteRelation<From, To>, FiniteRelationError> {
        let rules: Vec<_> = self
            .paths()
            .iter()
            .map(|path| {
                FiniteRewriteRule::new(Arc::clone(&path.input), Arc::clone(&path.output), path.cost)
            })
            .collect();
        TypedFiniteRelation::from_rules(&rules, limits)
    }
}

/// Lazy lling-llang composition with marker-checked intermediate tapes.
pub struct TypedFiniteComposition<From, Through, To> {
    inner: LazyComposition<
        VectorWfst<char, TropicalWeight>,
        VectorWfst<char, TropicalWeight>,
        char,
        TropicalWeight,
    >,
    path_upper_bound: u64,
    state_upper_bound: u64,
    memory_upper_bound: u64,
    max_path_units: usize,
    preflight: ApproximateCompletion,
    marker: PhantomData<fn(From) -> (Through, To)>,
}

impl<From, Through, To> TypedFiniteComposition<From, Through, To> {
    /// Observes the complete extensional relation when preflight bounds permit.
    ///
    /// The component relations are disjoint unions of linear acyclic paths.
    /// Their product and accepting-path frontiers are therefore bounded before
    /// lling-llang's iterative accepting-path iterator is entered.
    pub fn observe(
        mut self,
        cancellation: &ApproximateCancellation,
    ) -> Result<FiniteRelationObservation<From, To>, FiniteRelationError> {
        let mut limitations = match &self.preflight {
            ApproximateCompletion::Complete => BTreeSet::new(),
            ApproximateCompletion::Incomplete(limits) => limits.iter().copied().collect(),
        };
        let mut observed_paths = 0_u64;
        let mut extensional = BTreeMap::<(String, String), u64>::new();
        if limitations.is_empty() {
            let paths = self.inner.accepting_paths();
            for path in paths {
                if cancellation.is_cancelled() {
                    limitations.insert(ApproximateLimit::Cancelled);
                    break;
                }
                observed_paths = observed_paths
                    .checked_add(1)
                    .ok_or(FiniteRelationError::Limit(ApproximateLimit::TotalWork))?;
                if observed_paths > self.path_upper_bound {
                    limitations.insert(ApproximateLimit::TotalWork);
                    break;
                }
                let cost = exact_observed_cost(path.weight)?;
                let input: String = path.inputs.into_iter().collect();
                let output: String = path.outputs.into_iter().collect();
                extensional
                    .entry((input, output))
                    .and_modify(|current| *current = (*current).min(cost))
                    .or_insert(cost);
            }
        }
        let paths = extensional
            .into_iter()
            .map(|((input, output), cost)| FiniteRelationPath {
                input: Arc::from(input),
                output: Arc::from(output),
                cost,
            })
            .collect();
        Ok(FiniteRelationObservation {
            paths,
            completion: completion(limitations),
            counters: FiniteCompositionCounters {
                state_upper_bound: self.state_upper_bound,
                path_upper_bound: self.path_upper_bound,
                memory_upper_bound: self.memory_upper_bound,
                max_path_units: self.max_path_units,
                observed_paths,
            },
            marker: PhantomData,
        })
    }
}

fn exact_observed_cost(weight: TropicalWeight) -> Result<u64, FiniteRelationError> {
    let value = weight.value();
    if !value.is_finite()
        || value.is_sign_negative()
        || value.fract() != 0.0
        || value > MAX_EXACT_TROPICAL_INTEGER as f64
    {
        return Err(FiniteRelationError::InexactObservedCost(value.to_bits()));
    }
    Ok(value as u64)
}

fn validate_relation_limits(limits: &ApproximateLimits) -> Result<(), FiniteRelationError> {
    for (name, value) in [
        ("max_candidates", limits.max_candidates),
        ("max_input_units", limits.max_input_units),
    ] {
        if value == 0 {
            return Err(FiniteRelationError::ZeroLimit(name));
        }
    }
    for (name, value) in [
        ("max_work_units", limits.max_work_units),
        ("max_state_count", limits.max_state_count),
        ("max_memory_bytes", limits.max_memory_bytes),
    ] {
        if value == 0 {
            return Err(FiniteRelationError::ZeroLimit(name));
        }
    }
    Ok(())
}

fn completion(limitations: BTreeSet<ApproximateLimit>) -> ApproximateCompletion {
    if limitations.is_empty() {
        ApproximateCompletion::Complete
    } else {
        ApproximateCompletion::Incomplete(limitations.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[derive(Debug)]
    enum A {}
    #[derive(Debug)]
    enum B {}
    #[derive(Debug)]
    enum C {}
    #[derive(Debug)]
    enum D {}

    fn rule(input: &str, output: &str, cost: u64) -> FiniteRewriteRule {
        FiniteRewriteRule::new(input, output, cost)
    }

    #[test]
    fn typed_composition_delegates_to_lling_and_adds_tropical_costs() {
        let limits = ApproximateLimits::default();
        let left = TypedFiniteRelation::<A, B>::from_rules(&[rule("a", "b", 2)], &limits).unwrap();
        let right = TypedFiniteRelation::<B, C>::from_rules(&[rule("b", "c", 3)], &limits).unwrap();
        let observed = left
            .compose(&right, &limits)
            .unwrap()
            .observe(&ApproximateCancellation::default())
            .unwrap()
            .into_complete()
            .unwrap();

        assert_eq!(
            observed.paths(),
            &[FiniteRelationPath {
                input: Arc::from("a"),
                output: Arc::from("c"),
                cost: 5,
            }]
        );
    }

    #[test]
    fn complete_extensional_observations_satisfy_left_and_right_identity() {
        let limits = ApproximateLimits::default();
        let relation = TypedFiniteRelation::<A, B>::from_rules(
            &[rule("a", "x", 1), rule("b", "y", 2)],
            &limits,
        )
        .unwrap();
        let left_identity = TypedFiniteRelation::<A, A>::identity(["a", "b"], &limits).unwrap();
        let right_identity = TypedFiniteRelation::<B, B>::identity(["x", "y"], &limits).unwrap();

        let left = left_identity
            .compose(&relation, &limits)
            .unwrap()
            .observe(&ApproximateCancellation::default())
            .unwrap()
            .into_complete()
            .unwrap();
        let right = relation
            .compose(&right_identity, &limits)
            .unwrap()
            .observe(&ApproximateCancellation::default())
            .unwrap()
            .into_complete()
            .unwrap();
        assert_eq!(left.paths(), right.paths());
    }

    #[test]
    fn complete_materialization_supports_extensional_associativity() {
        let limits = ApproximateLimits::default();
        let ab = TypedFiniteRelation::<A, B>::from_rules(&[rule("a", "b", 1)], &limits).unwrap();
        let bc = TypedFiniteRelation::<B, C>::from_rules(&[rule("b", "c", 2)], &limits).unwrap();
        let cd = TypedFiniteRelation::<C, D>::from_rules(&[rule("c", "d", 4)], &limits).unwrap();

        let ab_bc = ab
            .compose(&bc, &limits)
            .unwrap()
            .observe(&ApproximateCancellation::default())
            .unwrap()
            .into_complete()
            .unwrap()
            .materialize(&limits)
            .unwrap();
        let left = ab_bc
            .compose(&cd, &limits)
            .unwrap()
            .observe(&ApproximateCancellation::default())
            .unwrap()
            .into_complete()
            .unwrap();

        let bc_cd = bc
            .compose(&cd, &limits)
            .unwrap()
            .observe(&ApproximateCancellation::default())
            .unwrap()
            .into_complete()
            .unwrap()
            .materialize(&limits)
            .unwrap();
        let right = ab
            .compose(&bc_cd, &limits)
            .unwrap()
            .observe(&ApproximateCancellation::default())
            .unwrap()
            .into_complete()
            .unwrap();
        assert_eq!(left.paths(), right.paths());
    }

    #[test]
    fn incomplete_observation_cannot_be_materialized() {
        let build_limits = ApproximateLimits::default();
        let relation = TypedFiniteRelation::<A, B>::from_rules(
            &[rule("a", "x", 0), rule("b", "y", 0)],
            &build_limits,
        )
        .unwrap();
        let identity = TypedFiniteRelation::<B, B>::identity(["x", "y"], &build_limits).unwrap();
        let observe_limits = ApproximateLimits {
            max_candidates: 1,
            ..build_limits
        };
        let observed = relation
            .compose(&identity, &observe_limits)
            .unwrap()
            .observe(&ApproximateCancellation::default())
            .unwrap();

        assert!(observed.into_complete().is_err());
    }

    #[test]
    fn duallity_rules_bridge_into_the_typed_relation() {
        let rules = [duallity::RewriteRule::with_cost("ph", "f", 1.0).unwrap()];
        let relation =
            TypedFiniteRelation::<A, B>::from_duallity_rules(&rules, &ApproximateLimits::default())
                .unwrap();
        assert_eq!(relation.rule_count(), 1);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn complete_extensional_observation_round_trips_without_marker_state() {
        let limits = ApproximateLimits::default();
        let left = TypedFiniteRelation::<A, B>::from_rules(&[rule("a", "b", 2)], &limits).unwrap();
        let right = TypedFiniteRelation::<B, C>::from_rules(&[rule("b", "c", 3)], &limits).unwrap();
        let observed = left
            .compose(&right, &limits)
            .unwrap()
            .observe(&ApproximateCancellation::default())
            .unwrap();

        let encoded = serde_json::to_vec(&observed).unwrap();
        let decoded: FiniteRelationObservation<A, C> = serde_json::from_slice(&encoded).unwrap();

        assert_eq!(decoded.paths, observed.paths);
        assert_eq!(decoded.completion, observed.completion);
        assert_eq!(decoded.counters, observed.counters);
        assert!(!String::from_utf8(encoded).unwrap().contains("marker"));
    }

    proptest! {
        #[test]
        fn composed_integer_cost_is_exact_sum(left in 0_u16..1000, right in 0_u16..1000) {
            let limits = ApproximateLimits::default();
            let ab = TypedFiniteRelation::<A, B>::from_rules(
                &[rule("source", "middle", u64::from(left))],
                &limits,
            ).unwrap();
            let bc = TypedFiniteRelation::<B, C>::from_rules(
                &[rule("middle", "target", u64::from(right))],
                &limits,
            ).unwrap();
            let observed = ab.compose(&bc, &limits).unwrap()
                .observe(&ApproximateCancellation::default()).unwrap()
                .into_complete().unwrap();
            prop_assert_eq!(observed.paths()[0].cost, u64::from(left) + u64::from(right));
        }
    }
}
