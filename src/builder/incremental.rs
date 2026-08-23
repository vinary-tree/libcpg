//! Bounded, caller-owned memoization for per-function graph derivations.
//!
//! The cache stores control-flow graph (CFG), data-flow graph (DFG), and
//! program-dependence graph (PDG) plans in function-local coordinates. A fresh
//! abstract syntax tree (AST) is always constructed, so cached node identifiers
//! can never escape the graph that created them. The full canonical key is kept
//! beside its digest and compared on every lookup; the digest is only an index.

use std::collections::BTreeMap;
use std::mem::size_of;
use std::sync::Arc;

use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};

use super::cfg::CfgPlan;
use super::dfg::{DfgPlan, FieldResolutionIndex};
use super::pdg::{existing_pdg_edges, PdgPlan};
use super::{call_arguments, CfgExtractor, CpgBuilderConfig, DfgExtractor, PdgBuilder};
use crate::{
    CfgEdgeKind, CodePropertyGraph, CpgEdgeKind, CpgNodeKind, DfgEdgeKind, Language, NodeId,
};

/// Canonical-key and cached-plan schema version.
///
/// Any semantic change to key construction, local-coordinate interpretation,
/// or the three plan algorithms must increment this value.
const FUNCTION_ANALYSIS_SCHEMA_VERSION: u32 = 1;
const KEY_DOMAIN: &[u8] = b"libcpg.function-analysis-cache";

/// Hard bounds for a [`FunctionAnalysisCache`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FunctionAnalysisCacheLimits {
    /// Maximum number of independently keyed CFG, DFG, and PDG plans.
    pub max_entries: usize,
    /// Maximum estimated resident bytes retained by plan and key allocations.
    pub max_bytes: usize,
}

impl FunctionAnalysisCacheLimits {
    /// Creates explicit entry and estimated-resident-byte bounds.
    pub const fn new(max_entries: usize, max_bytes: usize) -> Self {
        Self {
            max_entries,
            max_bytes,
        }
    }
}

impl Default for FunctionAnalysisCacheLimits {
    fn default() -> Self {
        Self::new(4_096, 64 * 1024 * 1024)
    }
}

/// Observable cache outcomes accumulated since construction or
/// [`FunctionAnalysisCache::reset_stats`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FunctionAnalysisCacheStats {
    /// Exact CFG-key hits.
    pub cfg_hits: u64,
    /// CFG keys not present in the cache.
    pub cfg_misses: u64,
    /// Exact DFG-key hits.
    pub dfg_hits: u64,
    /// Eligible DFG keys not present in the cache.
    pub dfg_misses: u64,
    /// DFG plans deliberately recomputed because their inputs cross the
    /// function-local cache boundary.
    pub dfg_dependency_bypasses: u64,
    /// Exact PDG-key hits.
    pub pdg_hits: u64,
    /// Eligible PDG keys not present in the cache.
    pub pdg_misses: u64,
    /// Functions whose source range or local AST could not form a canonical
    /// key and were therefore recomputed.
    pub key_bypasses: u64,
    /// Computed plans containing a node outside their canonical local scope.
    pub nonlocal_plan_bypasses: u64,
    /// Entries not retained because one entry or the configured zero capacity
    /// could not satisfy the cache bounds.
    pub capacity_bypasses: u64,
    /// Successful plan insertions or replacements.
    pub insertions: u64,
    /// Deterministic least-recently-used evictions.
    pub evictions: u64,
    /// Entries currently resident in the cache.
    pub entries: usize,
    /// Estimated bytes currently resident in keys, plans, and entry records.
    pub resident_bytes: usize,
    /// Canonical-key bytes included in resident bytes.
    pub resident_key_bytes: usize,
    /// Cached-plan bytes included in resident bytes.
    pub resident_artifact_bytes: usize,
    /// Entry-record bytes included in resident bytes.
    pub resident_metadata_bytes: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArtifactKind {
    Cfg = 1,
    Dfg = 2,
    Pdg = 3,
}

#[derive(Debug, Clone)]
enum CachedArtifact {
    Cfg(CfgPlan),
    Dfg(DfgPlan),
    Pdg(PdgPlan),
}

impl CachedArtifact {
    fn estimated_bytes(&self) -> usize {
        match self {
            Self::Cfg(plan) => size_of::<CfgPlan>()
                .saturating_add(plan.edges.len().saturating_mul(size_of::<(
                    NodeId,
                    NodeId,
                    CpgEdgeKind,
                )>()))
                .saturating_add(plan.exits.len().saturating_mul(size_of::<NodeId>())),
            Self::Dfg(plan) => size_of::<DfgPlan>().saturating_add(
                plan.edges
                    .len()
                    .saturating_mul(size_of::<(NodeId, NodeId, CpgEdgeKind)>()),
            ),
            Self::Pdg(plan) => size_of::<PdgPlan>().saturating_add(
                plan.control_edges
                    .len()
                    .saturating_add(plan.data_edges.len())
                    .saturating_mul(size_of::<(NodeId, NodeId)>()),
            ),
        }
    }
}

#[derive(Debug)]
struct CacheEntry {
    key: Box<[u8]>,
    artifact: Arc<CachedArtifact>,
    last_used: u128,
    estimated_bytes: usize,
}

#[derive(Debug, Clone, Copy)]
enum DigestMode {
    Fnv1a64,
    #[cfg(test)]
    Constant(u64),
}

impl DigestMode {
    fn digest(self, bytes: &[u8]) -> u64 {
        match self {
            Self::Fnv1a64 => {
                let mut digest = 0xcbf2_9ce4_8422_2325_u64;
                for &byte in bytes {
                    digest ^= u64::from(byte);
                    digest = digest.wrapping_mul(0x0000_0100_0000_01b3);
                }
                digest
            }
            #[cfg(test)]
            Self::Constant(digest) => digest,
        }
    }
}

/// A bounded cache for exact, unchanged per-function CFG, DFG, and PDG plans.
///
/// Ownership is deliberately explicit: create one cache for the editor,
/// request, or analysis session whose reuse lifetime you control, then pass a
/// mutable reference to cached builder methods. There is no process-global
/// state. Keys retain their canonical bytes and plans retain only local node
/// ordinals, so hash collisions and rebuilt graph identifiers cannot change a
/// result.
///
/// DFG reuse is conservative. A function with an external caller, a preceding
/// lexical binding, a nested function, or receiver-field resolution is
/// recomputed because those dependencies are not function-local. This is an
/// exact fallback, not an approximation.
#[derive(Debug)]
pub struct FunctionAnalysisCache {
    limits: FunctionAnalysisCacheLimits,
    entries: BTreeMap<u64, Vec<CacheEntry>>,
    entry_count: usize,
    resident_bytes: usize,
    resident_key_bytes: usize,
    resident_artifact_bytes: usize,
    clock: u128,
    stats: FunctionAnalysisCacheStats,
    digest_mode: DigestMode,
}

impl FunctionAnalysisCache {
    /// Creates an empty cache with default bounds.
    pub fn new() -> Self {
        Self::with_limits(FunctionAnalysisCacheLimits::default())
    }

