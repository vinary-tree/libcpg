//! Control Flow Graph extraction from CPG.
//!
//! This module provides algorithms to extract CFG edges from the AST
//! portion of a Code Property Graph. It identifies control flow constructs
//! and creates appropriate edges between basic blocks.

use rustc_hash::{FxHashMap, FxHashSet};
use smallvec::SmallVec;

use crate::{
    CodePropertyGraph, CpgEdgeKind, CpgNodeKind, CfgEdgeKind, NodeId,
};

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

/// CFG extractor that adds control flow edges to a CPG.
#[derive(Debug)]
pub struct CfgExtractor {
    config: CfgExtractorConfig,
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
        let functions: Vec<NodeId> = cpg
            .functions()
            .map(|n| n.id)
            .collect();

        for func_id in functions {
            self.extract_function_cfg(cpg, func_id);
        }
    }

    /// Extracts CFG edges for a single function.
    pub fn extract_function_cfg(&self, cpg: &mut CodePropertyGraph, function: NodeId) {
        // Mark function entry
        cpg.add_cfg_entry(function);

        // Build CFG using a context that tracks loop and try structures
        let mut ctx = CfgContext::new();

        // Get function body (should be a Block child)
        let children = cpg.ast_children(function);
        if let Some(&body) = children.last() {
            // Connect function to body entry
            cpg.connect_unique(function, body, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential));

            // Process the function body
            let exits = self.process_node(cpg, body, &mut ctx);

            // Mark all exit points
            for exit in exits {
                cpg.add_cfg_exit(exit);
            }
        }
    }

    /// Processes a node and returns the set of exit points from this node.
    fn process_node(
        &self,
        cpg: &mut CodePropertyGraph,
        node_id: NodeId,
        ctx: &mut CfgContext,
    ) -> SmallVec<[NodeId; 4]> {
        let kind = match cpg.node(node_id) {
            Some(n) => n.kind.clone(),
            None => return smallvec::smallvec![node_id],
        };

        // `process_node` and the `process_*` handlers are mutually recursive
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
            CpgNodeKind::Block { .. } => self.process_block(cpg, node_id, ctx),
            CpgNodeKind::If => self.process_if(cpg, node_id, ctx),
            CpgNodeKind::While => self.process_while(cpg, node_id, ctx),
            CpgNodeKind::For => self.process_for(cpg, node_id, ctx),
            CpgNodeKind::Loop => self.process_loop(cpg, node_id, ctx),
            CpgNodeKind::Match => self.process_match(cpg, node_id, ctx),
            CpgNodeKind::Return => self.process_return(cpg, node_id, ctx),
            CpgNodeKind::Break => self.process_break(cpg, node_id, ctx),
            CpgNodeKind::Continue => self.process_continue(cpg, node_id, ctx),
            CpgNodeKind::Try => self.process_try(cpg, node_id, ctx),
            CpgNodeKind::Throw => self.process_throw(cpg, node_id, ctx),
            CpgNodeKind::Call { .. } if self.config.include_call_edges => {
                self.process_call(cpg, node_id, ctx)
            }
            _ => {
                // For other nodes, process children sequentially
                self.process_sequential(cpg, node_id, ctx)
            }
        };

        ctx.on_path.remove(&node_id);
        exits
    }

    /// Processes a block (sequence of statements).
    fn process_block(
        &self,
        cpg: &mut CodePropertyGraph,
        block_id: NodeId,
        ctx: &mut CfgContext,
    ) -> SmallVec<[NodeId; 4]> {
        let children = cpg.ast_children(block_id);

        if children.is_empty() {
            return smallvec::smallvec![block_id];
        }

        // Connect block entry to first child
        if let Some(&first) = children.first() {
            cpg.connect_unique(block_id, first, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential));
        }

        // Process each child and chain them together
        let mut current_exits: SmallVec<[NodeId; 4]> = smallvec::smallvec![];
        let mut all_exits: SmallVec<[NodeId; 4]> = smallvec::smallvec![];
        let mut terminated = false;

        for (i, &child_id) in children.iter().enumerate() {
            // Connect previous exits to this child
            for &exit in &current_exits {
                cpg.connect_unique(exit, child_id, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential));
            }

            // Process this child
            let child_exits = self.process_node(cpg, child_id, ctx);

            // Check if this child terminates (return, break, continue, throw)
            let child_kind = cpg.node(child_id).map(|n| n.kind.clone());
            let is_terminator = matches!(
                child_kind.as_ref(),
                Some(CpgNodeKind::Return)
                    | Some(CpgNodeKind::Break)
                    | Some(CpgNodeKind::Continue)
                    | Some(CpgNodeKind::Throw)
            );

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

    /// Processes an if statement.
    fn process_if(
        &self,
        cpg: &mut CodePropertyGraph,
        if_id: NodeId,
        ctx: &mut CfgContext,
    ) -> SmallVec<[NodeId; 4]> {
        let children = cpg.ast_children(if_id);
        let mut exits = SmallVec::new();

        // Children: [condition, then_branch, else_branch?]
        // The condition is typically the first child

        match children.len() {
            0 => {
                return smallvec::smallvec![if_id];
            }
            1 => {
                // Just condition, no branches - unusual but handle it
                cpg.connect_unique(if_id, children[0], CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential));
                exits.push(children[0]);
            }
            2 => {
                // condition + then branch, no else
                let condition = children[0];
                let then_branch = children[1];

                // if -> condition
                cpg.connect_unique(if_id, condition, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential));

                // condition -> then (true)
                cpg.connect_unique(condition, then_branch, CpgEdgeKind::ControlFlow(CfgEdgeKind::ConditionalTrue));

                // Process then branch
                let then_exits = self.process_node(cpg, then_branch, ctx);
                exits.extend(then_exits);

                // If no else, condition itself is an exit (false case falls through)
                exits.push(condition);
            }
            _ => {
                // condition + then + else (or more complex)
                let condition = children[0];
                let then_branch = children[1];

                // if -> condition
                cpg.connect_unique(if_id, condition, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential));

                // condition -> then (true)
                cpg.connect_unique(condition, then_branch, CpgEdgeKind::ControlFlow(CfgEdgeKind::ConditionalTrue));

                // Process then branch
                let then_exits = self.process_node(cpg, then_branch, ctx);
                exits.extend(then_exits);

                // Handle else branch (might be Else node or direct block)
                let else_branch = children[2];
                let else_kind = cpg.node(else_branch).map(|n| n.kind.clone());

                // Determine actual else content
                let actual_else = if matches!(else_kind.as_ref(), Some(CpgNodeKind::Else)) {
                    // Get the block inside the Else node
                    cpg.ast_children(else_branch).first().copied().unwrap_or(else_branch)
                } else {
                    else_branch
                };

                // condition -> else (false)
                cpg.connect_unique(condition, actual_else, CpgEdgeKind::ControlFlow(CfgEdgeKind::ConditionalFalse));

                // Process else branch
                let else_exits = self.process_node(cpg, actual_else, ctx);
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
    fn process_while(
        &self,
        cpg: &mut CodePropertyGraph,
        while_id: NodeId,
        ctx: &mut CfgContext,
    ) -> SmallVec<[NodeId; 4]> {
        let children = cpg.ast_children(while_id);

        if children.len() < 2 {
            return smallvec::smallvec![while_id];
        }

        let condition = children[0];
        let body = children[1];

        // Push loop context for break/continue handling
        ctx.push_loop(while_id, condition);

        // while -> condition
        cpg.connect_unique(while_id, condition, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential));

        // condition -> body (true)
        cpg.connect_unique(condition, body, CpgEdgeKind::ControlFlow(CfgEdgeKind::ConditionalTrue));

        // Process body
        let body_exits = self.process_node(cpg, body, ctx);

        // body exits -> condition (loop back)
        for &exit in &body_exits {
            cpg.connect_unique(exit, condition, CpgEdgeKind::ControlFlow(CfgEdgeKind::LoopBack));
        }

        // Pop loop context and collect break targets
        let loop_ctx = ctx.pop_loop();

        // Build exits: condition (false) + break targets
        let mut exits: SmallVec<[NodeId; 4]> = smallvec::smallvec![condition];
        exits.extend(loop_ctx.break_targets);

        exits
    }

    /// Processes a for loop.
    fn process_for(
        &self,
        cpg: &mut CodePropertyGraph,
        for_id: NodeId,
        ctx: &mut CfgContext,
    ) -> SmallVec<[NodeId; 4]> {
        let children = cpg.ast_children(for_id);

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
        cpg.connect_unique(for_id, body, CpgEdgeKind::ControlFlow(CfgEdgeKind::ConditionalTrue));

        // Process body
        let body_exits = self.process_node(cpg, body, ctx);

        // body exits -> header (loop back)
        for &exit in &body_exits {
            cpg.connect_unique(exit, header, CpgEdgeKind::ControlFlow(CfgEdgeKind::LoopBack));
        }

        let loop_ctx = ctx.pop_loop();

        // Exits: for node itself (loop exit) + break targets
        let mut exits: SmallVec<[NodeId; 4]> = smallvec::smallvec![for_id];
        exits.extend(loop_ctx.break_targets);

        exits
    }

    /// Processes an infinite loop.
    fn process_loop(
        &self,
        cpg: &mut CodePropertyGraph,
        loop_id: NodeId,
        ctx: &mut CfgContext,
    ) -> SmallVec<[NodeId; 4]> {
        let children = cpg.ast_children(loop_id);

        if children.is_empty() {
            return smallvec::smallvec![loop_id];
        }

        let body = children[0];

        ctx.push_loop(loop_id, loop_id);

        // loop -> body
        cpg.connect_unique(loop_id, body, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential));

        // Process body
        let body_exits = self.process_node(cpg, body, ctx);

        // body exits -> loop (loop back)
        for &exit in &body_exits {
            cpg.connect_unique(exit, loop_id, CpgEdgeKind::ControlFlow(CfgEdgeKind::LoopBack));
        }

        let loop_ctx = ctx.pop_loop();

        // Infinite loop only exits via break
        loop_ctx.break_targets.into_iter().collect()
    }

    /// Processes a match/switch statement.
    fn process_match(
        &self,
        cpg: &mut CodePropertyGraph,
        match_id: NodeId,
        ctx: &mut CfgContext,
    ) -> SmallVec<[NodeId; 4]> {
        let children = cpg.ast_children(match_id);

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
                cpg.connect_unique(match_id, first, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential));
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
                let arm_children = cpg.ast_children(arm);
                if let Some(&arm_body) = arm_children.last() {
                    cpg.connect_unique(arm, arm_body, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential));
                    let arm_exits = self.process_node(cpg, arm_body, ctx);
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
    fn process_return(
        &self,
        cpg: &mut CodePropertyGraph,
        return_id: NodeId,
        ctx: &mut CfgContext,
    ) -> SmallVec<[NodeId; 4]> {
        let children = cpg.ast_children(return_id);

        // Process return value expression if present
        if let Some(&expr) = children.first() {
            cpg.connect_unique(return_id, expr, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential));
            let _ = self.process_node(cpg, expr, ctx);
        }

        // Return has no exits within the function
        // (the caller marks function exit points)
        smallvec::smallvec![return_id]
    }

    /// Processes a break statement.
    fn process_break(
        &self,
        cpg: &mut CodePropertyGraph,
        break_id: NodeId,
        ctx: &mut CfgContext,
    ) -> SmallVec<[NodeId; 4]> {
        // Record break for the enclosing loop
        if let Some(loop_ctx) = ctx.loop_stack.last_mut() {
            loop_ctx.break_targets.push(break_id);

            // Create break edge to loop header
            cpg.connect_unique(break_id, loop_ctx.loop_id, CpgEdgeKind::ControlFlow(CfgEdgeKind::Break));
        }

        // Break has no normal exits
        smallvec::smallvec![]
    }

    /// Processes a continue statement.
    fn process_continue(
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
    fn process_try(
        &self,
        cpg: &mut CodePropertyGraph,
        try_id: NodeId,
        ctx: &mut CfgContext,
    ) -> SmallVec<[NodeId; 4]> {
        if !self.config.include_exceptions {
            return self.process_sequential(cpg, try_id, ctx);
        }

        let children = cpg.ast_children(try_id);
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
            cpg.connect_unique(try_id, body, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential));

            // Push try context for exception handling
            ctx.push_try(try_id, catch_blocks.clone());

            let body_exits = self.process_node(cpg, body, ctx);

            ctx.pop_try();

            // Connect body exits to finally (if present) or add to exits
            if let Some(finally) = finally_block {
                for &exit in &body_exits {
                    cpg.connect_unique(exit, finally, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential));
                }
            } else {
                exits.extend(body_exits);
            }
        }

        // Process catch blocks
        for &catch in &catch_blocks {
            let catch_children = cpg.ast_children(catch);
            if let Some(&catch_body) = catch_children.last() {
                cpg.connect_unique(catch, catch_body, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential));
                let catch_exits = self.process_node(cpg, catch_body, ctx);

                if let Some(finally) = finally_block {
                    for &exit in &catch_exits {
                        cpg.connect_unique(exit, finally, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential));
                    }
                } else {
                    exits.extend(catch_exits);
                }
            }
        }

        // Process finally block
        if let Some(finally) = finally_block {
            let finally_exits = self.process_node(cpg, finally, ctx);
            exits.extend(finally_exits);
        }

        if exits.is_empty() {
            smallvec::smallvec![try_id]
        } else {
            exits
        }
    }

    /// Processes a throw statement.
    fn process_throw(
        &self,
        cpg: &mut CodePropertyGraph,
        throw_id: NodeId,
        ctx: &mut CfgContext,
    ) -> SmallVec<[NodeId; 4]> {
        if !self.config.include_exceptions {
            return smallvec::smallvec![throw_id];
        }

        let children = cpg.ast_children(throw_id);

        // Process thrown expression
        if let Some(&expr) = children.first() {
            cpg.connect_unique(throw_id, expr, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential));
            let _ = self.process_node(cpg, expr, ctx);
        }

        // Connect to catch handlers in the try stack
        if let Some(try_ctx) = ctx.try_stack.last() {
            for &catch in &try_ctx.catch_handlers {
                cpg.connect_unique(throw_id, catch, CpgEdgeKind::ControlFlow(CfgEdgeKind::Throw));
            }
        }

        // Throw has no normal exits
        smallvec::smallvec![]
    }

    /// Processes a function call.
    fn process_call(
        &self,
        cpg: &mut CodePropertyGraph,
        call_id: NodeId,
        ctx: &mut CfgContext,
    ) -> SmallVec<[NodeId; 4]> {
        let children = cpg.ast_children(call_id);

        // Process arguments
        for &arg in &children {
            cpg.connect_unique(call_id, arg, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential));
            let _ = self.process_node(cpg, arg, ctx);
        }

        // Check if we have a target function
        if let Some(node) = cpg.node(call_id) {
            if let CpgNodeKind::Call { target: Some(target), .. } = &node.kind {
                let target = *target;
                // Create call edge
                cpg.connect_unique(call_id, target, CpgEdgeKind::ControlFlow(CfgEdgeKind::Call));
            }
        }

        smallvec::smallvec![call_id]
    }

    /// Processes nodes sequentially (fallback for non-control-flow nodes).
    fn process_sequential(
        &self,
        cpg: &mut CodePropertyGraph,
        node_id: NodeId,
        ctx: &mut CfgContext,
    ) -> SmallVec<[NodeId; 4]> {
        let children = cpg.ast_children(node_id);

        if children.is_empty() {
            return smallvec::smallvec![node_id];
        }

        // Connect to first child
        if let Some(&first) = children.first() {
            cpg.connect_unique(node_id, first, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential));
        }

        // Process all children
        let mut exits = smallvec::smallvec![];
        for &child in &children {
            exits = self.process_node(cpg, child, ctx);
        }

        exits
    }
}

