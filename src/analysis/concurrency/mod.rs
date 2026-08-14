//! Deterministic, advisory message-passing analyses.
//!
//! The core is feature-free. Language frontends normalize concrete syntax
//! into [`crate::MsgKind`] metadata, while these analyses use only CPG nodes,
//! AST relations, and typed def-use/reference evidence.

mod channels;
mod events;

pub use crate::MsgKind;
pub use channels::{channel_report, ChannelFinding, ChannelFindingKind};
pub use events::{extract_events, ChannelEvent, ChannelId};