    /// Creates an empty cache with caller-selected hard bounds.
    pub fn with_limits(limits: FunctionAnalysisCacheLimits) -> Self {
        Self {
            limits,
            entries: BTreeMap::new(),
            entry_count: 0,
            resident_bytes: 0,
            resident_key_bytes: 0,
            resident_artifact_bytes: 0,
            clock: 0,
            stats: FunctionAnalysisCacheStats::default(),
            digest_mode: DigestMode::Fnv1a64,
        }
    }

    /// Returns the active bounds.
    pub const fn limits(&self) -> FunctionAnalysisCacheLimits {
        self.limits
    }

    /// Returns cumulative outcomes plus current occupancy.
    pub fn stats(&self) -> FunctionAnalysisCacheStats {
        FunctionAnalysisCacheStats {
            entries: self.entry_count,
            resident_bytes: self.resident_bytes,
            resident_key_bytes: self.resident_key_bytes,
            resident_artifact_bytes: self.resident_artifact_bytes,
            resident_metadata_bytes: self
                .resident_bytes
                .saturating_sub(self.resident_key_bytes)
                .saturating_sub(self.resident_artifact_bytes),
            ..self.stats
        }
    }

    /// Resets cumulative outcomes without discarding cached plans.
    pub fn reset_stats(&mut self) {
        self.stats = FunctionAnalysisCacheStats::default();
    }

    /// Removes every plan while preserving bounds and cumulative outcomes.
    pub fn clear(&mut self) {
        self.clear_entries();
    }

    pub(super) fn is_disabled(&self) -> bool {
        self.limits.max_entries == 0 || self.limits.max_bytes == 0
    }

    pub(super) fn record_capacity_bypass(&mut self) {
        increment(&mut self.stats.capacity_bypasses);
    }

    fn clear_entries(&mut self) {
        while self.entries.pop_first().is_some() {}
        self.entry_count = 0;
        self.resident_bytes = 0;
        self.resident_key_bytes = 0;
        self.resident_artifact_bytes = 0;
    }

    fn tick(&mut self) -> u128 {
        self.clock = self.clock.saturating_add(1);
        self.clock
    }

    fn lookup(&mut self, kind: ArtifactKind, key: &[u8]) -> Option<Arc<CachedArtifact>> {
        let digest = self.digest_mode.digest(key);
        let now = self.tick();
        let result = self.entries.get_mut(&digest).and_then(|bucket| {
            bucket.iter_mut().find_map(|entry| {
                if entry.key.as_ref() == key {
                    entry.last_used = now;
                    Some(Arc::clone(&entry.artifact))
                } else {
                    None
                }
            })
        });
        match (kind, result.is_some()) {
            (ArtifactKind::Cfg, true) => {
                self.stats.cfg_hits = self.stats.cfg_hits.saturating_add(1)
            }
            (ArtifactKind::Cfg, false) => {
                self.stats.cfg_misses = self.stats.cfg_misses.saturating_add(1)
            }
            (ArtifactKind::Dfg, true) => {
                self.stats.dfg_hits = self.stats.dfg_hits.saturating_add(1)
            }
            (ArtifactKind::Dfg, false) => {
                self.stats.dfg_misses = self.stats.dfg_misses.saturating_add(1)
            }
            (ArtifactKind::Pdg, true) => {
                self.stats.pdg_hits = self.stats.pdg_hits.saturating_add(1)
            }
            (ArtifactKind::Pdg, false) => {
                self.stats.pdg_misses = self.stats.pdg_misses.saturating_add(1)
            }
        }
        result
    }

    fn insert(&mut self, key: Vec<u8>, artifact: Arc<CachedArtifact>) {
        let key_bytes = key.len();
        let artifact_bytes = artifact.estimated_bytes();
        let estimated_bytes = size_of::<CacheEntry>()
            .saturating_add(key_bytes)
            .saturating_add(artifact_bytes);
        if self.limits.max_entries == 0
            || estimated_bytes > self.limits.max_bytes
            || self.limits.max_bytes == 0
        {
            self.stats.capacity_bypasses = self.stats.capacity_bypasses.saturating_add(1);
            return;
        }

        let digest = self.digest_mode.digest(&key);
        let now = self.tick();
        let bucket = self.entries.entry(digest).or_default();
        if let Some(entry) = bucket
            .iter_mut()
            .find(|entry| entry.key.as_ref() == key.as_slice())
        {
            let previous_key_bytes = entry.key.len();
            let previous_artifact_bytes = entry.artifact.estimated_bytes();
            self.resident_bytes = self
                .resident_bytes
                .saturating_sub(entry.estimated_bytes)
                .saturating_add(estimated_bytes);
            self.resident_key_bytes = self
                .resident_key_bytes
                .saturating_sub(previous_key_bytes)
                .saturating_add(key_bytes);
            self.resident_artifact_bytes = self
                .resident_artifact_bytes
                .saturating_sub(previous_artifact_bytes)
                .saturating_add(artifact_bytes);
            entry.artifact = artifact;
            entry.last_used = now;
            entry.estimated_bytes = estimated_bytes;
        } else {
            bucket.push(CacheEntry {
                key: key.into_boxed_slice(),
                artifact,
                last_used: now,
                estimated_bytes,
            });
            self.entry_count = self
                .entry_count
                .checked_add(1)
                .expect("cache entry count fits usize");
            self.resident_bytes = self.resident_bytes.saturating_add(estimated_bytes);
            self.resident_key_bytes = self.resident_key_bytes.saturating_add(key_bytes);
            self.resident_artifact_bytes =
                self.resident_artifact_bytes.saturating_add(artifact_bytes);
        }
        self.stats.insertions = self.stats.insertions.saturating_add(1);
        self.enforce_limits();
    }

    fn enforce_limits(&mut self) {
        while self.entry_count > self.limits.max_entries
            || self.resident_bytes > self.limits.max_bytes
        {
            let victim = self
                .entries
                .iter()
                .flat_map(|(&digest, bucket)| {
                    bucket
                        .iter()
                        .enumerate()
                        .map(move |(index, entry)| (entry.last_used, digest, index))
                })
                .min();
            let Some((_, digest, index)) = victim else {
                break;
            };
            let mut remove_bucket = false;
            if let Some(bucket) = self.entries.get_mut(&digest) {
                let removed = bucket.remove(index);
                self.entry_count = self
                    .entry_count
                    .checked_sub(1)
                    .expect("an eviction removes one resident cache entry");
                self.resident_bytes = self.resident_bytes.saturating_sub(removed.estimated_bytes);
                self.resident_key_bytes = self.resident_key_bytes.saturating_sub(removed.key.len());
                self.resident_artifact_bytes = self
                    .resident_artifact_bytes
                    .saturating_sub(removed.artifact.estimated_bytes());
                remove_bucket = bucket.is_empty();
            }
            if remove_bucket {
                self.entries.remove(&digest);
            }
            self.stats.evictions = self.stats.evictions.saturating_add(1);
        }
    }

    #[cfg(test)]
    fn force_digest_for_tests(&mut self, digest: u64) {
        self.digest_mode = DigestMode::Constant(digest);
    }
}

