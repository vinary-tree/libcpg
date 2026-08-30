From Stdlib Require Import Bool.Bool.
From Stdlib Require Import Lists.List.
Import ListNotations.

Module SemanticExpressionFlow.

(** A path is represented in control-flow order.  These actions are the
    abstract transfer function for one semantic expression class. *)
Inductive FlowAction : Type :=
| Generate
| Kill
| Preserve.

Definition forward_step (action : FlowAction) (available : bool) : bool :=
  match action with
  | Generate => true
  | Kill => false
  | Preserve => available
  end.

Fixpoint forward_path (initial : bool) (path : list FlowAction) : bool :=
  match path with
  | [] => initial
  | action :: rest => forward_path (forward_step action initial) rest
  end.

(** Anticipability is a backward must fact.  Scanning from a program point
    toward an exit, the first generation establishes the use and the first
    kill invalidates it. *)
Fixpoint anticipates (path : list FlowAction) : bool :=
  match path with
  | [] => false
  | Generate :: _ => true
  | Kill :: _ => false
  | Preserve :: rest => anticipates rest
  end.

Definition must_available (paths : list (list FlowAction)) : bool :=
  forallb (fun path => forward_path false path) paths.

Definition must_anticipate (paths : list (list FlowAction)) : bool :=
  forallb anticipates paths.

Lemma forward_path_append :
  forall prefix suffix initial,
    forward_path initial (prefix ++ suffix) =
    forward_path (forward_path initial prefix) suffix.
Proof.
  induction prefix as [| action rest induction]; intros suffix initial.
  - reflexivity.
  - simpl. apply induction.
Qed.

Theorem terminal_generate_establishes_availability :
  forall prefix initial,
    forward_path initial (prefix ++ [Generate]) = true.
Proof.
  intros prefix initial.
  rewrite forward_path_append.
  simpl. reflexivity.
Qed.

Theorem terminal_kill_removes_availability :
  forall prefix initial,
    forward_path initial (prefix ++ [Kill]) = false.
Proof.
  intros prefix initial.
  rewrite forward_path_append.
  simpl. reflexivity.
Qed.

Theorem must_available_is_path_universal :
  forall paths path,
    must_available paths = true ->
    In path paths ->
    forward_path false path = true.
Proof.
  intros paths path must contains.
  unfold must_available in must.
  apply forallb_forall with (x := path) in must; assumption.
Qed.

Theorem must_anticipate_is_path_universal :
  forall paths path,
    must_anticipate paths = true ->
    In path paths ->
    anticipates path = true.
Proof.
  intros paths path must contains.
  unfold must_anticipate in must.
  apply forallb_forall with (x := path) in must; assumption.
Qed.

Theorem generation_is_anticipated :
  forall suffix, anticipates (Generate :: suffix) = true.
Proof. reflexivity. Qed.

Theorem kill_blocks_anticipation :
  forall suffix, anticipates (Kill :: suffix) = false.
Proof. reflexivity. Qed.

Theorem preservation_defers_anticipation :
  forall suffix, anticipates (Preserve :: suffix) = anticipates suffix.
Proof. reflexivity. Qed.

Record Evidence : Type := {
  both_admitted : bool;
  both_pure : bool;
  profiles_same : bool;
  contexts_same : bool;
  exact_replay : bool;
  alpha_replay : bool;
  alpha_bijection : bool;
  provenance_complete : bool;
  saturation_complete : bool;
  dataflow_complete : bool;
  within_budget : bool;
  available_all_paths : bool;
  dominant_producer : bool;
  kill_free : bool;
  busy_all_paths : bool;
  placement_dominates : bool;
  multiple_occurrences : bool;
  evaluator_has_unequal_values : bool
}.

Definition exact_proof (evidence : Evidence) : bool :=
  both_admitted evidence &&
  (both_pure evidence &&
  (profiles_same evidence &&
  (contexts_same evidence &&
  (exact_replay evidence && provenance_complete evidence)))).

Definition alpha_proof (evidence : Evidence) : bool :=
  both_admitted evidence &&
  (both_pure evidence &&
  (profiles_same evidence &&
  (negb (contexts_same evidence) &&
  (alpha_replay evidence &&
  (alpha_bijection evidence && provenance_complete evidence))))).

Definition has_proof (evidence : Evidence) : bool :=
  exact_proof evidence || alpha_proof evidence.

