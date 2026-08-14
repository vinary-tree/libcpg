//! Deterministic, advisory message-passing analyses.
//!
//! The core is feature-free. Language frontends normalize concrete syntax
//! into [`crate::MsgKind`] metadata, while these analyses use only CPG nodes,
//! AST relations, and typed def-use/reference evidence.

mod channels;
mod deadlock;
mod events;

pub use crate::{LockKind, LockMode, MsgKind};
pub use channels::{channel_report, ChannelFinding, ChannelFindingKind};
pub use deadlock::{
    communication_deadlocks, DeadlockFinding, DeadlockFindingKind, DeadlockWitness,
};
pub use events::{extract_events, ChannelEvent, ChannelId};
