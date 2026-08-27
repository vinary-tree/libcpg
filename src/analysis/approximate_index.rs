//! Reusable indexed approximate search over typed structural sequences.
//!
//! A [`libdictenstein::BijectiveMap`] assigns every corpus token a reversible
//! `u64` vocabulary identifier. A second
//! [`libdictenstein::dynamic_dawg::DynamicDawgU64`] recognizes sequences of
//! those identifiers and stores the stable libcpg candidate slots associated
//! with each sequence. `liblevenshtein` intersects its edit automaton with that
//! dictionary. No token is concatenated into text and no hash value is used as
//! an identity.

use std::collections::{BTreeMap, BTreeSet, BinaryHeap};
use std::mem::size_of;
use std::sync::Arc;

use libdictenstein::bijective::{BijectiveDictionary, BijectiveMap};
use libdictenstein::dynamic_dawg::DynamicDawgU64;
use liblevenshtein::transducer::{Algorithm, Transducer};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use super::approximate::{
    canonical_candidates, finish_report, provenance, validate_config, ApproximateCancellation,
    ApproximateCompletion, ApproximateConfig, ApproximateConsumer, ApproximateCounters,
    ApproximateError, ApproximateLimit, ApproximateLimits, ApproximateMethod, ApproximateRational,
    ApproximateRecord, ApproximateReport, ApproximateTape, CanonicalCandidate, PendingEvidence,
};

/// Resource census for construction of a reusable structural-sequence index.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ApproximateTokenIndexCounters {
    /// Records supplied before any build limit was applied.
    pub supplied_candidates: usize,
    /// Records admitted to the sequence dictionary.
    pub indexed_candidates: usize,
    /// Distinct token sequences stored in the dictionary.
    pub indexed_sequences: usize,
    /// Distinct reversible vocabulary terms.
    pub vocabulary_terms: usize,
    /// Token units retained across indexed candidates, including duplicates.
    pub retained_units: usize,
    /// Nodes in the resulting `u64` directed acyclic word graph (DAWG).
    pub dictionary_nodes: usize,
    /// Proven upper bound on edges in the resulting DAWG.
    pub dictionary_edge_upper_bound: usize,
    /// Work charged to stable ordering, token census, encoding, and insertion.
    pub work_units: u64,
    /// Conservative peak-memory estimate used for admission.
    pub peak_memory_bytes: u64,
}

/// Immutable reusable index for token-structured approximate candidates.
///
/// The live dictionary and transducer state are intentionally not serializable.
/// Evidence emitted by [`search`](Self::search) remains canonically
/// serializable through [`ApproximateReport`]. Clone is constant depth because
/// the libdictenstein backends share immutable revisions through `Arc` owners.
#[derive(Clone, Debug)]
pub struct ApproximateTokenIndex {
    records: Vec<ApproximateRecord>,
    vocabulary: BijectiveMap<u64>,
    sequences: DynamicDawgU64<Vec<u64>>,
    completion: ApproximateCompletion,
    counters: ApproximateTokenIndexCounters,
    build_limits: ApproximateLimits,
    max_sequence_units: usize,
    max_sequence_multiplicity: usize,
    distinct_sequence_units: usize,
}