Definition cse_candidate (evidence : Evidence) : bool :=
  exact_proof evidence &&
  (dataflow_complete evidence &&
  (available_all_paths evidence &&
  (dominant_producer evidence && kill_free evidence))).

Definition pre_candidate (evidence : Evidence) : bool :=
  exact_proof evidence &&
  (dataflow_complete evidence &&
  (multiple_occurrences evidence &&
  (busy_all_paths evidence && placement_dominates evidence))).

Theorem cse_requires_exact_proof :
  forall evidence,
    cse_candidate evidence = true -> exact_proof evidence = true.
Proof.
  intros evidence accepted.
  unfold cse_candidate in accepted.
  now apply andb_true_iff in accepted as [proof _].
Qed.

Theorem cse_requires_complete_must_availability :
  forall evidence,
    cse_candidate evidence = true ->
    dataflow_complete evidence = true /\
    available_all_paths evidence = true /\
    dominant_producer evidence = true /\
    kill_free evidence = true.
Proof.
  intros evidence accepted.
  unfold cse_candidate in accepted.
  apply andb_true_iff in accepted as [_ accepted].
  apply andb_true_iff in accepted as [complete accepted].
  apply andb_true_iff in accepted as [available accepted].
  apply andb_true_iff in accepted as [dominates unmodified].
  repeat split; assumption.
Qed.

Theorem availability_without_dominance_rejects_cse :
  forall evidence,
    dominant_producer evidence = false ->
    cse_candidate evidence = false.
Proof.
  intros evidence absent.
  unfold cse_candidate.
  destruct (exact_proof evidence); simpl; [| reflexivity].
  destruct (dataflow_complete evidence); simpl; [| reflexivity].
  destruct (available_all_paths evidence); simpl; [| reflexivity].
  now rewrite absent.
Qed.

Theorem intervening_kill_rejects_cse :
  forall evidence,
    kill_free evidence = false ->
    cse_candidate evidence = false.
Proof.
  intros evidence killed.
  unfold cse_candidate.
  destruct (exact_proof evidence); simpl; [| reflexivity].
  destruct (dataflow_complete evidence); simpl; [| reflexivity].
  destruct (available_all_paths evidence); simpl; [| reflexivity].
  destruct (dominant_producer evidence); simpl; [| reflexivity].
  exact killed.
Qed.

Theorem pre_requires_proof_and_anticipability :
  forall evidence,
    pre_candidate evidence = true ->
    exact_proof evidence = true /\
    dataflow_complete evidence = true /\
    multiple_occurrences evidence = true /\
    busy_all_paths evidence = true /\
    placement_dominates evidence = true.
Proof.
  intros evidence accepted.
  unfold pre_candidate in accepted.
  apply andb_true_iff in accepted as [proof accepted].
  apply andb_true_iff in accepted as [complete accepted].
  apply andb_true_iff in accepted as [multiple accepted].
  apply andb_true_iff in accepted as [busy dominates].
  repeat split; assumption.
Qed.

Inductive DiffStatus : Type :=
| ProvenEquivalent
| ProvenDifferent
| Limited
| Unknown.

Definition evidence_limited (evidence : Evidence) : bool :=
  negb (within_budget evidence) ||
  (negb (saturation_complete evidence) ||
  (negb (provenance_complete evidence) ||
   negb (dataflow_complete evidence))).

Definition classify (evidence : Evidence) : DiffStatus :=
  if has_proof evidence then ProvenEquivalent
  else if evaluator_has_unequal_values evidence then ProvenDifferent
  else if evidence_limited evidence then Limited
  else Unknown.

Definition consistent_evidence (evidence : Evidence) : Prop :=
  has_proof evidence = true ->
  evaluator_has_unequal_values evidence = false.

Theorem equivalent_classification_has_replayable_proof :
  forall evidence,
    classify evidence = ProvenEquivalent -> has_proof evidence = true.
Proof.
  intros evidence classified.
  unfold classify in classified.
  destruct (has_proof evidence) eqn:proof.
  - reflexivity.
  - destruct (evaluator_has_unequal_values evidence);
      destruct (evidence_limited evidence); discriminate.
Qed.

Theorem different_classification_has_evaluator_witness :
  forall evidence,
    classify evidence = ProvenDifferent ->
    has_proof evidence = false /\
    evaluator_has_unequal_values evidence = true.
