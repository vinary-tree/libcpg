//! Control Flow Graph extraction from CPG.
//!
//! This module extracts CFG edges from the AST portion of a Code Property
//! Graph. A typed heap-backed pushdown machine identifies control-flow
//! constructs and emits their edges without mapping source depth onto native
//! Rust call frames.

use rustc_hash::{FxHashMap, FxHashSet};
use smallvec::SmallVec;

use crate::{CfgEdgeKind, CodePropertyGraph, CpgEdgeKind, CpgNodeKind, CpgNodeKindTag, NodeId};

/// Configuration for CFG extraction.
#[derive(Debug, Clone)]
pub struct CfgExtractorConfig {
    /// Whether to create edges for implicit fallthrough.
    pub include_fallthrough: bool,
    /// Whether to track exception control flow.
    pub include_exceptions: bool,
    /// Whether to create edges for function calls.
    pub include_call_edges: bool,
}

impl Default for CfgExtractorConfig {
    fn default() -> Self {
        Self {
            include_fallthrough: true,
            include_exceptions: true,
            include_call_edges: true,
        }
    }
}

/// Stack-safe CFG extractor that adds control-flow edges to a CPG.
///
/// Source-shaped control is evaluated by a typed continuation machine whose
/// frames and intermediate exit sets live on the heap. Native call depth is
/// therefore independent of AST depth.
#[derive(Debug)]
pub struct CfgExtractor {
    config: CfgExtractorConfig,
}

type CfgExits = SmallVec<[NodeId; 4]>;

/// One instruction or continuation in CFG extraction's pushdown machine.
#[derive(Debug)]
enum CfgFrame {
    Enter(NodeId),
    Leave(NodeId),
    BlockAfterChild {
        block: NodeId,
        children: Vec<NodeId>,
        index: usize,
        all_exits: CfgExits,
        terminated: bool,
    },
    IfAfterThenWithoutElse {
        condition: NodeId,
    },
    IfAfterThen {
        if_node: NodeId,
        condition: NodeId,
        else_branch: NodeId,
    },
    IfAfterElse {
        if_node: NodeId,
        exits: CfgExits,
    },
    WhileAfterBody {
        condition: NodeId,
    },
    ForAfterBody {
        header: NodeId,
    },
    LoopAfterBody {
        header: NodeId,
    },
    MatchNext {
        match_node: NodeId,
        source: NodeId,
        arms: Vec<NodeId>,
        index: usize,
        exits: CfgExits,
    },
    MatchAfterArm {
        match_node: NodeId,
        source: NodeId,
        arms: Vec<NodeId>,
        index: usize,
        exits: CfgExits,
    },
    ReturnAfterExpression {
        return_node: NodeId,
    },
    ThrowAfterExpression {
        throw_node: NodeId,
    },
    CallAfterArgument {
        call: NodeId,
        arguments: Vec<NodeId>,
        next_index: usize,
    },
    SequentialAfterChild {
        children: Vec<NodeId>,
        index: usize,
    },
    TryAfterBody {
        try_node: NodeId,
        catches: Vec<NodeId>,
        finally: Option<NodeId>,
        exits: CfgExits,
    },
    TryCatchNext {
        try_node: NodeId,
        catches: Vec<NodeId>,
        index: usize,
        finally: Option<NodeId>,
        exits: CfgExits,
    },
    TryAfterCatch {
        try_node: NodeId,
        catches: Vec<NodeId>,
        next_index: usize,
        finally: Option<NodeId>,
        exits: CfgExits,
    },
    TryAfterFinally {
        try_node: NodeId,
        exits: CfgExits,
    },
}

impl CfgExtractor {
    /// Creates a new CFG extractor with default configuration.
    pub fn new() -> Self {
        Self {
            config: CfgExtractorConfig::default(),
        }
    }

    /// Creates a CFG extractor with custom configuration.
    pub fn with_config(config: CfgExtractorConfig) -> Self {
        Self { config }
    }

    /// Extracts CFG edges for all functions in the CPG.
    pub fn extract(&self, cpg: &mut CodePropertyGraph) {
        // Find all function nodes
        let functions: Vec<NodeId> = cpg.functions().map(|n| n.id).collect();

        for func_id in functions {
            self.extract_function_cfg(cpg, func_id);
        }
    }

    /// Executes the former recursive extractor on fixed-bounded test inputs.
    #[cfg(test)]
    fn extract_recursive_oracle(&self, cpg: &mut CodePropertyGraph) {
        let functions: Vec<NodeId> = cpg.functions().map(|node| node.id).collect();
        for function in functions {
            self.extract_function_cfg_recursive_oracle(cpg, function);
        }
    }

    /// Extracts CFG edges for a single function.
    pub fn extract_function_cfg(&self, cpg: &mut CodePropertyGraph, function: NodeId) {
        // Mark function entry
        cpg.add_cfg_entry(function);

        // Build CFG using a context that tracks loop and try structures
        let mut ctx = CfgContext::new();

        // Get function body (should be a Block child)
        let children = cpg.ast_children(function).to_vec();
        if let Some(&body) = children.last() {
            // Connect function to body entry
            cpg.connect_unique(
                function,
                body,
                CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
            );

            // Process the function body
            let exits = self.process_node(cpg, body, &mut ctx);

            // Mark all exit points
            for exit in exits {
                cpg.add_cfg_exit(exit);
            }
        }
    }

    /// Executes the former recursive equations on fixed-bounded test inputs.
    #[cfg(test)]
    fn extract_function_cfg_recursive_oracle(&self, cpg: &mut CodePropertyGraph, function: NodeId) {
        cpg.add_cfg_entry(function);
        let mut ctx = CfgContext::new();
        if let Some(&body) = cpg.ast_children(function).last() {
            cpg.connect_unique(
                function,
                body,
                CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
            );
            for exit in self.process_node_recursive(cpg, body, &mut ctx) {
                cpg.add_cfg_exit(exit);
            }
        }
    }

