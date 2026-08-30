; Forbidden states for the libcpg expression-equivalence reporting boundary.
(set-logic QF_LIA)

(declare-const left_admitted Bool)
(declare-const right_admitted Bool)
(declare-const same_scope Bool)
(declare-const same_profile Bool)
(declare-const same_binding_context Bool)
(declare-const alpha_bijection_witness Bool)
(declare-const equality_reached Bool)
(declare-const proof_replayable Bool)
(declare-const provenance_complete Bool)
(declare-const saturated Bool)
(declare-const reported_equivalent Bool)
(declare-const reported_alpha_clone Bool)
(declare-const reported_not_equivalent Bool)
(declare-const unsupported Bool)
(declare-const cpg_unchanged Bool)
(declare-const legacy_clone_unchanged Bool)

(define-fun boundary_admitted () Bool
  (and left_admitted right_admitted same_scope same_profile))

(define-fun pair_supported () Bool
  (and boundary_admitted (or same_binding_context alpha_bijection_witness)))

(assert (= unsupported (not pair_supported)))
(assert (= reported_equivalent
  (and boundary_admitted same_binding_context equality_reached proof_replayable
       provenance_complete)))
(assert (= reported_alpha_clone
  (and boundary_admitted (not same_binding_context) alpha_bijection_witness
       equality_reached proof_replayable provenance_complete)))
(assert (= reported_not_equivalent
  (and pair_supported (not equality_reached) saturated provenance_complete)))

(echo "positive evidence cannot include an inadmissible endpoint")
(push)
(assert reported_equivalent)
(assert (or (not left_admitted) (not right_admitted)))
(check-sat)
(pop)

(echo "positive evidence cannot cross scope")
(push)
(assert reported_equivalent)
(assert (not same_scope))
(check-sat)
(pop)

(echo "positive evidence cannot cross semantic profile")
(push)
(assert reported_equivalent)
(assert (not same_profile))
(check-sat)
(pop)

(echo "positive evidence requires reached equality")
(push)
(assert reported_equivalent)
(assert (not equality_reached))
(check-sat)
(pop)

(echo "positive evidence requires an independently replayable proof")
(push)
(assert reported_equivalent)
(assert (not proof_replayable))
(check-sat)
(pop)

(echo "positive evidence requires complete provenance")
(push)
(assert reported_equivalent)
(assert (not provenance_complete))
(check-sat)
(pop)

(echo "exact semantic equality cannot cross binding contexts")
(push)
(assert reported_equivalent)
(assert (not same_binding_context))
(check-sat)
(pop)

(echo "alpha-clone evidence requires distinct binding contexts")
(push)
(assert reported_alpha_clone)
(assert same_binding_context)
(check-sat)
(pop)

(echo "alpha-clone evidence requires a bijective-renaming witness")
(push)
(assert reported_alpha_clone)
(assert (not alpha_bijection_witness))
(check-sat)
(pop)

(echo "clean absence requires complete saturation")
(push)
(assert reported_not_equivalent)
(assert (not saturated))
(check-sat)
(pop)

(echo "clean absence cannot be returned for unsupported inputs")
(push)
(assert reported_not_equivalent)
(assert unsupported)
(check-sat)
(pop)

(declare-const used_work Int)
(declare-const step_work Int)
(declare-const max_work Int)
(declare-const commit_work Bool)
(assert (= commit_work
  (and (<= 0 used_work) (<= 0 step_work) (<= (+ used_work step_work) max_work))))

(echo "an accepted work transaction cannot exceed its cap")
(push)
(assert commit_work)
(assert (> (+ used_work step_work) max_work))
(check-sat)
(pop)

(declare-const used_nodes Int)
(declare-const added_nodes Int)
(declare-const max_nodes Int)
(declare-const commit_nodes Bool)
(assert (= commit_nodes
  (and (<= 0 used_nodes) (<= 0 added_nodes) (<= (+ used_nodes added_nodes) max_nodes))))

(echo "an accepted node transaction cannot exceed its cap")
(push)
(assert commit_nodes)
(assert (> (+ used_nodes added_nodes) max_nodes))
(check-sat)
(pop)

(declare-const pair_left Int)
(declare-const pair_right Int)
(declare-const emitted_pair Bool)
(assert (= emitted_pair (< pair_left pair_right)))

(echo "a public pair cannot be reflexive or reversed")
(push)
(assert emitted_pair)
(assert (>= pair_left pair_right))
(check-sat)
(pop)

(echo "the analysis cannot mutate the CPG or legacy clone report")
(push)
(assert cpg_unchanged)
(assert legacy_clone_unchanged)
(assert (or (not cpg_unchanged) (not legacy_clone_unchanged)))
(check-sat)
(pop)