impl ApproximateTokenIndex {
    /// Builds a deterministic, bounded vocabulary and sequence dictionary.
    ///
    /// Candidate identifiers are sorted before admission. Equal token
    /// sequences share one DAWG term whose value is the ordered set of record
    /// slots, so duplicate structures never overwrite one another. A build cap
    /// yields an incomplete index; subsequent searches preserve that state and
    /// therefore cannot report a complete no-match.
    pub fn build(
        candidates: &[ApproximateRecord],
        limits: &ApproximateLimits,
    ) -> Result<Self, ApproximateError> {
        validate_config(&ApproximateConfig {
            limits: limits.clone(),
            ..ApproximateConfig::default()
        })?;

        let supplied_candidates = candidates.len();
        let ordering_bytes = checked_bytes(
            supplied_candidates,
            size_of::<CanonicalCandidate<'_>>(),
            "indexed candidate ordering",
        )?;
        if ordering_bytes > limits.max_memory_bytes {
            return Ok(Self::empty(
                limits,
                supplied_candidates,
                BTreeSet::from([ApproximateLimit::Memory]),
                ordering_bytes,
            ));
        }
        let ordering_work = heap_ordering_work(supplied_candidates)?;
        if ordering_work > limits.max_work_units {
            return Ok(Self::empty(
                limits,
                supplied_candidates,
                BTreeSet::from([ApproximateLimit::TotalWork]),
                ordering_bytes,
            ));
        }

        let ordered = canonical_candidates(candidates);
        for duplicate in ordered.windows(2) {
            if duplicate[0].record.id == duplicate[1].record.id {
                return Err(ApproximateError::DuplicateCandidate(duplicate[0].record.id));
            }
        }
        for item in &ordered {
            let record = item.record;
            if !matches!(record.tape.as_ref(), ApproximateTape::Tokens(_)) {
                return Err(ApproximateError::IndexedCandidateRequiresTokens {
                    candidate: record.id,
                });
            }
        }

        let mut limitations = BTreeSet::new();
        if ordered.len() > limits.max_candidates {
            limitations.insert(ApproximateLimit::CandidateCount);
        }
        let mut admitted = Vec::with_capacity(ordered.len().min(limits.max_candidates));
        let mut retained_units = 0usize;
        let mut token_text_bytes = 0usize;
        let mut work_units = ordering_work;
        let mut peak_memory_bytes = ordering_bytes;
        for item in ordered.into_iter().take(limits.max_candidates) {
            let record = item.record;
            let ApproximateTape::Tokens(tokens) = record.tape.as_ref() else {
                unreachable!("all indexed records were validated as token tapes");
            };
            if tokens.len() > limits.max_input_units {
                limitations.insert(ApproximateLimit::InputUnits);
                continue;
            }
            let record_text_bytes = tokens.iter().try_fold(0usize, |bytes, token| {
                bytes
                    .checked_add(token.len())
                    .ok_or(ApproximateError::IntegerDomain("indexed token bytes"))
            })?;
            // Charge record visitation, token census plus sequence insertion,
            // and two UTF-8 vocabulary passes (ordering/build and lookup).
            let record_work = u64::try_from(tokens.len())
                .ok()
                .and_then(|units| units.checked_mul(2))
                .and_then(|work| {
                    u64::try_from(record_text_bytes)
                        .ok()
                        .and_then(|bytes| bytes.checked_mul(2))
                        .and_then(|bytes| work.checked_add(bytes))
                })
                .and_then(|units| units.checked_add(1))
                .ok_or(ApproximateError::IntegerDomain("indexed build work"))?;
            let Some(next_work) = work_units.checked_add(record_work) else {
                limitations.insert(ApproximateLimit::TotalWork);
                break;
            };
            if next_work > limits.max_work_units {
                limitations.insert(ApproximateLimit::TotalWork);
                break;
            }
            let next_units = retained_units
                .checked_add(tokens.len())
                .ok_or(ApproximateError::IntegerDomain("indexed token units"))?;
            let next_text_bytes = token_text_bytes
                .checked_add(record_text_bytes)
                .ok_or(ApproximateError::IntegerDomain("indexed token bytes"))?;
            let graph_item_bound = next_units
                .checked_mul(2)
                .and_then(|items| items.checked_add(1))
                .ok_or(ApproximateError::IntegerDomain("indexed DAWG graph bound"))?;
            let estimate = estimated_index_bytes(
                ordering_bytes,
                admitted.len() + 1,
                next_units,
                next_units,
                next_text_bytes,
                graph_item_bound,
            )?;
            if estimate > limits.max_memory_bytes {
                limitations.insert(ApproximateLimit::Memory);
                break;
            }
            work_units = next_work;
            retained_units = next_units;
            token_text_bytes = next_text_bytes;
            peak_memory_bytes = peak_memory_bytes.max(estimate);
            admitted.push(record.clone());
        }

        let mut vocabulary_terms = admitted
            .iter()
            .flat_map(|record| match record.tape.as_ref() {
                ApproximateTape::Tokens(tokens) => tokens.iter().map(AsRef::as_ref),
                ApproximateTape::Text(_) => unreachable!("index admits only token tapes"),
            })
            .collect::<Vec<_>>();
        vocabulary_terms = BinaryHeap::from(vocabulary_terms).into_sorted_vec();
        vocabulary_terms.dedup();
        if vocabulary_terms.len() > limits.max_unique_tokens {
            limitations.insert(ApproximateLimit::UniqueTokens);
            return Ok(Self::empty(
                limits,
                supplied_candidates,
                limitations,
                peak_memory_bytes,
            ));
        }
        let vocabulary_text_bytes = vocabulary_terms.iter().try_fold(0usize, |bytes, term| {
            bytes
                .checked_add(term.len())
                .ok_or(ApproximateError::IntegerDomain("vocabulary text bytes"))
        })?;
        let estimate = estimated_index_bytes(
            ordering_bytes,
            admitted.len(),
            retained_units,
            vocabulary_terms.len(),
            vocabulary_text_bytes,
            retained_units
                .checked_mul(2)
                .and_then(|items| items.checked_add(1))
                .ok_or(ApproximateError::IntegerDomain("indexed DAWG graph bound"))?,
        )?;
        if estimate > limits.max_memory_bytes {
            limitations.insert(ApproximateLimit::Memory);
            return Ok(Self::empty(
                limits,
                supplied_candidates,
                limitations,
                estimate,
            ));
        }

        let vocabulary = BijectiveMap::try_from_iter(
            vocabulary_terms
                .iter()
                .enumerate()
                .map(|(index, term)| {
                    u64::try_from(index)
                        .ok()
                        .and_then(|index| index.checked_add(1))
                        .map(|identifier| (*term, identifier))
                        .ok_or(ApproximateError::IntegerDomain("vocabulary identifier"))
                })
                .collect::<Result<Vec<_>, _>>()?,
        )
        .map_err(|_| ApproximateError::IntegerDomain("vocabulary bijection"))?;

        let mut grouped = BTreeMap::<Vec<u64>, Vec<u64>>::new();
        let mut max_sequence_units = 0usize;
        for (slot, record) in admitted.iter().enumerate() {
            let ApproximateTape::Tokens(tokens) = record.tape.as_ref() else {
                unreachable!("index admits only token tapes");
            };
            let mut encoded = Vec::with_capacity(tokens.len());
            for token in tokens {
                encoded.push(
                    vocabulary
                        .get_value(token.as_ref())
                        .ok_or(ApproximateError::IntegerDomain("vocabulary forward lookup"))?,
                );
            }
            max_sequence_units = max_sequence_units.max(encoded.len());
            grouped.entry(encoded).or_default().push(
                u64::try_from(slot)
                    .map_err(|_| ApproximateError::IntegerDomain("indexed candidate slot"))?,
            );
        }
        let distinct_sequence_units = grouped.keys().try_fold(0usize, |total, sequence| {
            total
                .checked_add(sequence.len())
                .ok_or(ApproximateError::IntegerDomain(
                    "distinct indexed sequence units",
                ))
        })?;
        let max_sequence_multiplicity = grouped.values().map(Vec::len).max().unwrap_or(0);
        let sequences = DynamicDawgU64::<Vec<u64>>::new();
        for (sequence, slots) in grouped {
            if !sequences.insert_sequence_with_value(&sequence, slots) {
                return Err(ApproximateError::IntegerDomain(
                    "unique indexed sequence insertion",
                ));
            }
        }
        let dictionary_nodes = sequences.node_count();
        // Prefix sharing and DAWG minimization can only reduce the edge count
        // below the sum of distinct sequence lengths, so this is a safe public
        // upper bound without traversing the freshly built index again.
        let dictionary_edge_upper_bound = distinct_sequence_units;
        let graph_items = dictionary_nodes
            .checked_add(dictionary_edge_upper_bound)
            .ok_or(ApproximateError::IntegerDomain("indexed dictionary graph"))?;
        let final_estimate = estimated_index_bytes(
            ordering_bytes,
            admitted.len(),
            retained_units,
            vocabulary_terms.len(),
            vocabulary_text_bytes,
            graph_items,
        )?;
        peak_memory_bytes = peak_memory_bytes.max(final_estimate);
        if final_estimate > limits.max_memory_bytes {
            limitations.insert(ApproximateLimit::Memory);
            return Ok(Self::empty(
                limits,
                supplied_candidates,
                limitations,
                final_estimate,
            ));
        }

        Ok(Self {
            counters: ApproximateTokenIndexCounters {
                supplied_candidates,
                indexed_candidates: admitted.len(),
                indexed_sequences: sequences.term_count(),
                vocabulary_terms: vocabulary_terms.len(),
                retained_units,
                dictionary_nodes,
                dictionary_edge_upper_bound,
                work_units,
                peak_memory_bytes,
            },
            records: admitted,
            vocabulary,
            sequences,
            completion: completion(limitations),
            build_limits: limits.clone(),
            max_sequence_units,
            max_sequence_multiplicity,
            distinct_sequence_units,
        })
    }

