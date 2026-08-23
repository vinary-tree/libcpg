//! Deterministic monotone data-flow framework over basic-block CFGs.
//!
//! The solver implements Kildall's worklist algorithm over [`BlockCfg`]. It is
//! feature-free, read-only, direction-polymorphic, and deterministic under CPG
//! insertion-order permutations.

mod conditions;
mod expr_key;
mod expressions;
mod facts;
mod intervals;
mod liveness;
mod sccp;

use std::{cmp::Reverse, collections::BinaryHeap, fmt::Debug};

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
use std::sync::OnceLock;

#[cfg(target_arch = "x86")]
use std::arch::x86 as bitset_arch;
#[cfg(target_arch = "x86_64")]
use std::arch::x86_64 as bitset_arch;

#[cfg(test)]
use std::collections::VecDeque;

use crate::CodePropertyGraph;

use super::{AnalysisError, BlockCfg, BlockEdge, BlockId, LoopForest};

pub(crate) use conditions::guard_expression;
pub use expr_key::{
    ExprKey, ExprKeyError, ExprKeyLimit, ExprKeyLimits, ExprKeyMetrics, ExprKeyRef, ExprOperand,
    ExprOperandRef, ExprOperands, ExprOperandsIter,
};
pub use expressions::{
    available_expressions, very_busy_expressions, AvailableExpressions, VeryBusyExpressions,
};
pub use facts::{
    expression_facts, expression_facts_with_limits, variable_facts, ExprFacts, ExprUniverse,
    FactUniverse, VarFacts, VariableDefinition,
};
pub(crate) use intervals::{
    comparison_regions, expression_value_ranges, node_value_ranges, reverse_comparison,
};
pub use intervals::{value_ranges, Bound, Interval, IntervalEnv, IntervalReport};
pub use liveness::{dead_stores, liveness, DeadStore, DeadStoreReport, LivenessResult};
pub(crate) use sccp::{fold_binary, fold_unary, literal_value};
pub use sccp::{sccp, ConstLattice, ConstValue, SccpResult};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Direction in which a data-flow problem propagates facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Direction {
    /// Entry toward exits; predecessor output meets at block input.
    Forward,
    /// Exits toward entry; successor input meets at block output.
    Backward,
}

/// Path confluence represented by an analysis domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Confluence {
    /// A fact holds when it is present on at least one path.
    May,
    /// A fact holds only when it is present on every path.
    Must,
}

/// Join-semilattice used by the monotone solver.
pub trait Lattice: Clone + PartialEq + Debug {
    /// Joins `other` into `self`, returning whether the semantic value changed.
    fn join(&mut self, other: &Self) -> bool;

    /// Returns whether `self` is below or equal to `other`.
    fn leq(&self, other: &Self) -> bool;
}

/// Dense finite bit-set domain with union as its lattice join.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct BitSetDomain {
    width: usize,
    words: Vec<u64>,
}

// Criterion crossover measurements retain scalar execution below eight words:
// a four-word vector improves, but its five-word tail regresses, whereas every
// measured width from eight words onward is neutral or faster under AVX2.
const AVX2_MIN_WORDS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BitSetKernel {
    Scalar,
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    Avx2,
}

#[inline(always)]
fn scalar_union_words(into: &mut [u64], from: &[u64]) -> bool {
    let mut changed = false;
    for (into, from) in into.iter_mut().zip(from) {
        let next = *into | *from;
        changed |= next != *into;
        *into = next;
    }
    changed
}

#[inline(always)]
fn scalar_intersection_words(into: &mut [u64], from: &[u64]) -> bool {
    let mut changed = false;
    for (into, from) in into.iter_mut().zip(from) {
        let next = *into & *from;
        changed |= next != *into;
        *into = next;
    }
    changed
}

#[inline(always)]
fn scalar_subset_words(left: &[u64], right: &[u64]) -> bool {
    left.iter()
        .zip(right)
        .all(|(left, right)| left & !right == 0)
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn detected_bitset_kernel() -> BitSetKernel {
    static KERNEL: OnceLock<BitSetKernel> = OnceLock::new();
    *KERNEL.get_or_init(|| {
        if std::is_x86_feature_detected!("avx2") {
            BitSetKernel::Avx2
        } else {
            BitSetKernel::Scalar
        }
    })
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
const fn detected_bitset_kernel() -> BitSetKernel {
    BitSetKernel::Scalar
}

#[cfg(test)]
std::thread_local! {
    static TEST_BITSET_KERNEL: std::cell::Cell<Option<BitSetKernel>> = const {
        std::cell::Cell::new(None)
    };
}

#[inline]
fn selected_bitset_kernel() -> BitSetKernel {
    #[cfg(test)]
    if let Some(kernel) = TEST_BITSET_KERNEL.with(std::cell::Cell::get) {
        return kernel;
    }
    detected_bitset_kernel()
}

#[cfg(test)]
fn with_test_bitset_kernel<R>(kernel: BitSetKernel, operation: impl FnOnce() -> R) -> R {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    if kernel == BitSetKernel::Avx2 {
        assert!(
            std::is_x86_feature_detected!("avx2"),
            "a forced AVX2 test requires AVX2 hardware"
        );
    }

    struct Restore(Option<BitSetKernel>);
    impl Drop for Restore {
        fn drop(&mut self) {
            TEST_BITSET_KERNEL.with(|selected| selected.set(self.0));
        }
    }

    let previous = TEST_BITSET_KERNEL.with(|selected| selected.replace(Some(kernel)));
    let restore = Restore(previous);
    let result = operation();
    drop(restore);
    result
}

#[inline(always)]
fn union_words(into: &mut [u64], from: &[u64]) -> bool {
    if into.len() < AVX2_MIN_WORDS {
        return scalar_union_words(into, from);
    }
    match selected_bitset_kernel() {
        BitSetKernel::Scalar => scalar_union_words(into, from),
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        BitSetKernel::Avx2 => {
            // SAFETY: the cached dispatcher returns `Avx2` only after runtime
            // feature detection (or a test-only override that checks it).
            unsafe { avx2_union_words(into, from) }
        }
    }
}

#[inline(always)]
fn intersection_words(into: &mut [u64], from: &[u64]) -> bool {
    if into.len() < AVX2_MIN_WORDS {
        return scalar_intersection_words(into, from);
    }
    match selected_bitset_kernel() {
        BitSetKernel::Scalar => scalar_intersection_words(into, from),
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        BitSetKernel::Avx2 => {
            // SAFETY: the cached dispatcher returns `Avx2` only after runtime
            // feature detection (or a test-only override that checks it).
            unsafe { avx2_intersection_words(into, from) }
        }
    }
}

/// Applies four `u64` lanes per iteration with unaligned loads and stores.
///
/// # Safety
///
/// The caller must establish AVX2 support. Rust slice validity supplies the
/// pointer, length, initialization, and exclusive-destination requirements;
/// the loop processes only complete four-word chunks and delegates its tail to
/// the scalar oracle.
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
unsafe fn avx2_union_words(into: &mut [u64], from: &[u64]) -> bool {
    debug_assert_eq!(into.len(), from.len());
    let vector_words = into.len() / 4 * 4;
    let mut changed = false;
    let mut offset = 0;
    while offset < vector_words {
        // SAFETY: `offset..offset + 4` lies in both slices. Unaligned AVX2
        // loads/stores accept the allocation alignment supplied by `Vec<u64>`.
        let equality_mask = unsafe {
            let into_pointer = into.as_mut_ptr().add(offset).cast::<bitset_arch::__m256i>();
            let from_pointer = from.as_ptr().add(offset).cast::<bitset_arch::__m256i>();
            let current = bitset_arch::_mm256_loadu_si256(into_pointer);
            let incoming = bitset_arch::_mm256_loadu_si256(from_pointer);
            let next = bitset_arch::_mm256_or_si256(current, incoming);
            bitset_arch::_mm256_storeu_si256(into_pointer, next);
            bitset_arch::_mm256_cmpeq_epi64(current, next)
        };
        changed |= bitset_arch::_mm256_movemask_epi8(equality_mask) != -1;
        offset += 4;
    }
    changed | scalar_union_words(&mut into[vector_words..], &from[vector_words..])
}

/// Applies four `u64` lanes per iteration with unaligned loads and stores.
///
/// # Safety
///
/// The caller must establish AVX2 support. Rust slice validity supplies the
/// pointer, length, initialization, and exclusive-destination requirements;
/// the loop processes only complete four-word chunks and delegates its tail to
/// the scalar oracle.
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
unsafe fn avx2_intersection_words(into: &mut [u64], from: &[u64]) -> bool {
    debug_assert_eq!(into.len(), from.len());
    let vector_words = into.len() / 4 * 4;
    let mut changed = false;
    let mut offset = 0;
    while offset < vector_words {
        // SAFETY: `offset..offset + 4` lies in both slices. Unaligned AVX2
        // loads/stores accept the allocation alignment supplied by `Vec<u64>`.
        let equality_mask = unsafe {
            let into_pointer = into.as_mut_ptr().add(offset).cast::<bitset_arch::__m256i>();
            let from_pointer = from.as_ptr().add(offset).cast::<bitset_arch::__m256i>();
            let current = bitset_arch::_mm256_loadu_si256(into_pointer);
            let incoming = bitset_arch::_mm256_loadu_si256(from_pointer);
            let next = bitset_arch::_mm256_and_si256(current, incoming);
            bitset_arch::_mm256_storeu_si256(into_pointer, next);
            bitset_arch::_mm256_cmpeq_epi64(current, next)
        };
        changed |= bitset_arch::_mm256_movemask_epi8(equality_mask) != -1;
        offset += 4;
    }
    changed | scalar_intersection_words(&mut into[vector_words..], &from[vector_words..])
}

impl BitSetDomain {
    /// Creates an empty set over `width` possible facts.
    pub fn new(width: usize) -> Self {
        Self {
            width,
            words: vec![0; width.div_ceil(64)],
        }
    }

    /// Creates the set containing every fact in the domain.
    pub fn full(width: usize) -> Self {
        let mut result = Self {
            width,
            words: vec![u64::MAX; width.div_ceil(64)],
        };
        result.mask_unused_bits();
        result
    }

    /// Creates a set from dense fact identifiers, ignoring out-of-range ids.
    pub fn from_indices(width: usize, indices: impl IntoIterator<Item = u32>) -> Self {
        let mut result = Self::new(width);
        for index in indices {
            result.insert(index);
        }
        result
    }

    /// Returns the fixed fact-domain width.
    pub const fn width(&self) -> usize {
        self.width
    }

    /// Returns whether the set contains no facts.
    pub fn is_empty(&self) -> bool {
        self.words.iter().all(|word| *word == 0)
    }

    /// Returns the number of contained facts.
    pub fn len(&self) -> usize {
        self.words
            .iter()
            .map(|word| word.count_ones() as usize)
            .sum()
    }

    /// Returns whether `fact` is present.
    pub fn contains(&self, fact: u32) -> bool {
        let fact = fact as usize;
        fact < self.width
            && self
                .words
                .get(fact / 64)
                .is_some_and(|word| word & (1u64 << (fact % 64)) != 0)
    }

    /// Inserts `fact`, returning whether the set changed.
    pub fn insert(&mut self, fact: u32) -> bool {
        let fact = fact as usize;
        if fact >= self.width {
            return false;
        }
        let mask = 1u64 << (fact % 64);
        let word = &mut self.words[fact / 64];
        let changed = *word & mask == 0;
        *word |= mask;
        changed
    }

    /// Removes `fact`, returning whether the set changed.
    pub fn remove(&mut self, fact: u32) -> bool {
        let fact = fact as usize;
        if fact >= self.width {
            return false;
        }
        let mask = 1u64 << (fact % 64);
        let word = &mut self.words[fact / 64];
        let changed = *word & mask != 0;
        *word &= !mask;
        changed
    }

    /// Removes all facts.
    pub fn clear(&mut self) {
        self.words.fill(0);
    }

    /// Intersects this set with `other`.
    ///
    /// # Panics
    ///
    /// Panics when the domains have different widths.
    pub fn intersect_with(&mut self, other: &Self) -> bool {
        self.assert_same_width(other);
        intersection_words(&mut self.words, &other.words)
    }

    /// Iterates present dense fact identifiers in ascending order.
    pub fn iter(&self) -> impl Iterator<Item = u32> + '_ {
        (0..self.width as u32).filter(|&fact| self.contains(fact))
    }

    fn assert_same_width(&self, other: &Self) {
        assert_eq!(
            self.width, other.width,
            "cannot combine bit-set domains with different widths"
        );
    }

    fn mask_unused_bits(&mut self) {
        let used = self.width % 64;
        if used != 0 {
            if let Some(last) = self.words.last_mut() {
                *last &= (1u64 << used) - 1;
            }
        }
    }
}

impl Lattice for BitSetDomain {
    fn join(&mut self, other: &Self) -> bool {
        self.assert_same_width(other);
        union_words(&mut self.words, &other.words)
    }

    fn leq(&self, other: &Self) -> bool {
        self.assert_same_width(other);
        scalar_subset_words(&self.words, &other.words)
    }
}

/// Finite must-analysis domain with intersection as its lattice join.
///
/// `Universe(width)` is the lattice bottom: every fact is provisionally true
/// before path information arrives. `Set` stores a concrete intersection.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum MustBits {
    /// Every fact in the fixed-width universe.
    Universe(usize),
    /// A concrete fact set.
    Set(BitSetDomain),
}