    /// Processes one AST region with a typed heap continuation machine.
    fn process_node(
        &self,
        cpg: &mut CodePropertyGraph,
        node_id: NodeId,
        ctx: &mut CfgContext,
    ) -> CfgExits {
        let mut frames = vec![CfgFrame::Enter(node_id)];
        let mut values: Vec<CfgExits> = Vec::new();

        while let Some(frame) = frames.pop() {
            match frame {
                CfgFrame::Enter(node) => {
                    let Some(kind) = cpg.node_tag(node) else {
                        values.push(smallvec::smallvec![node]);
                        continue;
                    };

                    // An active-path set, not a global visited set, gives a
                    // malformed back edge the same self-exit as the former
                    // recursive guard while still replaying shared DAG nodes.
                    if !ctx.on_path.insert(node) {
                        values.push(smallvec::smallvec![node]);
                        continue;
                    }
                    frames.push(CfgFrame::Leave(node));

                    match kind {
                        CpgNodeKindTag::Block => {
                            let children = cpg.ast_children(node).to_vec();
                            if let Some(&first) = children.first() {
                                cpg.connect_unique(
                                    node,
                                    first,
                                    CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
                                );
                                frames.push(CfgFrame::BlockAfterChild {
                                    block: node,
                                    children,
                                    index: 0,
                                    all_exits: SmallVec::new(),
                                    terminated: false,
                                });
                                frames.push(CfgFrame::Enter(first));
                            } else {
                                values.push(smallvec::smallvec![node]);
                            }
                        }
                        CpgNodeKindTag::If => {
                            let children = cpg.ast_children(node).to_vec();
                            match children.as_slice() {
                                [] => values.push(smallvec::smallvec![node]),
                                [condition] => {
                                    cpg.connect_unique(
                                        node,
                                        *condition,
                                        CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
                                    );
                                    values.push(smallvec::smallvec![*condition]);
                                }
                                [condition, then_branch] => {
                                    cpg.connect_unique(
                                        node,
                                        *condition,
                                        CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
                                    );
                                    cpg.connect_unique(
                                        *condition,
                                        *then_branch,
                                        CpgEdgeKind::ControlFlow(CfgEdgeKind::ConditionalTrue),
                                    );
                                    frames.push(CfgFrame::IfAfterThenWithoutElse {
                                        condition: *condition,
                                    });
                                    frames.push(CfgFrame::Enter(*then_branch));
                                }
                                [condition, then_branch, else_branch, ..] => {
                                    cpg.connect_unique(
                                        node,
                                        *condition,
                                        CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
                                    );
                                    cpg.connect_unique(
                                        *condition,
                                        *then_branch,
                                        CpgEdgeKind::ControlFlow(CfgEdgeKind::ConditionalTrue),
                                    );
                                    frames.push(CfgFrame::IfAfterThen {
                                        if_node: node,
                                        condition: *condition,
                                        else_branch: *else_branch,
                                    });
                                    frames.push(CfgFrame::Enter(*then_branch));
                                }
                            }
                        }
                        CpgNodeKindTag::While => {
                            let children = cpg.ast_children(node).to_vec();
                            let body = (children.len() >= 2)
                                .then(|| {
                                    children
                                        .iter()
                                        .rev()
                                        .copied()
                                        .find(|&child| {
                                            matches!(
                                                cpg.node(child).map(|node| &node.kind),
                                                Some(CpgNodeKind::Block { .. })
                                            )
                                        })
                                        .or_else(|| children.last().copied())
                                })
                                .flatten();
                            let condition = body.and_then(|body| {
                                children
                                    .iter()
                                    .copied()
                                    .filter(|&child| child != body)
                                    .find(|&child| !is_loop_syntax_token(cpg, child))
                                    .or_else(|| {
                                        children.iter().copied().find(|&child| child != body)
                                    })
                            });
                            if let (Some(body), Some(condition)) = (body, condition) {
                                ctx.push_loop(node, condition);
                                cpg.connect_unique(
                                    node,
                                    condition,
                                    CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
                                );
                                cpg.connect_unique(
                                    condition,
                                    body,
                                    CpgEdgeKind::ControlFlow(CfgEdgeKind::ConditionalTrue),
                                );
                                frames.push(CfgFrame::WhileAfterBody { condition });
                                frames.push(CfgFrame::Enter(body));
                            } else {
                                values.push(smallvec::smallvec![node]);
                            }
                        }
                        CpgNodeKindTag::For => {
                            let children = cpg.ast_children(node).to_vec();
                            if let Some(&body) = children.last() {
                                ctx.push_loop(node, node);
                                cpg.connect_unique(
                                    node,
                                    body,
                                    CpgEdgeKind::ControlFlow(CfgEdgeKind::ConditionalTrue),
                                );
                                frames.push(CfgFrame::ForAfterBody { header: node });
                                frames.push(CfgFrame::Enter(body));
                            } else {
                                values.push(smallvec::smallvec![node]);
                            }
                        }
                        CpgNodeKindTag::Loop => {
                            let children = cpg.ast_children(node).to_vec();
                            if let Some(body) = children
                                .iter()
                                .rev()
                                .copied()
                                .find(|&child| {
                                    matches!(
                                        cpg.node(child).map(|node| &node.kind),
                                        Some(CpgNodeKind::Block { .. })
                                    )
                                })
                                .or_else(|| children.first().copied())
                            {
                                ctx.push_loop(node, node);
                                cpg.connect_unique(
                                    node,
                                    body,
                                    CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
                                );
                                frames.push(CfgFrame::LoopAfterBody { header: node });
                                frames.push(CfgFrame::Enter(body));
                            } else {
                                values.push(smallvec::smallvec![node]);
                            }
                        }
                        CpgNodeKindTag::Match => {
                            let children = cpg.ast_children(node).to_vec();
                            if children.is_empty() {
                                values.push(smallvec::smallvec![node]);
                                continue;
                            }
                            let (source, arms_start) = if children.len() > 1
                                && !matches!(
                                    cpg.node(children[0]).map(|child| &child.kind),
                                    Some(CpgNodeKind::MatchArm)
                                ) {
                                cpg.connect_unique(
                                    node,
                                    children[0],
                                    CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
                                );
                                (children[0], 1)
                            } else {
                                (node, 0)
                            };
                            frames.push(CfgFrame::MatchNext {
                                match_node: node,
                                source,
                                arms: children[arms_start..].to_vec(),
                                index: 0,
                                exits: SmallVec::new(),
                            });
                        }
                        CpgNodeKindTag::Return => {
                            if let Some(&expression) = cpg.ast_children(node).first() {
                                cpg.connect_unique(
                                    node,
                                    expression,
                                    CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
                                );
                                frames.push(CfgFrame::ReturnAfterExpression { return_node: node });
                                frames.push(CfgFrame::Enter(expression));
                            } else {
                                values.push(smallvec::smallvec![node]);
                            }
                        }
                        CpgNodeKindTag::Break => {
                            if let Some(loop_context) = ctx.loop_stack.last_mut() {
                                loop_context.break_targets.push(node);
                                cpg.connect_unique(
                                    node,
                                    loop_context.loop_id,
                                    CpgEdgeKind::ControlFlow(CfgEdgeKind::Break),
                                );
                            }
                            values.push(SmallVec::new());
                        }
                        CpgNodeKindTag::Continue => {
                            if let Some(loop_context) = ctx.loop_stack.last() {
                                cpg.connect_unique(
                                    node,
                                    loop_context.continue_target,
                                    CpgEdgeKind::ControlFlow(CfgEdgeKind::Continue),
                                );
                            }
                            values.push(SmallVec::new());
                        }
                        CpgNodeKindTag::Try if self.config.include_exceptions => {
                            let children = cpg.ast_children(node).to_vec();
                            let mut body = None;
                            let mut catches = Vec::new();
                            let mut finally = None;
                            for child in children {
                                match cpg.node(child).map(|child| &child.kind) {
                                    Some(CpgNodeKind::Block { .. }) if body.is_none() => {
                                        body = Some(child);
                                    }
                                    Some(CpgNodeKind::Catch) => catches.push(child),
                                    Some(CpgNodeKind::Finally) => finally = Some(child),
                                    _ => {}
                                }
                            }
                            if let Some(body) = body {
                                cpg.connect_unique(
                                    node,
                                    body,
                                    CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
                                );
                                ctx.push_try(node, catches.clone());
                                frames.push(CfgFrame::TryAfterBody {
                                    try_node: node,
                                    catches,
                                    finally,
                                    exits: SmallVec::new(),
                                });
                                frames.push(CfgFrame::Enter(body));
                            } else {
                                frames.push(CfgFrame::TryCatchNext {
                                    try_node: node,
                                    catches,
                                    index: 0,
                                    finally,
                                    exits: SmallVec::new(),
                                });
                            }
                        }
                        CpgNodeKindTag::Throw if self.config.include_exceptions => {
                            if let Some(&expression) = cpg.ast_children(node).first() {
                                cpg.connect_unique(
                                    node,
                                    expression,
                                    CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
                                );
                                frames.push(CfgFrame::ThrowAfterExpression { throw_node: node });
                                frames.push(CfgFrame::Enter(expression));
                            } else {
                                Self::emit_throw_edges(cpg, node, ctx);
                                values.push(SmallVec::new());
                            }
                        }
                        CpgNodeKindTag::Throw => values.push(smallvec::smallvec![node]),
                        CpgNodeKindTag::Call if self.config.include_call_edges => {
                            let arguments = cpg.ast_children(node).to_vec();
                            if let Some(&argument) = arguments.first() {
                                cpg.connect_unique(
                                    node,
                                    argument,
                                    CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
                                );
                                frames.push(CfgFrame::CallAfterArgument {
                                    call: node,
                                    arguments,
                                    next_index: 1,
                                });
                                frames.push(CfgFrame::Enter(argument));
                            } else {
                                Self::emit_call_edge(cpg, node);
                                values.push(smallvec::smallvec![node]);
                            }
                        }
                        _ => Self::begin_sequential(cpg, node, &mut frames, &mut values),
                    }
                }
                CfgFrame::Leave(node) => {
                    ctx.on_path.remove(&node);
                }
                CfgFrame::BlockAfterChild {
                    block,
                    children,
                    index,
                    mut all_exits,
                    mut terminated,
                } => {
                    let child = children[index];
                    let child_exits = values.pop().expect("a block child produces one exit set");
                    let is_direct_terminator = matches!(
                        cpg.node(child).map(|child| &child.kind),
                        Some(CpgNodeKind::Return)
                            | Some(CpgNodeKind::Break)
                            | Some(CpgNodeKind::Continue)
                            | Some(CpgNodeKind::Throw)
                    );
                    let has_only_function_terminal_exits = !child_exits.is_empty()
                        && child_exits.iter().all(|&exit| {
                            matches!(
                                cpg.node(exit).map(|exit| &exit.kind),
                                Some(CpgNodeKind::Return) | Some(CpgNodeKind::Throw)
                            )
                        });
                    let is_terminator = is_direct_terminator || has_only_function_terminal_exits;
                    let current_exits = if is_terminator {
                        all_exits.extend(child_exits);
                        terminated = true;
                        SmallVec::new()
                    } else {
                        child_exits
                    };

                    let next_index = index + 1;
                    if let Some(&next) = children.get(next_index) {
                        for &exit in &current_exits {
                            cpg.connect_unique(
                                exit,
                                next,
                                CpgEdgeKind::ControlFlow(Self::continuation_edge_kind(
                                    cpg, exit, next,
                                )),
                            );
                        }
                        frames.push(CfgFrame::BlockAfterChild {
                            block,
                            children,
                            index: next_index,
                            all_exits,
                            terminated,
                        });
                        frames.push(CfgFrame::Enter(next));
                    } else {
                        if !terminated {
                            all_exits.extend(current_exits);
                        }
                        values.push(Self::exits_or_node(all_exits, block));
                    }
                }
                CfgFrame::IfAfterThenWithoutElse { condition } => {
                    let mut exits = values.pop().expect("an if branch produces one exit set");
                    exits.push(condition);
                    values.push(exits);
                }
                CfgFrame::IfAfterThen {
                    if_node,
                    condition,
                    else_branch,
                } => {
                    let exits = values.pop().expect("the then branch produces one exit set");
                    let actual_else = if matches!(
                        cpg.node(else_branch).map(|branch| &branch.kind),
                        Some(CpgNodeKind::Else)
                    ) {
                        cpg.ast_children(else_branch)
                            .first()
                            .copied()
                            .unwrap_or(else_branch)
                    } else {
                        else_branch
                    };
                    cpg.connect_unique(
                        condition,
                        actual_else,
                        CpgEdgeKind::ControlFlow(CfgEdgeKind::ConditionalFalse),
                    );
                    frames.push(CfgFrame::IfAfterElse { if_node, exits });
                    frames.push(CfgFrame::Enter(actual_else));
                }
                CfgFrame::IfAfterElse { if_node, mut exits } => {
                    exits.extend(values.pop().expect("the else branch produces one exit set"));
                    values.push(Self::exits_or_node(exits, if_node));
                }
                CfgFrame::WhileAfterBody { condition } => {
                    let body_exits = values.pop().expect("a while body produces one exit set");
                    for exit in body_exits {
                        cpg.connect_unique(
                            exit,
                            condition,
                            CpgEdgeKind::ControlFlow(CfgEdgeKind::LoopBack),
                        );
                    }
                    let loop_context = ctx.pop_loop();
                    let mut exits = smallvec::smallvec![condition];
                    exits.extend(loop_context.break_targets);
                    values.push(exits);
                }
                CfgFrame::ForAfterBody { header } => {
                    let body_exits = values.pop().expect("a for body produces one exit set");
                    for exit in body_exits {
                        cpg.connect_unique(
                            exit,
                            header,
                            CpgEdgeKind::ControlFlow(CfgEdgeKind::LoopBack),
                        );
                    }
                    let loop_context = ctx.pop_loop();
                    let mut exits = smallvec::smallvec![header];
                    exits.extend(loop_context.break_targets);
                    values.push(exits);
                }
                CfgFrame::LoopAfterBody { header } => {
                    let body_exits = values.pop().expect("a loop body produces one exit set");
                    for exit in body_exits {
                        cpg.connect_unique(
                            exit,
                            header,
                            CpgEdgeKind::ControlFlow(CfgEdgeKind::LoopBack),
                        );
                    }
                    values.push(ctx.pop_loop().break_targets.into_iter().collect());
                }
                CfgFrame::MatchNext {
                    match_node,
                    source,
                    arms,
                    index,
                    exits,
                } => {
                    let mut cursor = index;
                    let mut scheduled = false;
                    let mut exits = Some(exits);
                    while let Some(&arm) = arms.get(cursor) {
                        cursor += 1;
                        if !matches!(
                            cpg.node(arm).map(|arm| &arm.kind),
                            Some(CpgNodeKind::MatchArm)
                        ) {
                            continue;
                        }
                        cpg.connect_unique(
                            source,
                            arm,
                            CpgEdgeKind::ControlFlow(CfgEdgeKind::Case),
                        );
                        if let Some(&body) = cpg.ast_children(arm).last() {
                            cpg.connect_unique(
                                arm,
                                body,
                                CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
                            );
                            frames.push(CfgFrame::MatchAfterArm {
                                match_node,
                                source,
                                arms,
                                index: cursor,
                                exits: exits.take().expect("scheduled match transfers its exits"),
                            });
                            frames.push(CfgFrame::Enter(body));
                            scheduled = true;
                            break;
                        }
                        exits
                            .as_mut()
                            .expect("match exits remain available")
                            .push(arm);
                    }
                    if !scheduled {
                        values.push(Self::exits_or_node(
                            exits.expect("unscheduled match retains its exits"),
                            match_node,
                        ));
                    }
                }
                CfgFrame::MatchAfterArm {
                    match_node,
                    source,
                    arms,
                    index,
                    mut exits,
                } => {
                    exits.extend(values.pop().expect("a match arm produces one exit set"));
                    frames.push(CfgFrame::MatchNext {
                        match_node,
                        source,
                        arms,
                        index,
                        exits,
                    });
                }
                CfgFrame::ReturnAfterExpression { return_node } => {
                    let _ = values
                        .pop()
                        .expect("a return expression produces one exit set");
                    values.push(smallvec::smallvec![return_node]);
                }
                CfgFrame::ThrowAfterExpression { throw_node } => {
                    let _ = values
                        .pop()
                        .expect("a thrown expression produces one exit set");
                    Self::emit_throw_edges(cpg, throw_node, ctx);
                    values.push(SmallVec::new());
                }
                CfgFrame::CallAfterArgument {
                    call,
                    arguments,
                    next_index,
                } => {
                    let _ = values.pop().expect("a call argument produces one exit set");
                    if let Some(&argument) = arguments.get(next_index) {
                        cpg.connect_unique(
                            call,
                            argument,
                            CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
                        );
                        frames.push(CfgFrame::CallAfterArgument {
                            call,
                            arguments,
                            next_index: next_index + 1,
                        });
                        frames.push(CfgFrame::Enter(argument));
                    } else {
                        Self::emit_call_edge(cpg, call);
                        values.push(smallvec::smallvec![call]);
                    }
                }
                CfgFrame::SequentialAfterChild { children, index } => {
                    let exits = values
                        .pop()
                        .expect("a sequential child produces one exit set");
                    let next_index = index + 1;
                    if let Some(&next) = children.get(next_index) {
                        for &exit in &exits {
                            cpg.connect_unique(
                                exit,
                                next,
                                CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
                            );
                        }
                        frames.push(CfgFrame::SequentialAfterChild {
                            children,
                            index: next_index,
                        });
                        frames.push(CfgFrame::Enter(next));
                    } else {
                        values.push(exits);
                    }
                }
                CfgFrame::TryAfterBody {
                    try_node,
                    catches,
                    finally,
                    mut exits,
                } => {
                    let body_exits = values.pop().expect("a try body produces one exit set");
                    ctx.pop_try();
                    if let Some(finally) = finally {
                        for exit in body_exits {
                            cpg.connect_unique(
                                exit,
                                finally,
                                CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
                            );
                        }
                    } else {
                        exits.extend(body_exits);
                    }
                    frames.push(CfgFrame::TryCatchNext {
                        try_node,
                        catches,
                        index: 0,
                        finally,
                        exits,
                    });
                }
                CfgFrame::TryCatchNext {
                    try_node,
                    catches,
                    index,
                    finally,
                    exits,
                } => {
                    let mut cursor = index;
                    let mut catch_body = None;
                    while let Some(&catch) = catches.get(cursor) {
                        cursor += 1;
                        if let Some(&body) = cpg.ast_children(catch).last() {
                            catch_body = Some((catch, body));
                            break;
                        }
                    }
                    if let Some((catch, body)) = catch_body {
                        cpg.connect_unique(
                            catch,
                            body,
                            CpgEdgeKind::ControlFlow(CfgEdgeKind::Catch),
                        );
                        frames.push(CfgFrame::TryAfterCatch {
                            try_node,
                            catches,
                            next_index: cursor,
                            finally,
                            exits,
                        });
                        frames.push(CfgFrame::Enter(body));
                    } else if let Some(finally) = finally {
                        frames.push(CfgFrame::TryAfterFinally { try_node, exits });
                        frames.push(CfgFrame::Enter(finally));
                    } else {
                        values.push(Self::exits_or_node(exits, try_node));
                    }
                }
                CfgFrame::TryAfterCatch {
                    try_node,
                    catches,
                    next_index,
                    finally,
                    mut exits,
                } => {
                    let catch_exits = values.pop().expect("a catch body produces one exit set");
                    if let Some(finally) = finally {
                        for exit in catch_exits {
                            cpg.connect_unique(
                                exit,
                                finally,
                                CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
                            );
                        }
                    } else {
                        exits.extend(catch_exits);
                    }
                    frames.push(CfgFrame::TryCatchNext {
                        try_node,
                        catches,
                        index: next_index,
                        finally,
                        exits,
                    });
                }
                CfgFrame::TryAfterFinally {
                    try_node,
                    mut exits,
                } => {
                    exits.extend(values.pop().expect("a finally block produces one exit set"));
                    values.push(Self::exits_or_node(exits, try_node));
                }
            }
        }

        assert_eq!(
            values.len(),
            1,
            "one CFG input node must produce exactly one exit set"
        );
        values.pop().expect("the CFG machine produced its result")
    }

    fn begin_sequential(
        cpg: &mut CodePropertyGraph,
        node: NodeId,
        frames: &mut Vec<CfgFrame>,
        values: &mut Vec<CfgExits>,
    ) {
        let children = cpg.ast_children(node).to_vec();
        if let Some(&first) = children.first() {
            cpg.connect_unique(
                node,
                first,
                CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
            );
            frames.push(CfgFrame::SequentialAfterChild { children, index: 0 });
            frames.push(CfgFrame::Enter(first));
        } else {
            values.push(smallvec::smallvec![node]);
        }
    }

    fn emit_call_edge(cpg: &mut CodePropertyGraph, call: NodeId) {
        if let Some(CpgNodeKind::Call {
            target: Some(target),
            ..
        }) = cpg.node(call).map(|node| &node.kind)
        {
            cpg.connect_unique(call, *target, CpgEdgeKind::ControlFlow(CfgEdgeKind::Call));
        }
    }

    fn emit_throw_edges(cpg: &mut CodePropertyGraph, throw_node: NodeId, ctx: &CfgContext) {
        if let Some(try_context) = ctx.try_stack.last() {
            for &catch in &try_context.catch_handlers {
                cpg.connect_unique(
                    throw_node,
                    catch,
                    CpgEdgeKind::ControlFlow(CfgEdgeKind::Throw),
                );
            }
        }
    }

    fn exits_or_node(exits: CfgExits, node: NodeId) -> CfgExits {
        if exits.is_empty() {
            smallvec::smallvec![node]
        } else {
            exits
        }
    }

