//! Deterministic, bounded advisory typestate analysis.
//!
//! A [`PropertyFsm`] describes a finite-state property over call events. The
//! analysis seeds one abstract object per allocation call, follows that object
//! through typed DFG/reference evidence, and propagates `(object, state)` facts
//! over the canonical per-function CFG. All traversal state lives in ordered
//! heap collections; CPG depth is never mapped to the native call stack.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::builder::call_arguments;
use crate::{CfgEdgeKind, CodePropertyGraph, CpgEdgeKind, CpgNodeKind, DfgEdgeKind, NodeId};

use super::cfg_scope::function_cfg_view;
use super::resolve::call_callee_path_evidence;
use super::{function_scope_nodes, IfdsCaps};

/// Stable name of one finite property state.
pub type PropertyState = Arc<str>;

/// Stable name of one finite property event.
pub type PropertyEvent = Arc<str>;

#[cfg(feature = "serde")]
mod ordered_map {
    use std::collections::BTreeMap;

    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<K, V, S>(map: &BTreeMap<K, V>, serializer: S) -> Result<S::Ok, S::Error>
    where
        K: Ord + Serialize,
        V: Serialize,
        S: Serializer,
    {
        map.iter().collect::<Vec<_>>().serialize(serializer)
    }

    pub fn deserialize<'de, K, V, D>(deserializer: D) -> Result<BTreeMap<K, V>, D::Error>
    where
        K: Ord + Deserialize<'de>,
        V: Deserialize<'de>,
        D: Deserializer<'de>,
    {
        let entries = Vec::<(K, V)>::deserialize(deserializer)?;
        let mut map = BTreeMap::new();
        for (key, value) in entries {
            if map.insert(key, value).is_some() {
                return Err(serde::de::Error::custom("duplicate ordered-map key"));
            }
        }
        Ok(map)
    }
}

/// A deterministic finite-state property automaton.
///
/// `allocations[event] = state` declares that a call classified as `event`
/// creates a fresh abstract object at that call site in `state`. Ordinary
/// transitions use `(from, event) -> to`; a missing transition is forbidden
/// and enters the absorbing [`error`](Self::error) state. `accepting` is
/// retained in terminal summaries for clients such as leak detectors; this
/// module itself reports forbidden transitions, not non-accepting exits.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct PropertyFsm {
    /// Closed state vocabulary, including [`error`](Self::error).
    pub states: BTreeSet<PropertyState>,
    /// Closed event vocabulary.
    pub alphabet: BTreeSet<PropertyEvent>,
    /// Deterministic ordinary transition relation.
    #[cfg_attr(feature = "serde", serde(with = "ordered_map"))]
    pub transitions: BTreeMap<(PropertyState, PropertyEvent), PropertyState>,
    /// States that represent acceptable object state at a function exit.
    pub accepting: BTreeSet<PropertyState>,
    /// Absorbing state entered by every forbidden transition.
    pub error: PropertyState,
    /// Allocation event to post-allocation state.
    #[cfg_attr(feature = "serde", serde(with = "ordered_map"))]
    pub allocations: BTreeMap<PropertyEvent, PropertyState>,
}

impl PropertyFsm {
    /// Creates an empty property automaton with the named absorbing error state.
    pub fn new(error: impl Into<PropertyState>) -> Self {
        let error = error.into();
        Self {
            states: BTreeSet::from([Arc::clone(&error)]),
            alphabet: BTreeSet::new(),
            transitions: BTreeMap::new(),
            accepting: BTreeSet::new(),
            error,
            allocations: BTreeMap::new(),
        }
    }

    /// Adds a state to the closed vocabulary.
    pub fn with_state(mut self, state: impl Into<PropertyState>) -> Self {
        self.states.insert(state.into());
        self
    }

    /// Adds an event to the closed vocabulary.
    pub fn with_event(mut self, event: impl Into<PropertyEvent>) -> Self {
        self.alphabet.insert(event.into());
        self
    }

    /// Adds a deterministic ordinary transition and its vocabulary entries.
    pub fn with_transition(
        mut self,
        from: impl Into<PropertyState>,
        event: impl Into<PropertyEvent>,
        to: impl Into<PropertyState>,
    ) -> Self {
        let from = from.into();
        let event = event.into();
        let to = to.into();
        self.states.insert(Arc::clone(&from));
        self.states.insert(Arc::clone(&to));
        self.alphabet.insert(Arc::clone(&event));
        self.transitions.insert((from, event), to);
        self
    }

    /// Declares an allocation event and its post-allocation state.
    pub fn with_allocation(
        mut self,
        event: impl Into<PropertyEvent>,
        state: impl Into<PropertyState>,
    ) -> Self {
        let event = event.into();
        let state = state.into();
        self.alphabet.insert(Arc::clone(&event));
        self.states.insert(Arc::clone(&state));
        self.allocations.insert(event, state);
        self
    }