    /// Returns the index-build completion state retained by every search.
    pub fn completion(&self) -> &ApproximateCompletion {
        &self.completion
    }

    /// Returns the immutable build resource census.
    pub const fn counters(&self) -> ApproximateTokenIndexCounters {
        self.counters
    }

    /// Returns the stable candidate records admitted to the index.
    pub fn records(&self) -> &[ApproximateRecord] {
        &self.records
    }

    /// Looks up a corpus vocabulary term without allocating a new identifier.
    pub fn token_id(&self, token: &str) -> Option<u64> {
        self.vocabulary.get_value(token)
    }

    /// Reverse-decodes one corpus vocabulary identifier.
    pub fn token(&self, identifier: u64) -> Option<String> {
        BijectiveDictionary::get_term(&self.vocabulary, &identifier).map(|term| term.into_owned())
    }

    /// Searches the reusable `u64` sequence dictionary through liblevenshtein.
    ///
    /// Traversal is admitted only after conservative whole-index work, state,
    /// and memory preflight. Cooperative cancellation is checked before entry
    /// and between yielded terms; a currently executing bounded iterator step
    /// finishes before cancellation is observed.
    pub fn search(
        &self,
        query: &ApproximateRecord,
        consumer: ApproximateConsumer,
        config: &ApproximateConfig,
        cancellation: &ApproximateCancellation,
    ) -> Result<ApproximateReport, ApproximateError> {
        validate_config(config)?;
        if config.method == ApproximateMethod::DamerauLevenshtein
            && config.max_distance > Algorithm::MAX_DAMERAU_DISTANCE
        {
            return Err(ApproximateError::MethodDistanceBound {
                method: config.method,
                requested: config.max_distance,
                maximum: Algorithm::MAX_DAMERAU_DISTANCE,
            });
        }
        let ApproximateTape::Tokens(query_tokens) = query.tape.as_ref() else {
            return Err(ApproximateError::IndexedQueryRequiresTokens);
        };

        let mut limitations = match &self.completion {
            ApproximateCompletion::Complete => BTreeSet::new(),
            ApproximateCompletion::Incomplete(limits) => limits.iter().copied().collect(),
        };
        let mut report_provenance = provenance(query, self.records.iter(), config);
        report_provenance.method_identity = Arc::from(format!(
            "{}.libdictenstein-u64-index.v1",
            report_provenance.method_identity
        ));
        // The build policy is part of replay identity even when it admitted the
        // complete corpus.
        report_provenance.config_digest = combine_digest(
            report_provenance.config_digest,
            stable_limits_digest(&self.build_limits),
        );

        let mut counters = ApproximateCounters {
            candidate_count: self.counters.supplied_candidates,
            admitted_candidates: self.records.len(),
            index_build_work_units: self.counters.work_units,
            index_build_peak_memory_bytes: self.counters.peak_memory_bytes,
            peak_memory_bytes: self.counters.peak_memory_bytes,
            ..ApproximateCounters::default()
        };
        let mut fatal = false;
        if self.records.len() > config.limits.max_candidates {
            limitations.insert(ApproximateLimit::CandidateCount);
            fatal = true;
        }
        if query_tokens.len() > config.limits.max_input_units
            || self.max_sequence_units > config.limits.max_input_units
        {
            limitations.insert(ApproximateLimit::InputUnits);
            fatal = true;
        }
        let query_workspace_floor = checked_bytes(
            query_tokens.len(),
            size_of::<&str>() + size_of::<u64>() * 2,
            "indexed query workspace",
        )?;
        if self
            .counters
            .peak_memory_bytes
            .checked_add(query_workspace_floor)
            .is_none_or(|bytes| bytes > config.limits.max_memory_bytes)
        {
            limitations.insert(ApproximateLimit::Memory);
            fatal = true;
        }
        if fatal {
            return Ok(finish_report(
                Vec::new(),
                limitations,
                report_provenance,
                counters,
                consumer,
                config,
            ));
        }

        let encoded_query = self.encode_query(query_tokens)?;
        if self
            .counters
            .vocabulary_terms
            .checked_add(encoded_query.unique_unknown)
            .is_none_or(|terms| terms > config.limits.max_unique_tokens)
        {
            limitations.insert(ApproximateLimit::UniqueTokens);
            fatal = true;
        }
        let query_span = encoded_query
            .units
            .len()
            .checked_add(1)
            .ok_or(ApproximateError::IntegerDomain("indexed query span"))?;
        let max_candidate_span = self
            .max_sequence_units
            .checked_add(1)
            .ok_or(ApproximateError::IntegerDomain("indexed candidate span"))?;
        let pair_cells = checked_product(query_span, max_candidate_span, "indexed pair work")?;
        counters.peak_state_count = pair_cells;
        if pair_cells > config.limits.max_pair_cells {
            limitations.insert(ApproximateLimit::PairWork);
            fatal = true;
        }

        let sequence_spans = self
            .distinct_sequence_units
            .checked_add(self.counters.indexed_sequences)
            .ok_or(ApproximateError::IntegerDomain("indexed sequence spans"))?;
        let traversal_work = checked_product(query_span, sequence_spans, "indexed traversal work")?;
        let work_units = traversal_work
            .checked_add(encoded_query.work_units)
            .ok_or(ApproximateError::IntegerDomain("indexed query work"))?;
        counters.work_units = work_units;
        let combined_work = self
            .counters
            .work_units
            .checked_add(work_units)
            .ok_or(ApproximateError::IntegerDomain("indexed total work"))?;
        if combined_work > config.limits.max_work_units {
            limitations.insert(ApproximateLimit::TotalWork);
            fatal = true;
        }
        let band = config
            .max_distance
            .checked_mul(2)
            .and_then(|width| width.checked_add(3))
            .ok_or(ApproximateError::IntegerDomain("indexed automaton band"))?;
        let state_count = checked_product(
            self.counters.dictionary_nodes,
            band,
            "indexed automaton states",
        )?;
        counters.peak_state_count = counters.peak_state_count.max(state_count);
        if state_count > config.limits.max_state_count {
            limitations.insert(ApproximateLimit::StateCount);
            fatal = true;
        }
        let query_memory = checked_bytes(
            encoded_query.units.len(),
            size_of::<u64>() * 2,
            "encoded indexed query",
        )?
        .checked_add(encoded_query.temporary_memory_bytes)
        .ok_or(ApproximateError::IntegerDomain("indexed query memory"))?;
        let traversal_memory = checked_bytes(
            usize::try_from(state_count)
                .map_err(|_| ApproximateError::IntegerDomain("indexed automaton states"))?,
            size_of::<usize>() * 4,
            "indexed traversal frontier",
        )?;
        let retained_memory = checked_bytes(
            config.top_k.min(self.records.len()),
            size_of::<PendingEvidence>(),
            "indexed retained evidence",
        )?;
        let yielded_memory = checked_bytes(
            self.max_sequence_units,
            size_of::<u64>(),
            "indexed yielded sequence",
        )?
        .checked_add(checked_bytes(
            self.max_sequence_multiplicity,
            size_of::<u64>(),
            "indexed yielded candidate slots",
        )?)
        .ok_or(ApproximateError::IntegerDomain(
            "indexed yielded match memory",
        ))?;
        let resident_memory = self
            .counters
            .peak_memory_bytes
            .checked_add(query_memory)
            .and_then(|bytes| bytes.checked_add(traversal_memory))
            .and_then(|bytes| bytes.checked_add(retained_memory))
            .and_then(|bytes| bytes.checked_add(yielded_memory))
            .ok_or(ApproximateError::IntegerDomain("indexed resident memory"))?;
        counters.peak_memory_bytes = resident_memory;
        if resident_memory > config.limits.max_memory_bytes {
            limitations.insert(ApproximateLimit::Memory);
            fatal = true;
        }
        if cancellation.is_cancelled() {
            limitations.insert(ApproximateLimit::Cancelled);
            fatal = true;
        }
        if fatal {
            return Ok(finish_report(
                Vec::new(),
                limitations,
                report_provenance,
                counters,
                consumer,
                config,
            ));
        }

        let transducer = Transducer::new(self.sequences.clone(), algorithm(config.method));
        let mut retained = BinaryHeap::with_capacity(config.top_k.min(self.records.len()));
        let mut cancelled = false;
        for (matched_sequence, distance, slots) in
            transducer.query_units_values(&encoded_query.units, config.max_distance)
        {
            if cancellation.is_cancelled() {
                limitations.insert(ApproximateLimit::Cancelled);
                cancelled = true;
                break;
            }
            counters.matched_candidates =
                counters.matched_candidates.checked_add(slots.len()).ok_or(
                    ApproximateError::IntegerDomain("indexed matched candidates"),
                )?;
            let raw = u64::try_from(distance)
                .map_err(|_| ApproximateError::IntegerDomain("indexed edit cost"))?;
            let normalization =
                u64::try_from(encoded_query.units.len().max(matched_sequence.len()).max(1))
                    .map_err(|_| ApproximateError::IntegerDomain("indexed normalization length"))?;
            for slot in slots {
                if cancellation.is_cancelled() {
                    limitations.insert(ApproximateLimit::Cancelled);
                    cancelled = true;
                    break;
                }
                let record =
                    self.records
                        .get(usize::try_from(slot).map_err(|_| {
                            ApproximateError::IntegerDomain("indexed candidate slot")
                        })?)
                        .ok_or(ApproximateError::IntegerDomain("indexed candidate slot"))?;
                retained.push(PendingEvidence {
                    candidate: record.id,
                    candidate_tape: Arc::clone(&record.tape),
                    raw_cost: ApproximateRational::new(raw, 1),
                    normalized_cost: ApproximateRational::new(raw, normalization),
                });
                if retained.len() > config.top_k {
                    retained.pop();
                }
            }
            if cancelled {
                break;
            }
        }
        if !cancelled {
            counters.examined_candidates = self.records.len();
        }
        Ok(finish_report(
            retained.into_sorted_vec(),
            limitations,
            report_provenance,
            counters,
            consumer,
            config,
        ))
    }

