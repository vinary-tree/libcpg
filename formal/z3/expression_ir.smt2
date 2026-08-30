; Boolean abstraction of the public rewrite-certificate boundary.
(set-logic QF_UF)

(declare-const purity_effects_known Bool)
(declare-const evaluation_order_known Bool)
(declare-const integer_semantics_known Bool)
(declare-const floating_point_semantics_known Bool)
(declare-const exception_semantics_known Bool)
(declare-const short_circuit_semantics_known Bool)
(declare-const alias_memory_semantics_known Bool)
(declare-const call_semantics_known Bool)
(declare-const volatile_atomic_semantics_known Bool)
(declare-const binding_scope_semantics_known Bool)
(declare-const law_allowed Bool)
(declare-const lowering_complete Bool)
(declare-const pure_subtree Bool)

(define-fun profile_complete () Bool
  (and purity_effects_known
       evaluation_order_known
       integer_semantics_known
       floating_point_semantics_known
       exception_semantics_known
       short_circuit_semantics_known
       alias_memory_semantics_known
       call_semantics_known
       volatile_atomic_semantics_known
       binding_scope_semantics_known))

(define-fun eligible () Bool
  (and lowering_complete profile_complete pure_subtree law_allowed))

(echo "eligible requires complete lowering")
(push)
(assert eligible)
(assert (not lowering_complete))
(check-sat)
(pop)

(echo "eligible requires a complete profile")
(push)
(assert eligible)
(assert (not profile_complete))
(check-sat)
(pop)

(echo "eligible requires a pure subtree")
(push)
(assert eligible)
(assert (not pure_subtree))
(check-sat)
(pop)

(echo "eligible requires the named law")
(push)
(assert eligible)
(assert (not law_allowed))
(check-sat)
(pop)

(echo "any missing semantic dimension denies eligibility")
(push)
(assert lowering_complete)
(assert pure_subtree)
(assert law_allowed)
(assert (or (not purity_effects_known)
            (not evaluation_order_known)
            (not integer_semantics_known)
            (not floating_point_semantics_known)
            (not exception_semantics_known)
            (not short_circuit_semantics_known)
            (not alias_memory_semantics_known)
            (not call_semantics_known)
            (not volatile_atomic_semantics_known)
            (not binding_scope_semantics_known)))
(assert eligible)
(check-sat)
(pop)