    /// Marks a state as acceptable at function exit.
    pub fn with_accepting(mut self, state: impl Into<PropertyState>) -> Self {
        let state = state.into();
        self.states.insert(Arc::clone(&state));
        self.accepting.insert(state);
        self
    }

    /// Returns every structural problem in deterministic order.
    pub fn validate(&self) -> BTreeSet<PropertyFsmIssue> {
        let mut issues = BTreeSet::new();
        for state in &self.states {
            if state.is_empty() {
                issues.insert(PropertyFsmIssue::EmptyState);
            }
        }
        for event in &self.alphabet {
            if event.is_empty() {
                issues.insert(PropertyFsmIssue::EmptyEvent);
            }
        }
        if !self.states.contains(&self.error) {
            issues.insert(PropertyFsmIssue::UnknownErrorState {
                state: Arc::clone(&self.error),
            });
        }
        if self.accepting.contains(&self.error) {
            issues.insert(PropertyFsmIssue::AcceptingErrorState);
        }
        if self.allocations.is_empty() {
            issues.insert(PropertyFsmIssue::NoAllocationEvent);
        }
        for (event, state) in &self.allocations {
            if !self.alphabet.contains(event) {
                issues.insert(PropertyFsmIssue::UnknownAllocationEvent {
                    event: Arc::clone(event),
                });
            }
            if !self.states.contains(state) {
                issues.insert(PropertyFsmIssue::UnknownAllocationState {
                    event: Arc::clone(event),
                    state: Arc::clone(state),
                });
            }
            if state == &self.error {
                issues.insert(PropertyFsmIssue::AllocationEntersError {
                    event: Arc::clone(event),
                });
            }
        }
        for ((from, event), to) in &self.transitions {
            if !self.states.contains(from) {
                issues.insert(PropertyFsmIssue::UnknownTransitionState {
                    state: Arc::clone(from),
                });
            }
            if !self.alphabet.contains(event) {
                issues.insert(PropertyFsmIssue::UnknownTransitionEvent {
                    event: Arc::clone(event),
                });
            }
            if !self.states.contains(to) {
                issues.insert(PropertyFsmIssue::UnknownTransitionState {
                    state: Arc::clone(to),
                });
            }
            if self.allocations.contains_key(event) {
                issues.insert(PropertyFsmIssue::AllocationHasOrdinaryTransition {
                    event: Arc::clone(event),
                });
            }
            if from == &self.error && to != &self.error {
                issues.insert(PropertyFsmIssue::NonAbsorbingErrorTransition {
                    event: Arc::clone(event),
                    to: Arc::clone(to),
                });
            }
        }
        for state in &self.accepting {
            if !self.states.contains(state) {
                issues.insert(PropertyFsmIssue::UnknownAcceptingState {
                    state: Arc::clone(state),
                });
            }
        }
        issues
    }

    fn event_for_path(&self, path: &str) -> Option<PropertyEvent> {
        if let Some(event) = self.alphabet.iter().find(|event| event.as_ref() == path) {
            return Some(Arc::clone(event));
        }
        let terminal = path.rsplit("::").next().unwrap_or(path);
        self.alphabet
            .iter()
            .find(|event| event.as_ref() == terminal)
            .cloned()
    }
}

/// Structural invalidity in a [`PropertyFsm`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum PropertyFsmIssue {
    /// At least one state name is empty.
    EmptyState,
    /// At least one event name is empty.
    EmptyEvent,
    /// The designated error state is absent from `states`.
    UnknownErrorState {
        /// Missing state.
        state: PropertyState,
    },
    /// The error state was also marked accepting.
    AcceptingErrorState,
    /// No event creates an abstract object.
    NoAllocationEvent,
    /// An allocation key is absent from `alphabet`.
    UnknownAllocationEvent {
        /// Invalid event.
        event: PropertyEvent,
    },
    /// An allocation target is absent from `states`.
    UnknownAllocationState {
        /// Allocation event.
        event: PropertyEvent,
        /// Invalid state.
        state: PropertyState,
    },
    /// An allocation immediately enters the error state.
    AllocationEntersError {
        /// Invalid allocation event.
        event: PropertyEvent,
    },
    /// A transition source or target is absent from `states`.
    UnknownTransitionState {
        /// Invalid state.
        state: PropertyState,
    },
    /// A transition label is absent from `alphabet`.
    UnknownTransitionEvent {
        /// Invalid event.
        event: PropertyEvent,
    },
    /// Allocation and ordinary transition semantics were assigned to one event.
    AllocationHasOrdinaryTransition {
        /// Ambiguous event.
        event: PropertyEvent,
    },
    /// A transition leaves the designated absorbing error state.
    NonAbsorbingErrorTransition {
        /// Event that would leave error.
        event: PropertyEvent,
        /// Non-error target.
        to: PropertyState,
    },
    /// An accepting state is absent from `states`.
    UnknownAcceptingState {
        /// Invalid state.
        state: PropertyState,
    },
}