impl Default for CfgExtractor {
    fn default() -> Self {
        Self::new()
    }
}

/// Context for CFG extraction, tracking loop and try structures.
#[derive(Debug, Default)]
struct CfgContext {
    /// Stack of enclosing loops.
    loop_stack: Vec<LoopContext>,
    /// Stack of enclosing try blocks.
    try_stack: Vec<TryContext>,
    /// Nodes currently on the `process_node` recursion path, used to cut
    /// `AstChild` cycles in malformed graphs (see `process_node`).
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
    pub fn identify(&self, cpg: &CodePropertyGraph, function: NodeId) -> FxHashMap<NodeId, Vec<NodeId>> {
        let descendants = cpg.ast_descendants(function);
        let mut leaders: FxHashSet<NodeId> = FxHashSet::default();
        let mut blocks: FxHashMap<NodeId, Vec<NodeId>> = FxHashMap::default();

        // Find leaders:
        // 1. First statement in function
        // 2. Target of any branch (conditional, loop, etc.)
        // 3. Statement immediately after a branch

        // The function itself is a leader
        leaders.insert(function);

        for &node_id in &descendants {
            // Check if this node is a branch target
            let predecessors = cpg.cfg_predecessors(node_id);
            for (_, edge_kind) in &predecessors {
                if edge_kind.is_conditional() || edge_kind.is_loop() {
                    leaders.insert(node_id);
                    break;
                }
            }

            // Check if this node is a branching instruction
            let node_kind = cpg.node(node_id).map(|n| n.kind.clone());
            if matches!(
                node_kind.as_ref(),
                Some(CpgNodeKind::If)
                    | Some(CpgNodeKind::While)
                    | Some(CpgNodeKind::For)
                    | Some(CpgNodeKind::Loop)
                    | Some(CpgNodeKind::Match)
                    | Some(CpgNodeKind::Return)
                    | Some(CpgNodeKind::Break)
                    | Some(CpgNodeKind::Continue)
            ) {
                // Mark successors as leaders
                for (succ, _) in cpg.cfg_successors(node_id) {
                    leaders.insert(succ);
                }
            }
        }

        // Build blocks starting from each leader
        for &leader in &leaders {
            let mut block = vec![leader];
            let mut current = leader;

            loop {
                let successors = cpg.cfg_successors(current);

                // Continue if exactly one sequential successor that's not a leader
                if successors.len() == 1 {
                    let (succ, kind) = &successors[0];
                    if matches!(kind, CfgEdgeKind::Sequential) && !leaders.contains(succ) {
                        block.push(*succ);
                        current = *succ;
                        continue;
                    }
                }

                break;
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
    use crate::{CpgNode, SourceRange, Language, ScopeId, MethodSignature, Visibility};

    fn create_test_function(cpg: &mut CodePropertyGraph) -> NodeId {
        let func = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Function {
                signature: MethodSignature {
                    name: "test".into(),
                    params: Default::default(),
                    return_type: None,
                    is_static: false,
                    is_async: false,
                    visibility: Visibility::Public,
                },
            },
            SourceRange::default(),
        ));

        let body = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Block { scope: ScopeId::GLOBAL },
            SourceRange::default(),
        ));