    fn encode_query(&self, tokens: &[Arc<str>]) -> Result<EncodedQuery, ApproximateError> {
        let mut unknown = tokens
            .iter()
            .filter(|token| self.vocabulary.get_value(token.as_ref()).is_none())
            .map(AsRef::as_ref)
            .collect::<Vec<_>>();
        unknown = BinaryHeap::from(unknown).into_sorted_vec();
        unknown.dedup();
        let query_text_bytes = tokens.iter().try_fold(0usize, |bytes, token| {
            bytes
                .checked_add(token.len())
                .ok_or(ApproximateError::IntegerDomain("query token bytes"))
        })?;
        let lexical_passes = u64::from(usize::BITS - tokens.len().max(1).leading_zeros())
            .checked_add(2)
            .ok_or(ApproximateError::IntegerDomain("query vocabulary work"))?;
        let work_units = u64::try_from(query_text_bytes)
            .ok()
            .and_then(|bytes| bytes.checked_mul(lexical_passes))
            .and_then(|work| {
                u64::try_from(tokens.len())
                    .ok()
                    .and_then(|units| units.checked_mul(3))
                    .and_then(|units| work.checked_add(units))
            })
            .ok_or(ApproximateError::IntegerDomain("query vocabulary work"))?;
        let temporary_memory_bytes = checked_bytes(
            tokens.len(),
            size_of::<&str>(),
            "query unknown-token ordering",
        )?;
        let first_unknown = u64::try_from(self.counters.vocabulary_terms)
            .ok()
            .and_then(|identifier| identifier.checked_add(1))
            .ok_or(ApproximateError::IntegerDomain(
                "query vocabulary identifier",
            ))?;
        let mut encoded = Vec::with_capacity(tokens.len());
        for token in tokens {
            encoded.push(
                self.vocabulary
                    .get_value(token.as_ref())
                    .or_else(|| {
                        unknown
                            .binary_search_by(|candidate| candidate.cmp(&token.as_ref()))
                            .ok()
                            .and_then(|offset| u64::try_from(offset).ok())
                            .and_then(|offset| first_unknown.checked_add(offset))
                    })
                    .ok_or(ApproximateError::IntegerDomain("query vocabulary lookup"))?,
            );
        }
        Ok(EncodedQuery {
            units: encoded,
            unique_unknown: unknown.len(),
            work_units,
            temporary_memory_bytes,
        })
    }