    /// Former recursive specification retained only for fixed-bounded tests.
    #[cfg(test)]
    fn process_node_recursive(
        &self,
        cpg: &mut CodePropertyGraph,
        node_id: NodeId,
        ctx: &mut CfgContext,
    ) -> SmallVec<[NodeId; 4]> {
        let kind = match cpg.node(node_id) {
            Some(n) => n.kind.clone(),
            None => return smallvec::smallvec![node_id],
        };

        // The former `process_node` and `process_*` handlers were mutually recursive
        // over `ast_children`. A well-formed AST is a tree, so every node is
        // entered exactly once and this guard never fires. A *malformed* graph
        // can contain an `AstChild` cycle, which would otherwise send the
        // descent into unbounded recursion and overflow the stack — a crash
        // reachable from any caller that hand-builds a CPG. Refusing to
        // re-enter a node already on the current path cuts the cycle (the node
        // becomes its own exit) and leaves tree inputs bit-for-bit unchanged.
        // A *path* set rather than a global visited set is used so that a node
        // legitimately reachable twice from disjoint branches is still
        // processed each time.
        if !ctx.on_path.insert(node_id) {
            return smallvec::smallvec![node_id];
        }

        let exits = match &kind {
            CpgNodeKind::Block { .. } => self.recursive_process_block(cpg, node_id, ctx),
            CpgNodeKind::If => self.recursive_process_if(cpg, node_id, ctx),
            CpgNodeKind::While => self.recursive_process_while(cpg, node_id, ctx),
            CpgNodeKind::For => self.recursive_process_for(cpg, node_id, ctx),
            CpgNodeKind::Loop => self.recursive_process_loop(cpg, node_id, ctx),
            CpgNodeKind::Match => self.recursive_process_match(cpg, node_id, ctx),
            CpgNodeKind::Return => self.recursive_process_return(cpg, node_id, ctx),
            CpgNodeKind::Break => self.recursive_process_break(cpg, node_id, ctx),
            CpgNodeKind::Continue => self.recursive_process_continue(cpg, node_id, ctx),
            CpgNodeKind::Try => self.recursive_process_try(cpg, node_id, ctx),
            CpgNodeKind::Throw => self.recursive_process_throw(cpg, node_id, ctx),
            CpgNodeKind::Call { .. } if self.config.include_call_edges => {
                self.recursive_process_call(cpg, node_id, ctx)
            }
            _ => {
                // For other nodes, process children sequentially
                self.recursive_process_sequential(cpg, node_id, ctx)
            }
        };

        ctx.on_path.remove(&node_id);
        exits
    }

    /// Processes a block (sequence of statements).
    #[cfg(test)]
    fn recursive_process_block(
        &self,
        cpg: &mut CodePropertyGraph,
        block_id: NodeId,
        ctx: &mut CfgContext,
    ) -> SmallVec<[NodeId; 4]> {
        let children = cpg.ast_children(block_id).to_vec();

        if children.is_empty() {
            return smallvec::smallvec![block_id];
        }

        // Connect block entry to first child
        if let Some(&first) = children.first() {
            cpg.connect_unique(
                block_id,
                first,
                CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
            );
        }

        // Process each child and chain them together
        let mut current_exits: SmallVec<[NodeId; 4]> = smallvec::smallvec![];
        let mut all_exits: SmallVec<[NodeId; 4]> = smallvec::smallvec![];
        let mut terminated = false;

        for (i, &child_id) in children.iter().enumerate() {
            // Connect previous exits to this child
            for &exit in &current_exits {
                cpg.connect_unique(
                    exit,
                    child_id,
                    CpgEdgeKind::ControlFlow(Self::continuation_edge_kind(cpg, exit, child_id)),
                );
            }

            // Process this child
            let child_exits = self.process_node_recursive(cpg, child_id, ctx);

            // Check if this child terminates (return, break, continue, throw)
            let child_kind = cpg.node(child_id).map(|n| n.kind.clone());
            let is_direct_terminator = matches!(
                child_kind.as_ref(),
                Some(CpgNodeKind::Return)
                    | Some(CpgNodeKind::Break)
                    | Some(CpgNodeKind::Continue)
                    | Some(CpgNodeKind::Throw)
            );
            // Several grammars retain a transparent statement wrapper around
            // `return` (Rust's `expression_statement` is one example). Branch
            // constructs can likewise expose only terminal exits when every
            // arm returns. Propagate that fact instead of wiring the terminal
            // node into the following statement. Break/continue exits are not
            // included here: loop handlers deliberately promote those nodes
            // to ordinary exits of the enclosing loop.
            let has_only_function_terminal_exits = !child_exits.is_empty()
                && child_exits.iter().all(|&exit| {
                    matches!(
                        cpg.node(exit).map(|node| &node.kind),
                        Some(CpgNodeKind::Return) | Some(CpgNodeKind::Throw)
                    )
                });
            let is_terminator = is_direct_terminator || has_only_function_terminal_exits;

            if is_terminator {
                // Terminator doesn't pass control to next statement
                all_exits.extend(child_exits);
                terminated = true;

                // If there are more children after this, they're unreachable
                // but we still process them for completeness
                if i + 1 < children.len() {
                    current_exits.clear();
                }
            } else {
                current_exits = child_exits;
            }
        }

        if !terminated {
            all_exits.extend(current_exits);
        }

        if all_exits.is_empty() {
            smallvec::smallvec![block_id]
        } else {
            all_exits
        }
    }

    /// Classifies the fallthrough from one completed statement to the next.
    ///
    /// Loop handlers deliberately return their normal and `break` exits to the
    /// enclosing block. That block owns the one continuation-emission site, so it
    /// can retype a formerly sequential edge without adding a second edge. An
    /// exit leaves a loop when the source is nested in that loop (or is its header)
    /// and the next statement is not.
    fn continuation_edge_kind(
        cpg: &CodePropertyGraph,
        source: NodeId,
        target: NodeId,
    ) -> CfgEdgeKind {
        let mut target_scope: FxHashSet<NodeId> = cpg.ast_ancestors(target).into_iter().collect();
        target_scope.insert(target);

        if std::iter::once(source)
            .chain(cpg.ast_ancestors(source))
            .filter_map(|node| cpg.node(node).map(|node| (node.id, &node.kind)))
            .any(|(node, kind)| {
                let child_count = cpg.ast_children(node).len();
                let is_well_formed_loop = match kind {
                    CpgNodeKind::While => child_count >= 2,
                    CpgNodeKind::For | CpgNodeKind::Loop => child_count >= 1,
                    _ => false,
                };
                is_well_formed_loop && !target_scope.contains(&node)
            })
        {
            CfgEdgeKind::LoopExit
        } else {
            CfgEdgeKind::Sequential
        }
    }

    /// Processes an if statement.
    #[cfg(test)]
    fn recursive_process_if(
        &self,
        cpg: &mut CodePropertyGraph,
        if_id: NodeId,
        ctx: &mut CfgContext,
    ) -> SmallVec<[NodeId; 4]> {
        let children = cpg.ast_children(if_id).to_vec();
        let mut exits = SmallVec::new();

        // Children: [condition, then_branch, else_branch?]
        // The condition is typically the first child

        match children.len() {
            0 => {
                return smallvec::smallvec![if_id];
            }
            1 => {
                // Just condition, no branches - unusual but handle it
                cpg.connect_unique(
                    if_id,
                    children[0],
                    CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
                );
                exits.push(children[0]);
            }
            2 => {
                // condition + then branch, no else
                let condition = children[0];
                let then_branch = children[1];

                // if -> condition
                cpg.connect_unique(
                    if_id,
                    condition,
                    CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
                );

                // condition -> then (true)
                cpg.connect_unique(
                    condition,
                    then_branch,
                    CpgEdgeKind::ControlFlow(CfgEdgeKind::ConditionalTrue),
                );

                // Process then branch
                let then_exits = self.process_node_recursive(cpg, then_branch, ctx);
                exits.extend(then_exits);

                // If no else, condition itself is an exit (false case falls through)
                exits.push(condition);
            }
            _ => {
                // condition + then + else (or more complex)
                let condition = children[0];
                let then_branch = children[1];

                // if -> condition
                cpg.connect_unique(
                    if_id,
                    condition,
                    CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
                );

                // condition -> then (true)
                cpg.connect_unique(
                    condition,
                    then_branch,
                    CpgEdgeKind::ControlFlow(CfgEdgeKind::ConditionalTrue),
                );

                // Process then branch
                let then_exits = self.process_node_recursive(cpg, then_branch, ctx);
                exits.extend(then_exits);

                // Handle else branch (might be Else node or direct block)
                let else_branch = children[2];
                let else_kind = cpg.node(else_branch).map(|n| n.kind.clone());

                // Determine actual else content
                let actual_else = if matches!(else_kind.as_ref(), Some(CpgNodeKind::Else)) {
                    // Get the block inside the Else node
                    cpg.ast_children(else_branch)
                        .first()
                        .copied()
                        .unwrap_or(else_branch)
                } else {
                    else_branch
                };

                // condition -> else (false)
                cpg.connect_unique(
                    condition,
                    actual_else,
                    CpgEdgeKind::ControlFlow(CfgEdgeKind::ConditionalFalse),
                );

                // Process else branch
                let else_exits = self.process_node_recursive(cpg, actual_else, ctx);
                exits.extend(else_exits);
            }
        }

        if exits.is_empty() {
            smallvec::smallvec![if_id]
        } else {
            exits
        }
    }

    /// Processes a while loop.
    #[cfg(test)]
    fn recursive_process_while(
        &self,
        cpg: &mut CodePropertyGraph,
        while_id: NodeId,
        ctx: &mut CfgContext,
    ) -> SmallVec<[NodeId; 4]> {
        let children = cpg.ast_children(while_id).to_vec();

        if children.len() < 2 {
            return smallvec::smallvec![while_id];
        }

        let Some(body) = children
            .iter()
            .rev()
            .copied()
            .find(|&child| {
                matches!(
                    cpg.node(child).map(|node| &node.kind),
                    Some(CpgNodeKind::Block { .. })
                )
            })
            .or_else(|| children.last().copied())
        else {
            return smallvec::smallvec![while_id];
        };
        let Some(condition) = children
            .iter()
            .copied()
            .filter(|&child| child != body)
            .find(|&child| !is_loop_syntax_token(cpg, child))
            .or_else(|| children.iter().copied().find(|&child| child != body))
        else {
            return smallvec::smallvec![while_id];
        };

        // Push loop context for break/continue handling
        ctx.push_loop(while_id, condition);

        // while -> condition
        cpg.connect_unique(
            while_id,
            condition,
            CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
        );

        // condition -> body (true)
        cpg.connect_unique(
            condition,
            body,
            CpgEdgeKind::ControlFlow(CfgEdgeKind::ConditionalTrue),
        );

        // Process body
        let body_exits = self.process_node_recursive(cpg, body, ctx);

        // body exits -> condition (loop back)
        for &exit in &body_exits {
            cpg.connect_unique(
                exit,
                condition,
                CpgEdgeKind::ControlFlow(CfgEdgeKind::LoopBack),
            );
        }

        // Pop loop context and collect break targets
        let loop_ctx = ctx.pop_loop();

        // Build exits: condition (false) + break targets
        let mut exits: SmallVec<[NodeId; 4]> = smallvec::smallvec![condition];
        exits.extend(loop_ctx.break_targets);

        exits
    }

    /// Processes a for loop.
    #[cfg(test)]
    fn recursive_process_for(
        &self,
        cpg: &mut CodePropertyGraph,
        for_id: NodeId,
        ctx: &mut CfgContext,
    ) -> SmallVec<[NodeId; 4]> {
        let children = cpg.ast_children(for_id).to_vec();

        // For loops can have different structures:
        // - for (init; cond; update) body  -> 4 children
        // - for item in iterator body      -> 2-3 children

        if children.is_empty() {
            return smallvec::smallvec![for_id];
        }

        // For simplicity, treat for loop similarly to while
        // The header acts as both condition check and iteration
        let header = for_id;
        let body = children.last().copied().unwrap_or(for_id);

        ctx.push_loop(for_id, header);

        // for -> body (enter loop)
        cpg.connect_unique(
            for_id,
            body,
            CpgEdgeKind::ControlFlow(CfgEdgeKind::ConditionalTrue),
        );

        // Process body
        let body_exits = self.process_node_recursive(cpg, body, ctx);

        // body exits -> header (loop back)
        for &exit in &body_exits {
            cpg.connect_unique(
                exit,
                header,
                CpgEdgeKind::ControlFlow(CfgEdgeKind::LoopBack),
            );
        }

        let loop_ctx = ctx.pop_loop();

        // Exits: for node itself (loop exit) + break targets
        let mut exits: SmallVec<[NodeId; 4]> = smallvec::smallvec![for_id];
        exits.extend(loop_ctx.break_targets);

        exits
    }

    /// Processes an infinite loop.
    #[cfg(test)]
    fn recursive_process_loop(
        &self,
        cpg: &mut CodePropertyGraph,
        loop_id: NodeId,
        ctx: &mut CfgContext,
    ) -> SmallVec<[NodeId; 4]> {
        let children = cpg.ast_children(loop_id).to_vec();

        if children.is_empty() {
            return smallvec::smallvec![loop_id];
        }

        let body = children
            .iter()
            .rev()
            .copied()
            .find(|&child| {
                matches!(
                    cpg.node(child).map(|node| &node.kind),
                    Some(CpgNodeKind::Block { .. })
                )
            })
            .unwrap_or(children[0]);

        ctx.push_loop(loop_id, loop_id);

        // loop -> body
        cpg.connect_unique(
            loop_id,
            body,
            CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
        );

        // Process body
        let body_exits = self.process_node_recursive(cpg, body, ctx);

        // body exits -> loop (loop back)
        for &exit in &body_exits {
            cpg.connect_unique(
                exit,
                loop_id,
                CpgEdgeKind::ControlFlow(CfgEdgeKind::LoopBack),
            );
        }

        let loop_ctx = ctx.pop_loop();

        // Infinite loop only exits via break
        loop_ctx.break_targets.into_iter().collect()
    }

