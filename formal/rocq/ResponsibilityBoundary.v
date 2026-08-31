From Stdlib Require Import Bool.Bool.
From Stdlib Require Import Arith.PeanoNat.

Module ResponsibilityBoundary.

Record BoundaryInput : Type := {
  replayed_proof : bool;
  equality_complete : bool;
  datalog_complete : bool;
  stable_source_identity : bool;
  canonical_input : bool;
  shared_mutable_state_requested : bool;
  reverse_dependency_requested : bool;
  embedded_runtime_requested : bool;
  tuple_promoted_to_proof : bool;
  internal_class_id_requested : bool
}.

Definition architecture_safe (input : BoundaryInput) : bool :=
  negb (shared_mutable_state_requested input) &&
  (negb (reverse_dependency_requested input) &&
  (negb (embedded_runtime_requested input) &&
  (negb (tuple_promoted_to_proof input) &&
   negb (internal_class_id_requested input)))).

Definition publish_fact (input : BoundaryInput) : bool :=
  architecture_safe input &&
  (replayed_proof input &&
  (stable_source_identity input && canonical_input input)).

Definition allow_absence (input : BoundaryInput) : bool :=
  architecture_safe input &&
  (equality_complete input && datalog_complete input).

Inductive Completion : Type := Complete | Limited.

Definition completion (input : BoundaryInput) : Completion :=
  if equality_complete input && datalog_complete input
  then Complete
  else Limited.

Theorem published_fact_requires_replayed_proof :
  forall input,
    publish_fact input = true -> replayed_proof input = true.
Proof.
  intros input published.
  unfold publish_fact in published.
  apply andb_true_iff in published as [_ published].
  now apply andb_true_iff in published as [proof _].
Qed.

Theorem published_fact_requires_stable_canonical_identity :
  forall input,
    publish_fact input = true ->
    stable_source_identity input = true /\ canonical_input input = true.
Proof.
  intros input published.
  unfold publish_fact in published.
  apply andb_true_iff in published as [_ published].
  apply andb_true_iff in published as [_ published].
  now apply andb_true_iff in published.
Qed.

Theorem proof_positive_survives_incomplete_searches :
  forall input,
    architecture_safe input = true ->
    replayed_proof input = true ->
    stable_source_identity input = true ->
    canonical_input input = true ->
    publish_fact input = true.
Proof.
  intros input safe proof stable canonical.
  unfold publish_fact.
  now rewrite safe, proof, stable, canonical.
Qed.

Theorem absence_requires_both_fixed_points :
  forall input,
    allow_absence input = true ->
    equality_complete input = true /\ datalog_complete input = true.
Proof.
  intros input allowed.
  unfold allow_absence in allowed.
  apply andb_true_iff in allowed as [_ complete].
  now apply andb_true_iff in complete.
Qed.

Theorem limited_equality_forbids_absence :
  forall input,
    equality_complete input = false -> allow_absence input = false.
Proof.
  intros input limited.
  unfold allow_absence.
  destruct (architecture_safe input); simpl; [now rewrite limited | reflexivity].
Qed.

Theorem limited_datalog_forbids_absence :
  forall input,
    datalog_complete input = false -> allow_absence input = false.
Proof.
  intros input limited.
  unfold allow_absence.
  destruct (architecture_safe input); simpl; [| reflexivity].
  destruct (equality_complete input); simpl; [now rewrite limited | reflexivity].
Qed.

Theorem complete_status_means_both_fixed_points :
  forall input,
    completion input = Complete ->
    equality_complete input = true /\ datalog_complete input = true.
Proof.
  intros input completed.
  unfold completion in completed.
  destruct (equality_complete input && datalog_complete input) eqn:both;
    [now apply andb_true_iff in both | discriminate].
Qed.

Theorem limited_status_means_an_incomplete_fixed_point :
  forall input,
    completion input = Limited ->
    equality_complete input = false \/ datalog_complete input = false.
Proof.
  intros input limited.
  unfold completion in limited.
  destruct (equality_complete input) eqn:equality;
    destruct (datalog_complete input) eqn:datalog;
    simpl in limited; try discriminate; auto.
Qed.

Theorem shared_mutable_state_rejects_architecture :
  forall input,
    shared_mutable_state_requested input = true ->
    architecture_safe input = false.
Proof.
  intros input shared.
  unfold architecture_safe.
  now rewrite shared.
Qed.

