//! Backward variable liveness and advisory dead-store detection.

use std::{collections::BTreeMap, sync::Arc};

use crate::{CodePropertyGraph, DefinitionKind, NodeId};

use super::{
    solve, variable_facts, AnalysisError, BitSetDomain, BlockCfg, BlockId, Confluence,
    DataflowAnalysis, DataflowSolution, Direction, SolverConfig, VarFacts,
};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Per-node variable liveness for one function's reachable CFG.
///
/// Map values are sorted by variable name. Nodes outside the function's
/// reachable [`BlockCfg`] are deliberately absent.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct LivenessResult {
    /// Function whose reachable CFG was analyzed.
    pub function: NodeId,
    /// Variables live immediately before each reachable CFG node.
    pub live_in: BTreeMap<NodeId, Vec<Arc<str>>>,
    /// Variables live immediately after each reachable CFG node.
    pub live_out: BTreeMap<NodeId, Vec<Arc<str>>>,
}

/// One advisory definition whose value is not live immediately afterward.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct DeadStore {
    /// CPG node performing the definition.
    pub node: NodeId,
    /// Variable written by the definition.
    pub variable: Arc<str>,
    /// Syntactic definition classification.
    pub kind: DefinitionKind,
}

/// Deterministic advisory dead-store report for one function.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct DeadStoreReport {
    /// Function whose reachable CFG was analyzed.
    pub function: NodeId,
    /// Dead stores ordered by node and the canonical variable-fact order.
    pub dead: Vec<DeadStore>,
}

struct LivenessAnalysis<'facts> {
    facts: &'facts VarFacts,
}

impl LivenessAnalysis<'_> {
    fn transfer_node(&self, node: NodeId, state: &mut BitSetDomain) {
        for definition in self.facts.definitions_at(node) {
            state.remove(definition.variable);
        }
        for &variable in self.facts.uses_at(node) {
            state.insert(variable);
        }
    }
}

impl DataflowAnalysis for LivenessAnalysis<'_> {
    type Domain = BitSetDomain;

    const NAME: &'static str = "liveness";
    const DIRECTION: Direction = Direction::Backward;
    const CONFLUENCE: Confluence = Confluence::May;

    fn bottom(&self, _cfg: &BlockCfg) -> Self::Domain {
        BitSetDomain::new(self.facts.universe.len())
    }

    fn boundary(&self, _cfg: &BlockCfg) -> Self::Domain {
        BitSetDomain::new(self.facts.universe.len())
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
            self.transfer_node(node, state);
        }
    }
}

fn solve_function(
    cpg: &CodePropertyGraph,
    function: NodeId,
) -> Result<(BlockCfg, VarFacts, DataflowSolution<BitSetDomain>), AnalysisError> {
    let cfg = super::super::block_cfg(cpg, function)?;
    let facts = variable_facts(cpg, function);
    let solution = solve(
        cpg,
        &cfg,
        &LivenessAnalysis { facts: &facts },
        &SolverConfig::default(),
        None,
    )?;
    Ok((cfg, facts, solution))
}

fn names(state: &BitSetDomain, facts: &VarFacts) -> Vec<Arc<str>> {
    state
        .iter()
        .map(|variable| {
            facts
                .name(variable)
                .expect("bit states use the matching variable-fact universe")
                .clone()
        })
        .collect()
}

fn states_after_nodes(
    cfg: &BlockCfg,
    facts: &VarFacts,
    solution: &DataflowSolution<BitSetDomain>,
) -> BTreeMap<NodeId, BitSetDomain> {
    let analysis = LivenessAnalysis { facts };
    let mut states = BTreeMap::new();
    for block in &cfg.blocks {
        let mut state = solution
            .after
            .get(block.id.0 as usize)
            .expect("the solver returns one state per block")
            .clone();
        for &node in block.nodes.iter().rev() {
            states.insert(node, state.clone());
            analysis.transfer_node(node, &mut state);
        }
    }
    states
}

/// Computes exact backward-May variable liveness over a function's block CFG.
///
/// A node transfer removes variables defined at that node, then adds variables
/// used there. Successor states meet by union. The input CPG is never mutated;
/// unreachable AST nodes and nested functions are absent from the maps.
pub fn liveness(
    cpg: &CodePropertyGraph,
    function: NodeId,
) -> Result<LivenessResult, AnalysisError> {
    let (cfg, facts, solution) = solve_function(cpg, function)?;
    let analysis = LivenessAnalysis { facts: &facts };
    let node_outputs = states_after_nodes(&cfg, &facts, &solution);
    let mut live_in = BTreeMap::new();
    let mut live_out = BTreeMap::new();

    for (node, output) in node_outputs {
        let mut input = output.clone();
        analysis.transfer_node(node, &mut input);
        live_in.insert(node, names(&input, &facts));
        live_out.insert(node, names(&output, &facts));
    }

    Ok(LivenessResult {
        function,
        live_in,
        live_out,
    })
}

