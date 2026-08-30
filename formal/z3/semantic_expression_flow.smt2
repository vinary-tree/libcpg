; Forbidden states for proof/dataflow composition and semantic diff.
(set-logic QF_LIA)

(declare-const admitted_both Bool)
(declare-const pure_both Bool)
(declare-const same_profile Bool)
(declare-const same_context Bool)
(declare-const exact_replay Bool)
(declare-const alpha_replay Bool)
(declare-const alpha_bijection Bool)
(declare-const provenance_complete Bool)
(declare-const saturation_complete Bool)
(declare-const dataflow_complete Bool)
(declare-const within_budget Bool)
(declare-const available_all_paths Bool)
(declare-const dominant_producer Bool)
(declare-const kill_free Bool)
(declare-const busy_all_paths Bool)
(declare-const placement_dominates Bool)
(declare-const multiple_occurrences Bool)
(declare-const evaluator_different Bool)
(declare-const cpg_unchanged Bool)
(declare-const pair_left Int)
(declare-const pair_right Int)

(define-fun exact_proof () Bool
  (and admitted_both pure_both same_profile same_context exact_replay
       provenance_complete))
(define-fun alpha_proof () Bool
  (and admitted_both pure_both same_profile (not same_context) alpha_replay
       alpha_bijection provenance_complete))
(define-fun has_proof () Bool (or exact_proof alpha_proof))
(define-fun evidence_limited () Bool
  (or (not within_budget) (not saturation_complete)
      (not provenance_complete) (not dataflow_complete)))
(define-fun cse_candidate () Bool
  (and exact_proof dataflow_complete available_all_paths dominant_producer
       kill_free))
(define-fun pre_candidate () Bool
  (and exact_proof dataflow_complete multiple_occurrences busy_all_paths
       placement_dominates))

; 0 = equivalent, 1 = different, 2 = limited, 3 = unknown.
(declare-const diff_status Int)
(assert (= diff_status
  (ite has_proof 0
    (ite evaluator_different 1
      (ite evidence_limited 2 3)))))

; A replayed equality proof and an exact unequal closed evaluation are
; contradictory upstream evidence.  The implementation must fail closed.
(assert (not (and has_proof evaluator_different)))
(assert cpg_unchanged)
(assert (=> cse_candidate (< pair_left pair_right)))

(echo "CSE requires an exact replayable proof")
(push)
(assert cse_candidate)
(assert (not exact_proof))
(check-sat)
(pop)

(echo "CSE requires complete dataflow")
(push)
(assert cse_candidate)
(assert (not dataflow_complete))
(check-sat)
(pop)

(echo "CSE requires must availability")
(push)
(assert cse_candidate)
(assert (not available_all_paths))
(check-sat)
(pop)

(echo "CSE requires one dominating producer")
(push)
(assert cse_candidate)
(assert (not dominant_producer))
(check-sat)
(pop)

(echo "CSE rejects an intervening resolved-binding kill")
(push)
(assert cse_candidate)
(assert (not kill_free))
(check-sat)
(pop)

(echo "PRE requires an exact replayable proof")
(push)
(assert pre_candidate)
(assert (not exact_proof))
(check-sat)
(pop)

(echo "PRE requires complete dataflow")
(push)
(assert pre_candidate)
(assert (not dataflow_complete))
(check-sat)
(pop)

(echo "PRE requires multiple occurrences")
(push)
(assert pre_candidate)
(assert (not multiple_occurrences))
(check-sat)
(pop)

(echo "PRE requires must anticipability")
(push)
(assert pre_candidate)
(assert (not busy_all_paths))
(check-sat)
(pop)

(echo "PRE placement must dominate every occurrence")
(push)
(assert pre_candidate)
(assert (not placement_dominates))
(check-sat)
(pop)

(echo "proven equivalent requires a replayable proof")
(push)
(assert (= diff_status 0))
(assert (not has_proof))
(check-sat)
(pop)

(echo "proven different requires an exact evaluator witness")
(push)
(assert (= diff_status 1))
(assert (not evaluator_different))
(check-sat)
(pop)

(echo "proven different cannot coexist with equality proof")
(push)
(assert (= diff_status 1))
(assert has_proof)
(check-sat)
(pop)

(echo "limited cannot hide an equality proof")
(push)
(assert (= diff_status 2))
(assert has_proof)
(check-sat)
(pop)

(echo "limited cannot hide an evaluator difference")
(push)
(assert (= diff_status 2))
(assert evaluator_different)
(check-sat)
(pop)

(echo "limited requires at least one incomplete evidence dimension")
(push)
(assert (= diff_status 2))
(assert (not evidence_limited))
(check-sat)
(pop)

(echo "unknown cannot hide an equality proof")
(push)
(assert (= diff_status 3))
(assert has_proof)
(check-sat)
(pop)

(echo "unknown cannot hide an evaluator difference")
(push)
(assert (= diff_status 3))
(assert evaluator_different)
(check-sat)
(pop)

(echo "unknown requires complete evidence")
(push)
(assert (= diff_status 3))
(assert evidence_limited)
(check-sat)
(pop)

(echo "alpha proof requires distinct binding contexts")
(push)
(assert alpha_proof)
(assert same_context)
(check-sat)
(pop)

(echo "exact proof cannot cross binding contexts")
(push)
(assert exact_proof)
(assert (not same_context))
(check-sat)
(pop)

(echo "proof and unequal evaluation cannot both be accepted")
(push)
(assert has_proof)
(assert evaluator_different)
(check-sat)
(pop)

(echo "a published CSE pair is canonical")
(push)
(assert cse_candidate)
(assert (>= pair_left pair_right))
(check-sat)
(pop)

(echo "semantic analysis cannot mutate the CPG")
(push)
(assert (not cpg_unchanged))
(check-sat)
(pop)