/// Resource boundary that made a typestate report partial.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum TypestateCapEvent {
    /// More represented functions existed than `IfdsCaps::max_functions`.
    Functions {
        /// Functions observed.
        observed: usize,
        /// Configured cap.
        cap: usize,
    },
    /// Object-flow or CFG propagation exhausted the shared work budget.
    WorklistSteps {
        /// First step that could not be performed.
        needed: usize,
        /// Configured cap.
        cap: usize,
    },
    /// One node would retain too many distinct object/state facts.
    FactsPerNode {
        /// Owning function.
        function: NodeId,
        /// Affected program or data-flow point.
        node: NodeId,
        /// First unretained fact count.
        observed: usize,
        /// Configured cap.
        cap: usize,
    },
    /// Typed callee-path reconstruction exhausted `IfdsCaps::max_call_depth`.
    CallDepth {
        /// Call site whose event could not be classified completely.
        site: NodeId,
        /// Configured depth cap.
        cap: u32,
    },
}

/// Structural evidence that prevents a complete typestate claim.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum TypestateIssue {
    /// The supplied finite-state property is structurally invalid.
    Fsm(PropertyFsmIssue),
    /// A call that could affect the property had no typed target spelling.
    UnresolvedEvent {
        /// Unresolved call site.
        site: NodeId,
    },
    /// Conflicting represented targets did not agree on event classification.
    AmbiguousEvent {
        /// Ambiguous call site.
        site: NodeId,
    },
    /// An event-bearing function had no CFG relation beyond its entry node.
    MissingControlFlow {
        /// Function lacking usable control-flow evidence.
        function: NodeId,
    },
    /// No CFG program point contained an event site.
    MissingProgramPoint {
        /// Unplaced call site.
        site: NodeId,
    },
    /// A malformed AST assigned more than one parent to a node.
    AmbiguousAstParent {
        /// Multiply-parented node.
        node: NodeId,
    },
    /// The canonical per-function CFG view could not be constructed.
    InvalidFunctionCfg {
        /// Affected function.
        function: NodeId,
    },
}

/// One advisory forbidden-transition candidate with ordered witness evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct TypestateViolation {
    /// Abstract object identity: the allocation call site.
    pub object: NodeId,
    /// Call site that attempted the forbidden event.
    pub site: NodeId,
    /// State immediately before the event.
    pub from: PropertyState,
    /// Forbidden event.
    pub event: PropertyEvent,
    /// Allocation/event/CFG evidence in canonical execution order.
    pub witness: Vec<NodeId>,
    /// Whether a resource boundary made the overall report partial.
    pub capped: bool,
}

/// One possible object state at one concrete function exit.
///
/// Unlike [`TypestateAnalysis::terminal_states`], this evidence retains the
/// exit program point and one canonical allocation-to-exit witness. Consumers
/// such as resource-leak checkers can therefore report the path on which a
/// required terminal transition is absent without re-running CFG traversal.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct TypestateTerminal {
    /// Abstract object identity: the allocation call site.
    pub object: NodeId,
    /// Exit-like CFG program point reached by the object.
    pub exit: NodeId,
    /// Object state after all events at `exit` have been applied.
    pub state: PropertyState,
    /// Allocation/event/CFG evidence in canonical execution order.
    pub witness: Vec<NodeId>,
    /// Whether a resource boundary made the overall report partial.
    pub capped: bool,
}

/// Typestate findings, terminal states, and completeness evidence.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct TypestateAnalysis {
    /// Forbidden-transition candidates in canonical key order.
    pub violations: Vec<TypestateViolation>,
    /// Possible post-transfer states at function exits, keyed by allocation site.
    pub terminal_states: BTreeMap<NodeId, BTreeSet<PropertyState>>,
    /// Exit-specific terminal evidence in canonical object/exit/state order.
    pub terminals: Vec<TypestateTerminal>,
    /// Functions containing at least one analyzed allocation, in stable order.
    pub processed_functions: Vec<NodeId>,
    /// Allocation sites admitted by the analysis.
    pub processed_objects: BTreeSet<NodeId>,
    /// Object-flow and CFG worklist steps consumed.
    pub worklist_steps: usize,
    /// Whether an explicit resource cap stopped complete evaluation.
    pub capped: bool,
    /// Ordered resource-cap evidence.
    pub cap_events: BTreeSet<TypestateCapEvent>,
    /// Ordered structural/FSM issues.
    pub issues: BTreeSet<TypestateIssue>,
}

impl TypestateAnalysis {
    /// Returns whether the analysis examined all represented evidence.
    pub fn is_complete(&self) -> bool {
        !self.capped && self.issues.is_empty()
    }