impl MustBits {
    /// Creates the must-lattice bottom for `width` facts.
    pub const fn universe(width: usize) -> Self {
        Self::Universe(width)
    }

    /// Creates the empty must set for `width` facts.
    pub fn empty(width: usize) -> Self {
        Self::Set(BitSetDomain::new(width))
    }

    /// Creates a concrete must set.
    pub fn from_set(set: BitSetDomain) -> Self {
        Self::Set(set)
    }

    /// Returns the fixed fact-domain width.
    pub const fn width(&self) -> usize {
        match self {
            Self::Universe(width) => *width,
            Self::Set(set) => set.width(),
        }
    }

    /// Returns whether `fact` is present.
    pub fn contains(&self, fact: u32) -> bool {
        match self {
            Self::Universe(width) => (fact as usize) < *width,
            Self::Set(set) => set.contains(fact),
        }
    }

    /// Inserts `fact`, returning whether the semantic set changed.
    pub fn insert(&mut self, fact: u32) -> bool {
        match self {
            Self::Universe(_) => false,
            Self::Set(set) => set.insert(fact),
        }
    }

    /// Removes `fact`, returning whether the semantic set changed.
    pub fn remove(&mut self, fact: u32) -> bool {
        match self {
            Self::Universe(width) => {
                if (fact as usize) >= *width {
                    return false;
                }
                let mut set = BitSetDomain::full(*width);
                set.remove(fact);
                *self = Self::Set(set);
                true
            }
            Self::Set(set) => set.remove(fact),
        }
    }

    /// Iterates present dense fact identifiers in ascending order.
    pub fn iter(&self) -> impl Iterator<Item = u32> + '_ {
        (0..self.width() as u32).filter(|&fact| self.contains(fact))
    }

    fn assert_same_width(&self, other: &Self) {
        assert_eq!(
            self.width(),
            other.width(),
            "cannot combine must domains with different widths"
        );
    }

    fn as_concrete(&self) -> BitSetDomain {
        match self {
            Self::Universe(width) => BitSetDomain::full(*width),
            Self::Set(set) => set.clone(),
        }
    }
}

impl PartialEq for MustBits {
    fn eq(&self, other: &Self) -> bool {
        self.width() == other.width() && self.as_concrete() == other.as_concrete()
    }
}

impl Eq for MustBits {}

impl Lattice for MustBits {
    fn join(&mut self, other: &Self) -> bool {
        self.assert_same_width(other);
        let before = self.clone();
        match (&mut *self, other) {
            (Self::Universe(_), Self::Universe(_)) | (Self::Set(_), Self::Universe(_)) => {}
            (into @ Self::Universe(_), Self::Set(from)) => *into = Self::Set(from.clone()),
            (Self::Set(into), Self::Set(from)) => {
                into.intersect_with(from);
            }
        }
        *self != before
    }

    fn leq(&self, other: &Self) -> bool {
        self.assert_same_width(other);
        let left = self.as_concrete();
        let right = other.as_concrete();
        right.leq(&left)
    }
}

/// One monotone data-flow problem over a [`BlockCfg`].
pub trait DataflowAnalysis {
    /// Analysis fact domain.
    type Domain: Lattice;

    /// Stable diagnostic name.
    const NAME: &'static str;
    /// Propagation direction.
    const DIRECTION: Direction;
    /// Path confluence documented by the domain's join operation.
    const CONFLUENCE: Confluence;

    /// Returns the lattice bottom used for uninitialized blocks.
    fn bottom(&self, cfg: &BlockCfg) -> Self::Domain;

    /// Returns the entry boundary (forward) or exit boundary (backward).
    fn boundary(&self, cfg: &BlockCfg) -> Self::Domain;

    /// Applies the block transfer function in the analysis direction.
    fn transfer_block(
        &self,
        cpg: &CodePropertyGraph,
        cfg: &BlockCfg,
        block: BlockId,
        state: &mut Self::Domain,
    );

