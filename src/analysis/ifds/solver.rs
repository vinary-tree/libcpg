//! Bounded deterministic fixpoint solving over call-graph SCCs.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::{CfgEdgeKind, CodePropertyGraph, CpgEdgeKind, CpgNodeKind, NodeId};

use super::{
    CalleeRef, IfdsCapEvent, IfdsCaps, IfdsIssue, IfdsOutput, Lattice, SummaryCtx, SummaryDomain,
};
use crate::analysis::{call_graph_sccs, function_scope_nodes, SccDecomposition, SccProjection};

/// Solves procedure summaries with the CPG's evidence-bounded call resolver.
///
/// Components are visited in deterministic callees-first order. Acyclic
/// singleton components are evaluated once; recursive components start at
/// lattice bottom and use bounded Kleene iteration. An incomplete result is
/// returned with ordered cap or structural evidence rather than panicking.
pub fn solve_summaries<D: SummaryDomain>(
    cpg: &CodePropertyGraph,
    domain: &D,
    call_graph: &SccDecomposition,
    caps: &IfdsCaps,
) -> IfdsOutput<D> {
    let resolver = |call_site| default_callee_ref(cpg, call_site, caps);
    solve_summaries_with_resolver(cpg, domain, call_graph, caps, &resolver)
}

/// Solves procedure summaries with a caller-supplied call-site resolver.
///
/// The resolver is useful when a program loader has authoritative information
/// about external or unresolved callees that is not represented by CPG edges.
/// A domain must treat a [`CalleeRef::Resolved`] target absent from
/// [`SummaryCtx::summaries`] conservatively; the framework never indexes such a
/// target and therefore remains total for out-of-program resolver results.
pub fn solve_summaries_with_resolver<D: SummaryDomain>(
    cpg: &CodePropertyGraph,
    domain: &D,
    call_graph: &SccDecomposition,
    caps: &IfdsCaps,
    resolve: &dyn Fn(NodeId) -> CalleeRef,
) -> IfdsOutput<D> {
    let mut output = IfdsOutput::default();
    let functions: Vec<_> = cpg.functions().map(|node| node.id).collect();
    let mut functions = functions;
    functions.sort_unstable();

    if functions.len() > caps.max_functions {
        record_cap(
            &mut output,
            IfdsCapEvent::Functions {
                observed: functions.len(),
                cap: caps.max_functions,
            },
        );
        return output;
    }

    output.issues = validate_call_graph(cpg, call_graph, &functions);
    if !output.issues.is_empty() {
        return output;
    }

    let Some(component_order) = reverse_topological_components(call_graph, caps, &mut output)
    else {
        return output;
    };

    collect_seed_facts(cpg, domain, &functions, caps, &mut output);
    if output.capped || !output.issues.is_empty() {
        return output;
    }

    for &function in &functions {
        output.summaries.insert(function, D::Summary::default());
    }

    let mut processed = BTreeSet::new();
    for component_id in component_order {
        let component = &call_graph.components[component_id];
        if component.cyclic {
            if !solve_cyclic_component(
                cpg,
                domain,
                component_id,
                &component.nodes,
                caps,
                resolve,
                &mut processed,
                &mut output,
            ) {
                break;
            }
        } else if !solve_acyclic_component(
            cpg,
            domain,
            component_id,
            component.nodes[0],
            caps,
            resolve,
            &mut processed,
            &mut output,
        ) {
            break;
        }
    }

    output
}

