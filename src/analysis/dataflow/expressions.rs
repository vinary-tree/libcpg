//! Available-expression and very-busy-expression analyses.

use std::collections::{BTreeMap, BTreeSet};

use crate::{CodePropertyGraph, NodeId};

use super::{
    expression_facts, solve, variable_facts, AnalysisError, BlockCfg, BlockId, Confluence,
    DataflowAnalysis, DataflowSolution, Direction, ExprFacts, ExprKey, MustBits, SolverConfig,
};
use crate::analysis::{cfg_projection, dominator_tree, DominatorTree};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Forward-Must available expressions for one function's reachable CFG.
///
/// An expression is available before a node exactly when every path from the
/// function entry has evaluated it without subsequently defining one of its
/// operands. Reported redundant pairs are conservative: the first occurrence
/// dominates the second occurrence, and no path between them defines an
/// operand before reaching the second.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct AvailableExpressions {
    /// Function whose reachable CFG was analyzed.
    pub function: NodeId,
    /// Expressions available immediately before each reachable CFG node.
    pub available_in: BTreeMap<NodeId, Vec<ExprKey>>,
    /// Dominating and redundant expression occurrences, ordered by node id.
    pub redundant: Vec<(NodeId, NodeId)>,
}

/// Backward-Must very-busy (anticipable) expressions for one function.
///
/// An expression is very busy after a node exactly when every path to a
/// modeled exit evaluates it before defining one of its operands.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct VeryBusyExpressions {
    /// Function whose reachable CFG was analyzed.
    pub function: NodeId,
    /// Expressions very busy immediately after each reachable CFG node.
    pub busy_out: BTreeMap<NodeId, Vec<ExprKey>>,
    /// Advisory `(expression, placement)` PRE candidates in canonical order.
    pub hoistable: Vec<(ExprKey, NodeId)>,
}

struct ExpressionTransfer<'facts> {
    facts: &'facts ExprFacts,
    killed: BTreeMap<NodeId, Vec<u32>>,
}

impl<'facts> ExpressionTransfer<'facts> {
    fn new(cpg: &CodePropertyGraph, function: NodeId, facts: &'facts ExprFacts) -> Self {
        let variable_facts = variable_facts(cpg, function);
        let mut expressions_by_variable = vec![Vec::new(); facts.variables.len()];
        for expression in 0..facts.universe.len() as u32 {
            for &variable in facts.variables_of(expression) {
                expressions_by_variable[variable as usize].push(expression);
            }
        }

        let mut killed = BTreeMap::new();
        for (&node, definitions) in &variable_facts.definitions {
            let mut at_node = BTreeSet::new();
            for definition in definitions {
                let name = variable_facts
                    .name(definition.variable)
                    .expect("definitions use the matching variable-fact universe");
                let Some(variable) = facts.variables.id_of(name) else {
                    continue;
                };
                at_node.extend(expressions_by_variable[variable as usize].iter().copied());
            }
            if !at_node.is_empty() {
                killed.insert(node, at_node.into_iter().collect());
            }
        }

        Self { facts, killed }
    }

    fn transfer_node(&self, node: NodeId, state: &mut MustBits) {
        for &expression in self.killed.get(&node).map_or(&[][..], Vec::as_slice) {
            state.remove(expression);
        }
        for &expression in self.facts.generated_at(node) {
            state.insert(expression);
        }
    }
}

struct AvailableAnalysis<'transfer> {
    transfer: &'transfer ExpressionTransfer<'transfer>,
}

impl DataflowAnalysis for AvailableAnalysis<'_> {
    type Domain = MustBits;

    const NAME: &'static str = "available expressions";
    const DIRECTION: Direction = Direction::Forward;
    const CONFLUENCE: Confluence = Confluence::Must;

    fn bottom(&self, _cfg: &BlockCfg) -> Self::Domain {
        MustBits::universe(self.transfer.facts.universe.len())
    }

    fn boundary(&self, _cfg: &BlockCfg) -> Self::Domain {
        MustBits::empty(self.transfer.facts.universe.len())
    }

    fn transfer_block(
        &self,
        _cpg: &CodePropertyGraph,
        cfg: &BlockCfg,
        block: BlockId,
        state: &mut Self::Domain,
    ) {
        let block = cfg
            .block(block)
            .expect("the solver transfers only blocks in its BlockCfg");
        for &node in &block.nodes {
            self.transfer.transfer_node(node, state);
        }
    }
}