    /// Returns whether every terminal state for `object` is accepting.
    ///
    /// `None` means the object has no represented terminal state. Callers must
    /// also require [`is_complete`](Self::is_complete) before a clean verdict.
    pub fn object_is_accepting(&self, object: NodeId, fsm: &PropertyFsm) -> Option<bool> {
        self.terminal_states.get(&object).map(|states| {
            !states.is_empty() && states.iter().all(|state| fsm.accepting.contains(state))
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct FactKey {
    object: NodeId,
    state: PropertyState,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct TraceKey {
    node: NodeId,
    fact: FactKey,
}

#[derive(Debug, Clone)]
struct TraceStep {
    previous: Option<TraceKey>,
    via_sites: Vec<NodeId>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct QueueItem {
    distance: usize,
    key: TraceKey,
}

#[derive(Debug, Clone)]
struct AllocationSeed {
    object: NodeId,
    call: NodeId,
    definition: NodeId,
    state: PropertyState,
}

#[derive(Debug, Clone)]
enum CallClass {
    Event(PropertyEvent),
    Other,
    Unresolved,
    Ambiguous,
    Capped,
}

#[derive(Debug, Clone)]
enum OccurrenceKind {
    Allocate {
        object: NodeId,
        state: PropertyState,
    },
    Transition {
        event: PropertyEvent,
        objects: BTreeSet<NodeId>,
    },
}

#[derive(Debug, Clone)]
struct EventOccurrence {
    site: NodeId,
    kind: OccurrenceKind,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ViolationKey {
    object: NodeId,
    site: NodeId,
    from: PropertyState,
    event: PropertyEvent,
}

/// Computes a complete-evidence typestate report when caps and input evidence permit.
pub fn analyze_typestate(
    cpg: &CodePropertyGraph,
    fsm: &PropertyFsm,
    caps: &IfdsCaps,
) -> TypestateAnalysis {
    let mut analysis = TypestateAnalysis::default();
    for issue in fsm.validate() {
        analysis.issues.insert(TypestateIssue::Fsm(issue));
    }
    if !analysis.issues.is_empty() {
        return analysis;
    }

    let functions: Vec<_> = cpg
        .functions()
        .map(|node| node.id)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    if functions.len() > caps.max_functions {
        record_cap(
            &mut analysis,
            TypestateCapEvent::Functions {
                observed: functions.len(),
                cap: caps.max_functions,
            },
        );
        return analysis;
    }

    let mut violations = BTreeMap::<ViolationKey, TypestateViolation>::new();
    for function in functions {
        if !analyze_function(cpg, function, fsm, caps, &mut analysis, &mut violations) {
            break;
        }
    }
    analysis.violations = violations.into_values().collect();
    analysis.terminals.sort();
    analysis.terminals.dedup();
    if analysis.capped {
        for violation in &mut analysis.violations {
            violation.capped = true;
        }
        for terminal in &mut analysis.terminals {
            terminal.capped = true;
        }
    }
    analysis
}

/// Returns deterministic advisory forbidden-transition candidates.
///
/// Consumers making a negative claim must use [`analyze_typestate`] and require
/// [`TypestateAnalysis::is_complete`], because this projection cannot expose an
/// empty-but-capped or structurally incomplete result.
pub fn check_typestate(
    cpg: &CodePropertyGraph,
    fsm: &PropertyFsm,
    caps: &IfdsCaps,
) -> Vec<TypestateViolation> {
    analyze_typestate(cpg, fsm, caps).violations
}

fn analyze_function(
    cpg: &CodePropertyGraph,
    function: NodeId,
    fsm: &PropertyFsm,
    caps: &IfdsCaps,
    analysis: &mut TypestateAnalysis,
    violations: &mut BTreeMap<ViolationKey, TypestateViolation>,
) -> bool {
    let scope: BTreeSet<_> = function_scope_nodes(cpg, function).into_iter().collect();
    let calls: Vec<_> = scope
        .iter()
        .copied()
        .filter(|&node| {
            cpg.node(node)
                .is_some_and(|node| matches!(node.kind, CpgNodeKind::Call { .. }))
        })
        .map(|call| (source_key(cpg, call), call))
        .collect::<BTreeMap<_, _>>()
        .into_values()
        .collect();

    let mut classes = BTreeMap::new();
    let mut seeds = Vec::new();
    for &call in &calls {
        let class = classify_call(cpg, call, fsm, caps.max_call_depth);
        match &class {
            CallClass::Event(event) => {
                if let Some(state) = fsm.allocations.get(event) {
                    seeds.push(AllocationSeed {
                        object: call,
                        call,
                        definition: allocation_definition(cpg, function, call),
                        state: Arc::clone(state),
                    });
                }
            }
            CallClass::Unresolved => {
                analysis
                    .issues
                    .insert(TypestateIssue::UnresolvedEvent { site: call });
            }
            CallClass::Ambiguous => {
                analysis
                    .issues
                    .insert(TypestateIssue::AmbiguousEvent { site: call });
            }
            CallClass::Capped => {
                record_cap(
                    analysis,
                    TypestateCapEvent::CallDepth {
                        site: call,
                        cap: caps.max_call_depth,
                    },
                );
            }
            CallClass::Other => {}
        }
        classes.insert(call, class);
    }

    if seeds.is_empty() {
        return !analysis.capped;
    }
    let seeds: Vec<_> = seeds
        .into_iter()
        .map(|seed| (seed.object, seed))
        .collect::<BTreeMap<_, _>>()
        .into_values()
        .collect();
    for seed in &seeds {
        analysis.processed_objects.insert(seed.object);
    }

    let Some(tokens) = object_flow(cpg, function, &scope, &seeds, caps, analysis) else {
        return false;
    };
    let view = match function_cfg_view(cpg, function) {
        Ok(view) => view,
        Err(_) => {
            analysis
                .issues
                .insert(TypestateIssue::InvalidFunctionCfg { function });
            return true;
        }
    };

    let parent_by_child = ast_parent_index(cpg, &scope, analysis);
    let mut point_by_call = BTreeMap::new();
    for &call in &calls {
        match containing_program_point(call, &view.projection, &parent_by_child) {
            Some(point) => {
                point_by_call.insert(call, point);
            }
            None => {
                analysis
                    .issues
                    .insert(TypestateIssue::MissingProgramPoint { site: call });
            }
        }
    }

    let mut occurrences =
        BTreeMap::<NodeId, BTreeMap<((u32, u32, NodeId), NodeId), EventOccurrence>>::new();
    for seed in &seeds {
        let Some(&point) = point_by_call.get(&seed.call) else {
            continue;
        };
        let order = source_key(cpg, seed.call);
        occurrences.entry(point).or_default().insert(
            (order, seed.call),
            EventOccurrence {
                site: seed.call,
                kind: OccurrenceKind::Allocate {
                    object: seed.object,
                    state: Arc::clone(&seed.state),
                },
            },
        );
    }
    for &call in &calls {
        let Some(CallClass::Event(event)) = classes.get(&call) else {
            continue;
        };
        if fsm.allocations.contains_key(event) {
            continue;
        }
        let objects = call_objects(cpg, call, &tokens);
        if objects.is_empty() {
            continue;
        }
        let Some(&point) = point_by_call.get(&call) else {
            continue;
        };
        let order = source_key(cpg, call);
        occurrences.entry(point).or_default().insert(
            (order, call),
            EventOccurrence {
                site: call,
                kind: OccurrenceKind::Transition {
                    event: Arc::clone(event),
                    objects,
                },
            },
        );
    }

    let collapsed = view.projection.node_count() == 1
        && occurrences.keys().any(|&point| point == function)
        && occurrences
            .values()
            .flat_map(|events| events.values())
            .any(|event| event.site != function);
    if collapsed {
        analysis
            .issues
            .insert(TypestateIssue::MissingControlFlow { function });
    }

    let mut seed_point = BTreeMap::new();
    for seed in &seeds {
        if let Some(&point) = point_by_call.get(&seed.call) {
            seed_point.insert(seed.object, point);
        }
    }
    if seed_point.is_empty() {
        return true;
    }

    analysis.processed_functions.push(function);
    let recorded_exits: BTreeSet<_> = cpg.cfg_exits().iter().copied().collect();
    let mut exit_like: BTreeSet<_> = view
        .projection
        .nodes
        .iter()
        .copied()
        .filter(|node| {
            recorded_exits.contains(node)
                || matches!(
                    cpg.node(*node).map(|node| &node.kind),
                    Some(CpgNodeKind::Return | CpgNodeKind::Throw)
                )
        })
        .collect();
    // Legacy hand-built CFG fixtures may not register exits. Preserve their
    // established structural-sink semantics only when no builder-recorded or
    // explicit terminal exists. Parser-built expression-evaluation side nodes
    // are therefore never mistaken for function exits.
    if exit_like.is_empty() {
        exit_like.extend(
            view.exit_like
                .iter()
                .map(|&dense| view.projection.node_id(dense)),
        );
    }
    let mut facts = BTreeMap::<NodeId, BTreeSet<FactKey>>::new();
    let mut traces = BTreeMap::<TraceKey, TraceStep>::new();
    let mut queue = BTreeSet::<QueueItem>::new();
    for seed in &seeds {
        let Some(&point) = seed_point.get(&seed.object) else {
            continue;
        };
        let fact = FactKey {
            object: seed.object,
            state: Arc::clone(&seed.state),
        };
        if !admit_fact(
            function,
            point,
            fact,
            0,
            None,
            Vec::new(),
            caps,
            analysis,
            &mut facts,
            &mut traces,
            &mut queue,
        ) {
            return false;
        }
    }

    while let Some(item) = queue.pop_first() {
        if !consume_work(caps, analysis) {
            return false;
        }
        let mut state = Arc::clone(&item.key.fact.state);
        let mut active = seed_point.get(&item.key.fact.object).copied() != Some(item.key.node);
        let mut local_sites = vec![item.key.node];
        if let Some(events) = occurrences.get(&item.key.node) {
            for event in events.values() {
                match &event.kind {
                    OccurrenceKind::Allocate {
                        object,
                        state: allocated,
                    } if *object == item.key.fact.object => {
                        state = Arc::clone(allocated);
                        active = true;
                        append_unique(&mut local_sites, event.site);
                    }
                    OccurrenceKind::Transition {
                        event: label,
                        objects,
                    } if active && objects.contains(&item.key.fact.object) => {
                        append_unique(&mut local_sites, event.site);
                        if state == fsm.error {
                            continue;
                        }
                        let from = Arc::clone(&state);
                        let target = fsm
                            .transitions
                            .get(&(Arc::clone(&state), Arc::clone(label)))
                            .cloned()
                            .unwrap_or_else(|| Arc::clone(&fsm.error));
                        if target == fsm.error {
                            let witness = reconstruct_witness(&traces, &item.key, &local_sites);
                            insert_violation(
                                violations,
                                TypestateViolation {
                                    object: item.key.fact.object,
                                    site: event.site,
                                    from,
                                    event: Arc::clone(label),
                                    witness,
                                    capped: false,
                                },
                            );
                        }
                        state = target;
                    }
                    _ => {}
                }
            }
        }
        if !active {
            continue;
        }

        let out_fact = FactKey {
            object: item.key.fact.object,
            state,
        };
        let dense = view
            .projection
            .dense_index(item.key.node)
            .expect("queued typestate point belongs to the CFG projection");
        let successors: Vec<_> = view
            .projection
            .successors(dense)
            .iter()
            .map(|&successor| view.projection.node_id(successor))
            .collect();
        if exit_like.contains(&item.key.node) {
            analysis
                .terminal_states
                .entry(out_fact.object)
                .or_default()
                .insert(Arc::clone(&out_fact.state));
            analysis.terminals.push(TypestateTerminal {
                object: out_fact.object,
                exit: item.key.node,
                state: Arc::clone(&out_fact.state),
                witness: reconstruct_witness(&traces, &item.key, &local_sites),
                capped: false,
            });
        }
        for successor in successors {
            if !admit_fact(
                function,
                successor,
                out_fact.clone(),
                item.distance.saturating_add(1),
                Some(item.key.clone()),
                local_sites.clone(),
                caps,
                analysis,
                &mut facts,
                &mut traces,
                &mut queue,
            ) {
                return false;
            }
        }
    }
    true
}

fn classify_call(
    cpg: &CodePropertyGraph,
    call: NodeId,
    fsm: &PropertyFsm,
    max_depth: u32,
) -> CallClass {
    let path = call_callee_path_evidence(cpg, call, max_depth);
    if let Some(path) = path.path {
        return fsm
            .event_for_path(&path)
            .map(CallClass::Event)
            .unwrap_or(CallClass::Other);
    }

    let mut target_names = BTreeSet::new();
    if let Some(CpgNodeKind::Call {
        target: Some(target),
        ..
    }) = cpg.node(call).map(|node| &node.kind)
    {
        if let Some(name) = cpg.node(*target).and_then(|node| node.name()) {
            target_names.insert(name.to_owned());
        }
    }
    for edge in cpg.outgoing_edges(call) {
        if edge.kind.is_call() || matches!(edge.kind, CpgEdgeKind::ControlFlow(CfgEdgeKind::Call)) {
            if let Some(name) = cpg.node(edge.target).and_then(|node| node.name()) {
                target_names.insert(name.to_owned());
            }
        }
    }
    if !target_names.is_empty() {
        let classes: BTreeSet<_> = target_names
            .iter()
            .map(|name| fsm.event_for_path(name))
            .collect();
        if classes.len() == 1 {
            return match classes
                .into_iter()
                .next()
                .expect("one target classification")
            {
                Some(event) => CallClass::Event(event),
                None => CallClass::Other,
            };
        }
        return CallClass::Ambiguous;
    }
    if path.capped {
        CallClass::Capped
    } else {
        CallClass::Unresolved
    }
}

fn allocation_definition(cpg: &CodePropertyGraph, function: NodeId, call: NodeId) -> NodeId {
    for ancestor in cpg.ast_ancestors(call) {
        if ancestor == function {
            break;
        }
        if cpg.node(ancestor).is_some_and(|node| {
            matches!(
                node.kind,
                CpgNodeKind::Variable { .. } | CpgNodeKind::Assignment { .. }
            )
        }) {
            return ancestor;
        }
    }
    call
}

fn object_flow(
    cpg: &CodePropertyGraph,
    function: NodeId,
    scope: &BTreeSet<NodeId>,
    seeds: &[AllocationSeed],
    caps: &IfdsCaps,
    analysis: &mut TypestateAnalysis,
) -> Option<BTreeMap<NodeId, BTreeSet<NodeId>>> {
    let mut successors = BTreeMap::<NodeId, BTreeSet<NodeId>>::new();
    for edge in cpg.edges() {
        if !scope.contains(&edge.source) || !scope.contains(&edge.target) {
            continue;
        }
        match edge.kind {
            CpgEdgeKind::DataFlow(DfgEdgeKind::UseDef) => {
                successors
                    .entry(edge.target)
                    .or_default()
                    .insert(edge.source);
            }
            CpgEdgeKind::DataFlow(DfgEdgeKind::Alias) => {
                successors
                    .entry(edge.source)
                    .or_default()
                    .insert(edge.target);
                successors
                    .entry(edge.target)
                    .or_default()
                    .insert(edge.source);
            }
            CpgEdgeKind::DataFlow(_) => {
                successors
                    .entry(edge.source)
                    .or_default()
                    .insert(edge.target);
            }
            CpgEdgeKind::Reference => {
                successors
                    .entry(edge.target)
                    .or_default()
                    .insert(edge.source);
            }
            CpgEdgeKind::Definition | CpgEdgeKind::Declaration => {
                successors
                    .entry(edge.source)
                    .or_default()
                    .insert(edge.target);
            }
            _ => {}
        }
    }
    for &node in scope {
        if let Some(CpgNodeKind::Identifier {
            definition: Some(definition),
            ..
        }) = cpg.node(node).map(|node| &node.kind)
        {
            if scope.contains(definition) {
                successors.entry(*definition).or_default().insert(node);
            }
        }
    }

    let mut tokens = BTreeMap::<NodeId, BTreeSet<NodeId>>::new();
    let mut queue = BTreeSet::<(NodeId, NodeId)>::new();
    for seed in seeds {
        for node in BTreeSet::from([seed.call, seed.definition]) {
            if !admit_token(function, node, seed.object, caps, analysis, &mut tokens) {
                return None;
            }
            queue.insert((node, seed.object));
        }
    }
    while let Some((node, object)) = queue.pop_first() {
        if !consume_work(caps, analysis) {
            return None;
        }
        for &successor in successors.get(&node).into_iter().flatten() {
            if tokens
                .get(&successor)
                .is_some_and(|objects| objects.contains(&object))
            {
                continue;
            }
            if !admit_token(function, successor, object, caps, analysis, &mut tokens) {
                return None;
            }
            queue.insert((successor, object));
        }
    }
    Some(tokens)
}

fn admit_token(
    function: NodeId,
    node: NodeId,
    object: NodeId,
    caps: &IfdsCaps,
    analysis: &mut TypestateAnalysis,
    tokens: &mut BTreeMap<NodeId, BTreeSet<NodeId>>,
) -> bool {
    let at_node = tokens.entry(node).or_default();
    if at_node.contains(&object) {
        return true;
    }
    if at_node.len() >= caps.max_facts_per_node {
        record_cap(
            analysis,
            TypestateCapEvent::FactsPerNode {
                function,
                node,
                observed: at_node.len().saturating_add(1),
                cap: caps.max_facts_per_node,
            },
        );
        return false;
    }
    at_node.insert(object);
    true
}

fn call_objects(
    cpg: &CodePropertyGraph,
    call: NodeId,
    tokens: &BTreeMap<NodeId, BTreeSet<NodeId>>,
) -> BTreeSet<NodeId> {
    let mut evidence_nodes = BTreeSet::from([call]);
    for argument in call_arguments(cpg, call) {
        evidence_nodes.insert(argument);
        evidence_nodes.extend(cpg.ast_descendants(argument));
    }
    let is_method = cpg.node(call).is_some_and(|node| {
        matches!(
            node.kind,
            CpgNodeKind::Call {
                is_method: true,
                ..
            }
        )
    });
    if is_method {
        if let Some(receiver) = cpg.ast_children(call).first().copied() {
            evidence_nodes.insert(receiver);
            evidence_nodes.extend(cpg.ast_descendants(receiver));
        }
    }

    let mut objects = BTreeSet::new();
    for node in evidence_nodes {
        if let Some(at_node) = tokens.get(&node) {
            objects.extend(at_node.iter().copied());
        }
    }
    objects
}

fn ast_parent_index(
    cpg: &CodePropertyGraph,
    scope: &BTreeSet<NodeId>,
    analysis: &mut TypestateAnalysis,
) -> BTreeMap<NodeId, NodeId> {
    let mut choices = BTreeMap::<NodeId, BTreeSet<NodeId>>::new();
    for edge in cpg.edges() {
        if matches!(edge.kind, CpgEdgeKind::AstChild)
            && scope.contains(&edge.source)
            && scope.contains(&edge.target)
        {
            choices.entry(edge.target).or_default().insert(edge.source);
        }
    }
    for &node in scope {
        if let Some(parent) = cpg.ast_parent(node).filter(|parent| scope.contains(parent)) {
            choices.entry(node).or_default().insert(parent);
        }
    }
    let mut parents = BTreeMap::new();
    for (node, candidates) in choices {
        if candidates.len() > 1 {
            analysis
                .issues
                .insert(TypestateIssue::AmbiguousAstParent { node });
        }
        if let Some(parent) = candidates.first().copied() {
            parents.insert(node, parent);
        }
    }
    parents
}

fn containing_program_point(
    site: NodeId,
    projection: &super::GraphProjection,
    parents: &BTreeMap<NodeId, NodeId>,
) -> Option<NodeId> {
    let mut current = site;
    let mut visited = BTreeSet::new();
    while visited.insert(current) {
        if projection.dense_index(current).is_some() {
            return Some(current);
        }
        current = *parents.get(&current)?;
    }
    None
}

#[allow(clippy::too_many_arguments)]
fn admit_fact(
    function: NodeId,
    node: NodeId,
    fact: FactKey,
    distance: usize,
    previous: Option<TraceKey>,
    via_sites: Vec<NodeId>,
    caps: &IfdsCaps,
    analysis: &mut TypestateAnalysis,
    facts: &mut BTreeMap<NodeId, BTreeSet<FactKey>>,
    traces: &mut BTreeMap<TraceKey, TraceStep>,
    queue: &mut BTreeSet<QueueItem>,
) -> bool {
    let at_node = facts.entry(node).or_default();
    if at_node.contains(&fact) {
        return true;
    }
    if at_node.len() >= caps.max_facts_per_node {
        record_cap(
            analysis,
            TypestateCapEvent::FactsPerNode {
                function,
                node,
                observed: at_node.len().saturating_add(1),
                cap: caps.max_facts_per_node,
            },
        );
        return false;
    }
    at_node.insert(fact.clone());
    let key = TraceKey { node, fact };
    traces.insert(
        key.clone(),
        TraceStep {
            previous,
            via_sites,
        },
    );
    queue.insert(QueueItem { distance, key });
    true
}

fn reconstruct_witness(
    traces: &BTreeMap<TraceKey, TraceStep>,
    key: &TraceKey,
    local_sites: &[NodeId],
) -> Vec<NodeId> {
    let mut chain = Vec::new();
    let mut current = Some(key.clone());
    let mut seen = BTreeSet::new();
    while let Some(step_key) = current {
        if !seen.insert(step_key.clone()) {
            break;
        }
        current = traces.get(&step_key).and_then(|step| step.previous.clone());
        chain.push(step_key);
    }
    chain.reverse();

    let mut witness = vec![key.fact.object];
    for step_key in chain {
        if let Some(step) = traces.get(&step_key) {
            for &site in &step.via_sites {
                append_unique(&mut witness, site);
            }
        }
    }
    for &site in local_sites {
        append_unique(&mut witness, site);
    }
    witness
}

fn insert_violation(
    violations: &mut BTreeMap<ViolationKey, TypestateViolation>,
    violation: TypestateViolation,
) {
    let key = ViolationKey {
        object: violation.object,
        site: violation.site,
        from: Arc::clone(&violation.from),
        event: Arc::clone(&violation.event),
    };
    match violations.get_mut(&key) {
        Some(current)
            if (violation.witness.len(), &violation.witness)
                < (current.witness.len(), &current.witness) =>
        {
            *current = violation;
        }
        None => {
            violations.insert(key, violation);
        }
        _ => {}
    }
}

fn consume_work(caps: &IfdsCaps, analysis: &mut TypestateAnalysis) -> bool {
    if analysis.worklist_steps >= caps.max_worklist_steps {
        record_cap(
            analysis,
            TypestateCapEvent::WorklistSteps {
                needed: analysis.worklist_steps.saturating_add(1),
                cap: caps.max_worklist_steps,
            },
        );
        false
    } else {
        analysis.worklist_steps = analysis.worklist_steps.saturating_add(1);
        true
    }
}

fn record_cap(analysis: &mut TypestateAnalysis, event: TypestateCapEvent) {
    analysis.capped = true;
    analysis.cap_events.insert(event);
}

fn source_key(cpg: &CodePropertyGraph, node: NodeId) -> (u32, u32, NodeId) {
    cpg.node(node)
        .map(|node| (node.range.start, node.range.end, node.id))
        .unwrap_or((u32::MAX, u32::MAX, node))
}

fn append_unique(nodes: &mut Vec<NodeId>, node: NodeId) {
    if nodes.last().copied() != Some(node) {
        nodes.push(node);
    }
}

#[cfg(test)]
#[path = "typestate/tests.rs"]
mod tests;