    /// Optionally transforms a state as it crosses a typed block edge.
    ///
    /// Returning `None` means the edge is the identity transfer.
    fn transfer_edge(
        &self,
        _cpg: &CodePropertyGraph,
        _cfg: &BlockCfg,
        _edge: &BlockEdge,
        _state: &Self::Domain,
    ) -> Option<Self::Domain> {
        None
    }

    /// Widens `into` with `from` at an eligible loop header.
    fn widen(&self, into: &mut Self::Domain, from: &Self::Domain) -> bool {
        into.join(from)
    }
}

/// Resource and convergence controls for [`solve`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct SolverConfig {
    /// Hard cap on worklist extractions for any one block.
    pub max_visits_per_block: u32,
    /// Begin widening at loop headers after this many visits.
    pub widen_after: Option<u32>,
    /// Number of deterministic descending sweeps after the widened fixpoint.
    pub narrow_passes: u32,
}

impl Default for SolverConfig {
    fn default() -> Self {
        Self {
            max_visits_per_block: 256,
            widen_after: None,
            narrow_passes: 0,
        }
    }
}

/// Fixed-point block states and convergence evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct DataflowSolution<D> {
    /// State immediately before each block, indexed by `BlockId`.
    pub before: Vec<D>,
    /// State immediately after each block, indexed by `BlockId`.
    pub after: Vec<D>,
    /// Worklist extraction count for each block.
    pub visits: Vec<u32>,
}

impl<D: Clone> DataflowSolution<D> {
    /// Replays a node transfer to obtain the confluence-side state at `node`.
    ///
    /// For a forward problem this returns the state immediately **before** the
    /// node. For a backward problem it returns the state immediately **after**
    /// the node. `transfer_node` must be the node-level counterpart of the
    /// analysis's `transfer_block` implementation.
    pub fn state_at(
        &self,
        cfg: &BlockCfg,
        node: crate::NodeId,
        direction: Direction,
        mut transfer_node: impl FnMut(crate::NodeId, &mut D),
    ) -> Option<D> {
        let block_id = *cfg.block_of_node.get(&node)?;
        let block = cfg.block(block_id)?;
        match direction {
            Direction::Forward => {
                let mut state = self.before.get(block_id.0 as usize)?.clone();
                for &member in &block.nodes {
                    if member == node {
                        return Some(state);
                    }
                    transfer_node(member, &mut state);
                }
            }
            Direction::Backward => {
                let mut state = self.after.get(block_id.0 as usize)?.clone();
                for &member in block.nodes.iter().rev() {
                    if member == node {
                        return Some(state);
                    }
                    transfer_node(member, &mut state);
                }
            }
        }
        None
    }
}

/// Solves one monotone data-flow problem over a block CFG.
///
/// The worklist is a min-priority heap keyed by the block's deterministic RPO
/// (forward) or post-order (backward) position. Widening is applied only to
/// loop-header blocks, and the visit cap is a hard termination backstop.
pub fn solve<A: DataflowAnalysis>(
    cpg: &CodePropertyGraph,
    cfg: &BlockCfg,
    analysis: &A,
    config: &SolverConfig,
    loops: Option<&LoopForest>,
) -> Result<DataflowSolution<A::Domain>, AnalysisError> {
    solve_with_policy(cpg, cfg, analysis, config, loops, WorklistPolicy::Priority)
}

#[derive(Debug, Clone, Copy)]
enum WorklistPolicy {
    Priority,
    #[cfg(test)]
    Fifo,
    #[cfg(test)]
    Lifo,
}

enum WorkQueue {
    Priority(BinaryHeap<Reverse<(usize, BlockId)>>),
    #[cfg(test)]
    Fifo(VecDeque<BlockId>),
    #[cfg(test)]
    Lifo(Vec<BlockId>),
}

impl WorkQueue {
    fn new(policy: WorklistPolicy) -> Self {
        match policy {
            WorklistPolicy::Priority => Self::Priority(BinaryHeap::new()),
            #[cfg(test)]
            WorklistPolicy::Fifo => Self::Fifo(VecDeque::new()),
            #[cfg(test)]
            WorklistPolicy::Lifo => Self::Lifo(Vec::new()),
        }
    }

    fn push(&mut self, block: BlockId, priority: usize) {
        match self {
            Self::Priority(queue) => queue.push(Reverse((priority, block))),
            #[cfg(test)]
            Self::Fifo(queue) => queue.push_back(block),
            #[cfg(test)]
            Self::Lifo(queue) => queue.push(block),
        }
    }

    fn pop(&mut self) -> Option<BlockId> {
        match self {
            Self::Priority(queue) => queue.pop().map(|Reverse((_, block))| block),
            #[cfg(test)]
            Self::Fifo(queue) => queue.pop_front(),
            #[cfg(test)]
            Self::Lifo(queue) => queue.pop(),
        }
    }
}

fn solve_with_policy<A: DataflowAnalysis>(
    cpg: &CodePropertyGraph,
    cfg: &BlockCfg,
    analysis: &A,
    config: &SolverConfig,
    loops: Option<&LoopForest>,
    policy: WorklistPolicy,
) -> Result<DataflowSolution<A::Domain>, AnalysisError> {
    let block_count = cfg.blocks.len();
    let mut solution = DataflowSolution {
        before: (0..block_count).map(|_| analysis.bottom(cfg)).collect(),
        after: (0..block_count).map(|_| analysis.bottom(cfg)).collect(),
        visits: vec![0; block_count],
    };
    if block_count == 0 {
        return Ok(solution);
    }

    let order = match A::DIRECTION {
        Direction::Forward => cfg.reverse_post_order(),
        Direction::Backward => cfg.post_order(),
    };
    let mut priority = vec![usize::MAX; block_count];
    for (index, block) in order.iter().copied().enumerate() {
        if let Some(slot) = priority.get_mut(block.0 as usize) {
            *slot = index;
        }
    }

    let mut loop_headers = vec![false; block_count];
    if let Some(forest) = loops {
        for natural_loop in &forest.loops {
            if let Some(&block) = cfg.block_of_node.get(&natural_loop.header) {
                if let Some(is_header) = loop_headers.get_mut(block.0 as usize) {
                    *is_header = true;
                }
            }
        }
    }

    let mut queue = WorkQueue::new(policy);
    let mut queued = vec![false; block_count];
    for block in order.iter().copied() {
        enqueue(block, &priority, &mut queue, &mut queued);
    }

    while let Some(block) = queue.pop() {
        let index = block.0 as usize;
        queued[index] = false;
        if solution.visits[index] >= config.max_visits_per_block {
            return Err(AnalysisError::IterationLimitExceeded {
                analysis: A::NAME,
                limit: u64::from(config.max_visits_per_block),
            });
        }
        solution.visits[index] += 1;

        let changed = match A::DIRECTION {
            Direction::Forward => visit_forward(
                cpg,
                cfg,
                analysis,
                config,
                block,
                loop_headers[index],
                &mut solution,
            ),
            Direction::Backward => visit_backward(
                cpg,
                cfg,
                analysis,
                config,
                block,
                loop_headers[index],
                &mut solution,
            ),
        };

        if changed {
            match A::DIRECTION {
                Direction::Forward => {
                    for edge in cfg.successors(block) {
                        enqueue(edge.target, &priority, &mut queue, &mut queued);
                    }
                }
                Direction::Backward => {
                    for edge in cfg.predecessors(block) {
                        enqueue(edge.source, &priority, &mut queue, &mut queued);
                    }
                }
            }
        }
    }

    for _ in 0..config.narrow_passes {
        for &block in order {
            narrow_block(cpg, cfg, analysis, block, &mut solution);
        }
    }

    Ok(solution)
}

fn enqueue(block: BlockId, priority: &[usize], queue: &mut WorkQueue, queued: &mut [bool]) {
    let index = block.0 as usize;
    if queued.get(index).copied().unwrap_or(true) {
        return;
    }
    queued[index] = true;
    queue.push(block, priority[index]);
}

fn edge_state<A: DataflowAnalysis>(
    cpg: &CodePropertyGraph,
    cfg: &BlockCfg,
    analysis: &A,
    edge: &BlockEdge,
    state: &A::Domain,
) -> A::Domain {
    analysis
        .transfer_edge(cpg, cfg, edge, state)
        .unwrap_or_else(|| state.clone())
}