struct VeryBusyAnalysis<'transfer> {
    transfer: &'transfer ExpressionTransfer<'transfer>,
}

impl DataflowAnalysis for VeryBusyAnalysis<'_> {
    type Domain = MustBits;

    const NAME: &'static str = "very-busy expressions";
    const DIRECTION: Direction = Direction::Backward;
    const CONFLUENCE: Confluence = Confluence::Must;

    fn bottom(&self, _cfg: &BlockCfg) -> Self::Domain {
        MustBits::universe(self.transfer.facts.universe.len())
    }

    fn boundary(&self, _cfg: &BlockCfg) -> Self::Domain {
        MustBits::empty(self.transfer.facts.universe.len())
    }

    fn transfer_block(
        &self,
        _cpg: &CodePropertyGraph,
        cfg: &BlockCfg,
        block: BlockId,
        state: &mut Self::Domain,
    ) {
        let block = cfg
            .block(block)
            .expect("the solver transfers only blocks in its BlockCfg");
        for &node in block.nodes.iter().rev() {
            self.transfer.transfer_node(node, state);
        }
    }
}

fn solve_available<'facts>(
    cpg: &CodePropertyGraph,
    function: NodeId,
    facts: &'facts ExprFacts,
) -> Result<
    (
        BlockCfg,
        ExpressionTransfer<'facts>,
        DataflowSolution<MustBits>,
    ),
    AnalysisError,
> {
    let cfg = super::super::block_cfg(cpg, function)?;
    let transfer = ExpressionTransfer::new(cpg, function, facts);
    let solution = solve(
        cpg,
        &cfg,
        &AvailableAnalysis {
            transfer: &transfer,
        },
        &SolverConfig::default(),
        None,
    )?;
    Ok((cfg, transfer, solution))
}

fn solve_very_busy<'facts>(
    cpg: &CodePropertyGraph,
    function: NodeId,
    facts: &'facts ExprFacts,
) -> Result<
    (
        BlockCfg,
        ExpressionTransfer<'facts>,
        DataflowSolution<MustBits>,
    ),
    AnalysisError,
> {
    let cfg = super::super::block_cfg(cpg, function)?;
    let transfer = ExpressionTransfer::new(cpg, function, facts);
    let solution = solve(
        cpg,
        &cfg,
        &VeryBusyAnalysis {
            transfer: &transfer,
        },
        &SolverConfig::default(),
        None,
    )?;
    Ok((cfg, transfer, solution))
}

fn expression_values(state: &MustBits, facts: &ExprFacts) -> Vec<ExprKey> {
    state
        .iter()
        .map(|expression| {
            facts
                .universe
                .get(expression)
                .expect("bit states use the matching expression-fact universe")
                .clone()
        })
        .collect()
}

fn forward_node_inputs(
    cfg: &BlockCfg,
    transfer: &ExpressionTransfer<'_>,
    solution: &DataflowSolution<MustBits>,
) -> BTreeMap<NodeId, MustBits> {
    let mut states = BTreeMap::new();
    for block in &cfg.blocks {
        let mut state = solution
            .before
            .get(block.id.0 as usize)
            .expect("the solver returns one input state per block")
            .clone();
        for &node in &block.nodes {
            states.insert(node, state.clone());
            transfer.transfer_node(node, &mut state);
        }
    }
    states
}

fn backward_node_outputs(
    cfg: &BlockCfg,
    transfer: &ExpressionTransfer<'_>,
    solution: &DataflowSolution<MustBits>,
) -> BTreeMap<NodeId, MustBits> {
    let mut states = BTreeMap::new();
    for block in &cfg.blocks {
        let mut state = solution
            .after
            .get(block.id.0 as usize)
            .expect("the solver returns one output state per block")
            .clone();
        for &node in block.nodes.iter().rev() {
            states.insert(node, state.clone());
            transfer.transfer_node(node, &mut state);
        }
    }
    states
}

