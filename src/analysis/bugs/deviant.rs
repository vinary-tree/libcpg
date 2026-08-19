//! Population-based implicit-belief and deviant-behavior detection.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::{CodePropertyGraph, CpgEdgeKind, DfgEdgeKind, NodeId};

use super::super::{cfg_projection, AnalysisError};
use super::evidence::{
    call_evidence, call_owners, is_guard, ApiIdentity, CallEvidenceError, PopulationFraction,
    PopulationThreshold,
};

/// Configuration for [`analyze_deviant_behavior`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct DeviantBehaviorConfig {
    /// Minimum exact population support required to infer a belief.
    pub minimum_support: PopulationThreshold,
    /// Minimum comparable occurrences required to infer a belief.
    pub minimum_population: usize,
    /// Maximum DFG/PDG nodes examined when looking for a return-value check.
    pub max_return_evidence_nodes: usize,
    /// Maximum CFG nodes examined from one call occurrence.
    pub max_cfg_nodes_per_call: usize,
    /// Maximum typed AST depth used to reconstruct a callee path.
    pub max_callee_depth: u32,
}

impl Default for DeviantBehaviorConfig {
    fn default() -> Self {
        Self {
            minimum_support: PopulationThreshold::new(9, 10),
            minimum_population: 2,
            max_return_evidence_nodes: 100_000,
            max_cfg_nodes_per_call: 100_000,
            max_callee_depth: 256,
        }
    }
}

/// Inferred belief that one API call normally follows another.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CallOrderBelief {
    /// API occurrence from which the population is measured.
    pub predecessor: ApiIdentity,
    /// API expected on a later reachable CFG node.
    pub successor: ApiIdentity,
    /// Exact supporting occurrence fraction.
    pub support: PopulationFraction,
    /// Predecessor sites that reach the successor.
    pub supporting_sites: Vec<NodeId>,
}

/// Inferred belief that an API's return value is normally checked.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ReturnCheckBelief {
    /// API whose occurrences form the population.
    pub api: ApiIdentity,
    /// Exact checked-occurrence fraction.
    pub support: PopulationFraction,
    /// Checked call sites.
    pub supporting_sites: Vec<NodeId>,
    /// Representative guard reached from each supporting call.
    pub guard_witnesses: Vec<NodeId>,
}

/// Kind of population belief violated by a call site.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum DeviantBehaviorKind {
    /// The call does not reach an API that normally follows it.
    MissingFollowingCall {
        /// Expected later API.
        expected: ApiIdentity,
    },
    /// The call's return value does not reach a represented guard.
    UncheckedReturn,
}

/// One call occurrence that violates a population-supported implicit belief.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct DeviantBehavior {
    /// Deviating call site.
    pub site: NodeId,
    /// Function containing `site`.
    pub function: NodeId,
    /// API invoked at `site`.
    pub api: ApiIdentity,
    /// Violated belief kind.
    pub kind: DeviantBehaviorKind,
    /// Exact population support for the violated belief.
    pub peer_support: PopulationFraction,
    /// Sites carrying the expected behavior.
    pub supporting_sites: Vec<NodeId>,
}

/// Incomplete evidence excluded from implicit-belief inference.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum DeviantBehaviorIssue {
    /// A call did not belong to a represented function AST scope.
    CallOutsideFunction {
        /// Unscoped call site.
        site: NodeId,
    },
    /// No represented target or typed callee path identified the call.
    MissingCalleeIdentity {
        /// Unidentified call site.
        site: NodeId,
    },
    /// Callee-path reconstruction reached its explicit depth boundary.
    CalleeIdentityCapped {
        /// Capped call site.
        site: NodeId,
    },
    /// A call was absent from its function's represented CFG.
    CallOutsideCfg {
        /// Call lacking CFG evidence.
        site: NodeId,
        /// Containing function.
        function: NodeId,
    },
    /// Return-check evidence exceeded its explicit node budget.
    ReturnEvidenceCap {
        /// Call whose result was traversed.
        site: NodeId,
        /// First required count beyond the cap.
        needed: usize,
        /// Configured maximum.
        cap: usize,
    },
    /// Later-call evidence exceeded its explicit CFG node budget.
    CfgEvidenceCap {
        /// Predecessor call whose reachable CFG was traversed.
        site: NodeId,
        /// First required count beyond the cap.
        needed: usize,
        /// Configured maximum.
        cap: usize,
    },
}