fn forward_input<A: DataflowAnalysis>(
    cpg: &CodePropertyGraph,
    cfg: &BlockCfg,
    analysis: &A,
    block: BlockId,
    after: &[A::Domain],
) -> A::Domain {
    let mut input = analysis.bottom(cfg);
    if block == cfg.entry {
        input.join(&analysis.boundary(cfg));
    }
    for edge in cfg.predecessors(block) {
        if let Some(state) = after.get(edge.source.0 as usize) {
            input.join(&edge_state(cpg, cfg, analysis, edge, state));
        }
    }
    input
}

fn backward_output<A: DataflowAnalysis>(
    cpg: &CodePropertyGraph,
    cfg: &BlockCfg,
    analysis: &A,
    block: BlockId,
    before: &[A::Domain],
) -> A::Domain {
    let mut output = analysis.bottom(cfg);
    if cfg.exit_blocks.binary_search(&block).is_ok() {
        output.join(&analysis.boundary(cfg));
    }
    for edge in cfg.successors(block) {
        if let Some(state) = before.get(edge.target.0 as usize) {
            output.join(&edge_state(cpg, cfg, analysis, edge, state));
        }
    }
    output
}

fn update_output<A: DataflowAnalysis>(
    analysis: &A,
    config: &SolverConfig,
    is_loop_header: bool,
    visits: u32,
    current: &mut A::Domain,
    candidate: A::Domain,
) -> bool {
    let should_widen = is_loop_header
        && config
            .widen_after
            .is_some_and(|threshold| visits > threshold);
    if should_widen {
        analysis.widen(current, &candidate)
    } else if *current != candidate {
        *current = candidate;
        true
    } else {
        false
    }
}

fn visit_forward<A: DataflowAnalysis>(
    cpg: &CodePropertyGraph,
    cfg: &BlockCfg,
    analysis: &A,
    config: &SolverConfig,
    block: BlockId,
    is_loop_header: bool,
    solution: &mut DataflowSolution<A::Domain>,
) -> bool {
    let index = block.0 as usize;
    let input = forward_input(cpg, cfg, analysis, block, &solution.after);
    solution.before[index] = input.clone();
    let mut candidate = input;
    analysis.transfer_block(cpg, cfg, block, &mut candidate);
    update_output(
        analysis,
        config,
        is_loop_header,
        solution.visits[index],
        &mut solution.after[index],
        candidate,
    )
}

fn visit_backward<A: DataflowAnalysis>(
    cpg: &CodePropertyGraph,
    cfg: &BlockCfg,
    analysis: &A,
    config: &SolverConfig,
    block: BlockId,
    is_loop_header: bool,
    solution: &mut DataflowSolution<A::Domain>,
) -> bool {
    let index = block.0 as usize;
    let output = backward_output(cpg, cfg, analysis, block, &solution.before);
    solution.after[index] = output.clone();
    let mut candidate = output;
    analysis.transfer_block(cpg, cfg, block, &mut candidate);
    update_output(
        analysis,
        config,
        is_loop_header,
        solution.visits[index],
        &mut solution.before[index],
        candidate,
    )
}