/// Reports reachable non-parameter definitions whose value is not live after
/// the defining node.
///
/// The result is advisory: syntactic liveness does not model reflection,
/// volatile writes, destructors, or other language-specific side effects.
pub fn dead_stores(
    cpg: &CodePropertyGraph,
    function: NodeId,
) -> Result<DeadStoreReport, AnalysisError> {
    let (cfg, facts, solution) = solve_function(cpg, function)?;
    let node_outputs = states_after_nodes(&cfg, &facts, &solution);
    let mut dead = Vec::new();

    for (&node, definitions) in &facts.definitions {
        let Some(output) = node_outputs.get(&node) else {
            continue;
        };
        for definition in definitions {
            if definition.kind == DefinitionKind::Parameter || output.contains(definition.variable)
            {
                continue;
            }
            let variable = facts
                .name(definition.variable)
                .expect("definitions use the matching variable-fact universe")
                .clone();
            dead.push(DeadStore {
                node,
                variable,
                kind: definition.kind,
            });
        }
    }

    Ok(DeadStoreReport { function, dead })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CfgEdgeKind, CpgEdgeKind, CpgNode, CpgNodeKind, Language, Lattice, MethodSignature,
        ScopeId, SourceRange, Visibility,
    };
    use proptest::prelude::*;

    fn node(id: u32, kind: CpgNodeKind) -> CpgNode {
        CpgNode::new(NodeId::new(id), kind, SourceRange::default())
    }

    fn function() -> CpgNodeKind {
        CpgNodeKind::Function {
            signature: MethodSignature {
                name: "live".into(),
                params: Default::default(),
                return_type: None,
                is_static: false,
                is_async: false,
                visibility: Visibility::Private,
            },
        }
    }

    fn variable(name: &str) -> CpgNodeKind {
        CpgNodeKind::Variable {
            name: name.into(),
            var_type: None,
            is_mutable: true,
            scope: ScopeId::GLOBAL,
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

    fn oracle_fixture() -> (CodePropertyGraph, NodeId, NodeId) {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let function = cpg.add_node_with_id(node(0, function()));
        let parameter = cpg.add_node_with_id(node(
            1,
            CpgNodeKind::Parameter {
                name: "p".into(),
                param_type: None,
                is_variadic: false,
            },
        ));
        let unused_parameter = cpg.add_node_with_id(node(
            2,
            CpgNodeKind::Parameter {
                name: "unused".into(),
                param_type: None,
                is_variadic: false,
            },
        ));
        let branch = cpg.add_node_with_id(node(3, identifier("p")));
        let left = cpg.add_node_with_id(node(4, variable("x")));
        let right = cpg.add_node_with_id(node(5, variable("x")));
        let join = cpg.add_node_with_id(node(6, identifier("x")));
        let dead = cpg.add_node_with_id(node(7, variable("dead")));
        let loop_guard = cpg.add_node_with_id(node(8, identifier("p")));
        let loop_def = cpg.add_node_with_id(node(9, variable("z")));
        let loop_use = cpg.add_node_with_id(node(10, identifier("z")));
        let exit = cpg.add_node_with_id(node(11, CpgNodeKind::Return));

        for child in [
            parameter,
            unused_parameter,
            branch,
            left,
            right,
            join,
            dead,
            loop_guard,
            loop_def,
            loop_use,
            exit,
        ] {
            connect_ast(&mut cpg, function, child);
        }
        for (source, target, kind) in [
            (function, parameter, CfgEdgeKind::Sequential),
            (parameter, unused_parameter, CfgEdgeKind::Sequential),
            (unused_parameter, branch, CfgEdgeKind::Sequential),
            (branch, left, CfgEdgeKind::ConditionalTrue),
            (branch, right, CfgEdgeKind::ConditionalFalse),
            (left, join, CfgEdgeKind::Sequential),
            (right, join, CfgEdgeKind::Sequential),
            (join, dead, CfgEdgeKind::Sequential),
            (dead, loop_guard, CfgEdgeKind::Sequential),
            (loop_guard, loop_def, CfgEdgeKind::ConditionalTrue),
            (loop_guard, exit, CfgEdgeKind::ConditionalFalse),
            (loop_def, loop_use, CfgEdgeKind::Sequential),
            (loop_use, loop_guard, CfgEdgeKind::LoopBack),
        ] {
            connect_cfg(&mut cpg, source, target, kind);
        }
        (cpg, function, dead)
    }

    fn round_robin(
        cpg: &CodePropertyGraph,
        cfg: &BlockCfg,
        facts: &VarFacts,
    ) -> DataflowSolution<BitSetDomain> {
        let analysis = LivenessAnalysis { facts };
        let width = facts.universe.len();
        let mut solution = DataflowSolution {
            before: (0..cfg.blocks.len())
                .map(|_| BitSetDomain::new(width))
                .collect(),
            after: (0..cfg.blocks.len())
                .map(|_| BitSetDomain::new(width))
                .collect(),
            visits: vec![0; cfg.blocks.len()],
        };
        let max_passes = cfg
            .blocks
            .len()
            .saturating_mul(width.max(1))
            .saturating_add(1);

        for _ in 0..max_passes {
            let mut changed = false;
            for block in &cfg.blocks {
                let index = block.id.0 as usize;
                let mut output = BitSetDomain::new(width);
                for edge in cfg.successors(block.id) {
                    output.join(&solution.before[edge.target.0 as usize]);
                }
                let mut input = output.clone();
                analysis.transfer_block(cpg, cfg, block.id, &mut input);
                changed |= solution.after[index] != output;
                changed |= solution.before[index] != input;
                solution.after[index] = output;
                solution.before[index] = input;
            }
            if !changed {
                return solution;
            }
        }
        panic!("finite round-robin liveness did not converge within its lattice bound");
    }

    #[test]
    fn solver_matches_naive_round_robin_on_a_diamond_and_loop() {
        let (cpg, function, _) = oracle_fixture();
        let cfg = super::super::super::block_cfg(&cpg, function).expect("block CFG");
        let facts = variable_facts(&cpg, function);
        let actual = solve(
            &cpg,
            &cfg,
            &LivenessAnalysis { facts: &facts },
            &SolverConfig::default(),
            None,
        )
        .expect("liveness solution");
        let expected = round_robin(&cpg, &cfg, &facts);
        assert_eq!(actual.before, expected.before);
        assert_eq!(actual.after, expected.after);
    }

    #[test]
    fn reports_only_reachable_non_parameter_definitions_not_live_afterward() {
        let (cpg, function, dead_node) = oracle_fixture();
        let graph_size = (cpg.node_count(), cpg.edge_count());
        let result = liveness(&cpg, function).expect("liveness");
        let report = dead_stores(&cpg, function).expect("dead stores");

        assert_eq!(report.dead.len(), 1);
        assert_eq!(report.dead[0].node, dead_node);
        assert_eq!(&*report.dead[0].variable, "dead");
        assert_eq!(report.dead[0].kind, DefinitionKind::Declaration);
        assert!(!result.live_out[&dead_node].contains(&Arc::from("dead")));
        assert!(report
            .dead
            .iter()
            .all(|store| store.kind != DefinitionKind::Parameter));
        assert_eq!((cpg.node_count(), cpg.edge_count()), graph_size);
    }

    #[test]
    fn invalid_function_selectors_preserve_analysis_errors() {
        let (cpg, _, _) = oracle_fixture();
        assert_eq!(
            liveness(&cpg, NodeId::new(99)),
            Err(AnalysisError::UnknownNode(NodeId::new(99)))
        );
        assert_eq!(
            dead_stores(&cpg, NodeId::new(3)),
            Err(AnalysisError::NotAFunction(NodeId::new(3)))
        );
    }

    #[test]
    fn assignment_kind_is_preserved_and_unreachable_definitions_are_omitted() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let function = cpg.add_node_with_id(node(0, function()));
        let assignment = cpg.add_node_with_id(node(
            1,
            CpgNodeKind::Assignment {
                operator: "=".into(),
            },
        ));
        let target = cpg.add_node_with_id(node(2, identifier("x")));
        let exit = cpg.add_node_with_id(node(3, CpgNodeKind::Return));
        let unreachable = cpg.add_node_with_id(node(4, variable("unreachable")));
        for child in [assignment, exit, unreachable] {
            connect_ast(&mut cpg, function, child);
        }
        connect_ast(&mut cpg, assignment, target);
        connect_cfg(&mut cpg, function, assignment, CfgEdgeKind::Sequential);
        connect_cfg(&mut cpg, assignment, exit, CfgEdgeKind::Sequential);

        let live = liveness(&cpg, function).expect("liveness");
        let dead = dead_stores(&cpg, function).expect("dead stores");
        assert_eq!(dead.dead.len(), 1);
        assert_eq!(dead.dead[0].node, assignment);
        assert_eq!(&*dead.dead[0].variable, "x");
        assert_eq!(dead.dead[0].kind, DefinitionKind::Assignment);
        assert!(!live.live_in.contains_key(&unreachable));
        assert!(!live.live_out.contains_key(&target));
    }

    #[test]
    fn bodyless_function_has_empty_fact_states() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let function = cpg.add_node_with_id(node(0, function()));
        let live = liveness(&cpg, function).expect("bodyless liveness");
        let dead = dead_stores(&cpg, function).expect("bodyless dead stores");
        assert_eq!(live.live_in, BTreeMap::from([(function, Vec::new())]));
        assert_eq!(live.live_out, BTreeMap::from([(function, Vec::new())]));
        assert!(dead.dead.is_empty());
    }

    #[cfg(feature = "serde")]
    #[test]
    fn reports_round_trip_through_serde() {
        let (cpg, function, _) = oracle_fixture();
        let liveness = liveness(&cpg, function).expect("liveness");
        let dead = dead_stores(&cpg, function).expect("dead stores");
        let live_json = serde_json::to_string(&liveness).expect("serialize liveness");
        let dead_json = serde_json::to_string(&dead).expect("serialize dead stores");
        assert_eq!(
            serde_json::from_str::<LivenessResult>(&live_json).expect("deserialize liveness"),
            liveness
        );
        assert_eq!(
            serde_json::from_str::<DeadStoreReport>(&dead_json).expect("deserialize dead stores"),
            dead
        );
    }

    fn generated_fixture(kinds: &[u8], reverse: bool) -> (CodePropertyGraph, NodeId) {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let root = NodeId::new(0);
        let mut order: Vec<_> = (0..=kinds.len()).collect();
        if reverse {
            order.reverse();
        }
        for index in order {
            let id = NodeId::new(index as u32);
            let kind = if index == 0 {
                function()
            } else {
                let name = format!("v{}", index % 5);
                match kinds[index - 1] % 3 {
                    0 => variable(&name),
                    1 => identifier(&name),
                    _ => CpgNodeKind::Return,
                }
            };
            cpg.add_node_with_id(node(id.as_u32(), kind));
        }
        for index in 1..=kinds.len() {
            connect_ast(&mut cpg, root, NodeId::new(index as u32));
        }
        let mut edges: Vec<_> = (0..kinds.len())
            .map(|index| {
                (
                    NodeId::new(index as u32),
                    NodeId::new((index + 1) as u32),
                    CfgEdgeKind::Sequential,
                )
            })
            .collect();
        for (index, kind) in kinds.iter().copied().enumerate() {
            if kinds.len() > 1 && kind & 0b1000 != 0 {
                let target = (usize::from(kind) % kinds.len()) + 1;
                edges.push((
                    NodeId::new((index + 1) as u32),
                    NodeId::new(target as u32),
                    if target <= index + 1 {
                        CfgEdgeKind::LoopBack
                    } else {
                        CfgEdgeKind::ConditionalTrue
                    },
                ));
            }
        }
        let kind_rank = |kind: CfgEdgeKind| match kind {
            CfgEdgeKind::Sequential => 0,
            CfgEdgeKind::ConditionalTrue => 1,
            CfgEdgeKind::LoopBack => 2,
            _ => 3,
        };
        edges.sort_unstable_by_key(|(source, target, kind)| (*source, *target, kind_rank(*kind)));
        edges.dedup();
        if reverse {
            edges.reverse();
        }
        for (source, target, kind) in edges {
            connect_cfg(&mut cpg, source, target, kind);
        }
        (cpg, root)
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(96))]

        #[test]
        fn reports_are_deterministic_under_node_and_edge_insertion_permutations(
            kinds in prop::collection::vec(any::<u8>(), 1..24),
        ) {
            let (forward, function) = generated_fixture(&kinds, false);
            let (reverse, _) = generated_fixture(&kinds, true);
            let first = liveness(&forward, function).expect("forward liveness");
            let repeated = liveness(&forward, function).expect("repeated liveness");
            let permuted = liveness(&reverse, function).expect("permuted liveness");
            prop_assert_eq!(&first, &repeated);
            prop_assert_eq!(&first, &permuted);

            let first_dead = dead_stores(&forward, function).expect("forward dead stores");
            let permuted_dead = dead_stores(&reverse, function).expect("permuted dead stores");
            prop_assert_eq!(&first_dead, &permuted_dead);
            for store in &first_dead.dead {
                prop_assert!(!first.live_out[&store.node].contains(&store.variable));
                prop_assert_ne!(store.kind, DefinitionKind::Parameter);
            }
        }
    }
}