Theorem reverse_dependency_rejects_architecture :
  forall input,
    reverse_dependency_requested input = true ->
    architecture_safe input = false.
Proof.
  intros input reverse.
  unfold architecture_safe.
  destruct (shared_mutable_state_requested input); simpl; [reflexivity |].
  now rewrite reverse.
Qed.

Theorem embedded_runtime_rejects_architecture :
  forall input,
    embedded_runtime_requested input = true ->
    architecture_safe input = false.
Proof.
  intros input embedded.
  unfold architecture_safe.
  destruct (shared_mutable_state_requested input); simpl; [reflexivity |].
  destruct (reverse_dependency_requested input); simpl; [reflexivity |].
  now rewrite embedded.
Qed.

Theorem tuple_as_proof_rejects_architecture :
  forall input,
    tuple_promoted_to_proof input = true ->
    architecture_safe input = false.
Proof.
  intros input promoted.
  unfold architecture_safe.
  destruct (shared_mutable_state_requested input); simpl; [reflexivity |].
  destruct (reverse_dependency_requested input); simpl; [reflexivity |].
  destruct (embedded_runtime_requested input); simpl; [reflexivity |].
  now rewrite promoted.
Qed.

Theorem internal_class_id_rejects_architecture :
  forall input,
    internal_class_id_requested input = true ->
    architecture_safe input = false.
Proof.
  intros input internal.
  unfold architecture_safe.
  destruct (shared_mutable_state_requested input); simpl; [reflexivity |].
  destruct (reverse_dependency_requested input); simpl; [reflexivity |].
  destruct (embedded_runtime_requested input); simpl; [reflexivity |].
  destruct (tuple_promoted_to_proof input); simpl; [reflexivity |].
  now rewrite internal.
Qed.

(** Datalog tuples are consequences, not constructors of equality authority. *)
Definition datalog_tuple_authorizes_equality (_derived : bool) : bool := false.

Theorem datalog_tuple_never_authorizes_equality :
  forall derived, datalog_tuple_authorizes_equality derived = false.
Proof. reflexivity. Qed.

(** Public binary facts use stable source IDs in canonical order. *)
Definition canonical_pair (left right : nat) : nat * nat :=
  if left <=? right then (left, right) else (right, left).

Theorem canonical_pair_is_ordered :
  forall left right,
    fst (canonical_pair left right) <= snd (canonical_pair left right).
Proof.
  intros left right.
  unfold canonical_pair.
  destruct (left <=? right) eqn:ordered.
  - now apply Nat.leb_le in ordered.
  - apply Nat.leb_gt in ordered. apply Nat.lt_le_incl. assumption.
Qed.

Record Digests : Type := {
  cpg_digest : nat;
  semantic_report_digest : nat
}.

Definition export_owned_facts (_input : BoundaryInput) (digests : Digests) : Digests :=
  digests.

Theorem export_preserves_cpg_and_report :
  forall input digests,
    export_owned_facts input digests = digests.
Proof. reflexivity. Qed.

End ResponsibilityBoundary.

Print Assumptions ResponsibilityBoundary.published_fact_requires_replayed_proof.
Print Assumptions ResponsibilityBoundary.published_fact_requires_stable_canonical_identity.
Print Assumptions ResponsibilityBoundary.proof_positive_survives_incomplete_searches.
Print Assumptions ResponsibilityBoundary.absence_requires_both_fixed_points.
Print Assumptions ResponsibilityBoundary.limited_equality_forbids_absence.
Print Assumptions ResponsibilityBoundary.limited_datalog_forbids_absence.
Print Assumptions ResponsibilityBoundary.complete_status_means_both_fixed_points.
Print Assumptions ResponsibilityBoundary.limited_status_means_an_incomplete_fixed_point.
Print Assumptions ResponsibilityBoundary.shared_mutable_state_rejects_architecture.
Print Assumptions ResponsibilityBoundary.reverse_dependency_rejects_architecture.
Print Assumptions ResponsibilityBoundary.embedded_runtime_rejects_architecture.
Print Assumptions ResponsibilityBoundary.tuple_as_proof_rejects_architecture.
Print Assumptions ResponsibilityBoundary.internal_class_id_rejects_architecture.
Print Assumptions ResponsibilityBoundary.datalog_tuple_never_authorizes_equality.
Print Assumptions ResponsibilityBoundary.canonical_pair_is_ordered.
Print Assumptions ResponsibilityBoundary.export_preserves_cpg_and_report.