fn validate_call_graph(
    cpg: &CodePropertyGraph,
    call_graph: &SccDecomposition,
    functions: &[NodeId],
) -> BTreeSet<IfdsIssue> {
    let mut issues = BTreeSet::new();
    if call_graph.projection != SccProjection::CallGraph {
        issues.insert(IfdsIssue::NotCallGraph);
    }

    let function_set: BTreeSet<_> = functions.iter().copied().collect();
    let mut owner_by_node = BTreeMap::new();
    for (expected, component) in call_graph.components.iter().enumerate() {
        if component.id != expected {
            issues.insert(IfdsIssue::ComponentId {
                expected,
                actual: component.id,
            });
        }
        if component.nodes.is_empty() {
            issues.insert(IfdsIssue::EmptyComponent {
                component: expected,
            });
        }
        if component.nodes.windows(2).any(|pair| pair[0] >= pair[1]) {
            issues.insert(IfdsIssue::NonCanonicalMembers {
                component: expected,
            });
        }

        for &node in &component.nodes {
            if owner_by_node.insert(node, expected).is_some() {
                issues.insert(IfdsIssue::DuplicateMember { node });
            }
            if !function_set.contains(&node)
                || !cpg
                    .node(node)
                    .is_some_and(|node| matches!(node.kind, CpgNodeKind::Function { .. }))
            {
                issues.insert(IfdsIssue::NotAFunction { node });
            }
            if call_graph.component_by_node.get(&node).copied() != Some(expected) {
                issues.insert(IfdsIssue::Membership {
                    node,
                    component: Some(expected),
                });
            }
        }
    }

    for &function in functions {
        if !owner_by_node.contains_key(&function) {
            issues.insert(IfdsIssue::MissingFunction { node: function });
        }
    }
    for (&node, &component) in &call_graph.component_by_node {
        if owner_by_node.get(&node).copied() != Some(component) {
            issues.insert(IfdsIssue::Membership {
                node,
                component: owner_by_node.get(&node).copied(),
            });
        }
    }

    let component_count = call_graph.components.len();
    for edge in &call_graph.condensation_edges {
        if edge.source >= component_count
            || edge.target >= component_count
            || edge.source == edge.target
        {
            issues.insert(IfdsIssue::CondensationEdge {
                source: edge.source,
                target: edge.target,
            });
        }
    }
    if call_graph
        .condensation_edges
        .windows(2)
        .any(|pair| (pair[0].source, pair[0].target) >= (pair[1].source, pair[1].target))
    {
        issues.insert(IfdsIssue::NonCanonicalCondensation);
    }
    if condensation_is_cyclic(call_graph) {
        issues.insert(IfdsIssue::CyclicCondensation);
    }

    if call_graph != &call_graph_sccs(cpg) {
        issues.insert(IfdsIssue::StaleCallGraph);
    }
    issues
}

fn condensation_is_cyclic(call_graph: &SccDecomposition) -> bool {
    let component_count = call_graph.components.len();
    let mut indegree = vec![0usize; component_count];
    let mut successors = vec![BTreeSet::new(); component_count];
    for edge in &call_graph.condensation_edges {
        if edge.source >= component_count
            || edge.target >= component_count
            || edge.source == edge.target
        {
            continue;
        }
        if successors[edge.source].insert(edge.target) {
            indegree[edge.target] = indegree[edge.target].saturating_add(1);
        }
    }

    let mut ready: BTreeSet<_> = indegree
        .iter()
        .enumerate()
        .filter_map(|(component, &degree)| (degree == 0).then_some(component))
        .collect();
    let mut visited = 0usize;
    while let Some(component) = ready.pop_first() {
        visited = visited.saturating_add(1);
        for &target in &successors[component] {
            indegree[target] = indegree[target].saturating_sub(1);
            if indegree[target] == 0 {
                ready.insert(target);
            }
        }
    }
    visited != component_count
}

fn reverse_topological_components<D: SummaryDomain>(
    call_graph: &SccDecomposition,
    caps: &IfdsCaps,
    output: &mut IfdsOutput<D>,
) -> Option<Vec<usize>> {
    let component_count = call_graph.components.len();
    let mut remaining_callees = vec![0usize; component_count];
    let mut callers = vec![BTreeSet::new(); component_count];
    for edge in &call_graph.condensation_edges {
        remaining_callees[edge.source] = remaining_callees[edge.source].saturating_add(1);
        callers[edge.target].insert(edge.source);
    }

    let mut ready = BTreeSet::new();
    for (component, &remaining) in remaining_callees.iter().enumerate() {
        if remaining == 0 {
            let first = call_graph.components[component].nodes[0];
            ready.insert((first, component));
        }
    }

    let mut order = Vec::with_capacity(component_count);
    while let Some((_, component)) = ready.pop_first() {
        if !consume_work_step(caps, output) {
            return None;
        }
        order.push(component);
        for &caller in &callers[component] {
            remaining_callees[caller] = remaining_callees[caller].saturating_sub(1);
            if remaining_callees[caller] == 0 {
                let first = call_graph.components[caller].nodes[0];
                ready.insert((first, caller));
            }
        }
    }

    if order.len() != component_count {
        output.issues.insert(IfdsIssue::CyclicCondensation);
        return None;
    }
    Some(order)
}

