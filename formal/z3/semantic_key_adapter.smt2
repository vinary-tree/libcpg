; Forbidden states for fallible canonical semantic-key publication.
(set-logic QF_LIA)

(declare-const canonical_length Int)
(declare-const capacity Int)
(declare-const outcome Int) ; 1 = complete, 2 = refused
(declare-const published_length Int)
(declare-const prior_length Int)
(declare-const published_fingerprint Int)
(declare-const canonical_fingerprint Int)
(declare-const prior_fingerprint Int)

(assert (>= canonical_length 0))
(assert (>= capacity 0))
(assert (>= prior_length 0))
(assert (= outcome (ite (<= canonical_length capacity) 1 2)))
(assert (= published_length
  (ite (= outcome 1) canonical_length prior_length)))
(assert (= published_fingerprint
  (ite (= outcome 1) canonical_fingerprint prior_fingerprint)))

; Completion cannot publish a truncated or extended byte sequence.
(push)
(assert (= outcome 1))
(assert (distinct published_length canonical_length))
(check-sat)
(pop)

; Completion cannot publish bytes with a different canonical identity.
(push)
(assert (= outcome 1))
(assert (distinct published_fingerprint canonical_fingerprint))
(check-sat)
(pop)

; Refusal cannot change the prior public length.
(push)
(assert (= outcome 2))
(assert (distinct published_length prior_length))
(check-sat)
(pop)

; Refusal cannot change the prior public bytes.
(push)
(assert (= outcome 2))
(assert (distinct published_fingerprint prior_fingerprint))
(check-sat)
(pop)

; Complete publication requires sufficient effective capacity.
(push)
(assert (= outcome 1))
(assert (> canonical_length capacity))
(check-sat)
(pop)

; Sufficient effective capacity cannot be reported as a capacity refusal.
(push)
(assert (= outcome 2))
(assert (<= canonical_length capacity))
(check-sat)
(pop)