/// Inferred beliefs, deviations, and completeness evidence.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct DeviantBehaviorAnalysis {
    /// Inferred call-order beliefs.
    pub call_order_beliefs: Vec<CallOrderBelief>,
    /// Inferred return-check beliefs.
    pub return_check_beliefs: Vec<ReturnCheckBelief>,
    /// Canonically ordered advisory deviations.
    pub findings: Vec<DeviantBehavior>,
    /// Calls with complete identity evidence.
    pub analyzed_sites: Vec<NodeId>,
    /// Ordered evidence gaps excluded from population inference.
    pub issues: BTreeSet<DeviantBehaviorIssue>,
}

impl DeviantBehaviorAnalysis {
    /// Returns whether every candidate call had complete identity and traversal evidence.
    pub fn is_complete(&self) -> bool {
        self.issues.is_empty()
    }
}

#[derive(Debug)]
struct Site {
    id: NodeId,
    function: NodeId,
    api: ApiIdentity,
}

#[derive(Debug)]
struct OrderSample {
    site: usize,
    following: BTreeSet<ApiIdentity>,
}

/// Infers call-order and return-check beliefs and reports their deviations.
pub fn analyze_deviant_behavior(
    cpg: &CodePropertyGraph,
    config: &DeviantBehaviorConfig,
) -> Result<DeviantBehaviorAnalysis, AnalysisError> {
    validate_config(config)?;
    let owners = call_owners(cpg);
    let mut analysis = DeviantBehaviorAnalysis::default();
    let mut sites = Vec::new();
    let mut calls: Vec<_> = cpg.calls().map(|node| node.id).collect();
    calls.sort_unstable();
    for call in calls {
        let Some(&function) = owners.get(&call) else {
            analysis
                .issues
                .insert(DeviantBehaviorIssue::CallOutsideFunction { site: call });
            continue;
        };
        let api = match call_evidence(cpg, call, config.max_callee_depth) {
            Ok(evidence) => evidence.identity,
            Err(CallEvidenceError::Missing) => {
                analysis
                    .issues
                    .insert(DeviantBehaviorIssue::MissingCalleeIdentity { site: call });
                continue;
            }
            Err(CallEvidenceError::Capped) => {
                analysis
                    .issues
                    .insert(DeviantBehaviorIssue::CalleeIdentityCapped { site: call });
                continue;
            }
        };
        sites.push(Site {
            id: call,
            function,
            api,
        });
    }
    sites.sort_by_key(|site| site.id);
    analysis.analyzed_sites = sites.iter().map(|site| site.id).collect();

    infer_return_checks(cpg, config, &sites, &mut analysis);
    infer_call_order(cpg, config, &sites, &mut analysis)?;
    analysis.call_order_beliefs.sort();
    analysis.return_check_beliefs.sort();
    analysis.findings.sort();
    Ok(analysis)
}

/// Returns deviant-behavior findings while discarding completeness evidence.
///
/// Consumers making negative claims must use [`analyze_deviant_behavior`] and
/// require [`DeviantBehaviorAnalysis::is_complete`].
pub fn deviant_behaviors(
    cpg: &CodePropertyGraph,
    config: &DeviantBehaviorConfig,
) -> Result<Vec<DeviantBehavior>, AnalysisError> {
    Ok(analyze_deviant_behavior(cpg, config)?.findings)
}