    fn empty(
        limits: &ApproximateLimits,
        supplied_candidates: usize,
        limitations: BTreeSet<ApproximateLimit>,
        peak_memory_bytes: u64,
    ) -> Self {
        Self {
            records: Vec::new(),
            vocabulary: BijectiveMap::new(),
            sequences: DynamicDawgU64::new(),
            completion: completion(limitations),
            counters: ApproximateTokenIndexCounters {
                supplied_candidates,
                peak_memory_bytes,
                ..ApproximateTokenIndexCounters::default()
            },
            build_limits: limits.clone(),
            max_sequence_units: 0,
            max_sequence_multiplicity: 0,
            distinct_sequence_units: 0,
        }
    }
}

struct EncodedQuery {
    units: Vec<u64>,
    unique_unknown: usize,
    work_units: u64,
    temporary_memory_bytes: u64,
}

fn algorithm(method: ApproximateMethod) -> Algorithm {
    match method {
        ApproximateMethod::StandardLevenshtein => Algorithm::Standard,
        ApproximateMethod::OptimalStringAlignment => Algorithm::Transposition,
        ApproximateMethod::DamerauLevenshtein => Algorithm::DamerauLevenshtein,
        ApproximateMethod::GeneralizedMergeSplit => Algorithm::MergeAndSplit,
    }
}