fn collect_seed_facts<D: SummaryDomain>(
    cpg: &CodePropertyGraph,
    domain: &D,
    functions: &[NodeId],
    caps: &IfdsCaps,
    output: &mut IfdsOutput<D>,
) {
    for &function in functions {
        let scope = function_scope_nodes(cpg, function);
        let mut seeds = domain.seed_facts(cpg, function);
        seeds.sort_unstable();
        seeds.dedup();

        let mut retained: BTreeMap<NodeId, BTreeSet<D::Fact>> = BTreeMap::new();
        let mut observed_by_node = BTreeMap::<NodeId, usize>::new();
        for (node, fact) in seeds {
            if !cpg.contains_node(node) {
                output
                    .issues
                    .insert(IfdsIssue::UnknownSeedNode { function, node });
                continue;
            }
            if !scope.contains(&node) {
                output
                    .issues
                    .insert(IfdsIssue::SeedOutsideFunction { function, node });
                continue;
            }

            let observed = observed_by_node.entry(node).or_default();
            *observed = observed.saturating_add(1);
            if *observed <= caps.max_facts_per_node {
                retained.entry(node).or_default().insert(fact);
            }
        }

        for (node, observed) in observed_by_node {
            if observed > caps.max_facts_per_node {
                record_cap(
                    output,
                    IfdsCapEvent::FactsPerNode {
                        function,
                        node,
                        observed,
                        cap: caps.max_facts_per_node,
                    },
                );
            }
        }
        output.seed_facts.insert(function, retained);
    }
}

#[allow(clippy::too_many_arguments)]
fn solve_acyclic_component<D: SummaryDomain>(
    cpg: &CodePropertyGraph,
    domain: &D,
    component_id: usize,
    function: NodeId,
    caps: &IfdsCaps,
    resolve: &dyn Fn(NodeId) -> CalleeRef,
    processed: &mut BTreeSet<NodeId>,
    output: &mut IfdsOutput<D>,
) -> bool {
    if !consume_work_step(caps, output) {
        return false;
    }
    let candidate = {
        let ctx = SummaryCtx {
            summaries: &output.summaries,
            resolve,
            caps,
        };
        domain.summarize(cpg, function, &ctx)
    };
    record_processed(function, processed, output);
    output.component_iterations.insert(component_id, 1);
    merge_candidate(function, candidate, output).is_some()
}

#[allow(clippy::too_many_arguments)]
fn solve_cyclic_component<D: SummaryDomain>(
    cpg: &CodePropertyGraph,
    domain: &D,
    component_id: usize,
    functions: &[NodeId],
    caps: &IfdsCaps,
    resolve: &dyn Fn(NodeId) -> CalleeRef,
    processed: &mut BTreeSet<NodeId>,
    output: &mut IfdsOutput<D>,
) -> bool {
    if caps.max_scc_iters == 0 {
        output.component_iterations.insert(component_id, 0);
        record_cap(
            output,
            IfdsCapEvent::SccIterations {
                component: component_id,
                iterations: 0,
                cap: 0,
            },
        );
        return false;
    }

    let mut iterations = 0u32;
    loop {
        let mut changed = false;
        for &function in functions {
            if !consume_work_step(caps, output) {
                return false;
            }
            let candidate = {
                let ctx = SummaryCtx {
                    summaries: &output.summaries,
                    resolve,
                    caps,
                };
                domain.summarize(cpg, function, &ctx)
            };
            record_processed(function, processed, output);
            let Some(function_changed) = merge_candidate(function, candidate, output) else {
                return false;
            };
            changed |= function_changed;
        }

        iterations = iterations.saturating_add(1);
        output.component_iterations.insert(component_id, iterations);
        if !changed {
            return true;
        }
        if iterations >= caps.max_scc_iters {
            record_cap(
                output,
                IfdsCapEvent::SccIterations {
                    component: component_id,
                    iterations,
                    cap: caps.max_scc_iters,
                },
            );
            return false;
        }
    }
}