fn validate_config(config: &DeviantBehaviorConfig) -> Result<(), AnalysisError> {
    if !config.minimum_support.is_valid() {
        return Err(AnalysisError::InvalidInput {
            analysis: "deviant-behavior detection",
            input: "minimum_support",
            reason: "must have a positive denominator and numerator no greater than denominator",
        });
    }
    if config.minimum_population == 0 {
        return Err(AnalysisError::InvalidInput {
            analysis: "deviant-behavior detection",
            input: "minimum_population",
            reason: "must be positive",
        });
    }
    if config.max_return_evidence_nodes == 0 {
        return Err(AnalysisError::InvalidInput {
            analysis: "deviant-behavior detection",
            input: "max_return_evidence_nodes",
            reason: "must be positive",
        });
    }
    if config.max_cfg_nodes_per_call == 0 {
        return Err(AnalysisError::InvalidInput {
            analysis: "deviant-behavior detection",
            input: "max_cfg_nodes_per_call",
            reason: "must be positive",
        });
    }
    Ok(())
}

fn infer_return_checks(
    cpg: &CodePropertyGraph,
    config: &DeviantBehaviorConfig,
    sites: &[Site],
    analysis: &mut DeviantBehaviorAnalysis,
) {
    let mut by_api = BTreeMap::<ApiIdentity, Vec<(usize, Option<NodeId>)>>::new();
    for (index, site) in sites.iter().enumerate() {
        match return_guard(cpg, site.id, config.max_return_evidence_nodes) {
            Ok(guard) => by_api
                .entry(site.api.clone())
                .or_default()
                .push((index, guard)),
            Err(needed) => {
                analysis
                    .issues
                    .insert(DeviantBehaviorIssue::ReturnEvidenceCap {
                        site: site.id,
                        needed,
                        cap: config.max_return_evidence_nodes,
                    });
            }
        }
    }

    for (api, population) in by_api {
        if population.len() < config.minimum_population {
            continue;
        }
        let supporting: Vec<_> = population
            .iter()
            .filter_map(|(site, guard)| guard.map(|guard| (*site, guard)))
            .collect();
        let fraction = PopulationFraction::new(supporting.len(), population.len());
        if !config.minimum_support.accepts(fraction) {
            continue;
        }
        let supporting_sites: Vec<_> = supporting.iter().map(|(site, _)| sites[*site].id).collect();
        let guard_witnesses: Vec<_> = supporting.iter().map(|(_, guard)| *guard).collect();
        analysis.return_check_beliefs.push(ReturnCheckBelief {
            api: api.clone(),
            support: fraction,
            supporting_sites: supporting_sites.clone(),
            guard_witnesses,
        });
        for (site, guard) in population {
            if guard.is_none() {
                analysis.findings.push(DeviantBehavior {
                    site: sites[site].id,
                    function: sites[site].function,
                    api: api.clone(),
                    kind: DeviantBehaviorKind::UncheckedReturn,
                    peer_support: fraction,
                    supporting_sites: supporting_sites.clone(),
                });
            }
        }
    }
}

fn return_guard(
    cpg: &CodePropertyGraph,
    call: NodeId,
    max_nodes: usize,
) -> Result<Option<NodeId>, usize> {
    let mut reached = BTreeSet::from([call]);
    let mut queue = VecDeque::from([call]);
    while let Some(node) = queue.pop_front() {
        if is_guard(cpg, node) {
            return Ok(Some(node));
        }
        let mut successors: Vec<_> = cpg
            .outgoing_edges(node)
            .filter_map(|edge| {
                (matches!(edge.kind, CpgEdgeKind::DataDependence)
                    || matches!(
                        edge.kind,
                        CpgEdgeKind::DataFlow(kind) if !matches!(kind, DfgEdgeKind::UseDef)
                    ))
                .then_some(edge.target)
            })
            .collect();
        successors.sort_unstable();
        successors.dedup();
        for successor in successors {
            if reached.insert(successor) {
                if reached.len() > max_nodes {
                    return Err(reached.len());
                }
                queue.push_back(successor);
            }
        }
    }
    Ok(None)
}

