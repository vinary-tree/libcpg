//! Deterministic, advisory message-passing analyses.
//!
//! The core is feature-free. Language frontends normalize concrete syntax
//! into [`crate::MsgKind`] metadata, while these analyses use only CPG nodes,
//! AST relations, and typed def-use/reference evidence.

mod atomicity;
mod channels;
mod deadlock;
mod events;
mod evidence;
mod mhp;
#[cfg(feature = "rholang")]
mod name_flow;
mod race;
#[cfg(feature = "rholang")]
mod session;

pub use crate::{LockKind, LockMode, MsgKind};
pub use atomicity::{atomicity_candidates, AtomicityCandidate, AtomicityWitness};
pub use channels::{channel_report, ChannelFinding, ChannelFindingKind};
pub use deadlock::{
    communication_deadlocks, DeadlockFinding, DeadlockFindingKind, DeadlockWitness,
};
pub use events::{extract_events, ChannelEvent, ChannelId};
pub use mhp::{may_happen_in_parallel, MhpPair, MhpRelation, MAX_MHP_PAIRS};
#[cfg(feature = "rholang")]
pub use name_flow::{
    analyze_deadlock_freedom, analyze_deadlock_freedom_with_caps, analyze_name_flow,
    analyze_name_flow_with_caps, deadlock_freedom, materialize_name_flow_overlay,
    ChannelUsageLevel, DeadlockFreedomAnalysis, DeadlockFreedomIssue, NameFlowAnalysis,
    NameFlowCaps, NameFlowIssue, NameFlowOverlayError, NameFlowRelation,
    MAX_NAME_FLOW_COMMUNICATION_MATCHES, MAX_NAME_FLOW_DEPENDENCIES,
    MAX_NAME_FLOW_ORIGIN_ASSIGNMENTS, MAX_NAME_FLOW_RELATIONS, MAX_NAME_FLOW_WORK_ITEMS,
};
pub use race::{race_candidates, ConcurrentAccessKind, HeldLock, RaceCandidate, RaceWitness};
#[cfg(feature = "rholang")]
pub use session::{
    session_sketches, SessionAction, SessionConformance, SessionDirection, SessionEndpoint,
    SessionIssue, SessionIssueKind, SessionPayloadType, SessionSketch,
};