    /// Processes a match/switch statement.
    #[cfg(test)]
    fn recursive_process_match(
        &self,
        cpg: &mut CodePropertyGraph,
        match_id: NodeId,
        ctx: &mut CfgContext,
    ) -> SmallVec<[NodeId; 4]> {
        let children = cpg.ast_children(match_id).to_vec();

        if children.is_empty() {
            return smallvec::smallvec![match_id];
        }

        let mut exits = SmallVec::new();

        // First child might be the matched expression
        let (matched_expr, arms_start) = if children.len() > 1 {
            let first = children[0];
            let first_kind = cpg.node(first).map(|n| n.kind.clone());
            if matches!(first_kind.as_ref(), Some(CpgNodeKind::MatchArm)) {
                (None, 0)
            } else {
                cpg.connect_unique(
                    match_id,
                    first,
                    CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
                );
                (Some(first), 1)
            }
        } else {
            (None, 0)
        };

        let source = matched_expr.unwrap_or(match_id);

        // Process each match arm
        for &arm in &children[arms_start..] {
            let arm_kind = cpg.node(arm).map(|n| n.kind.clone());

            if matches!(arm_kind.as_ref(), Some(CpgNodeKind::MatchArm)) {
                // Connect source to arm
                cpg.connect_unique(source, arm, CpgEdgeKind::ControlFlow(CfgEdgeKind::Case));

                // Process arm body
                let arm_children = cpg.ast_children(arm).to_vec();
                if let Some(&arm_body) = arm_children.last() {
                    cpg.connect_unique(
                        arm,
                        arm_body,
                        CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
                    );
                    let arm_exits = self.process_node_recursive(cpg, arm_body, ctx);
                    exits.extend(arm_exits);
                } else {
                    exits.push(arm);
                }
            }
        }

        if exits.is_empty() {
            smallvec::smallvec![match_id]
        } else {
            exits
        }
    }

    /// Processes a return statement.
    #[cfg(test)]
    fn recursive_process_return(
        &self,
        cpg: &mut CodePropertyGraph,
        return_id: NodeId,
        ctx: &mut CfgContext,
    ) -> SmallVec<[NodeId; 4]> {
        let children = cpg.ast_children(return_id).to_vec();

        // Process return value expression if present
        if let Some(&expr) = children.first() {
            cpg.connect_unique(
                return_id,
                expr,
                CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
            );
            let _ = self.process_node_recursive(cpg, expr, ctx);
        }

        // Return has no exits within the function
        // (the caller marks function exit points)
        smallvec::smallvec![return_id]
    }

    /// Processes a break statement.
    #[cfg(test)]
    fn recursive_process_break(
        &self,
        cpg: &mut CodePropertyGraph,
        break_id: NodeId,
        ctx: &mut CfgContext,
    ) -> SmallVec<[NodeId; 4]> {
        // Record break for the enclosing loop
        if let Some(loop_ctx) = ctx.loop_stack.last_mut() {
            loop_ctx.break_targets.push(break_id);

            // Create break edge to loop header
            cpg.connect_unique(
                break_id,
                loop_ctx.loop_id,
                CpgEdgeKind::ControlFlow(CfgEdgeKind::Break),
            );
        }

        // Break has no normal exits
        smallvec::smallvec![]
    }

    /// Processes a continue statement.
    #[cfg(test)]
    fn recursive_process_continue(
        &self,
        cpg: &mut CodePropertyGraph,
        continue_id: NodeId,
        ctx: &mut CfgContext,
    ) -> SmallVec<[NodeId; 4]> {
        // Create continue edge to loop condition
        if let Some(loop_ctx) = ctx.loop_stack.last() {
            cpg.connect_unique(
                continue_id,
                loop_ctx.continue_target,
                CpgEdgeKind::ControlFlow(CfgEdgeKind::Continue),
            );
        }

        // Continue has no normal exits
        smallvec::smallvec![]
    }

    /// Processes a try block.
    #[cfg(test)]
    fn recursive_process_try(
        &self,
        cpg: &mut CodePropertyGraph,
        try_id: NodeId,
        ctx: &mut CfgContext,
    ) -> SmallVec<[NodeId; 4]> {
        if !self.config.include_exceptions {
            return self.recursive_process_sequential(cpg, try_id, ctx);
        }

        let children = cpg.ast_children(try_id).to_vec();
        let mut exits = SmallVec::new();

        // Find try body, catch blocks, and finally block
        let mut try_body = None;
        let mut catch_blocks = Vec::new();
        let mut finally_block = None;

        for &child in &children {
            let child_kind = cpg.node(child).map(|n| n.kind.clone());
            match child_kind.as_ref() {
                Some(CpgNodeKind::Block { .. }) if try_body.is_none() => {
                    try_body = Some(child);
                }
                Some(CpgNodeKind::Catch) => {
                    catch_blocks.push(child);
                }
                Some(CpgNodeKind::Finally) => {
                    finally_block = Some(child);
                }
                _ => {}
            }
        }

        // Process try body
        if let Some(body) = try_body {
            cpg.connect_unique(
                try_id,
                body,
                CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
            );

            // Push try context for exception handling
            ctx.push_try(try_id, catch_blocks.clone());

            let body_exits = self.process_node_recursive(cpg, body, ctx);

            ctx.pop_try();

            // Connect body exits to finally (if present) or add to exits
            if let Some(finally) = finally_block {
                for &exit in &body_exits {
                    cpg.connect_unique(
                        exit,
                        finally,
                        CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
                    );
                }
            } else {
                exits.extend(body_exits);
            }
        }

        // Process catch blocks
        for &catch in &catch_blocks {
            let catch_children = cpg.ast_children(catch).to_vec();
            if let Some(&catch_body) = catch_children.last() {
                cpg.connect_unique(
                    catch,
                    catch_body,
                    CpgEdgeKind::ControlFlow(CfgEdgeKind::Catch),
                );
                let catch_exits = self.process_node_recursive(cpg, catch_body, ctx);

                if let Some(finally) = finally_block {
                    for &exit in &catch_exits {
                        cpg.connect_unique(
                            exit,
                            finally,
                            CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
                        );
                    }
                } else {
                    exits.extend(catch_exits);
                }
            }
        }

        // Process finally block
        if let Some(finally) = finally_block {
            let finally_exits = self.process_node_recursive(cpg, finally, ctx);
            exits.extend(finally_exits);
        }

        if exits.is_empty() {
            smallvec::smallvec![try_id]
        } else {
            exits
        }
    }

    /// Processes a throw statement.
    #[cfg(test)]
    fn recursive_process_throw(
        &self,
        cpg: &mut CodePropertyGraph,
        throw_id: NodeId,
        ctx: &mut CfgContext,
    ) -> SmallVec<[NodeId; 4]> {
        if !self.config.include_exceptions {
            return smallvec::smallvec![throw_id];
        }

        let children = cpg.ast_children(throw_id).to_vec();

        // Process thrown expression
        if let Some(&expr) = children.first() {
            cpg.connect_unique(
                throw_id,
                expr,
                CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
            );
            let _ = self.process_node_recursive(cpg, expr, ctx);
        }

        // Connect to catch handlers in the try stack
        if let Some(try_ctx) = ctx.try_stack.last() {
            for &catch in &try_ctx.catch_handlers {
                cpg.connect_unique(
                    throw_id,
                    catch,
                    CpgEdgeKind::ControlFlow(CfgEdgeKind::Throw),
                );
            }
        }

        // Throw has no normal exits
        smallvec::smallvec![]
    }

    /// Processes a function call.
    #[cfg(test)]
    fn recursive_process_call(
        &self,
        cpg: &mut CodePropertyGraph,
        call_id: NodeId,
        ctx: &mut CfgContext,
    ) -> SmallVec<[NodeId; 4]> {
        let children = cpg.ast_children(call_id).to_vec();

        // Process arguments
        for &arg in &children {
            cpg.connect_unique(
                call_id,
                arg,
                CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
            );
            let _ = self.process_node_recursive(cpg, arg, ctx);
        }

        // Check if we have a target function
        if let Some(node) = cpg.node(call_id) {
            if let CpgNodeKind::Call {
                target: Some(target),
                ..
            } = &node.kind
            {
                let target = *target;
                // Create call edge
                cpg.connect_unique(call_id, target, CpgEdgeKind::ControlFlow(CfgEdgeKind::Call));
            }
        }

        smallvec::smallvec![call_id]
    }

    /// Processes nodes sequentially (fallback for non-control-flow nodes).
    #[cfg(test)]
    fn recursive_process_sequential(
        &self,
        cpg: &mut CodePropertyGraph,
        node_id: NodeId,
        ctx: &mut CfgContext,
    ) -> SmallVec<[NodeId; 4]> {
        let children = cpg.ast_children(node_id).to_vec();

        if children.is_empty() {
            return smallvec::smallvec![node_id];
        }

        // Connect to first child
        if let Some(&first) = children.first() {
            cpg.connect_unique(
                node_id,
                first,
                CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
            );
        }

        // Evaluate every child in AST order. Returning the final child's exits
        // without chaining the intervening children makes those exits
        // unreachable; loop handlers would then attach a back edge to an
        // unreachable expression operand in parser-built graphs.
        let mut exits: SmallVec<[NodeId; 4]> = smallvec::smallvec![];
        for &child in &children {
            for &exit in &exits {
                cpg.connect_unique(
                    exit,
                    child,
                    CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
                );
            }
            exits = self.process_node_recursive(cpg, child, ctx);
        }

        exits
    }
}

impl Default for CfgExtractor {
    fn default() -> Self {
        Self::new()
    }
}

fn is_loop_syntax_token(cpg: &CodePropertyGraph, node: NodeId) -> bool {
    matches!(
        cpg.node(node).map(|node| &node.kind),
        Some(CpgNodeKind::Unknown { kind })
            if matches!(kind.as_ref(), "while" | "until" | "do" | "(" | ")" | ":")
    )
}

/// Context for CFG extraction, tracking loop and try structures.
#[derive(Debug, Default)]
struct CfgContext {
    /// Stack of enclosing loops.
    loop_stack: Vec<LoopContext>,
    /// Stack of enclosing try blocks.
    try_stack: Vec<TryContext>,
    /// Nodes currently active between matching `Enter` and `Leave` frames,
    /// used to cut `AstChild` cycles in malformed graphs.
    on_path: FxHashSet<NodeId>,
}

impl CfgContext {
    fn new() -> Self {
        Self::default()
    }

    fn push_loop(&mut self, loop_id: NodeId, continue_target: NodeId) {
        self.loop_stack.push(LoopContext {
            loop_id,
            continue_target,
            break_targets: Vec::new(),
        });
    }

    fn pop_loop(&mut self) -> LoopContext {
        self.loop_stack.pop().unwrap_or_else(|| LoopContext {
            loop_id: NodeId::new(0),
            continue_target: NodeId::new(0),
            break_targets: Vec::new(),
        })
    }

    fn push_try(&mut self, try_id: NodeId, catch_handlers: Vec<NodeId>) {
        self.try_stack.push(TryContext {
            _try_id: try_id,
            catch_handlers,
        });
    }

    fn pop_try(&mut self) {
        self.try_stack.pop();
    }
}

/// Context for a loop structure.
#[derive(Debug)]
struct LoopContext {
    /// The loop node ID.
    loop_id: NodeId,
    /// Target for continue statements.
    continue_target: NodeId,
    /// Collected break target nodes.
    break_targets: Vec<NodeId>,
}

/// Context for a try block.
#[derive(Debug)]
struct TryContext {
    /// The try node ID.
    _try_id: NodeId,
    /// Catch handler nodes.
    catch_handlers: Vec<NodeId>,
}

/// Identifies basic blocks in a CFG.
///
/// This compatibility surface includes CFG fragments emitted for unreachable
/// AST descendants. New code should call the deterministic, typed
/// [`crate::analysis::block_cfg`] view directly to retain block identifiers,
/// typed edges, exit blocks, and traversal orders over the canonical reachable
/// function projection.
///
/// A basic block is a sequence of statements where:
/// - The first statement is the only entry point
/// - The last statement is the only exit point
/// - All statements execute sequentially
#[derive(Debug)]
pub struct BasicBlockIdentifier;

impl BasicBlockIdentifier {
    /// Creates a new basic block identifier.
    pub fn new() -> Self {
        Self
    }

    /// Identifies basic blocks within a function CFG.
    ///
    /// Returns a map from block leader node IDs to lists of nodes in the block.
    pub fn identify(
        &self,
        cpg: &CodePropertyGraph,
        function: NodeId,
    ) -> FxHashMap<NodeId, Vec<NodeId>> {
        let descendants = cpg.ast_descendants(function);
        let mut leaders: FxHashSet<NodeId> = FxHashSet::default();
        let mut blocks: FxHashMap<NodeId, Vec<NodeId>> = FxHashMap::default();

        leaders.insert(function);
        for &node_id in &descendants {
            if cpg
                .cfg_predecessors(node_id)
                .iter()
                .any(|(_, kind)| kind.is_conditional() || kind.is_loop())
            {
                leaders.insert(node_id);
            }

            let node_kind = cpg.node(node_id).map(|node| &node.kind);
            if matches!(
                node_kind,
                Some(
                    CpgNodeKind::If
                        | CpgNodeKind::While
                        | CpgNodeKind::For
                        | CpgNodeKind::Loop
                        | CpgNodeKind::Match
                        | CpgNodeKind::Return
                        | CpgNodeKind::Break
                        | CpgNodeKind::Continue
                )
            ) {
                leaders.extend(
                    cpg.cfg_successors(node_id)
                        .into_iter()
                        .map(|(successor, _)| successor),
                );
            }
        }

        let mut ordered_leaders: Vec<_> = leaders.iter().copied().collect();
        ordered_leaders.sort_unstable();
        for leader in ordered_leaders {
            let mut block = vec![leader];
            let mut current = leader;
            loop {
                let mut successors = cpg.cfg_successors(current);
                let Some((successor, CfgEdgeKind::Sequential)) = successors.next() else {
                    break;
                };
                if successors.next().is_some() {
                    break;
                }
                if leaders.contains(&successor) {
                    break;
                }
                block.push(successor);
                current = successor;
            }
            blocks.insert(leader, block);
        }
        blocks
    }
}