fn dominating_occurrence(
    graph: &crate::analysis::GraphProjection,
    transfer: &ExpressionTransfer<'_>,
    dominators: &DominatorTree,
    occurrences: &BTreeSet<NodeId>,
    expression: u32,
    node: NodeId,
) -> Option<NodeId> {
    dominators.dominators_of(node).skip(1).find(|candidate| {
        occurrences.contains(candidate)
            && !has_intervening_kill(graph, transfer, expression, *candidate, node)
    })
}

fn reaches_without_passing(
    graph: &crate::analysis::GraphProjection,
    start: NodeId,
    target: NodeId,
    barrier: Option<NodeId>,
) -> bool {
    let (Some(start), Some(target)) = (graph.dense_index(start), graph.dense_index(target)) else {
        return false;
    };
    let barrier = barrier.and_then(|node| graph.dense_index(node));
    let mut seen = vec![false; graph.node_count()];
    let mut work = vec![start];
    seen[start as usize] = true;
    while let Some(node) = work.pop() {
        if node == target {
            return true;
        }
        for &successor in graph.successors(node).iter().rev() {
            if Some(successor) == barrier && successor != target {
                continue;
            }
            if !seen[successor as usize] {
                seen[successor as usize] = true;
                work.push(successor);
            }
        }
    }
    false
}

fn has_intervening_kill(
    graph: &crate::analysis::GraphProjection,
    transfer: &ExpressionTransfer<'_>,
    expression: u32,
    first: NodeId,
    second: NodeId,
) -> bool {
    transfer.killed.iter().any(|(&kill, expressions)| {
        kill != first
            && expressions.binary_search(&expression).is_ok()
            && reaches_without_passing(graph, first, kill, Some(second))
            && reaches_without_passing(graph, kill, second, None)
    })
}

fn nearest_common_dominator(dominators: &DominatorTree, occurrences: &[NodeId]) -> Option<NodeId> {
    let first = *occurrences.first()?;
    dominators.dominators_of(first).find(|&candidate| {
        occurrences
            .iter()
            .all(|&node| dominators.dominates(candidate, node))
    })
}

/// Computes forward-Must available expressions and conservative CSE pairs.
///
/// Results are advisory. Structural equality and syntactic operand definitions
/// do not model volatile access, overloaded operators, traps, floating-point
/// environments, or other language-specific side effects.
pub fn available_expressions(
    cpg: &CodePropertyGraph,
    function: NodeId,
) -> Result<AvailableExpressions, AnalysisError> {
    let facts = expression_facts(cpg, function);
    let (cfg, transfer, solution) = solve_available(cpg, function, &facts)?;
    let node_inputs = forward_node_inputs(&cfg, &transfer, &solution);
    let projection = cfg_projection(cpg, function)?;
    let dominators = dominator_tree(&projection, function)?;

    let available_in = node_inputs
        .iter()
        .map(|(&node, state)| (node, expression_values(state, &facts)))
        .collect();
    let mut redundant = Vec::new();
    for (expression, occurrences) in facts.occurrences.iter().enumerate() {
        let reachable: BTreeSet<_> = occurrences
            .iter()
            .copied()
            .filter(|node| cfg.block_of_node.contains_key(node))
            .collect();
        for &second in &reachable {
            if !node_inputs
                .get(&second)
                .is_some_and(|state| state.contains(expression as u32))
            {
                continue;
            }
            if let Some(first) = dominating_occurrence(
                &projection,
                &transfer,
                &dominators,
                &reachable,
                expression as u32,
                second,
            ) {
                redundant.push((first, second));
            }
        }
    }
    redundant.sort_unstable();
    redundant.dedup();

    Ok(AvailableExpressions {
        function,
        available_in,
        redundant,
    })
}

