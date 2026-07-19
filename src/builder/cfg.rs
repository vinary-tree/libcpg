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
            cpg.connect(function, body, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential));

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

        match &kind {
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
        }
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
            cpg.connect(block_id, first, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential));
        }

        // Process each child and chain them together
        let mut current_exits: SmallVec<[NodeId; 4]> = smallvec::smallvec![];
        let mut all_exits: SmallVec<[NodeId; 4]> = smallvec::smallvec![];
        let mut terminated = false;

        for (i, &child_id) in children.iter().enumerate() {
            // Connect previous exits to this child
            for &exit in &current_exits {
                cpg.connect(exit, child_id, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential));
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
                cpg.connect(if_id, children[0], CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential));
                exits.push(children[0]);
            }
            2 => {
                // condition + then branch, no else
                let condition = children[0];
                let then_branch = children[1];

                // if -> condition
                cpg.connect(if_id, condition, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential));

                // condition -> then (true)
                cpg.connect(condition, then_branch, CpgEdgeKind::ControlFlow(CfgEdgeKind::ConditionalTrue));

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
                cpg.connect(if_id, condition, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential));

                // condition -> then (true)
                cpg.connect(condition, then_branch, CpgEdgeKind::ControlFlow(CfgEdgeKind::ConditionalTrue));

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
                cpg.connect(condition, actual_else, CpgEdgeKind::ControlFlow(CfgEdgeKind::ConditionalFalse));

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
        cpg.connect(while_id, condition, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential));

        // condition -> body (true)
        cpg.connect(condition, body, CpgEdgeKind::ControlFlow(CfgEdgeKind::ConditionalTrue));

        // Process body
        let body_exits = self.process_node(cpg, body, ctx);

        // body exits -> condition (loop back)
        for &exit in &body_exits {
            cpg.connect(exit, condition, CpgEdgeKind::ControlFlow(CfgEdgeKind::LoopBack));
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
        cpg.connect(for_id, body, CpgEdgeKind::ControlFlow(CfgEdgeKind::ConditionalTrue));

        // Process body
        let body_exits = self.process_node(cpg, body, ctx);

        // body exits -> header (loop back)
        for &exit in &body_exits {
            cpg.connect(exit, header, CpgEdgeKind::ControlFlow(CfgEdgeKind::LoopBack));
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
        cpg.connect(loop_id, body, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential));

        // Process body
        let body_exits = self.process_node(cpg, body, ctx);

        // body exits -> loop (loop back)
        for &exit in &body_exits {
            cpg.connect(exit, loop_id, CpgEdgeKind::ControlFlow(CfgEdgeKind::LoopBack));
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
                cpg.connect(match_id, first, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential));
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
                cpg.connect(source, arm, CpgEdgeKind::ControlFlow(CfgEdgeKind::Case));

                // Process arm body
                let arm_children = cpg.ast_children(arm);
                if let Some(&arm_body) = arm_children.last() {
                    cpg.connect(arm, arm_body, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential));
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
            cpg.connect(return_id, expr, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential));
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
            cpg.connect(break_id, loop_ctx.loop_id, CpgEdgeKind::ControlFlow(CfgEdgeKind::Break));
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
            cpg.connect(
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
            cpg.connect(try_id, body, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential));

            // Push try context for exception handling
            ctx.push_try(try_id, catch_blocks.clone());

            let body_exits = self.process_node(cpg, body, ctx);

            ctx.pop_try();

            // Connect body exits to finally (if present) or add to exits
            if let Some(finally) = finally_block {
                for &exit in &body_exits {
                    cpg.connect(exit, finally, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential));
                }
            } else {
                exits.extend(body_exits);
            }
        }

        // Process catch blocks
        for &catch in &catch_blocks {
            let catch_children = cpg.ast_children(catch);
            if let Some(&catch_body) = catch_children.last() {
                cpg.connect(catch, catch_body, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential));
                let catch_exits = self.process_node(cpg, catch_body, ctx);

                if let Some(finally) = finally_block {
                    for &exit in &catch_exits {
                        cpg.connect(exit, finally, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential));
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
            cpg.connect(throw_id, expr, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential));
            let _ = self.process_node(cpg, expr, ctx);
        }

        // Connect to catch handlers in the try stack
        if let Some(try_ctx) = ctx.try_stack.last() {
            for &catch in &try_ctx.catch_handlers {
                cpg.connect(throw_id, catch, CpgEdgeKind::ControlFlow(CfgEdgeKind::Throw));
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
            cpg.connect(call_id, arg, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential));
            let _ = self.process_node(cpg, arg, ctx);
        }

        // Check if we have a target function
        if let Some(node) = cpg.node(call_id) {
            if let CpgNodeKind::Call { target: Some(target), .. } = &node.kind {
                let target = *target;
                // Create call edge
                cpg.connect(call_id, target, CpgEdgeKind::ControlFlow(CfgEdgeKind::Call));
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
            cpg.connect(node_id, first, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential));
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

        cpg.connect(func, body, CpgEdgeKind::AstChild);
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
        cpg.connect(body, if_node, CpgEdgeKind::AstChild);

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
        cpg.connect(if_node, cond, CpgEdgeKind::AstChild);

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
        cpg.connect(if_node, then_branch, CpgEdgeKind::AstChild);

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
}