fn infer_call_order(
    cpg: &CodePropertyGraph,
    config: &DeviantBehaviorConfig,
    sites: &[Site],
    analysis: &mut DeviantBehaviorAnalysis,
) -> Result<(), AnalysisError> {
    let site_by_node: BTreeMap<_, _> = sites
        .iter()
        .enumerate()
        .map(|(index, site)| (site.id, index))
        .collect();
    let mut sites_by_function = BTreeMap::<NodeId, Vec<usize>>::new();
    for (index, site) in sites.iter().enumerate() {
        sites_by_function
            .entry(site.function)
            .or_default()
            .push(index);
    }

    let mut samples_by_api = BTreeMap::<ApiIdentity, Vec<OrderSample>>::new();
    for (function, function_sites) in sites_by_function {
        let cfg = cfg_projection(cpg, function)?;
        for index in function_sites {
            let site = &sites[index];
            let Some(start) = cfg.dense_index(site.id) else {
                analysis
                    .issues
                    .insert(DeviantBehaviorIssue::CallOutsideCfg {
                        site: site.id,
                        function,
                    });
                continue;
            };
            match following_apis(
                &cfg,
                start,
                sites,
                &site_by_node,
                config.max_cfg_nodes_per_call,
            ) {
                Ok(following) => {
                    samples_by_api
                        .entry(site.api.clone())
                        .or_default()
                        .push(OrderSample {
                            site: index,
                            following,
                        })
                }
                Err(needed) => {
                    analysis
                        .issues
                        .insert(DeviantBehaviorIssue::CfgEvidenceCap {
                            site: site.id,
                            needed,
                            cap: config.max_cfg_nodes_per_call,
                        });
                }
            }
        }
    }

    for (predecessor, samples) in samples_by_api {
        if samples.len() < config.minimum_population {
            continue;
        }
        let mut support = BTreeMap::<ApiIdentity, Vec<usize>>::new();
        for sample in &samples {
            for successor in &sample.following {
                if successor != &predecessor {
                    support
                        .entry(successor.clone())
                        .or_default()
                        .push(sample.site);
                }
            }
        }
        for (successor, supporting) in support {
            let fraction = PopulationFraction::new(supporting.len(), samples.len());
            if !config.minimum_support.accepts(fraction) {
                continue;
            }
            let supporting_sites: Vec<_> =
                supporting.iter().map(|&index| sites[index].id).collect();
            analysis.call_order_beliefs.push(CallOrderBelief {
                predecessor: predecessor.clone(),
                successor: successor.clone(),
                support: fraction,
                supporting_sites: supporting_sites.clone(),
            });
            for sample in &samples {
                if !sample.following.contains(&successor) {
                    let site = &sites[sample.site];
                    analysis.findings.push(DeviantBehavior {
                        site: site.id,
                        function: site.function,
                        api: predecessor.clone(),
                        kind: DeviantBehaviorKind::MissingFollowingCall {
                            expected: successor.clone(),
                        },
                        peer_support: fraction,
                        supporting_sites: supporting_sites.clone(),
                    });
                }
            }
        }
    }
    Ok(())
}

fn following_apis(
    cfg: &super::super::GraphProjection,
    start: u32,
    sites: &[Site],
    site_by_node: &BTreeMap<NodeId, usize>,
    max_nodes: usize,
) -> Result<BTreeSet<ApiIdentity>, usize> {
    let mut following = BTreeSet::new();
    let mut reached = BTreeSet::from([start]);
    let mut queue = VecDeque::from([start]);
    while let Some(node) = queue.pop_front() {
        if node != start {
            if let Some(&site) = site_by_node.get(&cfg.node_id(node)) {
                following.insert(sites[site].api.clone());
            }
        }
        for &successor in cfg.successors(node) {
            if reached.insert(successor) {
                if reached.len() > max_nodes {
                    return Err(reached.len());
                }
                queue.push_back(successor);
            }
        }
    }
    Ok(following)
}

#[cfg(test)]
#[path = "deviant/tests.rs"]
mod tests;