/// Computes backward-Must very-busy expressions and advisory PRE placements.
///
/// A placement is reported only for an expression with at least two reachable
/// evaluations. The placement is their nearest common dominator and the
/// expression is anticipable immediately before it. The report does not
/// rewrite the CPG and inherits the language-level limitations documented by
/// [`available_expressions`].
pub fn very_busy_expressions(
    cpg: &CodePropertyGraph,
    function: NodeId,
) -> Result<VeryBusyExpressions, AnalysisError> {
    let facts = expression_facts(cpg, function);
    let (cfg, transfer, solution) = solve_very_busy(cpg, function, &facts)?;
    let node_outputs = backward_node_outputs(&cfg, &transfer, &solution);
    let projection = cfg_projection(cpg, function)?;
    let dominators = dominator_tree(&projection, function)?;

    let busy_out = node_outputs
        .iter()
        .map(|(&node, state)| (node, expression_values(state, &facts)))
        .collect();
    let mut hoistable = Vec::new();
    for (expression, occurrences) in facts.occurrences.iter().enumerate() {
        let reachable: Vec<_> = occurrences
            .iter()
            .copied()
            .filter(|node| cfg.block_of_node.contains_key(node))
            .collect();
        if reachable.len() < 2 {
            continue;
        }
        let placement = nearest_common_dominator(&dominators, &reachable)
            .expect("reachable occurrences share the function entry as a dominator");
        let output = node_outputs
            .get(&placement)
            .expect("a reachable dominator has a materialized block-CFG state");
        let mut input = output.clone();
        transfer.transfer_node(placement, &mut input);
        if input.contains(expression as u32) {
            hoistable.push((
                facts
                    .universe
                    .get(expression as u32)
                    .expect("occurrences use the matching expression-fact universe")
                    .clone(),
                placement,
            ));
        }
    }
    hoistable.sort_unstable();
    hoistable.dedup();

    Ok(VeryBusyExpressions {
        function,
        busy_out,
        hoistable,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CfgEdgeKind, CpgEdgeKind, CpgNode, CpgNodeKind, ExprOperand, Language, LiteralKind,
        MethodSignature, SourceRange, Visibility,
    };
    use proptest::prelude::*;

    fn node(id: u32, kind: CpgNodeKind) -> CpgNode {
        CpgNode::new(NodeId::new(id), kind, SourceRange::default())
    }

    fn function_kind() -> CpgNodeKind {
        CpgNodeKind::Function {
            signature: MethodSignature {
                name: "expressions".into(),
                params: Default::default(),
                return_type: None,
                is_static: false,
                is_async: false,
                visibility: Visibility::Private,
            },
        }
    }

    fn identifier(name: &str) -> CpgNodeKind {
        CpgNodeKind::Identifier {
            name: name.into(),
            definition: None,
        }
    }

    fn connect_ast(cpg: &mut CodePropertyGraph, parent: NodeId, child: NodeId) {
        cpg.connect(parent, child, CpgEdgeKind::AstChild);
        cpg.node_mut(parent)
            .expect("AST parent")
            .children
            .push(child);
        cpg.node_mut(child).expect("AST child").parent = Some(parent);
    }

    fn connect_cfg(cpg: &mut CodePropertyGraph, source: NodeId, target: NodeId, kind: CfgEdgeKind) {
        cpg.connect(source, target, CpgEdgeKind::ControlFlow(kind));
    }

    fn expression_key() -> ExprKey {
        ExprKey {
            operator: "+".into(),
            operands: vec![ExprOperand::Variable("x".into()), ExprOperand::Integer(1)],
        }
    }

    fn add_expression(cpg: &mut CodePropertyGraph, expression: NodeId, child_base: u32) {
        let variable = cpg.add_node_with_id(node(child_base, identifier("x")));
        let literal = cpg.add_node_with_id(node(
            child_base + 1,
            CpgNodeKind::Literal {
                kind: LiteralKind::Integer(1),
            },
        ));
        connect_ast(cpg, expression, variable);
        connect_ast(cpg, expression, literal);
    }

    fn example_fixture() -> (CodePropertyGraph, [NodeId; 11]) {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let ids = std::array::from_fn(|index| NodeId::new(index as u32));
        let kinds = [
            function_kind(),
            CpgNodeKind::BinaryOp {
                operator: "+".into(),
            },
            CpgNodeKind::BinaryOp {
                operator: "+".into(),
            },
            CpgNodeKind::Assignment {
                operator: "=".into(),
            },
            CpgNodeKind::BinaryOp {
                operator: "+".into(),
            },
            identifier("condition"),
            CpgNodeKind::BinaryOp {
                operator: "+".into(),
            },
            CpgNodeKind::BinaryOp {
                operator: "+".into(),
            },
            identifier("join"),
            CpgNodeKind::BinaryOp {
                operator: "+".into(),
            },
            CpgNodeKind::Return,
        ];
        for (id, kind) in ids.into_iter().zip(kinds) {
            cpg.add_node_with_id(node(id.as_u32(), kind));
        }
        for &child in &ids[1..] {
            connect_ast(&mut cpg, ids[0], child);
        }
        for (offset, &expression) in [ids[1], ids[2], ids[4], ids[6], ids[7], ids[9]]
            .iter()
            .enumerate()
        {
            add_expression(&mut cpg, expression, 100 + offset as u32 * 2);
        }
        let assignment_target = cpg.add_node_with_id(node(200, identifier("x")));
        connect_ast(&mut cpg, ids[3], assignment_target);

        for pair in ids[..=5].windows(2) {
            connect_cfg(&mut cpg, pair[0], pair[1], CfgEdgeKind::Sequential);
        }
        connect_cfg(&mut cpg, ids[5], ids[6], CfgEdgeKind::ConditionalTrue);
        connect_cfg(&mut cpg, ids[5], ids[7], CfgEdgeKind::ConditionalFalse);
        connect_cfg(&mut cpg, ids[6], ids[8], CfgEdgeKind::Sequential);
        connect_cfg(&mut cpg, ids[7], ids[8], CfgEdgeKind::Sequential);
        connect_cfg(&mut cpg, ids[8], ids[9], CfgEdgeKind::Sequential);
        connect_cfg(&mut cpg, ids[9], ids[10], CfgEdgeKind::Sequential);
        (cpg, ids)
    }

    #[test]
    fn examples_distinguish_availability_kills_and_branch_anticipability() {
        let (cpg, ids) = example_fixture();
        let key = expression_key();
        let available = available_expressions(&cpg, ids[0]).expect("available expressions");
        let busy = very_busy_expressions(&cpg, ids[0]).expect("very-busy expressions");

        assert!(available.available_in[&ids[2]].contains(&key));
        assert!(!available.available_in[&ids[4]].contains(&key));
        assert!(available.available_in[&ids[8]].contains(&key));
        assert!(available.redundant.contains(&(ids[1], ids[2])));
        assert!(!available
            .redundant
            .iter()
            .any(|(_, second)| *second == ids[4]));

        assert!(busy.busy_out[&ids[5]].contains(&key));
        assert!(!busy.busy_out[&ids[2]].contains(&key));
        assert_eq!(busy.hoistable, vec![(key, ids[1])]);
    }

    #[test]
    fn reports_are_read_only_total_and_reject_invalid_selectors() {
        let (cpg, ids) = example_fixture();
        let before = format!("{cpg:?}");
        let available = available_expressions(&cpg, ids[0]).expect("available expressions");
        let busy = very_busy_expressions(&cpg, ids[0]).expect("very-busy expressions");
        assert_eq!(format!("{cpg:?}"), before);
        assert_eq!(available.function, ids[0]);
        assert_eq!(busy.function, ids[0]);
        assert_eq!(
            available_expressions(&cpg, NodeId::new(999)),
            Err(AnalysisError::UnknownNode(NodeId::new(999)))
        );
        assert_eq!(
            very_busy_expressions(&cpg, ids[1]),
            Err(AnalysisError::NotAFunction(ids[1]))
        );

        let mut bodyless = CodePropertyGraph::new(Language::Rust);
        let function = bodyless.add_node_with_id(node(0, function_kind()));
        assert_eq!(
            available_expressions(&bodyless, function)
                .expect("bodyless availability")
                .available_in,
            BTreeMap::from([(function, Vec::new())])
        );
        assert_eq!(
            very_busy_expressions(&bodyless, function)
                .expect("bodyless anticipability")
                .busy_out,
            BTreeMap::from([(function, Vec::new())])
        );
    }

    #[test]
    fn cyclic_control_flow_converges_with_must_path_semantics() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let function = cpg.add_node_with_id(node(0, function_kind()));
        let expression = cpg.add_node_with_id(node(
            1,
            CpgNodeKind::BinaryOp {
                operator: "+".into(),
            },
        ));
        let guard = cpg.add_node_with_id(node(2, identifier("condition")));
        let exit = cpg.add_node_with_id(node(3, CpgNodeKind::Return));
        for child in [expression, guard, exit] {
            connect_ast(&mut cpg, function, child);
        }
        add_expression(&mut cpg, expression, 100);
        connect_cfg(&mut cpg, function, expression, CfgEdgeKind::Sequential);
        connect_cfg(&mut cpg, expression, guard, CfgEdgeKind::Sequential);
        connect_cfg(&mut cpg, guard, expression, CfgEdgeKind::LoopBack);
        connect_cfg(&mut cpg, guard, exit, CfgEdgeKind::LoopExit);

        let key = expression_key();
        let available = available_expressions(&cpg, function).expect("cyclic availability");
        let busy = very_busy_expressions(&cpg, function).expect("cyclic anticipability");
        assert!(available.available_in[&guard].contains(&key));
        assert!(!available.available_in[&expression].contains(&key));
        assert!(!busy.busy_out[&guard].contains(&key));
    }

    #[test]
    fn branch_recomputation_does_not_reuse_a_killed_dominating_value() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let function = cpg.add_node_with_id(node(0, function_kind()));
        let old = cpg.add_node_with_id(node(
            1,
            CpgNodeKind::BinaryOp {
                operator: "+".into(),
            },
        ));
        let kill = cpg.add_node_with_id(node(
            2,
            CpgNodeKind::Assignment {
                operator: "=".into(),
            },
        ));
        let branch = cpg.add_node_with_id(node(3, identifier("condition")));
        let left = cpg.add_node_with_id(node(
            4,
            CpgNodeKind::BinaryOp {
                operator: "+".into(),
            },
        ));
        let right = cpg.add_node_with_id(node(
            5,
            CpgNodeKind::BinaryOp {
                operator: "+".into(),
            },
        ));
        let join = cpg.add_node_with_id(node(6, identifier("join")));
        let second = cpg.add_node_with_id(node(
            7,
            CpgNodeKind::BinaryOp {
                operator: "+".into(),
            },
        ));
        let exit = cpg.add_node_with_id(node(8, CpgNodeKind::Return));
        for child in [old, kill, branch, left, right, join, second, exit] {
            connect_ast(&mut cpg, function, child);
        }
        for (offset, expression) in [old, left, right, second].into_iter().enumerate() {
            add_expression(&mut cpg, expression, 100 + offset as u32 * 2);
        }
        let target = cpg.add_node_with_id(node(200, identifier("x")));
        connect_ast(&mut cpg, kill, target);
        connect_cfg(&mut cpg, function, old, CfgEdgeKind::Sequential);
        connect_cfg(&mut cpg, old, kill, CfgEdgeKind::Sequential);
        connect_cfg(&mut cpg, kill, branch, CfgEdgeKind::Sequential);
        connect_cfg(&mut cpg, branch, left, CfgEdgeKind::ConditionalTrue);
        connect_cfg(&mut cpg, branch, right, CfgEdgeKind::ConditionalFalse);
        connect_cfg(&mut cpg, left, join, CfgEdgeKind::Sequential);
        connect_cfg(&mut cpg, right, join, CfgEdgeKind::Sequential);
        connect_cfg(&mut cpg, join, second, CfgEdgeKind::Sequential);
        connect_cfg(&mut cpg, second, exit, CfgEdgeKind::Sequential);

        let available = available_expressions(&cpg, function).expect("availability");
        assert!(available.available_in[&second].contains(&expression_key()));
        assert!(!available
            .redundant
            .iter()
            .any(|(_, redundant)| *redundant == second));
    }

    #[test]
    fn unreachable_and_nested_expression_occurrences_stay_out_of_reports() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let function = cpg.add_node_with_id(node(0, function_kind()));
        let exit = cpg.add_node_with_id(node(1, CpgNodeKind::Return));
        let unreachable = cpg.add_node_with_id(node(
            2,
            CpgNodeKind::BinaryOp {
                operator: "+".into(),
            },
        ));
        let nested = cpg.add_node_with_id(node(3, function_kind()));
        let nested_expression = cpg.add_node_with_id(node(
            4,
            CpgNodeKind::BinaryOp {
                operator: "+".into(),
            },
        ));
        for child in [exit, unreachable, nested] {
            connect_ast(&mut cpg, function, child);
        }
        connect_ast(&mut cpg, nested, nested_expression);
        add_expression(&mut cpg, unreachable, 100);
        add_expression(&mut cpg, nested_expression, 200);
        connect_cfg(&mut cpg, function, exit, CfgEdgeKind::Sequential);

        let available = available_expressions(&cpg, function).expect("availability");
        let busy = very_busy_expressions(&cpg, function).expect("anticipability");
        for omitted in [unreachable, nested, nested_expression] {
            assert!(!available.available_in.contains_key(&omitted));
            assert!(!busy.busy_out.contains_key(&omitted));
        }
        assert!(available.redundant.is_empty());
        assert!(busy.hoistable.is_empty());
    }

    #[cfg(feature = "serde")]
    #[test]
    fn reports_round_trip_through_serde() {
        let (cpg, ids) = example_fixture();
        let available = available_expressions(&cpg, ids[0]).expect("available expressions");
        let busy = very_busy_expressions(&cpg, ids[0]).expect("very-busy expressions");
        let available_json = serde_json::to_string(&available).expect("serialize availability");
        let busy_json = serde_json::to_string(&busy).expect("serialize anticipability");
        assert_eq!(
            serde_json::from_str::<AvailableExpressions>(&available_json)
                .expect("deserialize availability"),
            available
        );
        assert_eq!(
            serde_json::from_str::<VeryBusyExpressions>(&busy_json)
                .expect("deserialize anticipability"),
            busy
        );
    }

    fn generated_fixture(
        actions: &[u8],
        raw_skips: &[(u8, u8)],
        reverse_nodes: bool,
        reverse_edges: bool,
    ) -> (CodePropertyGraph, NodeId, NodeId, Vec<Vec<usize>>) {
        let function = NodeId::new(0);
        let exit = NodeId::new(actions.len() as u32 + 1);
        let mut specifications = vec![(function, function_kind())];
        for (offset, action) in actions.iter().copied().enumerate() {
            let id = NodeId::new(offset as u32 + 1);
            let kind = match action % 3 {
                0 => CpgNodeKind::BinaryOp {
                    operator: "+".into(),
                },
                1 => CpgNodeKind::Assignment {
                    operator: "=".into(),
                },
                _ => identifier("neutral"),
            };
            specifications.push((id, kind));
            let child_base = 1_000 + offset as u32 * 3;
            match action % 3 {
                0 => {
                    specifications.push((NodeId::new(child_base), identifier("x")));
                    specifications.push((
                        NodeId::new(child_base + 1),
                        CpgNodeKind::Literal {
                            kind: LiteralKind::Integer(1),
                        },
                    ));
                }
                1 => specifications.push((NodeId::new(child_base), identifier("x"))),
                _ => {}
            }
        }
        specifications.push((exit, CpgNodeKind::Return));
        if reverse_nodes {
            specifications.reverse();
        }

        let mut cpg = CodePropertyGraph::new(Language::Rust);
        for (id, kind) in specifications {
            cpg.add_node_with_id(node(id.as_u32(), kind));
        }
        for (offset, action_kind) in actions.iter().copied().enumerate() {
            let action = NodeId::new(offset as u32 + 1);
            connect_ast(&mut cpg, function, action);
            let child_base = 1_000 + offset as u32 * 3;
            match action_kind % 3 {
                0 => {
                    connect_ast(&mut cpg, action, NodeId::new(child_base));
                    connect_ast(&mut cpg, action, NodeId::new(child_base + 1));
                }
                1 => connect_ast(&mut cpg, action, NodeId::new(child_base)),
                _ => {}
            }
        }
        connect_ast(&mut cpg, function, exit);

        let node_count = actions.len() + 2;
        let mut pairs: BTreeSet<_> = (0..node_count - 1).map(|i| (i, i + 1)).collect();
        for &(raw_source, raw_target) in raw_skips {
            let source = usize::from(raw_source) % node_count;
            let target = usize::from(raw_target) % node_count;
            if source + 1 < target {
                pairs.insert((source, target));
            }
        }
        let mut edges: Vec<_> = pairs.iter().copied().collect();
        if reverse_edges {
            edges.reverse();
        }
        for (source, target) in edges {
            let kind = if target == source + 1 {
                CfgEdgeKind::Sequential
            } else {
                CfgEdgeKind::ConditionalTrue
            };
            connect_cfg(
                &mut cpg,
                NodeId::new(source as u32),
                NodeId::new(target as u32),
                kind,
            );
        }

        let mut adjacency = vec![Vec::new(); node_count];
        for (source, target) in pairs {
            adjacency[source].push(target);
        }
        (cpg, function, exit, adjacency)
    }

    fn enumerate_paths(
        adjacency: &[Vec<usize>],
        current: usize,
        target: usize,
        path: &mut Vec<usize>,
        paths: &mut Vec<Vec<usize>>,
    ) {
        path.push(current);
        if current == target {
            paths.push(path.clone());
        } else {
            for &successor in &adjacency[current] {
                if successor <= target {
                    enumerate_paths(adjacency, successor, target, path, paths);
                }
            }
        }
        path.pop();
    }

    fn paths_between(adjacency: &[Vec<usize>], source: usize, target: usize) -> Vec<Vec<usize>> {
        let mut paths = Vec::new();
        enumerate_paths(adjacency, source, target, &mut Vec::new(), &mut paths);
        paths
    }

    fn available_on_path(path: &[usize], actions: &[u8]) -> bool {
        let mut available = false;
        for &node in path.iter().take(path.len().saturating_sub(1)) {
            if !(1..=actions.len()).contains(&node) {
                continue;
            }
            match actions[node - 1] % 3 {
                0 => available = true,
                1 => available = false,
                _ => {}
            }
        }
        available
    }

    fn busy_on_path(path: &[usize], actions: &[u8]) -> bool {
        for &node in path.iter().skip(1) {
            if !(1..=actions.len()).contains(&node) {
                continue;
            }
            match actions[node - 1] % 3 {
                0 => return true,
                1 => return false,
                _ => {}
            }
        }
        false
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(96))]

        #[test]
        fn must_results_equal_independent_path_oracles_and_are_deterministic(
            actions in prop::collection::vec(0u8..12, 1..=10),
            skips in prop::collection::vec((0u8..12, 0u8..12), 0..20),
        ) {
            let (cpg, function, exit, adjacency) = generated_fixture(&actions, &skips, false, false);
            let (permuted, permuted_function, _, _) =
                generated_fixture(&actions, &skips, true, true);
            let available = available_expressions(&cpg, function).expect("available expressions");
            let busy = very_busy_expressions(&cpg, function).expect("very-busy expressions");
            prop_assert_eq!(
                &available,
                &available_expressions(&permuted, permuted_function).expect("permuted availability")
            );
            prop_assert_eq!(
                &busy,
                &very_busy_expressions(&permuted, permuted_function).expect("permuted anticipability")
            );

            let key = expression_key();
            for node in 0..adjacency.len() {
                let id = NodeId::new(node as u32);
                let expected_available = paths_between(&adjacency, 0, node)
                    .iter()
                    .all(|path| available_on_path(path, &actions));
                let expected_busy = paths_between(&adjacency, node, exit.as_u32() as usize)
                    .iter()
                    .all(|path| busy_on_path(path, &actions));
                prop_assert_eq!(available.available_in[&id].contains(&key), expected_available);
                prop_assert_eq!(busy.busy_out[&id].contains(&key), expected_busy);
            }

            let projection = cfg_projection(&cpg, function).expect("CFG projection");
            let dominators = dominator_tree(&projection, function).expect("dominator tree");
            let occurrence_nodes: Vec<_> = actions
                .iter()
                .enumerate()
                .filter(|(_, action)| **action % 3 == 0)
                .map(|(offset, _)| NodeId::new(offset as u32 + 1))
                .collect();
            for &(first, second) in &available.redundant {
                prop_assert!(dominators.dominates(first, second));
                prop_assert!(available.available_in[&second].contains(&key));
                let preserves_first_value = paths_between(
                    &adjacency,
                    first.as_u32() as usize,
                    second.as_u32() as usize,
                )
                .iter()
                .all(|path| {
                    path.iter()
                        .skip(1)
                        .take(path.len().saturating_sub(2))
                        .all(|&node| actions[node - 1] % 3 != 1)
                });
                prop_assert!(preserves_first_value);
            }
            for (expression, placement) in &busy.hoistable {
                prop_assert_eq!(expression, &key);
                prop_assert!(occurrence_nodes
                    .iter()
                    .all(|&occurrence| dominators.dominates(*placement, occurrence)));
            }
        }
    }
}
