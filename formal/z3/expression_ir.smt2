; Boolean abstraction of the public rewrite-certificate boundary.
(set-logic QF_UFBVLIA)

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
(declare-const live_lowering Bool)

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
  (and lowering_complete profile_complete pure_subtree law_allowed live_lowering))

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

(echo "eligible requires live lowering evidence")
(push)
(assert eligible)
(assert (not live_lowering))
(check-sat)
(pop)

; Typed integer descriptors are well formed before they can support a law.
(declare-const integer_fixed Bool)
(declare-const integer_width Int)
(declare-const integer_arbitrary_precision Bool)

(define-fun integer_type_valid () Bool
  (or (and integer_fixed
           (<= 1 integer_width)
           (<= integer_width 128)
           (not integer_arbitrary_precision))
      (and (not integer_fixed) integer_arbitrary_precision)))

(echo "zero-width fixed integers are invalid")
(push)
(assert integer_fixed)
(assert (= integer_width 0))
(assert integer_type_valid)
(check-sat)
(pop)

(echo "fixed integers cannot claim arbitrary precision")
(push)
(assert integer_fixed)
(assert integer_arbitrary_precision)
(assert integer_type_valid)
(check-sat)
(pop)

; Masked shifts accept every signed count; checked and undefined shifts only
; accept counts in [0,width). All modes require a positive fixed width.
(define-fun shift_defined ((masked Bool) (width Int) (count Int)) Bool
  (and (> width 0)
       (or masked (and (<= 0 count) (< count width)))))

(echo "masked shifts accept negative counts")
(push)
(assert (not (shift_defined true 32 (- 1))))
(check-sat)
(pop)

(echo "checked shifts reject negative counts")
(push)
(assert (shift_defined false 32 (- 1)))
(check-sat)
(pop)

(echo "checked shifts reject counts at the width")
(push)
(assert (shift_defined false 32 32))
(check-sat)
(pop)

; Finite reassociation rejects both IEEE exceptional encodings and negative
; zero. Positive zero remains a valid finite value.
(define-fun binary64_finite_reassociation_literal
    ((bits (_ BitVec 64))) Bool
  (and (not (= ((_ extract 62 52) bits) #b11111111111))
       (not (= bits #x8000000000000000))))

(echo "finite reassociation rejects negative zero")
(push)
(assert (binary64_finite_reassociation_literal #x8000000000000000))
(check-sat)
(pop)

(echo "finite reassociation rejects infinity")
(push)
(assert (binary64_finite_reassociation_literal #x7ff0000000000000))
(check-sat)
(pop)

(echo "finite reassociation rejects NaN")
(push)
(assert (binary64_finite_reassociation_literal #x7ff8000000000001))
(check-sat)
(pop)