Proof.
  intros evidence classified.
  unfold classify in classified.
  destruct (has_proof evidence) eqn:proof; [discriminate |].
  destruct (evaluator_has_unequal_values evidence) eqn:different.
  - split; reflexivity.
  - destruct (evidence_limited evidence); discriminate.
Qed.

Theorem limited_classification_has_no_decisive_witness :
  forall evidence,
    classify evidence = Limited ->
    has_proof evidence = false /\
    evaluator_has_unequal_values evidence = false /\
    evidence_limited evidence = true.
Proof.
  intros evidence classified.
  unfold classify in classified.
  destruct (has_proof evidence) eqn:proof; [discriminate |].
  destruct (evaluator_has_unequal_values evidence) eqn:different;
    [discriminate |].
  destruct (evidence_limited evidence) eqn:limited;
    [repeat split; reflexivity | discriminate].
Qed.

Theorem unknown_classification_is_complete_and_undecided :
  forall evidence,
    classify evidence = Unknown ->
    has_proof evidence = false /\
    evaluator_has_unequal_values evidence = false /\
    evidence_limited evidence = false.
Proof.
  intros evidence classified.
  unfold classify in classified.
  destruct (has_proof evidence) eqn:proof; [discriminate |].
  destruct (evaluator_has_unequal_values evidence) eqn:different;
    [discriminate |].
  destruct (evidence_limited evidence) eqn:limited;
    [discriminate | repeat split; reflexivity].
Qed.

Theorem proof_takes_precedence_over_limits :
  forall evidence,
    has_proof evidence = true -> classify evidence = ProvenEquivalent.
Proof.
  intros evidence proof.
  unfold classify. now rewrite proof.
Qed.

Theorem evaluator_difference_takes_precedence_over_limits :
  forall evidence,
    has_proof evidence = false ->
    evaluator_has_unequal_values evidence = true ->
    classify evidence = ProvenDifferent.
Proof.
  intros evidence no_proof different.
  unfold classify. now rewrite no_proof, different.
Qed.

Theorem replayable_proof_excludes_evaluator_difference :
  forall evidence,
    consistent_evidence evidence ->
    has_proof evidence = true ->
    evaluator_has_unequal_values evidence = false.
Proof. intros evidence consistent proof. now apply consistent. Qed.

Definition preserve_cpg {Graph : Type} (graph : Graph) : Graph := graph.

Theorem semantic_flow_does_not_mutate_cpg :
  forall (Graph : Type) (graph : Graph), preserve_cpg graph = graph.
Proof. reflexivity. Qed.

End SemanticExpressionFlow.

Print Assumptions SemanticExpressionFlow.terminal_generate_establishes_availability.
Print Assumptions SemanticExpressionFlow.terminal_kill_removes_availability.
Print Assumptions SemanticExpressionFlow.must_available_is_path_universal.
Print Assumptions SemanticExpressionFlow.must_anticipate_is_path_universal.
Print Assumptions SemanticExpressionFlow.generation_is_anticipated.
Print Assumptions SemanticExpressionFlow.kill_blocks_anticipation.
Print Assumptions SemanticExpressionFlow.preservation_defers_anticipation.
Print Assumptions SemanticExpressionFlow.cse_requires_exact_proof.
Print Assumptions SemanticExpressionFlow.cse_requires_complete_must_availability.
Print Assumptions SemanticExpressionFlow.availability_without_dominance_rejects_cse.
Print Assumptions SemanticExpressionFlow.intervening_kill_rejects_cse.
Print Assumptions SemanticExpressionFlow.pre_requires_proof_and_anticipability.
Print Assumptions SemanticExpressionFlow.equivalent_classification_has_replayable_proof.
Print Assumptions SemanticExpressionFlow.different_classification_has_evaluator_witness.
Print Assumptions SemanticExpressionFlow.limited_classification_has_no_decisive_witness.
Print Assumptions SemanticExpressionFlow.unknown_classification_is_complete_and_undecided.
Print Assumptions SemanticExpressionFlow.proof_takes_precedence_over_limits.
Print Assumptions SemanticExpressionFlow.evaluator_difference_takes_precedence_over_limits.
Print Assumptions SemanticExpressionFlow.replayable_proof_excludes_evaluator_difference.
Print Assumptions SemanticExpressionFlow.semantic_flow_does_not_mutate_cpg.
