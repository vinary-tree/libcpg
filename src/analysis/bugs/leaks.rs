//! Resource-leak detection from finite-state terminal evidence.
//!
//! The detector delegates object identity, alias flow, event transfer, and CFG
//! propagation to typestate analysis. A leak candidate is a non-accepting
//! terminal state on at least one represented path to a function exit.

use std::collections::BTreeSet;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::{CodePropertyGraph, NodeId};

use super::super::{analyze_typestate, IfdsCaps, PropertyFsm, PropertyState, TypestateAnalysis};

/// Structural evidence that prevents a complete resource-leak verdict.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ResourceLeakIssue {
    /// Leak semantics require at least one acceptable terminal state.
    NoAcceptingState,
}

/// One allocation that can reach an exit in a non-accepting state.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ResourceLeak {
    /// Abstract object identity and allocation call site.
    pub object: NodeId,
    /// Exit-like CFG point reached without an accepting terminal transition.
    pub exit: NodeId,
    /// Non-accepting state observed at `exit`.
    pub state: PropertyState,
    /// Canonical allocation/event/CFG witness from allocation to `exit`.
    pub witness: Vec<NodeId>,
    /// Whether an explicit typestate resource cap made the report partial.
    pub capped: bool,
}

/// Resource-leak candidates plus the complete underlying typestate evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ResourceLeakAnalysis {
    /// Non-accepting object/exit/state triples in canonical order.
    pub leaks: Vec<ResourceLeak>,
    /// Typestate report from which terminal evidence was projected.
    pub typestate: TypestateAnalysis,
    /// Detector-specific structural issues in canonical order.
    pub issues: BTreeSet<ResourceLeakIssue>,
}

impl ResourceLeakAnalysis {
    /// Returns whether every represented path was examined under valid leak semantics.
    pub fn is_complete(&self) -> bool {
        self.issues.is_empty() && self.typestate.is_complete()
    }
}

/// Computes deterministic advisory resource-leak candidates.
///
/// The supplied finite-state machine defines allocation events, release
/// transitions, and acceptable terminal states. Missing transitions retain
/// typestate's absorbing-error semantics. The CPG is never mutated.
pub fn analyze_resource_leaks(
    cpg: &CodePropertyGraph,
    fsm: &PropertyFsm,
    caps: &IfdsCaps,
) -> ResourceLeakAnalysis {
    let typestate = analyze_typestate(cpg, fsm, caps);
    let mut issues = BTreeSet::new();
    if fsm.accepting.is_empty() {
        issues.insert(ResourceLeakIssue::NoAcceptingState);
    }

    let leaks = if issues.is_empty() {
        typestate
            .terminals
            .iter()
            .filter(|terminal| !fsm.accepting.contains(&terminal.state))
            .map(|terminal| ResourceLeak {
                object: terminal.object,
                exit: terminal.exit,
                state: terminal.state.clone(),
                witness: terminal.witness.clone(),
                capped: terminal.capped,
            })
            .collect()
    } else {
        Vec::new()
    };

    ResourceLeakAnalysis {
        leaks,
        typestate,
        issues,
    }
}

/// Returns the deterministic resource-leak finding projection.
///
/// Consumers making a negative claim must use [`analyze_resource_leaks`] and
/// require [`ResourceLeakAnalysis::is_complete`], because this projection
/// cannot expose an empty-but-capped or structurally invalid result.
pub fn resource_leaks(
    cpg: &CodePropertyGraph,
    fsm: &PropertyFsm,
    caps: &IfdsCaps,
) -> Vec<ResourceLeak> {
    analyze_resource_leaks(cpg, fsm, caps).leaks
}

#[cfg(test)]
#[path = "leaks/tests.rs"]
mod tests;