impl Default for FunctionAnalysisCache {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for FunctionAnalysisCache {
    fn drop(&mut self) {
        self.clear_entries();
    }
}

#[derive(Debug, Clone)]
pub(super) struct FunctionContext {
    nodes: Vec<NodeId>,
    local: LocalIndex,
    common_key: Vec<u8>,
    has_nested_function: bool,
}

struct SourceFunctionSeed {
    function: NodeId,
    nodes: Vec<NodeId>,
    has_nested_function: bool,
}

#[derive(Debug, Clone)]
enum LocalIndex {
    Contiguous { base: u32, len: u32 },
    Dense { base: u32, ordinals: Vec<u32> },
    Sparse(FxHashMap<NodeId, u32>),
}

impl LocalIndex {
    fn new(nodes: &[NodeId]) -> Option<Self> {
        let (&first, rest) = nodes.split_first()?;
        let len = u32::try_from(nodes.len()).ok()?;
        if nodes.iter().copied().enumerate().all(|(ordinal, node)| {
            u32::try_from(ordinal)
                .ok()
                .and_then(|ordinal| first.0.checked_add(ordinal))
                == Some(node.0)
        }) {
            return Some(Self::Contiguous { base: first.0, len });
        }
        let mut minimum = first.0;
        let mut maximum = first.0;
        for node in rest {
            minimum = minimum.min(node.0);
            maximum = maximum.max(node.0);
        }
        let span = u64::from(maximum) - u64::from(minimum) + 1;
        let dense_limit = nodes.len().saturating_mul(2).saturating_add(8);
        if usize::try_from(span)
            .ok()
            .is_some_and(|span| span <= dense_limit)
        {
            let span = usize::try_from(span).ok()?;
            let mut ordinals = vec![u32::MAX; span];
            for (ordinal, node) in nodes.iter().enumerate() {
                let slot = usize::try_from(node.0 - minimum).ok()?;
                ordinals[slot] = u32::try_from(ordinal).ok()?;
            }
            Some(Self::Dense {
                base: minimum,
                ordinals,
            })
        } else {
            Some(Self::Sparse(
                nodes
                    .iter()
                    .copied()
                    .enumerate()
                    .map(|(ordinal, node)| {
                        u32::try_from(ordinal).ok().map(|ordinal| (node, ordinal))
                    })
                    .collect::<Option<_>>()?,
            ))
        }
    }

    fn get(&self, node: NodeId) -> Option<u32> {
        match self {
            Self::Contiguous { base, len } => {
                let ordinal = node.0.checked_sub(*base)?;
                (ordinal < *len).then_some(ordinal)
            }
            Self::Dense { base, ordinals } => {
                let offset = usize::try_from(node.0.checked_sub(*base)?).ok()?;
                let ordinal = *ordinals.get(offset)?;
                (ordinal != u32::MAX).then_some(ordinal)
            }
            Self::Sparse(ordinals) => ordinals.get(&node).copied(),
        }
    }

    fn contains(&self, node: NodeId) -> bool {
        self.get(node).is_some()
    }
}

impl FunctionContext {
    fn for_builder(
        cpg: &CodePropertyGraph,
        function: NodeId,
        source: &str,
        language: Language,
        config: &CpgBuilderConfig,
        source_determines_tree: bool,
    ) -> Option<Self> {
        let mut context = Self::from_graph(cpg, function, Some(source), !source_determines_tree)?;
        context.append_builder_dimensions(language, config);
        Some(context)
    }

    fn for_source_seed(
        cpg: &CodePropertyGraph,
        seed: SourceFunctionSeed,
        source: &str,
        language: Language,
        config: &CpgBuilderConfig,
    ) -> Option<Self> {
        let mut context = Self::from_parts(
            cpg,
            seed.function,
            seed.nodes,
            seed.has_nested_function,
            Some(source),
            false,
        )?;
        context.append_builder_dimensions(language, config);
        Some(context)
    }

    fn append_builder_dimensions(&mut self, language: Language, config: &CpgBuilderConfig) {
        put_bytes(&mut self.common_key, language.name().as_bytes());
        put_bool(&mut self.common_key, config.retain_source);
        put_bool(&mut self.common_key, config.build_cfg);
        put_bool(&mut self.common_key, config.build_dfg);
        put_bool(&mut self.common_key, config.include_comments);
        put_usize(&mut self.common_key, config.max_file_size);
        put_bool(&mut self.common_key, config.resolve_imports);
        put_bool(&mut self.common_key, config.parallel_analysis);
    }

    fn for_pdg(cpg: &CodePropertyGraph, function: NodeId) -> Option<Self> {
        Self::from_graph(cpg, function, None, true)
    }

    fn from_graph(
        cpg: &CodePropertyGraph,
        function: NodeId,
        source: Option<&str>,
        include_tree_shape: bool,
    ) -> Option<Self> {
        cpg.node(function)?;
        let mut nodes = Vec::new();
        let mut seen = FxHashSet::default();
        let mut stack = vec![function];
        let mut has_nested_function = false;

        while let Some(node) = stack.pop() {
            if !seen.insert(node) {
                continue;
            }
            cpg.node(node)?;
            nodes.push(node);
            for &child in cpg.ast_children(node).iter().rev() {
                let child_node = cpg.node(child)?;
                if child != function && matches!(child_node.kind, CpgNodeKind::Function { .. }) {
                    has_nested_function = true;
                    continue;
                }
                stack.push(child);
            }
        }

        Self::from_parts(
            cpg,
            function,
            nodes,
            has_nested_function,
            source,
            include_tree_shape,
        )
    }

    fn from_parts(
        cpg: &CodePropertyGraph,
        function: NodeId,
        nodes: Vec<NodeId>,
        has_nested_function: bool,
        source: Option<&str>,
        include_tree_shape: bool,
    ) -> Option<Self> {
        let root = cpg.node(function)?;
        let local = LocalIndex::new(&nodes)?;

        let function_source = match source {
            Some(source) => Some(source.as_bytes().get(
                usize::try_from(root.range.start).ok()?..usize::try_from(root.range.end).ok()?,
            )?),
            None => None,
        };
        let shape_capacity = if include_tree_shape {
            nodes.len().saturating_mul(24)
        } else {
            0
        };
        let mut common_key = Vec::with_capacity(
            KEY_DOMAIN
                .len()
                .saturating_add(96)
                .saturating_add(function_source.map_or(0, <[u8]>::len))
                .saturating_add(shape_capacity),
        );
        put_bytes(&mut common_key, KEY_DOMAIN);
        put_u32(&mut common_key, FUNCTION_ANALYSIS_SCHEMA_VERSION);
        match function_source {
            Some(function_source) => {
                put_u8(&mut common_key, 1);
                put_bytes(&mut common_key, function_source);
            }
            None => put_u8(&mut common_key, 0),
        }
        put_u32(&mut common_key, u32::try_from(nodes.len()).ok()?);
        put_bool(&mut common_key, has_nested_function);

        if include_tree_shape {
            let base = i64::from(root.range.start);
            for &id in &nodes {
                let node = cpg.node(id)?;
                put_i64(&mut common_key, i64::from(node.range.start) - base);
                put_i64(&mut common_key, i64::from(node.range.end) - base);
                put_u8(&mut common_key, node.kind.tag() as u8);
                if let CpgNodeKind::Unknown { kind } = &node.kind {
                    put_bytes(&mut common_key, kind.as_bytes());
                }
                put_u32(
                    &mut common_key,
                    u32::try_from(cpg.ast_children(id).len()).ok()?,
                );
                for &child in cpg.ast_children(id) {
                    match local.get(child) {
                        Some(ordinal) => {
                            put_u8(&mut common_key, 1);
                            put_u32(&mut common_key, ordinal);
                        }
                        None if cpg.node(child).is_some_and(|node| {
                            matches!(node.kind, CpgNodeKind::Function { .. })
                        }) =>
                        {
                            put_u8(&mut common_key, 2)
                        }
                        None => return None,
                    }
                }
            }
        }

        Some(Self {
            nodes,
            local,
            common_key,
            has_nested_function,
        })
    }