fn merge_candidate<D: SummaryDomain>(
    function: NodeId,
    candidate: D::Summary,
    output: &mut IfdsOutput<D>,
) -> Option<bool> {
    let current = output
        .summaries
        .get_mut(&function)
        .expect("every represented function has an initialized summary");

    if !current.subsumes(current) || !candidate.subsumes(&candidate) {
        output.issues.insert(IfdsIssue::LatticeLaw { function });
        return None;
    }
    let mut idempotence_probe = candidate.clone();
    if idempotence_probe.join(&candidate) || !idempotence_probe.subsumes(&candidate) {
        output.issues.insert(IfdsIssue::LatticeLaw { function });
        return None;
    }

    let already_subsumed = current.subsumes(&candidate);
    let mut joined = current.clone();
    let changed = joined.join(&candidate);
    if changed == already_subsumed || !joined.subsumes(current) || !joined.subsumes(&candidate) {
        output.issues.insert(IfdsIssue::LatticeLaw { function });
        return None;
    }
    if changed {
        *current = joined;
    }
    Some(changed)
}

fn record_processed<D: SummaryDomain>(
    function: NodeId,
    processed: &mut BTreeSet<NodeId>,
    output: &mut IfdsOutput<D>,
) {
    if processed.insert(function) {
        output.processed_functions.push(function);
    }
}

fn consume_work_step<D: SummaryDomain>(caps: &IfdsCaps, output: &mut IfdsOutput<D>) -> bool {
    if output.worklist_steps >= caps.max_worklist_steps {
        record_cap(
            output,
            IfdsCapEvent::WorklistSteps {
                needed: output.worklist_steps.saturating_add(1),
                cap: caps.max_worklist_steps,
            },
        );
        false
    } else {
        output.worklist_steps += 1;
        true
    }
}

fn record_cap<D: SummaryDomain>(output: &mut IfdsOutput<D>, event: IfdsCapEvent) {
    output.capped = true;
    output.cap_events.insert(event);
}

fn default_callee_ref(cpg: &CodePropertyGraph, call_site: NodeId, caps: &IfdsCaps) -> CalleeRef {
    let Some(call) = cpg.node(call_site) else {
        return CalleeRef::Unresolved(call_label(cpg, call_site, caps));
    };
    if !matches!(call.kind, CpgNodeKind::Call { .. }) {
        return CalleeRef::Unresolved(call_label(cpg, call_site, caps));
    }

    let mut targets = BTreeSet::new();
    if let CpgNodeKind::Call {
        target: Some(target),
        ..
    } = &call.kind
    {
        if cpg
            .node(*target)
            .is_some_and(|node| matches!(node.kind, CpgNodeKind::Function { .. }))
        {
            targets.insert(*target);
        }
    }
    targets.extend(cpg.outgoing_edges(call_site).filter_map(|edge| {
        ((edge.kind.is_call() || matches!(edge.kind, CpgEdgeKind::ControlFlow(CfgEdgeKind::Call)))
            && cpg
                .node(edge.target)
                .is_some_and(|node| matches!(node.kind, CpgNodeKind::Function { .. })))
        .then_some(edge.target)
    }));

    if targets.len() == 1 {
        CalleeRef::Resolved(*targets.first().expect("one resolved call target"))
    } else {
        CalleeRef::Unresolved(call_label(cpg, call_site, caps))
    }
}

fn call_label(cpg: &CodePropertyGraph, call_site: NodeId, caps: &IfdsCaps) -> Arc<str> {
    let mut stack: Vec<_> = cpg
        .ast_children(call_site)
        .iter()
        .copied()
        .rev()
        .map(|node| (node, 1u32))
        .collect();
    let mut visited = BTreeSet::new();
    let mut steps = 0usize;
    while let Some((node, depth)) = stack.pop() {
        if depth > caps.max_call_depth || steps >= caps.max_worklist_steps || !visited.insert(node)
        {
            continue;
        }
        steps = steps.saturating_add(1);
        if let Some(node) = cpg.node(node) {
            if let Some(name) = node.name().filter(|name| !name.is_empty()) {
                return Arc::from(name);
            }
            if let Some(text) = node.text.as_deref().filter(|text| !text.is_empty()) {
                return Arc::from(text);
            }
            let next_depth = depth.saturating_add(1);
            let mut children = cpg.ast_children(node.id).to_vec();
            children.reverse();
            stack.extend(children.into_iter().map(|child| (child, next_depth)));
        }
    }

    if let Some(text) = cpg
        .node(call_site)
        .and_then(|node| node.text.as_deref())
        .filter(|text| !text.is_empty())
    {
        Arc::from(text)
    } else {
        Arc::from(format!("call@{}", call_site.as_u32()))
    }
}