fn completion(limitations: BTreeSet<ApproximateLimit>) -> ApproximateCompletion {
    if limitations.is_empty() {
        ApproximateCompletion::Complete
    } else {
        ApproximateCompletion::Incomplete(limitations.into_iter().collect())
    }
}

fn checked_product(
    left: usize,
    right: usize,
    domain: &'static str,
) -> Result<u64, ApproximateError> {
    left.checked_mul(right)
        .and_then(|value| u64::try_from(value).ok())
        .ok_or(ApproximateError::IntegerDomain(domain))
}

fn checked_bytes(
    count: usize,
    element_size: usize,
    domain: &'static str,
) -> Result<u64, ApproximateError> {
    checked_product(count, element_size, domain)
}

fn estimated_index_bytes(
    ordering_bytes: u64,
    records: usize,
    retained_units: usize,
    vocabulary_terms: usize,
    vocabulary_text_bytes: usize,
    graph_items: usize,
) -> Result<u64, ApproximateError> {
    // Each logical DAWG node/edge is charged at a deliberately conservative
    // owner budget that covers Arc publication, an edge snapshot, stored value
    // ownership, and allocator metadata. External cgroup verification remains
    // the hard process-level bound.
    const GRAPH_ITEM_OWNER_BYTES: usize = 512;
    const VOCABULARY_TERM_OWNER_BYTES: usize = 256;
    let record_bytes = checked_bytes(records, size_of::<ApproximateRecord>(), "indexed records")?;
    // During construction the borrowed vocabulary census, encoded sequence
    // keys, and DAWG publication can coexist. Account each unit as one fat
    // string reference plus two u64 representations, and every candidate as
    // one stored slot.
    let sequence_bytes = checked_bytes(
        retained_units,
        size_of::<&str>() + size_of::<u64>() * 2,
        "indexed sequence construction",
    )?;
    let slot_bytes = checked_bytes(records, size_of::<u64>(), "indexed candidate slots")?;
    let graph_owners = graph_items
        .checked_add(1)
        .ok_or(ApproximateError::IntegerDomain("indexed DAWG owners"))?;
    let graph_bytes = checked_bytes(graph_owners, GRAPH_ITEM_OWNER_BYTES, "indexed DAWG owners")?;
    let vocabulary_owner_bytes = checked_bytes(
        vocabulary_terms,
        VOCABULARY_TERM_OWNER_BYTES,
        "indexed vocabulary owners",
    )?;
    ordering_bytes
        .checked_add(record_bytes)
        .and_then(|bytes| bytes.checked_add(sequence_bytes))
        .and_then(|bytes| bytes.checked_add(slot_bytes))
        .and_then(|bytes| bytes.checked_add(graph_bytes))
        .and_then(|bytes| bytes.checked_add(vocabulary_owner_bytes))
        .and_then(|bytes| bytes.checked_add(u64::try_from(vocabulary_text_bytes).ok()?))
        .ok_or(ApproximateError::IntegerDomain("indexed resident memory"))
}

