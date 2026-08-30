; Parent-context obligations for the iterative tree-sitter AST work machine.
(set-logic QF_LIA)

(declare-const parent Int)
(declare-const child Int)
(declare-const scheduled_raw_parent Int)
(declare-const current_effective_parent Int)
(declare-const scheduled_effective_parent Int)
(declare-const parent_included Bool)
(declare-const carried_assignment Bool)
(declare-const oracle_assignment Bool)
(declare-const child_is_value Bool)

(assert (= scheduled_raw_parent parent))
(assert (= scheduled_effective_parent
  (ite parent_included parent current_effective_parent)))

(echo "scheduled child cannot lose its direct raw parent")
(push)
(assert (not (= scheduled_raw_parent parent)))
(check-sat)
(pop)

(echo "transparent parent preserves the effective parent")
(push)
(assert (not parent_included))
(assert (not (= scheduled_effective_parent current_effective_parent)))
(check-sat)
(pop)

(echo "included parent becomes the effective parent")
(push)
(assert parent_included)
(assert (not (= scheduled_effective_parent parent)))
(check-sat)
(pop)

(echo "equal carried and oracle parents cannot change ownership classification")
(push)
(assert (= carried_assignment oracle_assignment))
(assert (not (= (and carried_assignment child_is_value)
                (and oracle_assignment child_is_value))))
(check-sat)
(pop)