    fn key(&self, kind: ArtifactKind) -> Vec<u8> {
        let mut key = self.common_key.clone();
        put_u8(&mut key, kind as u8);
        key
    }

    fn local_node(&self, ordinal: u32) -> Option<NodeId> {
        self.nodes.get(usize::try_from(ordinal).ok()?).copied()
    }
}

fn put_u8(output: &mut Vec<u8>, value: u8) {
    output.push(value);
}

fn put_bool(output: &mut Vec<u8>, value: bool) {
    put_u8(output, u8::from(value));
}

fn put_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn put_i64(output: &mut Vec<u8>, value: i64) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn put_usize(output: &mut Vec<u8>, value: usize) {
    output.extend_from_slice(
        &u128::try_from(value)
            .expect("usize always fits u128")
            .to_le_bytes(),
    );
}

fn increment(counter: &mut u64) {
    *counter = counter.saturating_add(1);
}

fn put_bytes(output: &mut Vec<u8>, bytes: &[u8]) {
    put_usize(output, bytes.len());
    output.extend_from_slice(bytes);
}

fn local_ordinal(context: &FunctionContext, node: NodeId) -> Option<NodeId> {
    Some(NodeId::new(context.local.get(node)?))
}

fn global_node(context: &FunctionContext, local: NodeId) -> Option<NodeId> {
    context.local_node(local.0)
}

fn cfg_plan_is_function_local(plan: &CfgPlan, context: &FunctionContext) -> bool {
    context.local.contains(plan.function)
        && plan.edges.iter().all(|(source, target, _)| {
            context.local.contains(*source) && context.local.contains(*target)
        })
        && plan.exits.iter().all(|&exit| context.local.contains(exit))
}

fn local_cfg_plan_is_valid(plan: &CfgPlan, context: &FunctionContext) -> bool {
    global_node(context, plan.function).is_some()
        && plan.edges.iter().all(|(source, target, _)| {
            global_node(context, *source).is_some() && global_node(context, *target).is_some()
        })
        && plan
            .exits
            .iter()
            .all(|&exit| global_node(context, exit).is_some())
}

fn emit_validated_local_cfg(
    plan: &CfgPlan,
    context: &FunctionContext,
    cpg: &mut CodePropertyGraph,
) {
    let function = global_node(context, plan.function)
        .expect("validated local CFG function ordinal remains in range");
    cpg.add_cfg_entry(function);
    for (source, target, kind) in &plan.edges {
        cpg.connect_unique(
            global_node(context, *source)
                .expect("validated local CFG source ordinal remains in range"),
            global_node(context, *target)
                .expect("validated local CFG target ordinal remains in range"),
            kind.clone(),
        );
    }
    for &exit in &plan.exits {
        cpg.add_cfg_exit(
            global_node(context, exit).expect("validated local CFG exit ordinal remains in range"),
        );
    }
}

fn localize_and_emit_cfg(
    mut plan: CfgPlan,
    context: &FunctionContext,
    cpg: &mut CodePropertyGraph,
) -> CfgPlan {
    cpg.add_cfg_entry(plan.function);
    plan.function = local_ordinal(context, plan.function)
        .expect("function-local CFG function has a local ordinal");
    for (source, target, kind) in &mut plan.edges {
        cpg.connect_unique(*source, *target, kind.clone());
        *source =
            local_ordinal(context, *source).expect("function-local CFG source has a local ordinal");
        *target =
            local_ordinal(context, *target).expect("function-local CFG target has a local ordinal");
    }
    for exit in &mut plan.exits {
        cpg.add_cfg_exit(*exit);
        *exit = local_ordinal(context, *exit).expect("function-local CFG exit has a local ordinal");
    }
    plan
}

fn dfg_plan_is_function_local(plan: &DfgPlan, context: &FunctionContext) -> bool {
    plan.edges.iter().all(|(source, target, _)| {
        context.local.contains(*source) && context.local.contains(*target)
    })
}

fn local_dfg_plan_is_valid(plan: &DfgPlan, context: &FunctionContext) -> bool {
    plan.edges.iter().all(|(source, target, _)| {
        global_node(context, *source).is_some() && global_node(context, *target).is_some()
    })
}

fn emit_validated_local_dfg(
    plan: &DfgPlan,
    context: &FunctionContext,
    cpg: &mut CodePropertyGraph,
) {
    for (source, target, kind) in &plan.edges {
        cpg.connect_unique(
            global_node(context, *source)
                .expect("validated local DFG source ordinal remains in range"),
            global_node(context, *target)
                .expect("validated local DFG target ordinal remains in range"),
            kind.clone(),
        );
    }
}

fn localize_and_emit_dfg(
    mut plan: DfgPlan,
    context: &FunctionContext,
    cpg: &mut CodePropertyGraph,
) -> DfgPlan {
    for (source, target, kind) in &mut plan.edges {
        cpg.connect_unique(*source, *target, kind.clone());
        *source =
            local_ordinal(context, *source).expect("function-local DFG source has a local ordinal");
        *target =
            local_ordinal(context, *target).expect("function-local DFG target has a local ordinal");
    }
    plan
}

fn pdg_plan_is_function_local(plan: &PdgPlan, context: &FunctionContext) -> bool {
    plan.control_edges
        .iter()
        .chain(plan.data_edges.iter())
        .all(|&(source, target)| context.local.contains(source) && context.local.contains(target))
}

fn local_pdg_plan_is_valid(plan: &PdgPlan, context: &FunctionContext) -> bool {
    plan.control_edges
        .iter()
        .chain(plan.data_edges.iter())
        .all(|&(source, target)| {
            global_node(context, source).is_some() && global_node(context, target).is_some()
        })
}

fn emit_validated_local_pdg(
    plan: &PdgPlan,
    context: &FunctionContext,
    cpg: &mut CodePropertyGraph,
    existing: &mut FxHashSet<(u32, u32, bool)>,
) {
    for &(source, target) in &plan.control_edges {
        let source = global_node(context, source)
            .expect("validated local PDG control source ordinal remains in range");
        let target = global_node(context, target)
            .expect("validated local PDG control target ordinal remains in range");
        if existing.insert((source.0, target.0, true)) {
            cpg.connect(source, target, CpgEdgeKind::ControlDependence);
        }
    }
    for &(source, target) in &plan.data_edges {
        let source = global_node(context, source)
            .expect("validated local PDG data source ordinal remains in range");
        let target = global_node(context, target)
            .expect("validated local PDG data target ordinal remains in range");
        if existing.insert((source.0, target.0, false)) {
            cpg.connect(source, target, CpgEdgeKind::DataDependence);
        }
    }
}

fn localize_and_emit_pdg(
    mut plan: PdgPlan,
    context: &FunctionContext,
    cpg: &mut CodePropertyGraph,
    existing: &mut FxHashSet<(u32, u32, bool)>,
) -> PdgPlan {
    for (source, target) in &mut plan.control_edges {
        if existing.insert((source.0, target.0, true)) {
            cpg.connect(*source, *target, CpgEdgeKind::ControlDependence);
        }
        *source = local_ordinal(context, *source)
            .expect("function-local PDG control source has a local ordinal");
        *target = local_ordinal(context, *target)
            .expect("function-local PDG control target has a local ordinal");
    }
    for (source, target) in &mut plan.data_edges {
        if existing.insert((source.0, target.0, false)) {
            cpg.connect(*source, *target, CpgEdgeKind::DataDependence);
        }
        *source = local_ordinal(context, *source)
            .expect("function-local PDG data source has a local ordinal");
        *target = local_ordinal(context, *target)
            .expect("function-local PDG data target has a local ordinal");
    }
    plan
}

pub(super) fn prepare_builder_contexts(
    cpg: &CodePropertyGraph,
    source: &str,
    language: Language,
    config: &CpgBuilderConfig,
    source_determines_tree: bool,
) -> Vec<Option<FunctionContext>> {
    cpg.functions()
        .map(|node| {
            FunctionContext::for_builder(
                cpg,
                node.id,
                source,
                language,
                config,
                source_determines_tree,
            )
        })
        .collect()
}

pub(super) fn prepare_source_builder_contexts(
    cpg: &CodePropertyGraph,
    source: &str,
    language: Language,
    config: &CpgBuilderConfig,
) -> Vec<Option<FunctionContext>> {
    let mut owner_by_node: Vec<Option<usize>> = Vec::with_capacity(cpg.node_count());
    let mut seeds: Vec<SourceFunctionSeed> = Vec::new();

    for (dense, node) in cpg.nodes().enumerate() {
        let expected = NodeId::new(
            u32::try_from(dense).expect("parser-built CPG node count fits the NodeId domain"),
        );
        assert_eq!(
            node.id, expected,
            "parser-built CPG nodes remain dense and in insertion order"
        );
        let parent_owner = match node.parent {
            Some(parent) => {
                let parent = usize::try_from(parent.0)
                    .expect("parser-built parent NodeId fits the platform index domain");
                *owner_by_node
                    .get(parent)
                    .expect("a parser-built AST parent precedes its child")
            }
            None => None,
        };

        let owner = if matches!(&node.kind, CpgNodeKind::Function { .. }) {
            if let Some(parent_owner) = parent_owner {
                seeds[parent_owner].has_nested_function = true;
            }
            let owner = seeds.len();
            seeds.push(SourceFunctionSeed {
                function: node.id,
                nodes: vec![node.id],
                has_nested_function: false,
            });
            Some(owner)
        } else {
            if let Some(owner) = parent_owner {
                seeds[owner].nodes.push(node.id);
            }
            parent_owner
        };
        owner_by_node.push(owner);
    }

    assert_eq!(
        cpg.functions().count(),
        seeds.len(),
        "every parser-built function has exactly one source-key seed"
    );
    seeds
        .into_iter()
        .map(|seed| FunctionContext::for_source_seed(cpg, seed, source, language, config))
        .collect()
}

pub(super) fn extract_cfg_cached(
    extractor: &CfgExtractor,
    cpg: &mut CodePropertyGraph,
    config: &CpgBuilderConfig,
    contexts: &[Option<FunctionContext>],
    cache: &mut FunctionAnalysisCache,
) {
    let functions: Vec<NodeId> = cpg.functions().map(|node| node.id).collect();
    assert_eq!(
        functions.len(),
        contexts.len(),
        "every function must have exactly one prepared cache context"
    );
    let mut cached = Vec::with_capacity(functions.len());
    let mut keys = Vec::with_capacity(functions.len());

    for context in contexts {
        let Some(context) = context else {
            increment(&mut cache.stats.key_bypasses);
            cached.push(None);
            keys.push(None);
            continue;
        };
        let key = context.key(ArtifactKind::Cfg);
        let artifact = cache.lookup(ArtifactKind::Cfg, &key).filter(|artifact| {
            matches!(
                artifact.as_ref(),
                CachedArtifact::Cfg(plan) if local_cfg_plan_is_valid(plan, context)
            )
        });
        cached.push(artifact);
        keys.push(Some(key));
    }

    let fresh: Vec<Option<CfgPlan>> = if config.parallel_analysis {
        functions
            .par_iter()
            .copied()
            .zip(cached.par_iter())
            .map(|(function, cached)| {
                cached
                    .is_none()
                    .then(|| extractor.plan_function_cfg(cpg, function))
            })
            .collect()
    } else {
        functions
            .iter()
            .copied()
            .zip(&cached)
            .map(|(function, cached)| {
                cached
                    .is_none()
                    .then(|| extractor.plan_function_cfg(cpg, function))
            })
            .collect()
    };

    for (((fresh, cached), context), key) in fresh.into_iter().zip(cached).zip(contexts).zip(keys) {
        if let (Some(artifact), Some(context)) = (cached, context.as_ref()) {
            let CachedArtifact::Cfg(plan) = artifact.as_ref() else {
                unreachable!("a CFG-domain key can contain only a CFG plan");
            };
            emit_validated_local_cfg(plan, context, cpg);
            continue;
        }
        let Some(plan) = fresh else {
            increment(&mut cache.stats.nonlocal_plan_bypasses);
            continue;
        };
        let Some(context) = context.as_ref() else {
            plan.emit(cpg);
            continue;
        };
        if !cfg_plan_is_function_local(&plan, context) {
            increment(&mut cache.stats.nonlocal_plan_bypasses);
            plan.emit(cpg);
            continue;
        }
        let local = localize_and_emit_cfg(plan, context, cpg);
        let artifact = Arc::new(CachedArtifact::Cfg(local));
        cache.insert(
            key.expect("a canonical CFG context retains its computed key"),
            artifact,
        );
    }
}

pub(super) fn extract_dfg_cached(
    extractor: &DfgExtractor,
    cpg: &mut CodePropertyGraph,
    config: &CpgBuilderConfig,
    contexts: &mut [Option<FunctionContext>],
    cache: &mut FunctionAnalysisCache,
) {
    let functions: Vec<NodeId> = cpg.functions().map(|node| node.id).collect();
    assert_eq!(
        functions.len(),
        contexts.len(),
        "every function must have exactly one prepared cache context"
    );
    let mut eligible = Vec::with_capacity(functions.len());
    let mut cached = Vec::with_capacity(functions.len());
    let mut keys = Vec::with_capacity(functions.len());

    for (&function, context) in functions.iter().zip(contexts.iter_mut()) {
        let Some(context) = context else {
            increment(&mut cache.stats.key_bypasses);
            eligible.push(false);
            cached.push(None);
            keys.push(None);
            continue;
        };
        let is_eligible = dfg_dependencies_are_local(cpg, function, context);
        if !is_eligible {
            increment(&mut cache.stats.dfg_dependency_bypasses);
            eligible.push(false);
            cached.push(None);
            keys.push(None);
            continue;
        }
        if append_local_caller_key(cpg, function, context).is_none() {
            increment(&mut cache.stats.key_bypasses);
            eligible.push(false);
            cached.push(None);
            keys.push(None);
            continue;
        }
        eligible.push(true);
        let key = context.key(ArtifactKind::Dfg);
        let artifact = cache.lookup(ArtifactKind::Dfg, &key).filter(|artifact| {
            matches!(
                artifact.as_ref(),
                CachedArtifact::Dfg(plan) if local_dfg_plan_is_valid(plan, context)
            )
        });
        cached.push(artifact);
        keys.push(Some(key));
    }

    let field_index = cached
        .iter()
        .any(Option::is_none)
        .then(|| FieldResolutionIndex::new(cpg));

    let fresh: Vec<Option<DfgPlan>> = if config.parallel_analysis {
        functions
            .par_iter()
            .copied()
            .zip(cached.par_iter())
            .map(|(function, cached)| {
                cached
                    .is_none()
                    .then(|| extractor.plan_function_dfg(cpg, function, field_index.as_ref()))
            })
            .collect()
    } else {
        functions
            .iter()
            .copied()
            .zip(&cached)
            .map(|(function, cached)| {
                cached
                    .is_none()
                    .then(|| extractor.plan_function_dfg(cpg, function, field_index.as_ref()))
            })
            .collect()
    };

    for ((((fresh, cached), context), eligible), key) in fresh
        .into_iter()
        .zip(cached)
        .zip(contexts.iter())
        .zip(eligible)
        .zip(keys)
    {
        if let (Some(artifact), Some(context)) = (cached, context.as_ref()) {
            let CachedArtifact::Dfg(plan) = artifact.as_ref() else {
                unreachable!("a DFG-domain key can contain only a DFG plan");
            };
            emit_validated_local_dfg(plan, context, cpg);
            continue;
        }
        let Some(plan) = fresh else {
            increment(&mut cache.stats.nonlocal_plan_bypasses);
            continue;
        };
        let Some(context) = context.as_ref() else {
            plan.emit(cpg);
            continue;
        };
        if !eligible || !dfg_plan_is_function_local(&plan, context) {
            if eligible {
                increment(&mut cache.stats.nonlocal_plan_bypasses);
            }
            plan.emit(cpg);
            continue;
        }
        let local = localize_and_emit_dfg(plan, context, cpg);
        let artifact = Arc::new(CachedArtifact::Dfg(local));
        cache.insert(
            key.expect("an eligible DFG context retains its computed key"),
            artifact,
        );
    }
}

fn append_local_caller_key(
    cpg: &CodePropertyGraph,
    function: NodeId,
    context: &mut FunctionContext,
) -> Option<()> {
    let callers = cpg.callers(function);
    put_u32(&mut context.common_key, u32::try_from(callers.len()).ok()?);
    for caller in callers {
        put_u32(&mut context.common_key, context.local.get(caller)?);
        let arguments = call_arguments(cpg, caller);
        put_u32(
            &mut context.common_key,
            u32::try_from(arguments.len()).ok()?,
        );
        for argument in arguments {
            match context.local.get(argument) {
                Some(ordinal) => {
                    put_u8(&mut context.common_key, 1);
                    put_u32(&mut context.common_key, ordinal);
                }
                None => put_u8(&mut context.common_key, 0),
            }
        }
    }
    Some(())
}

fn dfg_dependencies_are_local(
    cpg: &CodePropertyGraph,
    function: NodeId,
    context: &FunctionContext,
) -> bool {
    if context.has_nested_function
        || cpg
            .callers(function)
            .iter()
            .any(|&caller| !context.local.contains(caller))
        || has_preceding_lexical_binding(cpg, function)
    {
        return false;
    }

    for &node in &context.nodes {
        let Some(CpgNodeKind::MemberAccess { .. }) = cpg.node(node).map(|node| &node.kind) else {
            continue;
        };
        let Some(&receiver) = cpg.ast_children(node).first() else {
            continue;
        };
        let receiver_is_owner = match cpg.node(receiver).map(|node| &node.kind) {
            Some(CpgNodeKind::Identifier { name, .. }) => matches!(name.as_ref(), "self" | "this"),
            Some(CpgNodeKind::Unknown { kind }) => matches!(kind.as_ref(), "self" | "this"),
            _ => false,
        };
        if receiver_is_owner {
            return false;
        }
    }
    true
}

fn has_preceding_lexical_binding(cpg: &CodePropertyGraph, function: NodeId) -> bool {
    let mut path = vec![function];
    let mut cursor = function;
    let mut seen = FxHashSet::default();
    seen.insert(function);
    while let Some(parent) = cpg.node(cursor).and_then(|node| node.parent) {
        if !seen.insert(parent) {
            return true;
        }
        path.push(parent);
        cursor = parent;
    }
    path.reverse();

    for pair in path.windows(2) {
        let parent = pair[0];
        let child_on_path = pair[1];
        let siblings = cpg.ast_children(parent);
        let already_source_ordered = siblings
            .windows(2)
            .all(|pair| lexical_order_key(cpg, pair[0]) <= lexical_order_key(cpg, pair[1]));
        if already_source_ordered {
            for &sibling in siblings {
                if sibling == child_on_path {
                    break;
                }
                if subtree_has_lexical_binding(cpg, sibling) {
                    return true;
                }
            }
        } else {
            let mut ordered = siblings.to_vec();
            ordered.sort_unstable_by_key(|&node| lexical_order_key(cpg, node));
            for sibling in ordered {
                if sibling == child_on_path {
                    break;
                }
                if subtree_has_lexical_binding(cpg, sibling) {
                    return true;
                }
            }
        }
    }
    false
}

fn lexical_order_key(cpg: &CodePropertyGraph, node: NodeId) -> (u32, u32, NodeId) {
    cpg.node(node)
        .map(|node| (node.range.start, node.range.end, node.id))
        .unwrap_or((u32::MAX, u32::MAX, node))
}

fn subtree_has_lexical_binding(cpg: &CodePropertyGraph, root: NodeId) -> bool {
    let Some(root_kind) = cpg.node(root).map(|node| &node.kind) else {
        return true;
    };
    if matches!(root_kind, CpgNodeKind::Function { .. }) {
        return false;
    }
    if matches!(
        root_kind,
        CpgNodeKind::Variable { .. }
            | CpgNodeKind::Parameter { .. }
            | CpgNodeKind::Assignment { .. }
    ) {
        return true;
    }

    let mut stack = vec![root];
    let mut seen = FxHashSet::default();
    while let Some(node) = stack.pop() {
        if !seen.insert(node) {
            continue;
        }
        let Some(kind) = cpg.node(node).map(|node| &node.kind) else {
            return true;
        };
        if matches!(kind, CpgNodeKind::Function { .. }) {
            continue;
        }
        if matches!(
            kind,
            CpgNodeKind::Variable { .. }
                | CpgNodeKind::Parameter { .. }
                | CpgNodeKind::Assignment { .. }
        ) {
            return true;
        }
        stack.extend(cpg.ast_children(node).iter().copied());
    }
    false
}

fn pdg_context(cpg: &CodePropertyGraph, function: NodeId) -> Option<FunctionContext> {
    let mut context = FunctionContext::for_pdg(cpg, function)?;
    if context.has_nested_function {
        return None;
    }
    let mut overlay = Vec::new();
    for edge in cpg
        .edges()
        .filter(|edge| edge.kind.is_cfg() || edge.kind.is_dfg())
    {
        let source = context.local.get(edge.source);
        let target = context.local.get(edge.target);
        match (source, target) {
            (Some(source), Some(target)) => {
                let (family, kind) = overlay_edge_key(&edge.kind)?;
                overlay.push((source, target, family, kind));
            }
            (None, None) => {}
            _ => return None,
        }
    }
    overlay.sort_unstable();
    put_usize(&mut context.common_key, overlay.len());
    for (source, target, family, kind) in overlay {
        put_u32(&mut context.common_key, source);
        put_u32(&mut context.common_key, target);
        put_u8(&mut context.common_key, family);
        put_u8(&mut context.common_key, kind);
    }
    Some(context)
}

fn overlay_edge_key(kind: &CpgEdgeKind) -> Option<(u8, u8)> {
    match kind {
        CpgEdgeKind::ControlFlow(kind) => Some((0, cfg_edge_key(*kind))),
        CpgEdgeKind::DataFlow(kind) => Some((1, dfg_edge_key(*kind))),
        _ => None,
    }
}

const fn cfg_edge_key(kind: CfgEdgeKind) -> u8 {
    match kind {
        CfgEdgeKind::Sequential => 0,
        CfgEdgeKind::ConditionalTrue => 1,
        CfgEdgeKind::ConditionalFalse => 2,
        CfgEdgeKind::LoopBack => 3,
        CfgEdgeKind::LoopExit => 4,
        CfgEdgeKind::Break => 5,
        CfgEdgeKind::Continue => 6,
        CfgEdgeKind::Return => 7,
        CfgEdgeKind::Throw => 8,
        CfgEdgeKind::Catch => 9,
        CfgEdgeKind::Call => 10,
        CfgEdgeKind::CallReturn => 11,
        CfgEdgeKind::Case => 12,
        CfgEdgeKind::DefaultCase => 13,
    }
}

const fn dfg_edge_key(kind: DfgEdgeKind) -> u8 {
    match kind {
        DfgEdgeKind::DefUse => 0,
        DfgEdgeKind::UseDef => 1,
        DfgEdgeKind::ReachingDef => 2,
        DfgEdgeKind::DataDependency => 3,
        DfgEdgeKind::Parameter => 4,
        DfgEdgeKind::ReturnValue => 5,
        DfgEdgeKind::FieldRead => 6,
        DfgEdgeKind::FieldWrite => 7,
        DfgEdgeKind::IndexRead => 8,
        DfgEdgeKind::IndexWrite => 9,
        DfgEdgeKind::Alias => 10,
        DfgEdgeKind::Dereference => 11,
        DfgEdgeKind::AddressOf => 12,
    }
}

pub(super) fn build_pdg_cached(
    builder: &PdgBuilder,
    cpg: &mut CodePropertyGraph,
    functions: Vec<NodeId>,
    parallel: bool,
    cache: &mut FunctionAnalysisCache,
) {
    if cache.is_disabled() {
        cache.record_capacity_bypass();
        let plans: Vec<_> = if parallel {
            functions
                .par_iter()
                .map(|&function| builder.plan(cpg, function))
                .collect()
        } else {
            functions
                .iter()
                .map(|&function| builder.plan(cpg, function))
                .collect()
        };
        let mut existing = existing_pdg_edges(cpg);
        for plan in plans {
            plan.emit(cpg, &mut existing);
        }
        return;
    }

    let contexts: Vec<_> = functions
        .iter()
        .map(|&function| pdg_context(cpg, function))
        .collect();
    let mut cached = Vec::with_capacity(functions.len());
    let mut keys = Vec::with_capacity(functions.len());
    for context in &contexts {
        let Some(context) = context else {
            increment(&mut cache.stats.key_bypasses);
            cached.push(None);
            keys.push(None);
            continue;
        };
        let key = context.key(ArtifactKind::Pdg);
        let artifact = cache.lookup(ArtifactKind::Pdg, &key).filter(|artifact| {
            matches!(
                artifact.as_ref(),
                CachedArtifact::Pdg(plan) if local_pdg_plan_is_valid(plan, context)
            )
        });
        cached.push(artifact);
        keys.push(Some(key));
    }

    let fresh: Vec<Option<PdgPlan>> = if parallel {
        functions
            .par_iter()
            .copied()
            .zip(cached.par_iter())
            .map(|(function, cached)| cached.is_none().then(|| builder.plan(cpg, function)))
            .collect()
    } else {
        functions
            .iter()
            .copied()
            .zip(&cached)
            .map(|(function, cached)| cached.is_none().then(|| builder.plan(cpg, function)))
            .collect()
    };

    let mut existing = existing_pdg_edges(cpg);
    for (((fresh, cached), context), key) in fresh.into_iter().zip(cached).zip(contexts).zip(keys) {
        if let (Some(artifact), Some(context)) = (cached, context.as_ref()) {
            let CachedArtifact::Pdg(plan) = artifact.as_ref() else {
                unreachable!("a PDG-domain key can contain only a PDG plan");
            };
            emit_validated_local_pdg(plan, context, cpg, &mut existing);
            continue;
        }
        let Some(plan) = fresh else {
            increment(&mut cache.stats.nonlocal_plan_bypasses);
            continue;
        };
        let Some(context) = context.as_ref() else {
            plan.emit(cpg, &mut existing);
            continue;
        };
        if !pdg_plan_is_function_local(&plan, context) {
            increment(&mut cache.stats.nonlocal_plan_bypasses);
            plan.emit(cpg, &mut existing);
            continue;
        }
        let local = localize_and_emit_pdg(plan, context, cpg, &mut existing);
        let artifact = Arc::new(CachedArtifact::Pdg(local));
        cache.insert(
            key.expect("a canonical PDG context retains its computed key"),
            artifact,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CpgEdge, CpgNode, EdgeId, MethodSignature, SourceRange, Visibility};

    fn empty_cfg(function: u32, exit: u32) -> Arc<CachedArtifact> {
        Arc::new(CachedArtifact::Cfg(CfgPlan {
            function: NodeId::new(function),
            edges: Vec::new(),
            exits: [NodeId::new(exit)].into_iter().collect(),
        }))
    }

    #[test]
    fn canonical_material_disambiguates_forced_digest_collisions() {
        let mut cache =
            FunctionAnalysisCache::with_limits(FunctionAnalysisCacheLimits::new(8, 64 * 1024));
        cache.force_digest_for_tests(7);
        let first = b"schema-one".to_vec();
        let second = b"schema-two".to_vec();
        cache.insert(first.clone(), empty_cfg(0, 1));
        cache.insert(second.clone(), empty_cfg(0, 2));

        let Some(first_artifact) = cache.lookup(ArtifactKind::Cfg, &first) else {
            panic!("first colliding key remains addressable");
        };
        let CachedArtifact::Cfg(first_plan) = first_artifact.as_ref() else {
            panic!("first colliding key retains its artifact kind");
        };
        let Some(second_artifact) = cache.lookup(ArtifactKind::Cfg, &second) else {
            panic!("second colliding key remains addressable");
        };
        let CachedArtifact::Cfg(second_plan) = second_artifact.as_ref() else {
            panic!("second colliding key retains its artifact kind");
        };
        assert_eq!(first_plan.exits.as_slice(), [NodeId::new(1)]);
        assert_eq!(second_plan.exits.as_slice(), [NodeId::new(2)]);
        assert!(cache.lookup(ArtifactKind::Cfg, b"schema-three").is_none());
    }

    #[test]
    fn versioned_key_change_is_an_exact_miss() {
        let mut cache = FunctionAnalysisCache::new();
        let mut original = KEY_DOMAIN.to_vec();
        put_u32(&mut original, FUNCTION_ANALYSIS_SCHEMA_VERSION);
        let mut changed = KEY_DOMAIN.to_vec();
        put_u32(
            &mut changed,
            FUNCTION_ANALYSIS_SCHEMA_VERSION.saturating_add(1),
        );
        cache.insert(original.clone(), empty_cfg(0, 0));
        assert!(cache
            .lookup(ArtifactKind::Cfg, &original)
            .is_some_and(|artifact| matches!(artifact.as_ref(), CachedArtifact::Cfg(_))));
        assert!(cache.lookup(ArtifactKind::Cfg, &changed).is_none());
    }

    fn cyclic_function_graph() -> CodePropertyGraph {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let function = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Function {
                signature: Box::new(MethodSignature {
                    name: "f".into(),
                    params: Default::default(),
                    return_type: None,
                    is_static: false,
                    is_async: false,
                    visibility: Visibility::Private,
                }),
            },
            SourceRange::from_bytes(0, 1),
        ));
        let block = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Block {
                scope: crate::ScopeId::GLOBAL,
            },
            SourceRange::from_bytes(0, 1),
        ));
        cpg.connect(function, block, CpgEdgeKind::AstChild);
        cpg.connect(block, function, CpgEdgeKind::AstChild);
        cpg.add_cfg_entry(function);
        cpg
    }

    #[test]
    fn malformed_cyclic_ast_is_bounded_and_never_replays_foreign_ids() {
        let config = CpgBuilderConfig::default().with_dfg(false);
        let extractor = CfgExtractor::new();
        let mut cache = FunctionAnalysisCache::new();
        let mut first = cyclic_function_graph();
        let first_contexts = prepare_builder_contexts(&first, "f", Language::Rust, &config, false);
        extract_cfg_cached(&extractor, &mut first, &config, &first_contexts, &mut cache);
        let first_edges: Vec<_> = first.edges().cloned().collect();

        let mut second = cyclic_function_graph();
        let second_contexts =
            prepare_builder_contexts(&second, "f", Language::Rust, &config, false);
        extract_cfg_cached(
            &extractor,
            &mut second,
            &config,
            &second_contexts,
            &mut cache,
        );
        assert_eq!(first_edges, second.edges().cloned().collect::<Vec<_>>());
        assert_eq!(cache.stats().cfg_hits, 1);
    }

    #[test]
    fn deterministic_lru_eviction_obeys_both_bounds() {
        let mut cache =
            FunctionAnalysisCache::with_limits(FunctionAnalysisCacheLimits::new(2, 16 * 1024));
        for key in [b"a".as_slice(), b"b", b"c"] {
            cache.insert(key.to_vec(), empty_cfg(0, 0));
        }
        let stats = cache.stats();
        assert_eq!(stats.entries, 2);
        assert_eq!(stats.evictions, 1);
        assert!(cache.lookup(ArtifactKind::Cfg, b"a").is_none());
        assert!(cache.lookup(ArtifactKind::Cfg, b"b").is_some());
        assert!(cache.lookup(ArtifactKind::Cfg, b"c").is_some());

        let mut disabled =
            FunctionAnalysisCache::with_limits(FunctionAnalysisCacheLimits::new(0, 0));
        disabled.insert(b"never".to_vec(), empty_cfg(0, 0));
        assert_eq!(disabled.stats().entries, 0);
        assert_eq!(disabled.stats().capacity_bypasses, 1);
    }

    #[test]
    fn local_plan_expansion_rejects_out_of_range_ordinals() {
        let cpg = cyclic_function_graph();
        let function = cpg.functions().next().expect("function exists").id;
        let context = FunctionContext::from_graph(&cpg, function, Some("f"), true)
            .expect("cyclic graph has a bounded canonical context");
        let malformed = CfgPlan {
            function: NodeId::new(u32::MAX),
            edges: vec![(
                NodeId::new(0),
                NodeId::new(u32::MAX),
                CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
            )],
            exits: [NodeId::new(u32::MAX)].into_iter().collect(),
        };
        assert!(!local_cfg_plan_is_valid(&malformed, &context));
    }

    #[test]
    fn pdg_key_is_independent_of_overlay_insertion_order() {
        fn graph(reverse: bool) -> CodePropertyGraph {
            let mut cpg = cyclic_function_graph();
            let mut nodes: Vec<_> = cpg.node_ids().collect();
            nodes.sort_unstable();
            let [function, block] = nodes.as_slice() else {
                panic!("fixture has exactly two nodes");
            };
            let cfg =
                CpgEdge::control_flow(EdgeId::new(20), *function, *block, CfgEdgeKind::Sequential);
            let dfg = CpgEdge::data_flow(EdgeId::new(21), *block, *function, DfgEdgeKind::DefUse);
            let edges = if reverse { [dfg, cfg] } else { [cfg, dfg] };
            for edge in edges {
                cpg.add_edge_with_id(edge)
                    .expect("fixture overlay endpoints exist");
            }
            cpg
        }

        let forward = graph(false);
        let reverse = graph(true);
        let forward_function = forward.functions().next().expect("function exists").id;
        let reverse_function = reverse.functions().next().expect("function exists").id;
        let forward_key = pdg_context(&forward, forward_function)
            .expect("forward-order graph has a PDG context")
            .key(ArtifactKind::Pdg);
        let reverse_key = pdg_context(&reverse, reverse_function)
            .expect("reverse-order graph has a PDG context")
            .key(ArtifactKind::Pdg);
        assert_eq!(forward_key, reverse_key);
    }

    #[cfg(feature = "lang-rust")]
    #[test]
    fn one_pass_source_contexts_equal_independent_graph_walks() {
        use crate::builder::{CpgBuilder, TreeSitterCpgBuilder};

        let source = r#"
fn first(value: i64) -> i64 { value + 1 }
fn outer(seed: i64) -> i64 {
    fn nested(input: i64) -> i64 { input * 2 }
    nested(seed)
}
fn last(value: i64) -> i64 { value - 1 }
"#;
        let config = CpgBuilderConfig::default().with_dfg(false);
        let cpg = TreeSitterCpgBuilder::with_config(config.clone())
            .build(source, Language::Rust)
            .expect("source-context fixture parses");

        let one_pass = prepare_source_builder_contexts(&cpg, source, Language::Rust, &config);
        let independent = prepare_builder_contexts(&cpg, source, Language::Rust, &config, true);
        assert_eq!(one_pass.len(), independent.len());

        for (one_pass, independent) in one_pass.iter().zip(&independent) {
            let one_pass = one_pass
                .as_ref()
                .expect("parser-built function has a one-pass context");
            let independent = independent
                .as_ref()
                .expect("parser-built function has an independent context");
            assert_eq!(one_pass.nodes, independent.nodes);
            assert_eq!(one_pass.common_key, independent.common_key);
            assert_eq!(
                one_pass.has_nested_function,
                independent.has_nested_function
            );
            for &node in &one_pass.nodes {
                assert_eq!(one_pass.local.get(node), independent.local.get(node));
            }
        }
    }
}