        cpg.connect_unique(func, body, CpgEdgeKind::AstChild);
        cpg.node_mut(body).unwrap().parent = Some(func);

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
        let if_node = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::If,
            SourceRange::default(),
        ).with_parent(body));
        cpg.connect_unique(body, if_node, CpgEdgeKind::AstChild);

        // Update body's children list
        if let Some(body_node) = cpg.node_mut(body) {
            body_node.children.push(if_node);
        }

        // Add condition as child of if
        let cond = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Identifier { name: "x".into(), definition: None },
            SourceRange::default(),
        ).with_parent(if_node));
        cpg.connect_unique(if_node, cond, CpgEdgeKind::AstChild);

        // Update if's children list
        if let Some(if_node_ref) = cpg.node_mut(if_node) {
            if_node_ref.children.push(cond);
        }

        // Add then branch as child of if
        let then_branch = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Block { scope: ScopeId::GLOBAL },
            SourceRange::default(),
        ).with_parent(if_node));
        cpg.connect_unique(if_node, then_branch, CpgEdgeKind::AstChild);

        // Update if's children list
        if let Some(if_node_ref) = cpg.node_mut(if_node) {
            if_node_ref.children.push(then_branch);
        }

        let extractor = CfgExtractor::new();
        extractor.extract(&mut cpg);

        // Check CFG edges were created - if should have control flow edges
        // The if node should connect to condition, and condition to branches
        let if_successors = cpg.cfg_successors(if_node);
        assert!(!if_successors.is_empty(), "if node should have CFG successors");
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
        if let Some(n) = cpg.node_mut(id) {
            n.parent = Some(parent);
        }
        if let Some(p) = cpg.node_mut(parent) {
            p.children.push(id);
        }
        id
    }

    fn ident(name: &str) -> CpgNodeKind {
        CpgNodeKind::Identifier { name: name.into(), definition: None }
    }

    fn block() -> CpgNodeKind {
        CpgNodeKind::Block { scope: ScopeId::GLOBAL }
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

        assert!(blocks.contains_key(&func), "the function is always a leader");
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
                .any(|(_, k)| *k == CfgEdgeKind::Break),
            "a loop-less `break` has no target"
        );
        assert!(
            !cpg.cfg_successors(cont)
                .iter()
                .any(|(_, k)| *k == CfgEdgeKind::Continue),
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
        assert!(cpg.cfg_exits().contains(&ret), "`return` exits the function");
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
        assert!(has_cfg(&cpg, catch, catch_body, CfgEdgeKind::Sequential));
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
        assert!(has_cfg(&cpg, catch, catch_body, CfgEdgeKind::Sequential));
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
                .any(|(_, k)| *k == CfgEdgeKind::Throw),
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
            CpgNodeKind::Call { target: Some(callee), is_method: false },
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
            CpgNodeKind::Call { target: Some(callee2), is_method: false },
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
        assert!(!branches(cond_only_if), "with no branches there is nothing to branch to");
        assert!(!branches(bare_while), "a `while` without a body never branches or loops");
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
                signature: MethodSignature {
                    name: "empty".into(),
                    params: Default::default(),
                    return_type: None,
                    is_static: false,
                    is_async: false,
                    visibility: Visibility::Private,
                },
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
        if let Some(n) = cpg.node_mut(inner) {
            n.children.push(func);
        }

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
        if let Some(n) = cpg.node_mut(else_b) {
            n.children.push(shared);
        }

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
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]

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
    }
}