fn heap_ordering_work(candidate_count: usize) -> Result<u64, ApproximateError> {
    if candidate_count < 2 {
        return u64::try_from(candidate_count)
            .map_err(|_| ApproximateError::IntegerDomain("indexed candidate ordering work"));
    }
    let levels = usize::BITS - candidate_count.leading_zeros();
    let comparison_bound = u64::from(levels)
        .checked_mul(2)
        .and_then(|levels| levels.checked_add(1))
        .ok_or(ApproximateError::IntegerDomain(
            "indexed candidate ordering work",
        ))?;
    u64::try_from(candidate_count)
        .ok()
        .and_then(|count| count.checked_mul(comparison_bound))
        .ok_or(ApproximateError::IntegerDomain(
            "indexed candidate ordering work",
        ))
}

fn stable_limits_digest(limits: &ApproximateLimits) -> u64 {
    // FNV-1a over an explicit field order avoids process-random hash state and
    // keeps build-limit identity stable across serde/debug formatting changes.
    let mut digest = 0xcbf2_9ce4_8422_2325u64;
    for bytes in [
        limits.max_candidates.to_le_bytes().as_slice(),
        limits.max_input_units.to_le_bytes().as_slice(),
        limits.max_pair_cells.to_le_bytes().as_slice(),
        limits.max_work_units.to_le_bytes().as_slice(),
        limits.max_state_count.to_le_bytes().as_slice(),
        limits.max_memory_bytes.to_le_bytes().as_slice(),
        limits.max_unique_tokens.to_le_bytes().as_slice(),
    ] {
        for &byte in bytes {
            digest ^= u64::from(byte);
            digest = digest.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    digest
}

fn combine_digest(left: u64, right: u64) -> u64 {
    let mut digest = left;
    for byte in right.to_le_bytes() {
        digest ^= u64::from(byte);
        digest = digest.wrapping_mul(0x0000_0100_0000_01b3);
    }
    digest
}