fn narrow_block<A: DataflowAnalysis>(
    cpg: &CodePropertyGraph,
    cfg: &BlockCfg,
    analysis: &A,
    block: BlockId,
    solution: &mut DataflowSolution<A::Domain>,
) {
    let index = block.0 as usize;
    match A::DIRECTION {
        Direction::Forward => {
            let input = forward_input(cpg, cfg, analysis, block, &solution.after);
            if input.leq(&solution.before[index]) {
                solution.before[index] = input.clone();
            }
            let mut candidate = input;
            analysis.transfer_block(cpg, cfg, block, &mut candidate);
            if candidate.leq(&solution.after[index]) {
                solution.after[index] = candidate;
            }
        }
        Direction::Backward => {
            let output = backward_output(cpg, cfg, analysis, block, &solution.before);
            if output.leq(&solution.after[index]) {
                solution.after[index] = output.clone();
            }
            let mut candidate = output;
            analysis.transfer_block(cpg, cfg, block, &mut candidate);
            if candidate.leq(&solution.before[index]) {
                solution.before[index] = candidate;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        cell::Cell,
        collections::{BTreeMap, BTreeSet},
        sync::Arc,
    };

    use proptest::prelude::*;

    use super::*;
    use crate::{
        block_cfg, loop_forest, CfgEdgeKind, CpgEdgeKind, CpgNode, CpgNodeKind, DefinitionKind,
        DfgEdgeKind, DfgExtractor, DfgExtractorConfig, Language, MethodSignature, NodeId, ScopeId,
        SourceRange, Visibility,
    };

    fn domain(width: usize, bits: u64) -> BitSetDomain {
        BitSetDomain::from_indices(
            width,
            (0..width as u32).filter(|&index| bits & (1u64 << index) != 0),
        )
    }

    fn bitset_kernel_outcome(
        kernel: BitSetKernel,
        left: &BitSetDomain,
        right: &BitSetDomain,
    ) -> (bool, BitSetDomain, bool, BitSetDomain, bool) {
        with_test_bitset_kernel(kernel, || {
            let mut union = left.clone();
            let union_changed = union.join(right);
            let mut intersection = left.clone();
            let intersection_changed = intersection.intersect_with(right);
            (
                union_changed,
                union,
                intersection_changed,
                intersection,
                left.leq(right),
            )
        })
    }

    proptest! {
        #[test]
        fn union_lattice_is_idempotent_commutative_and_associative(
            width in 0usize..64,
            a in any::<u64>(),
            b in any::<u64>(),
            c in any::<u64>(),
        ) {
            let a = domain(width, a);
            let b = domain(width, b);
            let c = domain(width, c);

            let mut idem = a.clone();
            idem.join(&a);
            prop_assert_eq!(idem, a.clone());

            let mut ab = a.clone();
            ab.join(&b);
            let mut ba = b.clone();
            ba.join(&a);
            prop_assert_eq!(ab.clone(), ba);

            let mut left = ab;
            left.join(&c);
            let mut bc = b;
            bc.join(&c);
            let mut right = a;
            right.join(&bc);
            prop_assert_eq!(left, right);
        }

        #[test]
        fn intersection_lattice_is_idempotent_commutative_and_associative(
            width in 0usize..64,
            a in any::<u64>(),
            b in any::<u64>(),
            c in any::<u64>(),
            universe_a in any::<bool>(),
            universe_b in any::<bool>(),
        ) {
            let make = |bits, universe| if universe {
                MustBits::Universe(width)
            } else {
                MustBits::Set(domain(width, bits))
            };
            let a = make(a, universe_a);
            let b = make(b, universe_b);
            let c = make(c, false);

            let mut idem = a.clone();
            idem.join(&a);
            prop_assert_eq!(idem, a.clone());

            let mut ab = a.clone();
            ab.join(&b);
            let mut ba = b.clone();
            ba.join(&a);
            prop_assert_eq!(ab.clone(), ba);

            let mut left = ab;
            left.join(&c);
            let mut bc = b;
            bc.join(&c);
            let mut right = a;
            right.join(&bc);
            prop_assert_eq!(left, right);
        }

        #[test]
        fn selected_word_kernels_match_the_scalar_oracle(
            pairs in prop::collection::vec((any::<u64>(), any::<u64>()), 0..260),
        ) {
            let width = pairs.len() * u64::BITS as usize;
            let left = BitSetDomain {
                width,
                words: pairs.iter().map(|(left, _)| *left).collect(),
            };
            let right = BitSetDomain {
                width,
                words: pairs.iter().map(|(_, right)| *right).collect(),
            };
            let scalar = bitset_kernel_outcome(BitSetKernel::Scalar, &left, &right);
            let selected = bitset_kernel_outcome(detected_bitset_kernel(), &left, &right);
            prop_assert_eq!(selected, scalar);
        }
    }

    #[test]
    fn selected_word_kernels_cover_boundaries_tails_and_change_flags() {
        for width in [
            0usize, 1, 63, 64, 65, 255, 256, 257, 511, 512, 513, 767, 768, 769, 1_023, 1_024,
            1_025, 8_191, 8_192, 8_193, 100_000,
        ] {
            let left = BitSetDomain::from_indices(
                width,
                (0..width)
                    .filter(|index| index % 2 == 0 || index % 17 == 0)
                    .map(|index| index as u32),
            );
            let right = BitSetDomain::from_indices(
                width,
                (0..width)
                    .filter(|index| index % 3 == 0 || index % 19 == 0)
                    .map(|index| index as u32),
            );
            let scalar = bitset_kernel_outcome(BitSetKernel::Scalar, &left, &right);
            let selected = bitset_kernel_outcome(detected_bitset_kernel(), &left, &right);
            assert_eq!(selected, scalar, "selected kernel differs at width {width}");

            let unchanged_union = bitset_kernel_outcome(BitSetKernel::Scalar, &left, &left);
            let selected_unchanged = bitset_kernel_outcome(detected_bitset_kernel(), &left, &left);
            assert_eq!(
                selected_unchanged, unchanged_union,
                "unchanged flags differ at width {width}"
            );
            assert!(!selected_unchanged.0);
            assert!(!selected_unchanged.2);
        }
    }

    #[test]
    fn selected_word_kernel_dispatch_is_thread_safe_and_stack_bounded() {
        let mut threads = Vec::new();
        for thread_index in 0..8usize {
            threads.push(
                std::thread::Builder::new()
                    .name(format!("bitset-kernel-{thread_index}"))
                    .stack_size(256 * 1024)
                    .spawn(move || {
                        let width = 100_000;
                        let left = BitSetDomain::from_indices(
                            width,
                            (0..width)
                                .filter(|index| (index + thread_index) % 5 == 0)
                                .map(|index| index as u32),
                        );
                        let right = BitSetDomain::from_indices(
                            width,
                            (0..width)
                                .filter(|index| (index + thread_index) % 7 == 0)
                                .map(|index| index as u32),
                        );
                        let scalar = bitset_kernel_outcome(BitSetKernel::Scalar, &left, &right);
                        for _ in 0..64 {
                            assert_eq!(
                                bitset_kernel_outcome(detected_bitset_kernel(), &left, &right),
                                scalar
                            );
                        }
                    })
                    .expect("spawn bounded-stack bit-set worker"),
            );
        }
        for thread in threads {
            thread.join().expect("bounded-stack bit-set worker");
        }
    }

    fn node(id: u32, kind: CpgNodeKind) -> CpgNode {
        CpgNode::new(NodeId::new(id), kind, SourceRange::default())
    }

    fn function() -> CpgNodeKind {
        CpgNodeKind::Function {
            signature: Box::new(MethodSignature {
                name: "flow".into(),
                params: Default::default(),
                return_type: None,
                is_static: false,
                is_async: false,
                visibility: Visibility::Private,
            }),
        }
    }

    fn block() -> CpgNodeKind {
        CpgNodeKind::Block {
            scope: ScopeId::GLOBAL,
        }
    }

    fn connect_ast(cpg: &mut CodePropertyGraph, parent: NodeId, child: NodeId) {
        cpg.connect(parent, child, CpgEdgeKind::AstChild);
    }

    fn connect_cfg(cpg: &mut CodePropertyGraph, source: NodeId, target: NodeId, kind: CfgEdgeKind) {
        cpg.connect(source, target, CpgEdgeKind::ControlFlow(kind));
    }

    fn flow_fixture(looping: bool) -> (CodePropertyGraph, BlockCfg) {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let function = cpg.add_node_with_id(node(0, function()));
        let guard = cpg.add_node_with_id(node(1, CpgNodeKind::If));
        let left = cpg.add_node_with_id(node(2, block()));
        let right = cpg.add_node_with_id(node(3, block()));
        let join = cpg.add_node_with_id(node(4, block()));
        for child in [guard, left, right, join] {
            cpg.connect(function, child, CpgEdgeKind::AstChild);
        }
        for (source, target, kind) in [
            (function, guard, CfgEdgeKind::Sequential),
            (guard, left, CfgEdgeKind::ConditionalTrue),
            (guard, right, CfgEdgeKind::ConditionalFalse),
            (left, join, CfgEdgeKind::Sequential),
            (right, join, CfgEdgeKind::Sequential),
        ] {
            cpg.connect(source, target, CpgEdgeKind::ControlFlow(kind));
        }
        if looping {
            cpg.connect(join, guard, CpgEdgeKind::ControlFlow(CfgEdgeKind::LoopBack));
        }
        let cfg = block_cfg(&cpg, function).expect("block CFG");
        (cpg, cfg)
    }

    fn generated_flow_fixture(
        node_count: usize,
        raw_edges: &[(usize, usize, u8)],
    ) -> (CodePropertyGraph, BlockCfg) {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let function = cpg.add_node_with_id(node(0, function()));
        for index in 1..node_count {
            let child = cpg.add_node_with_id(node(index as u32, block()));
            cpg.connect(function, child, CpgEdgeKind::AstChild);
        }
        for source in 0..node_count.saturating_sub(1) {
            cpg.connect(
                NodeId::new(source as u32),
                NodeId::new((source + 1) as u32),
                CpgEdgeKind::ControlFlow(CfgEdgeKind::ConditionalTrue),
            );
        }
        for &(source, target, kind) in raw_edges {
            let source = source % node_count;
            let target = target % node_count;
            cpg.connect(
                NodeId::new(source as u32),
                NodeId::new(target as u32),
                CpgEdgeKind::ControlFlow(match kind % 4 {
                    0 => CfgEdgeKind::ConditionalTrue,
                    1 => CfgEdgeKind::ConditionalFalse,
                    2 => CfgEdgeKind::LoopBack,
                    _ => CfgEdgeKind::LoopExit,
                }),
            );
        }
        let cfg = block_cfg(&cpg, function).expect("generated block CFG");
        (cpg, cfg)
    }

    struct BlockReach;

    impl DataflowAnalysis for BlockReach {
        type Domain = BitSetDomain;

        const NAME: &'static str = "block-reach";
        const DIRECTION: Direction = Direction::Forward;
        const CONFLUENCE: Confluence = Confluence::May;

        fn bottom(&self, cfg: &BlockCfg) -> Self::Domain {
            BitSetDomain::new(cfg.blocks.len())
        }

        fn boundary(&self, cfg: &BlockCfg) -> Self::Domain {
            BitSetDomain::new(cfg.blocks.len())
        }

        fn transfer_block(
            &self,
            _cpg: &CodePropertyGraph,
            _cfg: &BlockCfg,
            block: BlockId,
            state: &mut Self::Domain,
        ) {
            state.insert(block.0);
        }
    }

    #[test]
    fn priority_fifo_and_lifo_reach_the_same_solution() {
        let (cpg, cfg) = flow_fixture(true);
        let config = SolverConfig::default();
        let priority = solve_with_policy(
            &cpg,
            &cfg,
            &BlockReach,
            &config,
            None,
            WorklistPolicy::Priority,
        )
        .expect("priority solution");
        let fifo = solve_with_policy(&cpg, &cfg, &BlockReach, &config, None, WorklistPolicy::Fifo)
            .expect("FIFO solution");
        let lifo = solve_with_policy(&cpg, &cfg, &BlockReach, &config, None, WorklistPolicy::Lifo)
            .expect("LIFO solution");
        assert_eq!(priority.before, fifo.before);
        assert_eq!(priority.after, fifo.after);
        assert_eq!(priority.before, lifo.before);
        assert_eq!(priority.after, lifo.after);
    }

    proptest! {
        #[test]
        fn worklist_policy_does_not_change_generated_fixpoints(
            node_count in 1usize..20,
            raw_edges in prop::collection::vec((0usize..20, 0usize..20, any::<u8>()), 0..60),
        ) {
            let (cpg, cfg) = generated_flow_fixture(node_count, &raw_edges);
            let config = SolverConfig {
                max_visits_per_block: 4_096,
                ..SolverConfig::default()
            };
            let run = |policy| solve_with_policy(
                &cpg,
                &cfg,
                &BlockReach,
                &config,
                None,
                policy,
            ).expect("finite generated solution");
            let priority = run(WorklistPolicy::Priority);
            let fifo = run(WorklistPolicy::Fifo);
            let lifo = run(WorklistPolicy::Lifo);
            prop_assert_eq!(&priority.before, &fifo.before);
            prop_assert_eq!(&priority.after, &fifo.after);
            prop_assert_eq!(&priority.before, &lifo.before);
            prop_assert_eq!(&priority.after, &lifo.after);
        }
    }

    struct BackwardReach;

    impl DataflowAnalysis for BackwardReach {
        type Domain = BitSetDomain;

        const NAME: &'static str = "backward-reach";
        const DIRECTION: Direction = Direction::Backward;
        const CONFLUENCE: Confluence = Confluence::May;

        fn bottom(&self, cfg: &BlockCfg) -> Self::Domain {
            BitSetDomain::new(cfg.blocks.len())
        }

        fn boundary(&self, cfg: &BlockCfg) -> Self::Domain {
            BitSetDomain::new(cfg.blocks.len())
        }

        fn transfer_block(
            &self,
            _cpg: &CodePropertyGraph,
            _cfg: &BlockCfg,
            block: BlockId,
            state: &mut Self::Domain,
        ) {
            state.insert(block.0);
        }
    }

    #[test]
    fn backward_solver_propagates_from_all_exit_blocks() {
        let (cpg, cfg) = flow_fixture(false);
        let solution = solve(&cpg, &cfg, &BackwardReach, &SolverConfig::default(), None)
            .expect("backward solution");
        assert_eq!(
            solution.before[cfg.entry.0 as usize]
                .iter()
                .collect::<Vec<_>>(),
            (0..cfg.blocks.len() as u32).collect::<Vec<_>>()
        );
    }

    struct TrueEdgesOnly;

    impl DataflowAnalysis for TrueEdgesOnly {
        type Domain = BitSetDomain;

        const NAME: &'static str = "true-edges-only";
        const DIRECTION: Direction = Direction::Forward;
        const CONFLUENCE: Confluence = Confluence::May;

        fn bottom(&self, cfg: &BlockCfg) -> Self::Domain {
            BitSetDomain::new(cfg.blocks.len())
        }

        fn boundary(&self, cfg: &BlockCfg) -> Self::Domain {
            BitSetDomain::new(cfg.blocks.len())
        }

        fn transfer_block(
            &self,
            _cpg: &CodePropertyGraph,
            _cfg: &BlockCfg,
            block: BlockId,
            state: &mut Self::Domain,
        ) {
            state.insert(block.0);
        }

        fn transfer_edge(
            &self,
            _cpg: &CodePropertyGraph,
            cfg: &BlockCfg,
            edge: &BlockEdge,
            _state: &Self::Domain,
        ) -> Option<Self::Domain> {
            (edge.kind == CfgEdgeKind::ConditionalFalse)
                .then(|| BitSetDomain::new(cfg.blocks.len()))
        }
    }

    #[test]
    fn edge_transfer_can_refine_one_parallel_path() {
        let (cpg, cfg) = flow_fixture(false);
        let solution = solve(&cpg, &cfg, &TrueEdgesOnly, &SolverConfig::default(), None)
            .expect("edge-refined solution");
        let false_edge = cfg
            .edges
            .iter()
            .find(|edge| edge.kind == CfgEdgeKind::ConditionalFalse)
            .expect("false edge");
        assert!(solution.before[false_edge.target.0 as usize].is_empty());
    }

    struct BackwardTrueEdgesOnly;

    impl DataflowAnalysis for BackwardTrueEdgesOnly {
        type Domain = BitSetDomain;

        const NAME: &'static str = "backward-true-edges-only";
        const DIRECTION: Direction = Direction::Backward;
        const CONFLUENCE: Confluence = Confluence::May;

        fn bottom(&self, cfg: &BlockCfg) -> Self::Domain {
            BitSetDomain::new(cfg.blocks.len())
        }

        fn boundary(&self, cfg: &BlockCfg) -> Self::Domain {
            BitSetDomain::new(cfg.blocks.len())
        }

        fn transfer_block(
            &self,
            _cpg: &CodePropertyGraph,
            _cfg: &BlockCfg,
            block: BlockId,
            state: &mut Self::Domain,
        ) {
            state.insert(block.0);
        }

        fn transfer_edge(
            &self,
            _cpg: &CodePropertyGraph,
            cfg: &BlockCfg,
            edge: &BlockEdge,
            _state: &Self::Domain,
        ) -> Option<Self::Domain> {
            (edge.kind == CfgEdgeKind::ConditionalFalse)
                .then(|| BitSetDomain::new(cfg.blocks.len()))
        }
    }

    #[test]
    fn backward_edge_transfer_uses_the_original_typed_edge() {
        let (cpg, cfg) = flow_fixture(false);
        let solution = solve(
            &cpg,
            &cfg,
            &BackwardTrueEdgesOnly,
            &SolverConfig::default(),
            None,
        )
        .expect("backward edge-refined solution");
        let false_exit = cfg
            .edges
            .iter()
            .find(|edge| edge.kind == CfgEdgeKind::ConditionalFalse)
            .map(|edge| edge.target)
            .expect("false exit");
        let true_exit = cfg
            .edges
            .iter()
            .find(|edge| edge.kind == CfgEdgeKind::ConditionalTrue)
            .map(|edge| edge.target)
            .expect("true exit");
        let entry = &solution.before[cfg.entry.0 as usize];
        assert!(entry.contains(true_exit.0));
        assert!(!entry.contains(false_exit.0));
    }

    #[test]
    fn visit_cap_is_a_hard_backstop_on_a_loop() {
        let (cpg, cfg) = flow_fixture(true);
        let error = solve(
            &cpg,
            &cfg,
            &BlockReach,
            &SolverConfig {
                max_visits_per_block: 1,
                ..SolverConfig::default()
            },
            None,
        )
        .expect_err("one visit cannot close this loop");
        assert_eq!(
            error,
            AnalysisError::IterationLimitExceeded {
                analysis: "block-reach",
                limit: 1,
            }
        );
    }

    #[test]
    fn zero_visit_cap_rejects_before_the_counter_can_advance() {
        let (cpg, cfg) = flow_fixture(false);
        let error = solve(
            &cpg,
            &cfg,
            &BlockReach,
            &SolverConfig {
                max_visits_per_block: 0,
                ..SolverConfig::default()
            },
            None,
        )
        .expect_err("zero visits must reject the first extraction");
        assert_eq!(
            error,
            AnalysisError::IterationLimitExceeded {
                analysis: "block-reach",
                limit: 0,
            }
        );
    }

    struct WidenCounter(Cell<u32>);

    impl DataflowAnalysis for WidenCounter {
        type Domain = BitSetDomain;

        const NAME: &'static str = "widen-counter";
        const DIRECTION: Direction = Direction::Forward;
        const CONFLUENCE: Confluence = Confluence::May;

        fn bottom(&self, cfg: &BlockCfg) -> Self::Domain {
            BitSetDomain::new(cfg.blocks.len())
        }

        fn boundary(&self, cfg: &BlockCfg) -> Self::Domain {
            BitSetDomain::new(cfg.blocks.len())
        }

        fn transfer_block(
            &self,
            _cpg: &CodePropertyGraph,
            _cfg: &BlockCfg,
            block: BlockId,
            state: &mut Self::Domain,
        ) {
            state.insert(block.0);
        }

        fn widen(&self, into: &mut Self::Domain, from: &Self::Domain) -> bool {
            self.0.set(self.0.get() + 1);
            into.join(from)
        }
    }

    #[test]
    fn widening_is_confined_to_loop_header_blocks() {
        let (cpg, cfg) = flow_fixture(true);
        let forest = loop_forest(&cpg, cfg.function).expect("loop forest");
        assert!(!forest.loops.is_empty());
        let analysis = WidenCounter(Cell::new(0));
        solve(
            &cpg,
            &cfg,
            &analysis,
            &SolverConfig {
                widen_after: Some(1),
                ..SolverConfig::default()
            },
            Some(&forest),
        )
        .expect("widened solution");
        assert!(analysis.0.get() > 0);
    }

    struct DefaultWiden;

    impl DataflowAnalysis for DefaultWiden {
        type Domain = BitSetDomain;

        const NAME: &'static str = "default-widen";
        const DIRECTION: Direction = Direction::Forward;
        const CONFLUENCE: Confluence = Confluence::May;

        fn bottom(&self, cfg: &BlockCfg) -> Self::Domain {
            BitSetDomain::new(cfg.blocks.len())
        }

        fn boundary(&self, cfg: &BlockCfg) -> Self::Domain {
            BitSetDomain::new(cfg.blocks.len())
        }

        fn transfer_block(
            &self,
            _cpg: &CodePropertyGraph,
            _cfg: &BlockCfg,
            block: BlockId,
            state: &mut Self::Domain,
        ) {
            state.insert(block.0);
        }
    }

    #[test]
    fn default_widening_is_the_domain_join() {
        let (cpg, cfg) = flow_fixture(true);
        let forest = loop_forest(&cpg, cfg.function).expect("loop forest");
        let solution = solve(
            &cpg,
            &cfg,
            &DefaultWiden,
            &SolverConfig {
                widen_after: Some(1),
                ..SolverConfig::default()
            },
            Some(&forest),
        )
        .expect("default-widened solution");
        assert!(solution.visits.iter().any(|visits| *visits > 1));
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct ThreeLevel(u8);

    impl Lattice for ThreeLevel {
        fn join(&mut self, other: &Self) -> bool {
            let next = self.0.max(other.0);
            let changed = next != self.0;
            self.0 = next;
            changed
        }

        fn leq(&self, other: &Self) -> bool {
            self.0 <= other.0
        }
    }

    struct OvershootingAnalysis;

    impl DataflowAnalysis for OvershootingAnalysis {
        type Domain = ThreeLevel;

        const NAME: &'static str = "overshooting";
        const DIRECTION: Direction = Direction::Forward;
        const CONFLUENCE: Confluence = Confluence::May;

        fn bottom(&self, _cfg: &BlockCfg) -> Self::Domain {
            ThreeLevel(0)
        }

        fn boundary(&self, _cfg: &BlockCfg) -> Self::Domain {
            ThreeLevel(0)
        }

        fn transfer_block(
            &self,
            _cpg: &CodePropertyGraph,
            _cfg: &BlockCfg,
            _block: BlockId,
            state: &mut Self::Domain,
        ) {
            state.0 = 1;
        }

        fn widen(&self, into: &mut Self::Domain, _from: &Self::Domain) -> bool {
            let changed = into.0 != 2;
            into.0 = 2;
            changed
        }
    }

    #[test]
    fn narrowing_descends_from_a_widened_loop_header() {
        let (cpg, cfg) = flow_fixture(true);
        let forest = loop_forest(&cpg, cfg.function).expect("loop forest");
        let header = cfg.block_of_node[&forest.loops[0].header];
        let config = SolverConfig {
            widen_after: Some(1),
            narrow_passes: 1,
            ..SolverConfig::default()
        };
        let solution = solve(&cpg, &cfg, &OvershootingAnalysis, &config, Some(&forest))
            .expect("narrowed solution");
        assert_eq!(solution.after[header.0 as usize], ThreeLevel(1));
    }

    #[test]
    fn state_at_replays_forward_and_backward_node_order() {
        let (cpg, cfg) = flow_fixture(false);
        let solution =
            solve(&cpg, &cfg, &BlockReach, &SolverConfig::default(), None).expect("solution");
        let entry_block = cfg.block(cfg.entry).expect("entry block");
        let second = entry_block.nodes[1];
        let state = solution
            .state_at(&cfg, second, Direction::Forward, |node, state| {
                state.insert(cfg.block_of_node[&node].0);
            })
            .expect("state in block");
        assert!(state.contains(cfg.entry.0));
        assert!(solution
            .state_at(&cfg, NodeId::new(99), Direction::Forward, |_, _| {})
            .is_none());

        let mut inconsistent = cfg.clone();
        inconsistent
            .block_of_node
            .insert(NodeId::new(99), cfg.entry);
        assert!(solution
            .state_at(
                &inconsistent,
                NodeId::new(99),
                Direction::Forward,
                |_, _| {}
            )
            .is_none());
        inconsistent
            .block_of_node
            .insert(NodeId::new(99), BlockId::new(99));
        assert!(solution
            .state_at(
                &inconsistent,
                NodeId::new(99),
                Direction::Forward,
                |_, _| {}
            )
            .is_none());

        let backward = solve(
            &cpg,
            &cfg,
            &BackwardReach,
            &SolverConfig {
                narrow_passes: 1,
                ..SolverConfig::default()
            },
            None,
        )
        .expect("backward solution");
        let first = entry_block.nodes[0];
        let state = backward
            .state_at(&cfg, first, Direction::Backward, |node, state| {
                state.insert(cfg.block_of_node[&node].0);
            })
            .expect("backward state in block");
        assert!(state.contains(cfg.entry.0));
    }

    #[test]
    fn bitsets_and_must_sets_expose_total_dense_operations() {
        let mut bits = BitSetDomain::new(5);
        assert_eq!(bits.width(), 5);
        assert_eq!(bits.len(), 0);
        assert!(!bits.contains(2));
        assert!(bits.insert(4));
        assert!(!bits.insert(4));
        assert!(!bits.insert(8));
        assert!(!bits.contains(8));
        assert_eq!(bits.len(), 1);
        assert_eq!(bits.iter().collect::<Vec<_>>(), vec![4]);
        assert!(bits.remove(4));
        assert!(!bits.remove(4));
        assert!(!bits.remove(8));
        assert!(bits.is_empty());
        bits.insert(2);
        bits.clear();
        assert!(bits.is_empty());

        let full_word = BitSetDomain::full(64);
        assert_eq!(full_word.len(), 64);
        let masked = BitSetDomain::full(65);
        assert_eq!(masked.len(), 65);
        assert!(domain(5, 0b001).leq(&domain(5, 0b101)));
        assert!(!domain(5, 0b100).leq(&domain(5, 0b001)));

        let mut must = MustBits::Universe(5);
        assert_eq!(must.width(), 5);
        assert!(must.contains(4));
        assert!(!must.contains(8));
        assert!(!must.insert(2));
        assert!(!must.remove(8));
        assert!(must.remove(2));
        assert!(!must.contains(2));
        assert!(must.insert(2));
        assert_eq!(must, MustBits::Universe(5));
        assert_eq!(must.iter().collect::<Vec<_>>(), vec![0, 1, 2, 3, 4]);

        let mut concrete = MustBits::empty(5);
        assert!(concrete.insert(1));
        assert!(concrete.contains(1));
        assert_eq!(
            MustBits::from_set(BitSetDomain::full(5)),
            MustBits::universe(5)
        );
        assert_ne!(MustBits::universe(4), MustBits::universe(5));
    }

    #[test]
    fn malformed_public_cfg_and_loop_fields_remain_bounded() {
        let (cpg, mut cfg) = flow_fixture(true);
        let mut forest = loop_forest(&cpg, cfg.function).expect("loop forest");
        let missing_header = NodeId::new(99);
        forest.loops[0].header = missing_header;
        solve(
            &cpg,
            &cfg,
            &BlockReach,
            &SolverConfig::default(),
            Some(&forest),
        )
        .expect("missing external loop header is ignored");

        cfg.block_of_node.insert(missing_header, BlockId::new(99));
        let solution = solve(
            &cpg,
            &cfg,
            &BlockReach,
            &SolverConfig::default(),
            Some(&forest),
        )
        .expect("invalid external loop mapping is ignored");
        assert_eq!(solution.before.len(), cfg.blocks.len());

        cfg.blocks.clear();
        let empty = solve(&cpg, &cfg, &BlockReach, &SolverConfig::default(), None)
            .expect("externally emptied block vector is a bounded empty problem");
        assert!(empty.before.is_empty());
        assert!(empty.after.is_empty());
        assert!(empty.visits.is_empty());
    }

    #[test]
    fn narrowing_never_ascends_an_existing_solution() {
        let (cpg, cfg) = flow_fixture(false);
        let blank = || DataflowSolution {
            before: (0..cfg.blocks.len())
                .map(|_| BitSetDomain::new(cfg.blocks.len()))
                .collect(),
            after: (0..cfg.blocks.len())
                .map(|_| BitSetDomain::new(cfg.blocks.len()))
                .collect(),
            visits: vec![0; cfg.blocks.len()],
        };

        let mut forward = blank();
        narrow_block(&cpg, &cfg, &BlockReach, cfg.entry, &mut forward);
        assert!(forward.after[cfg.entry.0 as usize].is_empty());

        let mut backward = blank();
        narrow_block(&cpg, &cfg, &BackwardReach, cfg.entry, &mut backward);
        assert!(backward.before[cfg.entry.0 as usize].is_empty());
    }

    #[test]
    #[should_panic(expected = "different widths")]
    fn may_domains_reject_mismatched_widths() {
        let mut left = BitSetDomain::new(1);
        left.join(&BitSetDomain::new(2));
    }

    #[test]
    #[should_panic(expected = "different widths")]
    fn must_domains_reject_mismatched_widths() {
        let mut left = MustBits::universe(1);
        left.join(&MustBits::universe(2));
    }

    /// Private reaching-definitions instantiation used only as an independent
    /// solver oracle. Production consumers build analyses from the same public
    /// trait and fact-domain APIs.
    struct ReachingDefinitions {
        facts: FactUniverse<(Arc<str>, NodeId)>,
        definitions: BTreeMap<NodeId, Vec<(u32, u32)>>,
        by_variable: Vec<Vec<u32>>,
        parameters: Vec<u32>,
    }

    impl ReachingDefinitions {
        fn new(variable_facts: &VarFacts) -> Self {
            let mut values = Vec::new();
            for (&node, definitions) in &variable_facts.definitions {
                for definition in definitions {
                    values.push((
                        variable_facts
                            .name(definition.variable)
                            .expect("definition variable")
                            .clone(),
                        node,
                    ));
                }
            }
            let facts = FactUniverse::new(values);
            let mut definitions: BTreeMap<NodeId, Vec<(u32, u32)>> = BTreeMap::new();
            let mut by_variable = vec![Vec::new(); variable_facts.universe.len()];
            let mut parameters = Vec::new();
            for (&node, node_definitions) in &variable_facts.definitions {
                for definition in node_definitions {
                    let name = variable_facts
                        .name(definition.variable)
                        .expect("definition name")
                        .clone();
                    let fact = facts.id_of(&(name, node)).expect("definition fact");
                    definitions
                        .entry(node)
                        .or_default()
                        .push((definition.variable, fact));
                    by_variable[definition.variable as usize].push(fact);
                    if definition.kind == DefinitionKind::Parameter {
                        parameters.push(fact);
                    }
                }
            }
            parameters.sort_unstable();
            parameters.dedup();
            Self {
                facts,
                definitions,
                by_variable,
                parameters,
            }
        }

        fn transfer_node(&self, node: NodeId, state: &mut BitSetDomain) {
            for &(variable, fact) in self.definitions.get(&node).map_or(&[][..], Vec::as_slice) {
                for &killed in &self.by_variable[variable as usize] {
                    state.remove(killed);
                }
                state.insert(fact);
            }
        }

        fn concrete(&self, state: &BitSetDomain) -> BTreeSet<(Arc<str>, NodeId)> {
            state
                .iter()
                .map(|fact| self.facts.get(fact).expect("dense fact").clone())
                .collect()
        }
    }

    impl DataflowAnalysis for ReachingDefinitions {
        type Domain = BitSetDomain;

        const NAME: &'static str = "test-reaching-definitions";
        const DIRECTION: Direction = Direction::Forward;
        const CONFLUENCE: Confluence = Confluence::May;

        fn bottom(&self, _cfg: &BlockCfg) -> Self::Domain {
            BitSetDomain::new(self.facts.len())
        }

        fn boundary(&self, _cfg: &BlockCfg) -> Self::Domain {
            BitSetDomain::from_indices(self.facts.len(), self.parameters.iter().copied())
        }

        fn transfer_block(
            &self,
            _cpg: &CodePropertyGraph,
            cfg: &BlockCfg,
            block: BlockId,
            state: &mut Self::Domain,
        ) {
            for &node in &cfg.block(block).expect("valid block").nodes {
                self.transfer_node(node, state);
            }
        }
    }

    fn legacy_reaching_definitions(
        cpg: &CodePropertyGraph,
        function: NodeId,
        variables: &VarFacts,
    ) -> BTreeMap<NodeId, BTreeSet<(Arc<str>, NodeId)>> {
        let mut nodes: Vec<_> = cpg
            .ast_descendants(function)
            .into_iter()
            .filter(|&node| {
                !cpg.cfg_predecessors(node).is_empty() || !cpg.cfg_successors(node).is_empty()
            })
            .collect();
        nodes.sort_unstable();
        nodes.dedup();

        let mut before: BTreeMap<_, BTreeSet<_>> = nodes
            .iter()
            .copied()
            .map(|node| (node, BTreeSet::new()))
            .collect();
        let mut after = before.clone();
        after.insert(
            function,
            variables
                .definitions
                .iter()
                .flat_map(|(&node, definitions)| {
                    definitions
                        .iter()
                        .filter(|definition| definition.kind == DefinitionKind::Parameter)
                        .map(move |definition| {
                            (
                                variables
                                    .name(definition.variable)
                                    .expect("parameter name")
                                    .clone(),
                                node,
                            )
                        })
                })
                .collect(),
        );

        loop {
            let mut changed = false;
            for &node in &nodes {
                let mut input = BTreeSet::new();
                for (predecessor, _) in cpg.cfg_predecessors(node) {
                    if let Some(output) = after.get(&predecessor) {
                        input.extend(output.iter().cloned());
                    }
                }
                let mut output = input.clone();
                for definition in variables.definitions_at(node) {
                    let name = variables
                        .name(definition.variable)
                        .expect("definition name");
                    output.retain(|(reaching, _)| reaching != name);
                    output.insert((name.clone(), node));
                }
                changed |= before.get(&node) != Some(&input);
                changed |= after.get(&node) != Some(&output);
                before.insert(node, input);
                after.insert(node, output);
            }
            if !changed {
                return before;
            }
        }
    }

    fn reaching_fixture(cyclic: bool) -> (CodePropertyGraph, NodeId, NodeId) {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let function = cpg.add_node_with_id(node(0, function()));
        let parameter = cpg.add_node_with_id(node(
            1,
            CpgNodeKind::Parameter {
                name: "x".into(),
                param_type: None,
                is_variadic: false,
            },
        ));
        let variable = cpg.add_node_with_id(node(
            2,
            CpgNodeKind::Variable {
                name: "y".into(),
                var_type: None,
                is_mutable: true,
                scope: ScopeId::GLOBAL,
            },
        ));
        let first_use = cpg.add_node_with_id(node(
            3,
            CpgNodeKind::Identifier {
                name: "x".into(),
                definition: None,
            },
        ));
        let assignment = cpg.add_node_with_id(node(
            4,
            CpgNodeKind::Assignment {
                operator: "=".into(),
            },
        ));
        let target = cpg.add_node_with_id(node(
            5,
            CpgNodeKind::Identifier {
                name: "x".into(),
                definition: None,
            },
        ));
        let second_use = cpg.add_node_with_id(node(
            6,
            CpgNodeKind::Identifier {
                name: "x".into(),
                definition: None,
            },
        ));
        for child in [parameter, variable, first_use, assignment, second_use] {
            connect_ast(&mut cpg, function, child);
        }
        connect_ast(&mut cpg, assignment, target);
        for (source, target) in [
            (function, parameter),
            (parameter, variable),
            (variable, first_use),
            (first_use, assignment),
            (assignment, second_use),
        ] {
            connect_cfg(&mut cpg, source, target, CfgEdgeKind::Sequential);
        }
        if cyclic {
            connect_cfg(&mut cpg, second_use, first_use, CfgEdgeKind::LoopBack);
        }
        (cpg, function, first_use)
    }

    #[test]
    fn reaching_definitions_matches_retired_cfg_fixpoint_on_acyclic_input() {
        let (cpg, function, _) = reaching_fixture(false);
        let cfg = block_cfg(&cpg, function).expect("block CFG");
        let variables = variable_facts(&cpg, function);
        let analysis = ReachingDefinitions::new(&variables);
        let solution = solve(&cpg, &cfg, &analysis, &SolverConfig::default(), None)
            .expect("reaching-definition solution");
        let expected = legacy_reaching_definitions(&cpg, function, &variables);

        for (&node, expected_state) in &expected {
            let actual = solution
                .state_at(&cfg, node, Direction::Forward, |node, state| {
                    analysis.transfer_node(node, state);
                })
                .expect("CFG node state");
            assert_eq!(analysis.concrete(&actual), *expected_state, "at {node:?}");
        }
    }

    #[test]
    fn cyclic_reaching_definitions_cover_ast_ordered_def_use_edges_without_mutation() {
        let (mut cpg, function, _) = reaching_fixture(true);
        DfgExtractor::with_config(DfgExtractorConfig {
            include_parameters: false,
            include_return_values: false,
            include_field_access: false,
            ..DfgExtractorConfig::default()
        })
        .extract_function_dfg(&mut cpg, function);
        let cfg = block_cfg(&cpg, function).expect("block CFG");
        let variables = variable_facts(&cpg, function);
        let analysis = ReachingDefinitions::new(&variables);
        let graph_size = (cpg.node_count(), cpg.edge_count());
        let solution = solve(&cpg, &cfg, &analysis, &SolverConfig::default(), None)
            .expect("cyclic reaching-definition solution");
        assert_eq!((cpg.node_count(), cpg.edge_count()), graph_size);

        let mut checked = 0;
        for edge in cpg.edges() {
            if edge.kind != CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse)
                || !cfg.block_of_node.contains_key(&edge.target)
            {
                continue;
            }
            let Some(definition) = variables.definitions_at(edge.source).first() else {
                continue;
            };
            let fact = analysis
                .facts
                .id_of(&(
                    variables
                        .name(definition.variable)
                        .expect("definition name")
                        .clone(),
                    edge.source,
                ))
                .expect("definition fact");
            let state = solution
                .state_at(&cfg, edge.target, Direction::Forward, |node, state| {
                    analysis.transfer_node(node, state);
                })
                .expect("use-site state");
            assert!(
                state.contains(fact),
                "AST edge {:?}->{:?} must be covered",
                edge.source,
                edge.target
            );
            checked += 1;
        }
        assert!(checked >= 2, "fixture must exercise core def-use edges");
    }

    #[cfg(feature = "serde")]
    #[test]
    fn dataflow_solution_round_trips_through_serde() {
        let (cpg, cfg) = flow_fixture(false);
        let solution =
            solve(&cpg, &cfg, &BlockReach, &SolverConfig::default(), None).expect("solution");
        let json = serde_json::to_string(&solution).expect("serialize solution");
        let decoded: DataflowSolution<BitSetDomain> =
            serde_json::from_str(&json).expect("deserialize solution");
        assert_eq!(decoded, solution);
    }
}