impl Default for BasicBlockIdentifier {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CpgNode, Language, MethodSignature, ScopeId, SourceRange, Visibility};

    fn create_test_function(cpg: &mut CodePropertyGraph) -> NodeId {
        let func = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Function {
                signature: Box::new(MethodSignature {
                    name: "test".into(),
                    params: Default::default(),
                    return_type: None,
                    is_static: false,
                    is_async: false,
                    visibility: Visibility::Public,
                }),
            },
            SourceRange::default(),
        ));

        let body = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Block {
                scope: ScopeId::GLOBAL,
            },
            SourceRange::default(),
        ));

        cpg.connect_unique(func, body, CpgEdgeKind::AstChild);

        func
    }

    #[test]
    fn test_cfg_extractor_basic() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let func = create_test_function(&mut cpg);

        let extractor = CfgExtractor::new();
        extractor.extract(&mut cpg);

        // Function should be marked as CFG entry
        assert!(cpg.cfg_entries().contains(&func));
    }

    #[test]
    fn test_cfg_if_statement() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let func = create_test_function(&mut cpg);
        let body = cpg.ast_children(func)[0];

        // Add if statement as child of body
        let if_node = cpg.add_node(
            CpgNode::new(NodeId::new(0), CpgNodeKind::If, SourceRange::default()).with_parent(body),
        );
        cpg.connect_unique(body, if_node, CpgEdgeKind::AstChild);

        // Add condition as child of if
        let cond = cpg.add_node(
            CpgNode::new(
                NodeId::new(0),
                CpgNodeKind::Identifier {
                    name: "x".into(),
                    definition: None,
                },
                SourceRange::default(),
            )
            .with_parent(if_node),
        );
        cpg.connect_unique(if_node, cond, CpgEdgeKind::AstChild);

        // Add then branch as child of if
        let then_branch = cpg.add_node(
            CpgNode::new(
                NodeId::new(0),
                CpgNodeKind::Block {
                    scope: ScopeId::GLOBAL,
                },
                SourceRange::default(),
            )
            .with_parent(if_node),
        );
        cpg.connect_unique(if_node, then_branch, CpgEdgeKind::AstChild);

        let extractor = CfgExtractor::new();
        extractor.extract(&mut cpg);

        // Check CFG edges were created - if should have control flow edges
        // The if node should connect to condition, and condition to branches
        let if_successors = cpg.cfg_successors(if_node);
        assert!(
            !if_successors.is_empty(),
            "if node should have CFG successors"
        );
    }

    #[test]
    fn test_basic_block_identifier() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let func = create_test_function(&mut cpg);

        let identifier = BasicBlockIdentifier::new();
        let blocks = identifier.identify(&cpg, func);

        // Should have at least the function as a leader
        assert!(blocks.contains_key(&func));
    }

    // ---- helpers for the new tests (mirror the hand-built AST shape) ----

    fn ast_child(cpg: &mut CodePropertyGraph, parent: NodeId, kind: CpgNodeKind) -> NodeId {
        let id = cpg.add_node(CpgNode::new(NodeId::new(0), kind, SourceRange::default()));
        cpg.connect(parent, id, CpgEdgeKind::AstChild);
        id
    }

    fn ident(name: &str) -> CpgNodeKind {
        CpgNodeKind::Identifier {
            name: name.into(),
            definition: None,
        }
    }

    fn block() -> CpgNodeKind {
        CpgNodeKind::Block {
            scope: ScopeId::GLOBAL,
        }
    }

    fn assert_machine_matches_recursive_oracle(cpg: CodePropertyGraph, config: CfgExtractorConfig) {
        let original_nodes = cpg.node_count();
        let mut machine = cpg.clone();
        let mut oracle = cpg;

        CfgExtractor::with_config(config.clone()).extract(&mut machine);
        CfgExtractor::with_config(config).extract_recursive_oracle(&mut oracle);

        assert_eq!(machine.node_count(), original_nodes);
        assert_eq!(oracle.node_count(), original_nodes);
        assert_eq!(machine.cfg_entries(), oracle.cfg_entries());
        assert_eq!(machine.cfg_exits(), oracle.cfg_exits());
        assert_eq!(
            machine.edges().cloned().collect::<Vec<_>>(),
            oracle.edges().cloned().collect::<Vec<_>>(),
            "the typed machine must preserve exact edge ids, insertion order, labels, and kinds"
        );
    }

    /// Builds one bounded AST containing every mutually-recursive CFG handler.
    /// Its source order deliberately mixes nested loop, branch, match,
    /// exception, terminator, call, and fallback-sequential regions.
    fn comprehensive_cfg_fixture() -> CodePropertyGraph {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let (function, body) = shell(&mut cpg);
        let callee = create_test_function(&mut cpg);

        let sequential = ast_child(
            &mut cpg,
            body,
            CpgNodeKind::Assignment {
                operator: "=".into(),
            },
        );
        let _left = ast_child(&mut cpg, sequential, ident("left"));
        let _right = ast_child(&mut cpg, sequential, ident("right"));

        let if_node = ast_child(&mut cpg, body, CpgNodeKind::If);
        let _if_condition = ast_child(&mut cpg, if_node, ident("if_condition"));
        let then_block = ast_child(&mut cpg, if_node, block());
        let while_node = ast_child(&mut cpg, then_block, CpgNodeKind::While);
        let _while_condition = ast_child(&mut cpg, while_node, ident("while_condition"));
        let while_body = ast_child(&mut cpg, while_node, block());
        let _continue_node = ast_child(&mut cpg, while_body, CpgNodeKind::Continue);
        let _break_node = ast_child(&mut cpg, while_body, CpgNodeKind::Break);

        let else_wrapper = ast_child(&mut cpg, if_node, CpgNodeKind::Else);
        let else_block = ast_child(&mut cpg, else_wrapper, block());
        let for_node = ast_child(&mut cpg, else_block, CpgNodeKind::For);
        let _for_iterator = ast_child(&mut cpg, for_node, ident("iterator"));
        let for_body = ast_child(&mut cpg, for_node, block());
        let loop_node = ast_child(&mut cpg, for_body, CpgNodeKind::Loop);
        let loop_body = ast_child(&mut cpg, loop_node, block());
        let _loop_break = ast_child(&mut cpg, loop_body, CpgNodeKind::Break);

        let match_node = ast_child(&mut cpg, body, CpgNodeKind::Match);
        let _scrutinee = ast_child(&mut cpg, match_node, ident("scrutinee"));
        let first_arm = ast_child(&mut cpg, match_node, CpgNodeKind::MatchArm);
        let first_arm_body = ast_child(&mut cpg, first_arm, block());
        let return_node = ast_child(&mut cpg, first_arm_body, CpgNodeKind::Return);
        let _return_value = ast_child(&mut cpg, return_node, ident("return_value"));
        let _empty_arm = ast_child(&mut cpg, match_node, CpgNodeKind::MatchArm);

        let try_node = ast_child(&mut cpg, body, CpgNodeKind::Try);
        let try_body = ast_child(&mut cpg, try_node, block());
        let call = ast_child(
            &mut cpg,
            try_body,
            CpgNodeKind::Call {
                target: Some(callee),
                is_method: false,
            },
        );
        let _first_argument = ast_child(&mut cpg, call, ident("first_argument"));
        let nested_argument = ast_child(
            &mut cpg,
            call,
            CpgNodeKind::Assignment {
                operator: "+=".into(),
            },
        );
        let _nested_operand = ast_child(&mut cpg, nested_argument, ident("nested_operand"));
        let throw_node = ast_child(&mut cpg, try_body, CpgNodeKind::Throw);
        let _thrown_value = ast_child(&mut cpg, throw_node, ident("thrown_value"));

        let first_catch = ast_child(&mut cpg, try_node, CpgNodeKind::Catch);
        let first_catch_body = ast_child(&mut cpg, first_catch, block());
        let _caught = ast_child(&mut cpg, first_catch_body, ident("caught"));
        let _empty_catch = ast_child(&mut cpg, try_node, CpgNodeKind::Catch);
        let finally_node = ast_child(&mut cpg, try_node, CpgNodeKind::Finally);
        let finally_body = ast_child(&mut cpg, finally_node, block());
        let _cleanup = ast_child(&mut cpg, finally_body, ident("cleanup"));

        let _outside_break = ast_child(&mut cpg, body, CpgNodeKind::Break);
        let _outside_continue = ast_child(&mut cpg, body, CpgNodeKind::Continue);
        let outside_throw = ast_child(&mut cpg, body, CpgNodeKind::Throw);
        let _outside_thrown_value = ast_child(&mut cpg, outside_throw, ident("outside_throw"));

        assert_eq!(cpg.functions().next().map(|node| node.id), Some(function));
        cpg
    }

    #[test]
    fn typed_machine_matches_recursive_oracle_for_every_handler_and_configuration() {
        for mask in 0_u8..8 {
            let config = CfgExtractorConfig {
                include_fallthrough: mask & 1 != 0,
                include_exceptions: mask & 2 != 0,
                include_call_edges: mask & 4 != 0,
            };
            assert_machine_matches_recursive_oracle(comprehensive_cfg_fixture(), config.clone());

            let mut malformed = comprehensive_cfg_fixture();
            let function = malformed.functions().next().expect("main function").id;
            let deepest = malformed
                .node_ids()
                .last()
                .expect("comprehensive fixture has nodes");
            malformed.connect(deepest, function, CpgEdgeKind::AstChild);
            assert_machine_matches_recursive_oracle(malformed, config);
        }
    }

    /// `CfgExtractorConfig` field defaults and `CfgExtractor::with_config`
    /// construction. There is no public config getter, so `with_config` is
    /// exercised by confirming the custom extractor still runs and marks the
    /// function entry.
    #[test]
    fn cfg_extractor_config_fields_and_with_config() {
        let d = CfgExtractorConfig::default();
        assert!(d.include_fallthrough);
        assert!(d.include_exceptions);
        assert!(d.include_call_edges);

        let custom = CfgExtractorConfig {
            include_fallthrough: false,
            include_exceptions: false,
            include_call_edges: false,
        };
        let extractor = CfgExtractor::with_config(custom);

        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let func = create_test_function(&mut cpg);
        extractor.extract(&mut cpg);
        assert!(cpg.cfg_entries().contains(&func));
    }

    /// `BasicBlockIdentifier::identify` leaders: the function is always a leader,
    /// every block is non-empty and headed by its leader, and a conditional
    /// branch target (an `if`'s then-branch) is a leader.
    #[test]
    fn basic_block_leaders_include_function_and_branch_target() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let func = create_test_function(&mut cpg);
        let body = cpg.ast_children(func)[0];

        // body: if (x) { y }
        let if_node = ast_child(&mut cpg, body, CpgNodeKind::If);
        let _cond = ast_child(&mut cpg, if_node, ident("x"));
        let then_b = ast_child(&mut cpg, if_node, block());
        let _then_stmt = ast_child(&mut cpg, then_b, ident("y"));

        CfgExtractor::new().extract(&mut cpg);
        let blocks = BasicBlockIdentifier::new().identify(&cpg, func);

        assert!(
            blocks.contains_key(&func),
            "the function is always a leader"
        );
        for (leader, nodes) in &blocks {
            assert!(!nodes.is_empty(), "each basic block is non-empty");
            assert_eq!(nodes[0], *leader, "each block is headed by its leader");
        }
        // The then-branch is reached by a ConditionalTrue edge ⇒ it is a leader.
        assert!(
            blocks.contains_key(&then_b),
            "a conditional branch target must be a basic-block leader"
        );
    }

    /// The legacy leader map includes AST-scoped CFG fragments after a
    /// terminator, even though the canonical `BlockCfg` intentionally contains
    /// only the entry-reachable `FunctionCfgView`.
    #[test]
    fn basic_block_compatibility_retains_unreachable_loop_exit_targets() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let func = create_test_function(&mut cpg);
        let body = cpg.ast_children(func)[0];
        let _returned = ast_child(&mut cpg, body, CpgNodeKind::Return);
        let loop_node = ast_child(&mut cpg, body, CpgNodeKind::While);
        let _guard = ast_child(&mut cpg, loop_node, ident("condition"));
        let _loop_body = ast_child(&mut cpg, loop_node, block());
        let after = ast_child(&mut cpg, body, ident("after"));

        CfgExtractor::new().extract(&mut cpg);
        assert!(cpg.edges().any(|edge| {
            edge.target == after && edge.kind == CpgEdgeKind::ControlFlow(CfgEdgeKind::LoopExit)
        }));

        let blocks = BasicBlockIdentifier::new().identify(&cpg, func);
        assert!(blocks.contains_key(&after));
        assert!(
            !crate::analysis::block_cfg(&cpg, func)
                .expect("reachable block CFG")
                .block_of_node
                .contains_key(&after),
            "typed BlockCfg retains the canonical reachable scope"
        );
    }

    /// The FIXED behavior: `CfgExtractor::extract` routes every edge through
    /// `connect_unique`, so a second extraction on a graph with a branch adds no
    /// new CFG edges.
    #[test]
    fn cfg_extract_is_idempotent_on_branchy_graph() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let func = create_test_function(&mut cpg);
        let body = cpg.ast_children(func)[0];
        let while_node = ast_child(&mut cpg, body, CpgNodeKind::While);
        let _guard = ast_child(&mut cpg, while_node, ident("c"));
        let loop_body = ast_child(&mut cpg, while_node, block());
        let _work = ast_child(&mut cpg, loop_body, ident("w"));

        CfgExtractor::new().extract(&mut cpg);
        let after_first = cpg.stats().cfg_edges;
        assert!(after_first > 0, "the while loop must produce CFG edges");
        CfgExtractor::new().extract(&mut cpg);
        assert_eq!(
            after_first,
            cpg.stats().cfg_edges,
            "extract must be idempotent (connect_unique)"
        );
    }

    /// Leaving a `while` retypes the existing fallthrough edge as `LoopExit`.
    /// The pre-retyping golden counts prove that no edge or node is added and
    /// that cyclomatic complexity is unchanged.
    #[test]
    fn while_fallthrough_is_one_loop_exit_with_stable_graph_counts() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let (func, body) = shell(&mut cpg);
        let while_node = ast_child(&mut cpg, body, CpgNodeKind::While);
        let guard = ast_child(&mut cpg, while_node, ident("condition"));
        let loop_body = ast_child(&mut cpg, while_node, block());
        let _work = ast_child(&mut cpg, loop_body, ident("work"));
        let after = ast_child(&mut cpg, body, ident("after"));

        CfgExtractor::new().extract_function_cfg(&mut cpg, func);

        assert!(has_cfg(&cpg, guard, after, CfgEdgeKind::LoopExit));
        assert!(!has_cfg(&cpg, guard, after, CfgEdgeKind::Sequential));
        assert_eq!(cpg.stats().cfg_edges, 7, "the old edge-count golden");
        assert_eq!(cpg.cyclomatic_complexity(), 2, "the old complexity golden");

        let blocks = BasicBlockIdentifier::new().identify(&cpg, func);
        assert!(
            blocks.contains_key(&after),
            "a loop exit target is a leader"
        );
        assert!(
            blocks
                .values()
                .all(|nodes| !(nodes.contains(&guard) && nodes.contains(&after))),
            "a LoopExit edge terminates its source basic block"
        );
    }

    /// Both header-based `for` fallthrough and explicit `break` fallthrough
    /// use the same single continuation-emission site.
    #[test]
    fn for_and_break_continuations_are_loop_exits() {
        let mut for_cpg = CodePropertyGraph::new(Language::Rust);
        let (for_func, for_body) = shell(&mut for_cpg);
        let for_node = ast_child(&mut for_cpg, for_body, CpgNodeKind::For);
        let for_loop_body = ast_child(&mut for_cpg, for_node, block());
        let _work = ast_child(&mut for_cpg, for_loop_body, ident("work"));
        let after_for = ast_child(&mut for_cpg, for_body, ident("after_for"));
        CfgExtractor::new().extract_function_cfg(&mut for_cpg, for_func);
        assert!(has_cfg(
            &for_cpg,
            for_node,
            after_for,
            CfgEdgeKind::LoopExit
        ));

        let mut loop_cpg = CodePropertyGraph::new(Language::Rust);
        let (loop_func, outer_body) = shell(&mut loop_cpg);
        let loop_node = ast_child(&mut loop_cpg, outer_body, CpgNodeKind::Loop);
        let loop_body = ast_child(&mut loop_cpg, loop_node, block());
        let brk = ast_child(&mut loop_cpg, loop_body, CpgNodeKind::Break);
        let after_loop = ast_child(&mut loop_cpg, outer_body, ident("after_loop"));
        CfgExtractor::new().extract_function_cfg(&mut loop_cpg, loop_func);
        assert!(has_cfg(&loop_cpg, brk, after_loop, CfgEdgeKind::LoopExit));
    }

    // ================= per-handler control-flow semantics =================
    //
    // One test per `process_*` handler, asserting the *edges* the handler is
    // specified to produce rather than merely that it ran. Together these pin
    // the CFG construction rules of §2 of `docs/theory/02-control-flow-and-
    // complexity.md`: a loop back-edge returns to the loop header, `break`
    // leaves via the header, `continue` returns to the continue target, a
    // `switch` fans out `Case` edges, and an exception propagates to every
    // enclosing handler.

    /// True iff a CFG edge of exactly `kind` runs from `from` to `to`.
    fn has_cfg(cpg: &CodePropertyGraph, from: NodeId, to: NodeId, kind: CfgEdgeKind) -> bool {
        cpg.cfg_successors(from)
            .into_iter()
            .any(|(succ, k)| succ == to && k == kind)
    }

    /// A minimal `Function → Block` shell plus the body id.
    fn shell(cpg: &mut CodePropertyGraph) -> (NodeId, NodeId) {
        let func = create_test_function(cpg);
        let body = cpg.ast_children(func)[0];
        (func, body)
    }

    /// `loop { … }`: the body's exits loop back to the loop header, and the
    /// loop's only way out is a `break` (an infinite loop has no fallthrough).
    #[test]
    fn loop_back_edges_to_header_and_exits_only_via_break() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let (func, body) = shell(&mut cpg);

        let loop_node = ast_child(&mut cpg, body, CpgNodeKind::Loop);
        let loop_body = ast_child(&mut cpg, loop_node, block());
        let work = ast_child(&mut cpg, loop_body, ident("w"));
        let brk = ast_child(&mut cpg, loop_body, CpgNodeKind::Break);

        CfgExtractor::new().extract_function_cfg(&mut cpg, func);

        assert!(
            has_cfg(&cpg, loop_node, loop_body, CfgEdgeKind::Sequential),
            "loop enters its body"
        );
        assert!(
            has_cfg(&cpg, work, loop_node, CfgEdgeKind::LoopBack)
                || has_cfg(&cpg, loop_body, loop_node, CfgEdgeKind::LoopBack),
            "the body's exit loops back to the header"
        );
        assert!(
            has_cfg(&cpg, brk, loop_node, CfgEdgeKind::Break),
            "`break` leaves through the loop header"
        );
        // The break is the loop's only exit, so it is the function's exit too.
        assert!(cpg.cfg_exits().contains(&brk), "break is the loop's exit");
    }

    /// `while cond { … continue … }`: `continue` returns to the *condition*
    /// (the continue target), not to the `while` node itself.
    #[test]
    fn continue_targets_the_loop_condition() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let (func, body) = shell(&mut cpg);

        let while_node = ast_child(&mut cpg, body, CpgNodeKind::While);
        let cond = ast_child(&mut cpg, while_node, ident("c"));
        let loop_body = ast_child(&mut cpg, while_node, block());
        let cont = ast_child(&mut cpg, loop_body, CpgNodeKind::Continue);

        CfgExtractor::new().extract_function_cfg(&mut cpg, func);

        assert!(
            has_cfg(&cpg, while_node, cond, CfgEdgeKind::Sequential),
            "the loop evaluates its condition first"
        );
        assert!(
            has_cfg(&cpg, cond, loop_body, CfgEdgeKind::ConditionalTrue),
            "a true condition enters the body"
        );
        assert!(
            has_cfg(&cpg, cont, cond, CfgEdgeKind::Continue),
            "`continue` jumps to the continue target (the condition)"
        );
    }

    /// `break`/`continue` outside any loop have no enclosing loop context, so
    /// they emit no jump edge (rather than panicking or targeting node 0).
    #[test]
    fn break_and_continue_outside_a_loop_emit_no_jump() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let (func, body) = shell(&mut cpg);
        let brk = ast_child(&mut cpg, body, CpgNodeKind::Break);
        let cont = ast_child(&mut cpg, body, CpgNodeKind::Continue);

        CfgExtractor::new().extract_function_cfg(&mut cpg, func);

        assert!(
            !cpg.cfg_successors(brk)
                .iter()
                .any(|(_, k)| k == CfgEdgeKind::Break),
            "a loop-less `break` has no target"
        );
        assert!(
            !cpg.cfg_successors(cont)
                .iter()
                .any(|(_, k)| k == CfgEdgeKind::Continue),
            "a loop-less `continue` has no target"
        );
    }

    /// `for … { … }`: the header both enters the body and receives the
    /// back-edge, and the `for` node itself is the loop exit.
    #[test]
    fn for_loop_enters_body_and_receives_the_back_edge() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let (func, body) = shell(&mut cpg);

        let for_node = ast_child(&mut cpg, body, CpgNodeKind::For);
        let _binding = ast_child(&mut cpg, for_node, ident("i"));
        let loop_body = ast_child(&mut cpg, for_node, block());
        let work = ast_child(&mut cpg, loop_body, ident("w"));

        CfgExtractor::new().extract_function_cfg(&mut cpg, func);

        assert!(
            has_cfg(&cpg, for_node, loop_body, CfgEdgeKind::ConditionalTrue),
            "the header enters the body when the iterator yields"
        );
        assert!(
            has_cfg(&cpg, work, for_node, CfgEdgeKind::LoopBack),
            "the body loops back to the header"
        );
    }

    /// `match e { arm … }`: the scrutinee fans out one `Case` edge per arm, and
    /// each arm flows into its body.
    #[test]
    fn match_fans_out_case_edges_from_the_scrutinee() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let (func, body) = shell(&mut cpg);

        let match_node = ast_child(&mut cpg, body, CpgNodeKind::Match);
        let scrutinee = ast_child(&mut cpg, match_node, ident("e"));
        let arm_a = ast_child(&mut cpg, match_node, CpgNodeKind::MatchArm);
        let body_a = ast_child(&mut cpg, arm_a, block());
        let arm_b = ast_child(&mut cpg, match_node, CpgNodeKind::MatchArm);
        let body_b = ast_child(&mut cpg, arm_b, block());

        CfgExtractor::new().extract_function_cfg(&mut cpg, func);

        assert!(
            has_cfg(&cpg, match_node, scrutinee, CfgEdgeKind::Sequential),
            "the scrutinee is evaluated first"
        );
        for arm in [arm_a, arm_b] {
            assert!(
                has_cfg(&cpg, scrutinee, arm, CfgEdgeKind::Case),
                "every arm is a case successor of the scrutinee"
            );
        }
        assert!(has_cfg(&cpg, arm_a, body_a, CfgEdgeKind::Sequential));
        assert!(has_cfg(&cpg, arm_b, body_b, CfgEdgeKind::Sequential));
    }

    /// A `match` whose first child is already an arm (no separate scrutinee
    /// node) fans out from the `match` node itself. An arm with no body is its
    /// own exit.
    #[test]
    fn match_without_a_scrutinee_fans_out_from_itself() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let (func, body) = shell(&mut cpg);

        let match_node = ast_child(&mut cpg, body, CpgNodeKind::Match);
        let arm_a = ast_child(&mut cpg, match_node, CpgNodeKind::MatchArm);
        let arm_b = ast_child(&mut cpg, match_node, CpgNodeKind::MatchArm);

        CfgExtractor::new().extract_function_cfg(&mut cpg, func);

        assert!(has_cfg(&cpg, match_node, arm_a, CfgEdgeKind::Case));
        assert!(has_cfg(&cpg, match_node, arm_b, CfgEdgeKind::Case));
        // Body-less arms are their own exits.
        assert!(cpg.cfg_exits().contains(&arm_a) || cpg.cfg_exits().contains(&arm_b));
    }

    /// A childless `match` is inert: it is its own exit and adds no edges.
    #[test]
    fn empty_match_is_its_own_exit() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let (func, body) = shell(&mut cpg);
        let match_node = ast_child(&mut cpg, body, CpgNodeKind::Match);

        CfgExtractor::new().extract_function_cfg(&mut cpg, func);

        assert!(cpg.cfg_successors(match_node).is_empty());
        assert!(cpg.cfg_exits().contains(&match_node));
    }

    /// `return e`: the return flows into its value expression, and the return
    /// itself terminates the enclosing block (statements after it are not
    /// chained onto it).
    #[test]
    fn return_evaluates_its_value_and_terminates_the_block() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let (func, body) = shell(&mut cpg);

        let ret = ast_child(&mut cpg, body, CpgNodeKind::Return);
        let value = ast_child(&mut cpg, ret, ident("v"));
        let after = ast_child(&mut cpg, body, ident("dead"));

        CfgExtractor::new().extract_function_cfg(&mut cpg, func);

        assert!(
            has_cfg(&cpg, ret, value, CfgEdgeKind::Sequential),
            "the returned expression is evaluated"
        );
        assert!(
            !has_cfg(&cpg, ret, after, CfgEdgeKind::Sequential),
            "control does not fall through a `return`"
        );
        assert!(
            cpg.cfg_exits().contains(&ret),
            "`return` exits the function"
        );
    }

    /// A transparent parser wrapper around `return` preserves termination.
    #[test]
    fn wrapped_return_does_not_fall_through_to_the_next_statement() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let (func, body) = shell(&mut cpg);
        let wrapper = ast_child(
            &mut cpg,
            body,
            CpgNodeKind::Unknown {
                kind: "expression_statement".into(),
            },
        );
        let ret = ast_child(&mut cpg, wrapper, CpgNodeKind::Return);
        let after = ast_child(&mut cpg, body, ident("dead"));

        CfgExtractor::new().extract_function_cfg(&mut cpg, func);

        assert!(has_cfg(&cpg, wrapper, ret, CfgEdgeKind::Sequential));
        assert!(
            !cpg.cfg_predecessors(after)
                .iter()
                .any(|(source, _)| source == ret),
            "a terminal exit exposed by a wrapper must not reach its sibling"
        );
    }

    /// An `if` whose every branch returns terminates the enclosing block.
    #[test]
    fn all_returning_branches_do_not_fall_through() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let (func, body) = shell(&mut cpg);
        let branch = ast_child(&mut cpg, body, CpgNodeKind::If);
        let condition = ast_child(&mut cpg, branch, ident("condition"));
        let then_block = ast_child(&mut cpg, branch, block());
        let then_return = ast_child(&mut cpg, then_block, CpgNodeKind::Return);
        let else_block = ast_child(&mut cpg, branch, block());
        let else_return = ast_child(&mut cpg, else_block, CpgNodeKind::Return);
        let after = ast_child(&mut cpg, body, ident("dead"));

        CfgExtractor::new().extract_function_cfg(&mut cpg, func);

        assert!(has_cfg(
            &cpg,
            condition,
            then_block,
            CfgEdgeKind::ConditionalTrue
        ));
        assert!(has_cfg(
            &cpg,
            condition,
            else_block,
            CfgEdgeKind::ConditionalFalse
        ));
        assert!(
            [then_return, else_return].iter().all(|terminal| {
                !cpg.cfg_predecessors(after)
                    .iter()
                    .any(|(source, _)| source == *terminal)
            }),
            "no all-return branch may be threaded into the next statement"
        );
    }

    /// `try { … } catch { … } finally { … }`: the try body and every catch body
    /// funnel into the `finally`, which is the construct's single exit.
    #[test]
    fn try_catch_finally_funnels_both_paths_into_finally() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let (func, body) = shell(&mut cpg);

        let try_node = ast_child(&mut cpg, body, CpgNodeKind::Try);
        let try_body = ast_child(&mut cpg, try_node, block());
        let try_stmt = ast_child(&mut cpg, try_body, ident("t"));
        let catch = ast_child(&mut cpg, try_node, CpgNodeKind::Catch);
        let catch_body = ast_child(&mut cpg, catch, block());
        let catch_stmt = ast_child(&mut cpg, catch_body, ident("c"));
        let finally = ast_child(&mut cpg, try_node, CpgNodeKind::Finally);

        CfgExtractor::new().extract_function_cfg(&mut cpg, func);

        assert!(has_cfg(&cpg, try_node, try_body, CfgEdgeKind::Sequential));
        assert!(has_cfg(&cpg, catch, catch_body, CfgEdgeKind::Catch));
        assert!(
            has_cfg(&cpg, try_stmt, finally, CfgEdgeKind::Sequential),
            "the try body's exit reaches `finally`"
        );
        assert!(
            has_cfg(&cpg, catch_stmt, finally, CfgEdgeKind::Sequential),
            "the catch body's exit reaches `finally`"
        );
    }

    /// Without a `finally`, the try and catch exits are the construct's exits.
    #[test]
    fn try_without_finally_exits_through_its_bodies() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let (func, body) = shell(&mut cpg);

        let try_node = ast_child(&mut cpg, body, CpgNodeKind::Try);
        let try_body = ast_child(&mut cpg, try_node, block());
        let try_stmt = ast_child(&mut cpg, try_body, ident("t"));
        let catch = ast_child(&mut cpg, try_node, CpgNodeKind::Catch);
        let catch_body = ast_child(&mut cpg, catch, block());
        let catch_stmt = ast_child(&mut cpg, catch_body, ident("c"));

        CfgExtractor::new().extract_function_cfg(&mut cpg, func);

        let exits = cpg.cfg_exits();
        assert!(
            exits.contains(&try_stmt) && exits.contains(&catch_stmt),
            "both the normal and the handler path exit the construct"
        );
        assert!(has_cfg(&cpg, try_node, try_body, CfgEdgeKind::Sequential));
        assert!(has_cfg(&cpg, catch, catch_body, CfgEdgeKind::Catch));
    }

    /// `throw e` inside a `try` connects to every enclosing handler and, like
    /// `return`, evaluates its operand and terminates the block.
    #[test]
    fn throw_reaches_every_enclosing_handler() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let (func, body) = shell(&mut cpg);

        let try_node = ast_child(&mut cpg, body, CpgNodeKind::Try);
        let try_body = ast_child(&mut cpg, try_node, block());
        let throw = ast_child(&mut cpg, try_body, CpgNodeKind::Throw);
        let operand = ast_child(&mut cpg, throw, ident("e"));
        let catch_a = ast_child(&mut cpg, try_node, CpgNodeKind::Catch);
        let _catch_a_body = ast_child(&mut cpg, catch_a, block());
        let catch_b = ast_child(&mut cpg, try_node, CpgNodeKind::Catch);
        let _catch_b_body = ast_child(&mut cpg, catch_b, block());

        CfgExtractor::new().extract_function_cfg(&mut cpg, func);

        assert!(
            has_cfg(&cpg, throw, operand, CfgEdgeKind::Sequential),
            "the thrown expression is evaluated"
        );
        for catch in [catch_a, catch_b] {
            assert!(
                has_cfg(&cpg, throw, catch, CfgEdgeKind::Throw),
                "the exception may be taken by any enclosing handler"
            );
        }
    }

    /// With `include_exceptions = false`, `try` degrades to a plain sequential
    /// node and `throw` emits no handler edges.
    #[test]
    fn exceptions_disabled_degrades_try_and_throw_to_plain_flow() {
        let config = CfgExtractorConfig {
            include_exceptions: false,
            ..CfgExtractorConfig::default()
        };
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let (func, body) = shell(&mut cpg);

        let try_node = ast_child(&mut cpg, body, CpgNodeKind::Try);
        let try_body = ast_child(&mut cpg, try_node, block());
        let throw = ast_child(&mut cpg, try_body, CpgNodeKind::Throw);
        let catch = ast_child(&mut cpg, try_node, CpgNodeKind::Catch);
        let _catch_body = ast_child(&mut cpg, catch, block());

        CfgExtractor::with_config(config).extract_function_cfg(&mut cpg, func);

        assert!(
            !cpg.cfg_successors(throw)
                .iter()
                .any(|(_, k)| k == CfgEdgeKind::Throw),
            "no handler edges when exception tracking is off"
        );
        assert!(
            has_cfg(&cpg, try_node, try_body, CfgEdgeKind::Sequential),
            "`try` still flows into its first child sequentially"
        );
    }

    /// A `Call` with a resolved target gets a `Call` edge to the callee and
    /// evaluates its arguments; with `include_call_edges = false` the call is
    /// treated as an ordinary node instead.
    #[test]
    fn call_edges_follow_the_resolved_target_and_the_config() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let (func, body) = shell(&mut cpg);
        let callee = create_test_function(&mut cpg);

        let call = ast_child(
            &mut cpg,
            body,
            CpgNodeKind::Call {
                target: Some(callee),
                is_method: false,
            },
        );
        let arg = ast_child(&mut cpg, call, ident("a"));

        CfgExtractor::new().extract_function_cfg(&mut cpg, func);

        assert!(
            has_cfg(&cpg, call, arg, CfgEdgeKind::Sequential),
            "arguments are evaluated before the call"
        );
        assert!(
            has_cfg(&cpg, call, callee, CfgEdgeKind::Call),
            "a resolved call reaches its callee"
        );

        // Same graph shape, call edges disabled.
        let mut plain = CodePropertyGraph::new(Language::Rust);
        let (func2, body2) = shell(&mut plain);
        let callee2 = create_test_function(&mut plain);
        let call2 = ast_child(
            &mut plain,
            body2,
            CpgNodeKind::Call {
                target: Some(callee2),
                is_method: false,
            },
        );
        let config = CfgExtractorConfig {
            include_call_edges: false,
            ..CfgExtractorConfig::default()
        };
        CfgExtractor::with_config(config).extract_function_cfg(&mut plain, func2);
        assert!(
            !has_cfg(&plain, call2, callee2, CfgEdgeKind::Call),
            "call edges are suppressed when the config disables them"
        );
    }

    /// `if` with an `Else` wrapper: the false edge targets the block *inside*
    /// the `Else`, not the wrapper node.
    #[test]
    fn if_else_resolves_through_the_else_wrapper() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let (func, body) = shell(&mut cpg);

        let if_node = ast_child(&mut cpg, body, CpgNodeKind::If);
        let cond = ast_child(&mut cpg, if_node, ident("c"));
        let then_b = ast_child(&mut cpg, if_node, block());
        let else_node = ast_child(&mut cpg, if_node, CpgNodeKind::Else);
        let else_b = ast_child(&mut cpg, else_node, block());

        CfgExtractor::new().extract_function_cfg(&mut cpg, func);

        assert!(has_cfg(&cpg, cond, then_b, CfgEdgeKind::ConditionalTrue));
        assert!(
            has_cfg(&cpg, cond, else_b, CfgEdgeKind::ConditionalFalse),
            "the false edge skips the `Else` wrapper and targets its block"
        );
    }

    /// An `if` with a bare else branch (no `Else` wrapper) targets that branch
    /// directly.
    #[test]
    fn if_else_without_a_wrapper_targets_the_branch_directly() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let (func, body) = shell(&mut cpg);

        let if_node = ast_child(&mut cpg, body, CpgNodeKind::If);
        let cond = ast_child(&mut cpg, if_node, ident("c"));
        let then_b = ast_child(&mut cpg, if_node, block());
        let else_b = ast_child(&mut cpg, if_node, block());

        CfgExtractor::new().extract_function_cfg(&mut cpg, func);

        assert!(has_cfg(&cpg, cond, then_b, CfgEdgeKind::ConditionalTrue));
        assert!(has_cfg(&cpg, cond, else_b, CfgEdgeKind::ConditionalFalse));
    }

    /// Degenerate shapes are inert rather than panicking. "Inert" means they
    /// never *branch* or *loop*: each such node still takes part in ordinary
    /// sequential block chaining (it is its own exit, so the next statement is
    /// linked onto it), but emits none of the conditional or back-edges its
    /// well-formed counterpart would.
    #[test]
    fn degenerate_constructs_are_inert() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let (func, body) = shell(&mut cpg);

        let bare_if = ast_child(&mut cpg, body, CpgNodeKind::If);
        let cond_only_if = ast_child(&mut cpg, body, CpgNodeKind::If);
        let lone_cond = ast_child(&mut cpg, cond_only_if, ident("c"));
        let bare_while = ast_child(&mut cpg, body, CpgNodeKind::While);
        let _while_cond = ast_child(&mut cpg, bare_while, ident("c"));
        let bare_for = ast_child(&mut cpg, body, CpgNodeKind::For);
        let bare_loop = ast_child(&mut cpg, body, CpgNodeKind::Loop);
        let empty_block = ast_child(&mut cpg, body, block());

        CfgExtractor::new().extract_function_cfg(&mut cpg, func);

        let branches = |id: NodeId| {
            cpg.cfg_successors(id)
                .iter()
                .any(|(_, k)| k.is_conditional() || k.is_loop())
        };

        assert!(!branches(bare_if), "a childless `if` never branches");
        assert!(
            has_cfg(&cpg, cond_only_if, lone_cond, CfgEdgeKind::Sequential),
            "a condition-only `if` still evaluates its condition"
        );
        assert!(
            !branches(cond_only_if),
            "with no branches there is nothing to branch to"
        );
        assert!(
            !branches(bare_while),
            "a `while` without a body never branches or loops"
        );
        assert!(!branches(bare_for), "a childless `for` has no back-edge");
        assert!(!branches(bare_loop), "a childless `loop` has no back-edge");
        assert!(
            cpg.cfg_successors(empty_block).is_empty(),
            "an empty trailing block is a dead end"
        );
    }

    /// A function with no body produces an entry and nothing else, and the
    /// `Default` impls agree with `new`.
    #[test]
    fn bodyless_function_and_default_constructors() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let func = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Function {
                signature: Box::new(MethodSignature {
                    name: "empty".into(),
                    params: Default::default(),
                    return_type: None,
                    is_static: false,
                    is_async: false,
                    visibility: Visibility::Private,
                }),
            },
            SourceRange::default(),
        ));

        CfgExtractor::default().extract(&mut cpg);

        assert!(cpg.cfg_entries().contains(&func));
        assert!(cpg.cfg_exits().is_empty(), "no body ⇒ no exit points");
        // `::default()` rather than the unit literal on purpose: the `Default`
        // impl is what this assertion exercises.
        #[allow(clippy::default_constructed_unit_structs)]
        let identifier = BasicBlockIdentifier::default();
        assert!(identifier.identify(&cpg, func).contains_key(&func));
    }

    /// REGRESSION (malformed input): a cyclic `AstChild` edge used to send the
    /// mutually-recursive `process_*` handlers into an unbounded descent that
    /// overflowed the stack and aborted the process. The path guard in
    /// `process_node` cuts the cycle, so extraction terminates.
    ///
    /// Found by the `tests/robustness.rs` corruption suite.
    #[test]
    fn extraction_terminates_on_a_cyclic_ast() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let (func, body) = shell(&mut cpg);
        let inner = ast_child(&mut cpg, body, block());
        // The inner block claims the function as its own AST child.
        cpg.connect(inner, func, CpgEdgeKind::AstChild);

        CfgExtractor::new().extract_function_cfg(&mut cpg, func);

        // Termination is the property under test; the graph must also stay sane.
        assert!(cpg.cfg_entries().contains(&func));
        for edge in cpg.edges() {
            assert!(cpg.node(edge.source).is_some());
            assert!(cpg.node(edge.target).is_some());
        }
    }

    /// A node reachable twice from *disjoint* branches is still processed on
    /// each path — the guard tracks the current path, not a global visited set.
    #[test]
    fn a_shared_node_is_processed_on_every_path() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let (func, body) = shell(&mut cpg);

        let if_node = ast_child(&mut cpg, body, CpgNodeKind::If);
        let cond = ast_child(&mut cpg, if_node, ident("c"));
        let then_b = ast_child(&mut cpg, if_node, block());
        let else_b = ast_child(&mut cpg, if_node, block());
        // One statement shared by both branches (a DAG, not a tree).
        let shared = ast_child(&mut cpg, then_b, ident("shared"));
        cpg.connect(else_b, shared, CpgEdgeKind::AstChild);

        CfgExtractor::new().extract_function_cfg(&mut cpg, func);

        assert!(has_cfg(&cpg, cond, then_b, CfgEdgeKind::ConditionalTrue));
        assert!(has_cfg(&cpg, cond, else_b, CfgEdgeKind::ConditionalFalse));
        assert!(
            has_cfg(&cpg, then_b, shared, CfgEdgeKind::Sequential)
                && has_cfg(&cpg, else_b, shared, CfgEdgeKind::Sequential),
            "both branches flow into the shared statement"
        );
    }

    /// Nested loops: the inner `break` binds to the *innermost* loop.
    #[test]
    fn break_binds_to_the_innermost_loop() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let (func, body) = shell(&mut cpg);

        let outer = ast_child(&mut cpg, body, CpgNodeKind::While);
        let outer_cond = ast_child(&mut cpg, outer, ident("o"));
        let outer_body = ast_child(&mut cpg, outer, block());
        let inner = ast_child(&mut cpg, outer_body, CpgNodeKind::While);
        let _inner_cond = ast_child(&mut cpg, inner, ident("i"));
        let inner_body = ast_child(&mut cpg, inner, block());
        let brk = ast_child(&mut cpg, inner_body, CpgNodeKind::Break);

        CfgExtractor::new().extract_function_cfg(&mut cpg, func);

        assert!(
            has_cfg(&cpg, brk, inner, CfgEdgeKind::Break),
            "`break` leaves the innermost loop"
        );
        assert!(
            !has_cfg(&cpg, brk, outer, CfgEdgeKind::Break),
            "`break` does not leave the outer loop"
        );
        assert!(
            has_cfg(&cpg, outer, outer_cond, CfgEdgeKind::Sequential),
            "the outer loop is still wired normally"
        );
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use crate::testutil::*;
    use crate::{
        CpgNode, DfgEdgeKind, Language, MethodSignature, ScopeId, SourceRange, Visibility,
    };
    use proptest::prelude::*;

    #[derive(Debug, Clone)]
    enum CfgSpec {
        Plain(u8),
        Block(Vec<CfgSpec>),
        If {
            then_branch: Vec<CfgSpec>,
            else_branch: Option<Vec<CfgSpec>>,
            wrap_else: bool,
        },
        While(Vec<CfgSpec>),
        For(Vec<CfgSpec>),
        Loop(Vec<CfgSpec>),
        Match(Vec<Vec<CfgSpec>>),
        Return(Vec<CfgSpec>),
        Break,
        Continue,
        Try {
            body: Vec<CfgSpec>,
            catches: Vec<Vec<CfgSpec>>,
            finally: Option<Vec<CfgSpec>>,
        },
        Throw(Vec<CfgSpec>),
        Call(Vec<CfgSpec>),
        Sequential(Vec<CfgSpec>),
    }

    fn arb_cfg_spec() -> impl Strategy<Value = CfgSpec> {
        let leaf = prop_oneof![
            any::<u8>().prop_map(CfgSpec::Plain),
            Just(CfgSpec::Return(Vec::new())),
            Just(CfgSpec::Break),
            Just(CfgSpec::Continue),
            Just(CfgSpec::Throw(Vec::new())),
            Just(CfgSpec::Call(Vec::new())),
        ];

        leaf.prop_recursive(3, 64, 3, |inner| {
            let children = prop::collection::vec(inner.clone(), 0..=3);
            let branches = (
                children.clone(),
                prop::option::of(children.clone()),
                any::<bool>(),
            );
            let catches = prop::collection::vec(children.clone(), 0..=2);
            let try_region = (
                children.clone(),
                catches,
                prop::option::of(children.clone()),
            );

            prop_oneof![
                children.clone().prop_map(CfgSpec::Block),
                branches.prop_map(|(then_branch, else_branch, wrap_else)| CfgSpec::If {
                    then_branch,
                    else_branch,
                    wrap_else,
                }),
                children.clone().prop_map(CfgSpec::While),
                children.clone().prop_map(CfgSpec::For),
                children.clone().prop_map(CfgSpec::Loop),
                prop::collection::vec(children.clone(), 0..=3).prop_map(CfgSpec::Match),
                prop::collection::vec(inner.clone(), 0..=1).prop_map(CfgSpec::Return),
                try_region.prop_map(|(body, catches, finally)| CfgSpec::Try {
                    body,
                    catches,
                    finally,
                }),
                prop::collection::vec(inner.clone(), 0..=1).prop_map(CfgSpec::Throw),
                children.clone().prop_map(CfgSpec::Call),
                children.prop_map(CfgSpec::Sequential),
            ]
        })
    }

    fn cfg_kind_block() -> CpgNodeKind {
        CpgNodeKind::Block {
            scope: ScopeId::GLOBAL,
        }
    }

    fn cfg_ident(seed: u8) -> CpgNodeKind {
        CpgNodeKind::Identifier {
            name: format!("v{seed}").into(),
            definition: None,
        }
    }

    fn build_cfg_specs(
        cpg: &mut CodePropertyGraph,
        parent: NodeId,
        function: NodeId,
        specs: &[CfgSpec],
    ) {
        for spec in specs {
            build_cfg_spec(cpg, parent, function, spec);
        }
    }

    fn build_cfg_spec(
        cpg: &mut CodePropertyGraph,
        parent: NodeId,
        function: NodeId,
        spec: &CfgSpec,
    ) {
        match spec {
            CfgSpec::Plain(seed) => {
                wf_child(cpg, parent, cfg_ident(*seed));
            }
            CfgSpec::Block(children) => {
                let node = wf_child(cpg, parent, cfg_kind_block());
                build_cfg_specs(cpg, node, function, children);
            }
            CfgSpec::If {
                then_branch,
                else_branch,
                wrap_else,
            } => {
                let node = wf_child(cpg, parent, CpgNodeKind::If);
                wf_child(cpg, node, cfg_ident(1));
                let then_block = wf_child(cpg, node, cfg_kind_block());
                build_cfg_specs(cpg, then_block, function, then_branch);
                if let Some(else_branch) = else_branch {
                    let else_parent = if *wrap_else {
                        wf_child(cpg, node, CpgNodeKind::Else)
                    } else {
                        node
                    };
                    let else_block = wf_child(cpg, else_parent, cfg_kind_block());
                    build_cfg_specs(cpg, else_block, function, else_branch);
                }
            }
            CfgSpec::While(children) => {
                let node = wf_child(cpg, parent, CpgNodeKind::While);
                wf_child(cpg, node, cfg_ident(2));
                let body = wf_child(cpg, node, cfg_kind_block());
                build_cfg_specs(cpg, body, function, children);
            }
            CfgSpec::For(children) => {
                let node = wf_child(cpg, parent, CpgNodeKind::For);
                wf_child(cpg, node, cfg_ident(3));
                let body = wf_child(cpg, node, cfg_kind_block());
                build_cfg_specs(cpg, body, function, children);
            }
            CfgSpec::Loop(children) => {
                let node = wf_child(cpg, parent, CpgNodeKind::Loop);
                let body = wf_child(cpg, node, cfg_kind_block());
                build_cfg_specs(cpg, body, function, children);
            }
            CfgSpec::Match(arms) => {
                let node = wf_child(cpg, parent, CpgNodeKind::Match);
                wf_child(cpg, node, cfg_ident(4));
                for arm_specs in arms {
                    let arm = wf_child(cpg, node, CpgNodeKind::MatchArm);
                    let body = wf_child(cpg, arm, cfg_kind_block());
                    build_cfg_specs(cpg, body, function, arm_specs);
                }
            }
            CfgSpec::Return(values) => {
                let node = wf_child(cpg, parent, CpgNodeKind::Return);
                build_cfg_specs(cpg, node, function, values);
            }
            CfgSpec::Break => {
                wf_child(cpg, parent, CpgNodeKind::Break);
            }
            CfgSpec::Continue => {
                wf_child(cpg, parent, CpgNodeKind::Continue);
            }
            CfgSpec::Try {
                body,
                catches,
                finally,
            } => {
                let node = wf_child(cpg, parent, CpgNodeKind::Try);
                let body_node = wf_child(cpg, node, cfg_kind_block());
                build_cfg_specs(cpg, body_node, function, body);
                for catch_specs in catches {
                    let catch = wf_child(cpg, node, CpgNodeKind::Catch);
                    let catch_body = wf_child(cpg, catch, cfg_kind_block());
                    build_cfg_specs(cpg, catch_body, function, catch_specs);
                }
                if let Some(finally_specs) = finally {
                    let finally = wf_child(cpg, node, CpgNodeKind::Finally);
                    let finally_body = wf_child(cpg, finally, cfg_kind_block());
                    build_cfg_specs(cpg, finally_body, function, finally_specs);
                }
            }
            CfgSpec::Throw(values) => {
                let node = wf_child(cpg, parent, CpgNodeKind::Throw);
                build_cfg_specs(cpg, node, function, values);
            }
            CfgSpec::Call(arguments) => {
                let node = wf_child(
                    cpg,
                    parent,
                    CpgNodeKind::Call {
                        target: Some(function),
                        is_method: false,
                    },
                );
                build_cfg_specs(cpg, node, function, arguments);
            }
            CfgSpec::Sequential(children) => {
                let node = wf_child(
                    cpg,
                    parent,
                    CpgNodeKind::Assignment {
                        operator: "=".into(),
                    },
                );
                build_cfg_specs(cpg, node, function, children);
            }
        }
    }

    fn build_cfg_cpg(specs: Vec<CfgSpec>) -> CodePropertyGraph {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let function = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Function {
                signature: Box::new(MethodSignature {
                    name: "generated_cfg".into(),
                    params: Default::default(),
                    return_type: None,
                    is_static: false,
                    is_async: false,
                    visibility: Visibility::Public,
                }),
            },
            SourceRange::default(),
        ));
        let body = wf_child(&mut cpg, function, cfg_kind_block());
        build_cfg_specs(&mut cpg, body, function, &specs);
        cpg
    }

    fn arb_cfg_cpg() -> impl Strategy<Value = CodePropertyGraph> {
        prop::collection::vec(arb_cfg_spec(), 0..=5).prop_map(build_cfg_cpg)
    }

    fn cfg_edges(cpg: &CodePropertyGraph) -> Vec<crate::CpgEdge> {
        cpg.edges()
            .filter(|edge| matches!(edge.kind, CpgEdgeKind::ControlFlow(_)))
            .cloned()
            .collect()
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(192))]

        /// The pushdown machine is an exact defunctionalization of the former
        /// mutually-recursive equations on generated bounded ASTs. All three
        /// public configuration bits vary independently.
        #[test]
        fn prop_typed_machine_exactly_matches_recursive_oracle(
            cpg in arb_cfg_cpg(),
            mask in 0_u8..8,
        ) {
            let config = CfgExtractorConfig {
                include_fallthrough: mask & 1 != 0,
                include_exceptions: mask & 2 != 0,
                include_call_edges: mask & 4 != 0,
            };
            let mut machine = cpg.clone();
            let mut oracle = cpg;
            CfgExtractor::with_config(config.clone()).extract(&mut machine);
            CfgExtractor::with_config(config).extract_recursive_oracle(&mut oracle);

            prop_assert_eq!(machine.cfg_entries(), oracle.cfg_entries());
            prop_assert_eq!(machine.cfg_exits(), oracle.cfg_exits());
            prop_assert_eq!(
                machine.edges().cloned().collect::<Vec<_>>(),
                oracle.edges().cloned().collect::<Vec<_>>()
            );
        }

        /// Adding an arbitrary extra `AstChild` relation creates sharing, a
        /// duplicate, a self-loop, or a longer cycle. The bounded recursive
        /// oracle and the production machine must still terminate with exactly
        /// the same edge stream and entry/exit order.
        #[test]
        fn prop_malformed_ast_exactly_matches_recursive_oracle(
            mut cpg in arb_cfg_cpg(),
            source_index in any::<u16>(),
            target_index in any::<u16>(),
            mask in 0_u8..8,
        ) {
            let ids: Vec<_> = cpg.node_ids().collect();
            let source = ids[usize::from(source_index) % ids.len()];
            let target = ids[usize::from(target_index) % ids.len()];
            cpg.connect(source, target, CpgEdgeKind::AstChild);

            let config = CfgExtractorConfig {
                include_fallthrough: mask & 1 != 0,
                include_exceptions: mask & 2 != 0,
                include_call_edges: mask & 4 != 0,
            };
            let mut machine = cpg.clone();
            let mut oracle = cpg;
            CfgExtractor::with_config(config.clone()).extract(&mut machine);
            CfgExtractor::with_config(config).extract_recursive_oracle(&mut oracle);

            prop_assert_eq!(machine.cfg_entries(), oracle.cfg_entries());
            prop_assert_eq!(machine.cfg_exits(), oracle.cfg_exits());
            prop_assert_eq!(
                machine.edges().cloned().collect::<Vec<_>>(),
                oracle.edges().cloned().collect::<Vec<_>>()
            );
        }

        /// Reversing insertion of irrelevant non-AST edges cannot perturb CFG
        /// edge ids, emission order, endpoints, or kinds.
        #[test]
        fn prop_cfg_is_exact_under_non_ast_insertion_permutations(cpg in arb_cfg_cpg()) {
            let mut forward = cpg.clone();
            let mut reverse = cpg;
            let ids: Vec<_> = forward.node_ids().collect();
            let relations: Vec<_> = ids.windows(2).map(|pair| (pair[0], pair[1])).collect();

            for &(source, target) in &relations {
                forward.connect(
                    source,
                    target,
                    CpgEdgeKind::DataFlow(DfgEdgeKind::DataDependency),
                );
            }
            for &(source, target) in relations.iter().rev() {
                reverse.connect(
                    source,
                    target,
                    CpgEdgeKind::DataFlow(DfgEdgeKind::DataDependency),
                );
            }

            CfgExtractor::new().extract(&mut forward);
            CfgExtractor::new().extract(&mut reverse);
            prop_assert_eq!(forward.cfg_entries(), reverse.cfg_entries());
            prop_assert_eq!(forward.cfg_exits(), reverse.cfg_exits());
            prop_assert_eq!(cfg_edges(&forward), cfg_edges(&reverse));
        }

        /// On a well-formed CPG, a second `CfgExtractor::extract` leaves the CFG
        /// edge count unchanged — the fixed idempotency (all edges are added via
        /// `connect_unique`).
        #[test]
        fn prop_cfg_extract_idempotent(cpg in arb_well_formed_cpg()) {
            let mut cpg = cpg;
            CfgExtractor::new().extract(&mut cpg);
            let after_first = cpg.stats().cfg_edges;
            CfgExtractor::new().extract(&mut cpg);
            prop_assert_eq!(after_first, cpg.stats().cfg_edges);
        }

        /// Across bounded nested statement trees, every emitted sequential
        /// edge remains within its surrounding loops and every `LoopExit`
        /// crosses at least one loop boundary. Loop-exit targets are basic
        /// block leaders because `CfgEdgeKind::is_loop` classifies the edge.
        #[test]
        fn prop_loop_exit_classification_matches_ast_scope(mut cpg in arb_well_formed_cpg()) {
            CfgExtractor::new().extract(&mut cpg);
            let function = cpg.functions().next().expect("generated function").id;
            let blocks = BasicBlockIdentifier::new().identify(&cpg, function);

            for edge in cpg.edges() {
                let CpgEdgeKind::ControlFlow(kind) = &edge.kind else {
                    continue;
                };
                let expected = CfgExtractor::continuation_edge_kind(
                    &cpg,
                    edge.source,
                    edge.target,
                );
                if *kind == CfgEdgeKind::Sequential {
                    prop_assert_ne!(expected, CfgEdgeKind::LoopExit);
                }
                if *kind == CfgEdgeKind::LoopExit {
                    prop_assert_eq!(expected, CfgEdgeKind::LoopExit);
                    prop_assert!(blocks.contains_key(&edge.target));
                }
            }
        }
    }
}
