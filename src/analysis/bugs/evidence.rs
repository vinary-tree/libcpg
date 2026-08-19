//! Shared, exact evidence types for population-based advisory detectors.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::{CodePropertyGraph, CpgNodeKind, NodeId};

use super::super::function_scope_nodes;
use super::super::resolve::call_callee_path_evidence;

/// Canonical identity of one called application-programming interface (API).
///
/// Resolved identities compare their complete sorted target set. Named
/// identities are used only when the CPG has no represented target.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ApiIdentity {
    /// One or more represented dynamic-dispatch targets.
    Resolved {
        /// Sorted, deduplicated represented targets.
        targets: Vec<NodeId>,
        /// Human-readable reconstructed path or target-name summary.
        name: Arc<str>,
    },
    /// A typed callee path with no represented target.
    Named(Arc<str>),
}

impl ApiIdentity {
    /// Returns the deterministic human-readable API name.
    pub fn name(&self) -> &str {
        match self {
            Self::Resolved { name, .. } | Self::Named(name) => name,
        }
    }

    /// Returns represented targets, or an empty slice for an external API.
    pub fn targets(&self) -> &[NodeId] {
        match self {
            Self::Resolved { targets, .. } => targets,
            Self::Named(_) => &[],
        }
    }
}

/// Exact observed support for one population belief.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct PopulationFraction {
    /// Population members carrying the evidence.
    pub supporting: usize,
    /// Complete comparable population, including the anomalous member.
    pub population: usize,
}

impl PopulationFraction {
    /// Creates an exact support fraction.
    pub const fn new(supporting: usize, population: usize) -> Self {
        Self {
            supporting,
            population,
        }
    }

    /// Returns the presentation-only floating-point ratio.
    ///
    /// Detector decisions compare integers exactly and never use this value.
    pub fn ratio(self) -> f64 {
        if self.population == 0 {
            0.0
        } else {
            self.supporting as f64 / self.population as f64
        }
    }
}

/// Exact minimum support required to infer a population belief.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct PopulationThreshold {
    /// Minimum-support numerator.
    pub numerator: usize,
    /// Positive minimum-support denominator.
    pub denominator: usize,
}

impl PopulationThreshold {
    /// Creates a rational threshold. An analyzer rejects zero denominators or
    /// numerators greater than denominators as invalid configuration.
    pub const fn new(numerator: usize, denominator: usize) -> Self {
        Self {
            numerator,
            denominator,
        }
    }

    pub(crate) const fn is_valid(self) -> bool {
        self.denominator != 0 && self.numerator <= self.denominator
    }

    pub(crate) fn accepts(self, observed: PopulationFraction) -> bool {
        observed.population != 0
            && (observed.supporting as u128) * (self.denominator as u128)
                >= (self.numerator as u128) * (observed.population as u128)
    }
}

pub(crate) struct CallEvidence {
    pub(crate) identity: ApiIdentity,
    pub(crate) catalog_path: Arc<str>,
}

pub(crate) enum CallEvidenceError {
    Missing,
    Capped,
}

pub(crate) fn call_evidence(
    cpg: &CodePropertyGraph,
    call: NodeId,
    max_depth: u32,
) -> Result<CallEvidence, CallEvidenceError> {
    let reconstructed = call_callee_path_evidence(cpg, call, max_depth);
    let mut targets = BTreeSet::new();
    if let Some(CpgNodeKind::Call {
        target: Some(target),
        ..
    }) = cpg.node(call).map(|node| &node.kind)
    {
        if represented_function(cpg, *target) {
            targets.insert(*target);
        }
    }
    targets.extend(
        cpg.callees(call)
            .into_iter()
            .filter(|target| represented_function(cpg, *target)),
    );

    if !targets.is_empty() {
        let targets: Vec<_> = targets.into_iter().collect();
        let name = reconstructed
            .path
            .unwrap_or_else(|| target_names(cpg, &targets));
        if name.is_empty() {
            return Err(CallEvidenceError::Missing);
        }
        return Ok(CallEvidence {
            catalog_path: name.clone().into(),
            identity: ApiIdentity::Resolved {
                targets,
                name: name.into(),
            },
        });
    }

    match reconstructed.path {
        Some(path) if !path.is_empty() => Ok(CallEvidence {
            catalog_path: path.clone().into(),
            identity: ApiIdentity::Named(path.into()),
        }),
        _ if reconstructed.capped => Err(CallEvidenceError::Capped),
        _ => Err(CallEvidenceError::Missing),
    }
}

pub(crate) fn call_owners(cpg: &CodePropertyGraph) -> BTreeMap<NodeId, NodeId> {
    let mut functions: Vec<_> = cpg.functions().map(|node| node.id).collect();
    functions.sort_unstable();
    let mut owners = BTreeMap::new();
    for function in functions {
        let mut scope: Vec<_> = function_scope_nodes(cpg, function).into_iter().collect();
        scope.sort_unstable();
        for node in scope {
            if cpg
                .node(node)
                .is_some_and(|node| matches!(node.kind, CpgNodeKind::Call { .. }))
            {
                owners.entry(node).or_insert(function);
            }
        }
    }
    owners
}

pub(crate) fn is_guard(cpg: &CodePropertyGraph, node: NodeId) -> bool {
    cpg.outgoing_edges(node).any(|edge| {
        matches!(edge.kind, crate::CpgEdgeKind::ControlDependence)
            || match edge.kind {
                crate::CpgEdgeKind::ControlFlow(kind) => {
                    kind.is_conditional() || matches!(kind, crate::CfgEdgeKind::LoopExit)
                }
                _ => false,
            }
    })
}

fn represented_function(cpg: &CodePropertyGraph, target: NodeId) -> bool {
    cpg.node(target)
        .is_some_and(|node| matches!(node.kind, CpgNodeKind::Function { .. }))
}

fn target_names(cpg: &CodePropertyGraph, targets: &[NodeId]) -> String {
    targets
        .iter()
        .filter_map(|target| cpg.node(*target).and_then(|node| node.name()))
        .collect::<Vec<_>>()
        .join("|")
}
