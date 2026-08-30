From Stdlib Require Import Arith.PeanoNat.
From Stdlib Require Import Lists.List.
From Stdlib Require Import Sorting.Sorted.
From Stdlib Require Import Lia.
Import ListNotations.

Module ExpressionEquivalence.

Inductive EvidenceMode : Type :=
| PureLive
| IdentityOnly
| Unknown
| Persisted.

Inductive Completion : Type :=
| Saturated
| WorkLimited
| NodeLimited
| MatchLimited
| Cancelled
| Malformed
| ProofLimited.

Record Candidate : Type := {
  source : nat;
  scope : nat;
  profile : nat;
  mode : EvidenceMode
}.

Definition admitted (candidate : Candidate) : Prop :=
  mode candidate = PureLive.

Definition same_boundary (left right : Candidate) : Prop :=
  scope left = scope right /\ profile left = profile right.

Definition canonical_pair (left right : nat) : nat * nat :=
  if left <=? right then (left, right) else (right, left).

Theorem canonical_pair_ordered :
  forall left right,
    fst (canonical_pair left right) <= snd (canonical_pair left right).
Proof.
  intros left right.
  unfold canonical_pair.
  destruct (left <=? right) eqn:ordered.
  - apply Nat.leb_le in ordered. exact ordered.
  - apply Nat.leb_gt in ordered. apply Nat.lt_le_incl. exact ordered.
Qed.

Theorem canonical_pair_symmetric :
  forall left right,
    canonical_pair left right = canonical_pair right left.
Proof.
  intros left right.
  unfold canonical_pair.
  destruct (left <=? right) eqn:left_right;
  destruct (right <=? left) eqn:right_left.
  - apply Nat.leb_le in left_right.
    apply Nat.leb_le in right_left.
    assert (left = right) as equal.
    { apply Nat.le_antisymm; assumption. }
    subst; reflexivity.
  - reflexivity.
  - reflexivity.
  - apply Nat.leb_gt in left_right.
    apply Nat.leb_gt in right_left.
    lia.
Qed.

Section EqualityAndEvidence.

Variable rewrite_edge : Candidate -> Candidate -> Prop.

Inductive EqualityClosure : Candidate -> Candidate -> Prop :=
| equality_refl : forall candidate, EqualityClosure candidate candidate
| equality_edge : forall left right,
    rewrite_edge left right -> EqualityClosure left right
| equality_sym : forall left right,
    EqualityClosure left right -> EqualityClosure right left
| equality_trans : forall left middle right,
    EqualityClosure left middle ->
    EqualityClosure middle right ->
    EqualityClosure left right.

Definition SoundEdges : Prop :=
  forall left right,
    rewrite_edge left right ->
    admitted left /\ admitted right /\ same_boundary left right.

Definition PublicEquality (left right : Candidate) : Prop :=
  admitted left /\ admitted right /\ EqualityClosure left right.

Theorem public_equality_reflexive :
  forall candidate,
    admitted candidate -> PublicEquality candidate candidate.
Proof.
  intros candidate eligible.
  repeat split; try assumption.
  constructor.
Qed.

Theorem public_equality_symmetric :
  forall left right,
    PublicEquality left right -> PublicEquality right left.
Proof.
  intros left right [left_ok [right_ok equality]].
  repeat split; try assumption.
  now apply equality_sym.
Qed.

Theorem public_equality_transitive :
  forall left middle right,
    PublicEquality left middle ->
    PublicEquality middle right ->
    PublicEquality left right.
Proof.
  intros left middle right [left_ok [_ left_middle]]
    [_ [right_ok middle_right]].
  split; [exact left_ok |].
  split; [exact right_ok |].
  eapply equality_trans; eassumption.
Qed.

Lemma equality_closure_confined :
  SoundEdges ->
  forall left right,
    EqualityClosure left right -> same_boundary left right.
Proof.
  intros sound left right equality.
  induction equality.
  - split; reflexivity.
  - destruct (sound _ _ H) as [_ [_ boundary]]. exact boundary.
  - destruct IHequality as [scope_eq profile_eq].
    split; symmetry; assumption.
  - destruct IHequality1 as [left_scope left_profile].
    destruct IHequality2 as [right_scope right_profile].
    split; etransitivity; eassumption.
Qed.

Theorem public_equality_same_boundary :
  SoundEdges ->
  forall left right,
    PublicEquality left right -> same_boundary left right.
Proof.
  intros sound left right [_ [_ equality]].
  now eapply equality_closure_confined.
Qed.

Variable proof_edge : Candidate -> Candidate -> Prop.

Definition ProofEdgesSound : Prop :=
  forall left right, proof_edge left right -> EqualityClosure left right.

Inductive ReplayClosure : Candidate -> Candidate -> Prop :=
| replay_refl : forall candidate, ReplayClosure candidate candidate
| replay_edge : forall left right,
    proof_edge left right -> ReplayClosure left right
| replay_sym : forall left right,
    ReplayClosure left right -> ReplayClosure right left
| replay_trans : forall left middle right,
    ReplayClosure left middle ->
    ReplayClosure middle right ->
    ReplayClosure left right.

Theorem replay_is_equality :
  ProofEdgesSound ->
  forall left right,
    ReplayClosure left right -> EqualityClosure left right.
Proof.
  intros sound left right replay.
  induction replay.
  - constructor.
  - apply sound. assumption.
  - now apply equality_sym.
  - eapply equality_trans; eassumption.
Qed.

Record PositiveEvidence (left right : Candidate) : Prop := {
  positive_equality : PublicEquality left right;
  positive_replay : ReplayClosure left right
}.

Theorem positive_evidence_is_admitted_replayable_equality :
  ProofEdgesSound ->
  forall left right,
    PositiveEvidence left right ->
    admitted left /\ admitted right /\
    EqualityClosure left right /\ ReplayClosure left right.
Proof.
  intros sound left right positive.
  destruct positive as [[left_ok [right_ok _]] replay].
  split; [exact left_ok |].
  split; [exact right_ok |].
  split.
  - exact (replay_is_equality sound left right replay).
  - exact replay.
Qed.

Record NegativeEvidence (left right : Candidate) : Type := {
  negative_completion : Completion;
  negative_is_saturated : negative_completion = Saturated;
  negative_left_admitted : admitted left;
  negative_right_admitted : admitted right;
  negative_same_boundary : same_boundary left right;
  negative_absent : ~ PublicEquality left right
}.

Theorem limited_completion_has_no_negative_evidence :
  forall left right completion,
    completion <> Saturated ->
    ~ (exists evidence : NegativeEvidence left right,
         negative_completion left right evidence = completion).
Proof.
  intros left right completion limited [evidence observed].
  apply limited.
  rewrite <- observed.
  exact (negative_is_saturated left right evidence).
Qed.

Theorem unsupported_has_no_negative_evidence :
  forall left right,
    (~ admitted left \/ ~ admitted right \/ ~ same_boundary left right) ->
    NegativeEvidence left right -> False.
Proof.
  intros left right unsupported evidence.
  destruct unsupported as [left_bad | [right_bad | boundary_bad]].
  - exact (left_bad (negative_left_admitted left right evidence)).
  - exact (right_bad (negative_right_admitted left right evidence)).
  - exact (boundary_bad (negative_same_boundary left right evidence)).
Qed.

End EqualityAndEvidence.

Theorem equality_grows_monotonically :
  forall (smaller_edge larger_edge : Candidate -> Candidate -> Prop),
    (forall left right, smaller_edge left right -> larger_edge left right) ->
    forall left right,
      @EqualityClosure smaller_edge left right ->
      @EqualityClosure larger_edge left right.
Proof.
  intros smaller_edge larger_edge includes left right equality.
  induction equality.
  - constructor.
  - apply equality_edge. now apply includes.
  - now apply equality_sym.
  - eapply equality_trans; eassumption.
Qed.

Definition can_commit (used cost maximum : nat) : Prop :=
  used + cost <= maximum.

Theorem committed_resource_stays_within_cap :
  forall used cost maximum,
    can_commit used cost maximum -> used + cost <= maximum.
Proof. intros used cost maximum permitted. exact permitted. Qed.

Definition pair_rank (pair : nat * nat) : nat :=
  fst pair * 1024 + snd pair.

Definition append_canonical
    (prior : list (nat * nat)) (next : nat * nat) : Prop :=
  fst next < snd next /\
  Forall (fun prior_pair => pair_rank prior_pair < pair_rank next) prior.

Theorem append_canonical_preserves_strict_order :
  forall prior next,
    StronglySorted (fun left right => pair_rank left < pair_rank right) prior ->
    append_canonical prior next ->
    StronglySorted (fun left right => pair_rank left < pair_rank right)
      (prior ++ [next]).
Proof.
  intros prior next sorted.
  revert next.
  induction sorted as [| head tail tail_sorted induction head_before_tail];
    intros next [ordered greater].
  - simpl. constructor; constructor.
  - inversion greater as [| observed_head observed_tail head_before_next tail_before_next].
    subst observed_head observed_tail.
    simpl.
    constructor.
    + apply induction. split; assumption.
    + apply Forall_app. split.
      * exact head_before_tail.
      * constructor; [exact head_before_next | constructor].
Qed.

Definition preserve_legacy_clone {A : Type} (legacy : A) : A := legacy.

Theorem legacy_clone_result_is_unchanged :
  forall (A : Type) (legacy : A), preserve_legacy_clone legacy = legacy.
Proof. reflexivity. Qed.

End ExpressionEquivalence.

Print Assumptions ExpressionEquivalence.public_equality_reflexive.
Print Assumptions ExpressionEquivalence.public_equality_symmetric.
Print Assumptions ExpressionEquivalence.public_equality_transitive.
Print Assumptions ExpressionEquivalence.public_equality_same_boundary.
Print Assumptions ExpressionEquivalence.positive_evidence_is_admitted_replayable_equality.
Print Assumptions ExpressionEquivalence.limited_completion_has_no_negative_evidence.
Print Assumptions ExpressionEquivalence.unsupported_has_no_negative_evidence.
Print Assumptions ExpressionEquivalence.equality_grows_monotonically.
Print Assumptions ExpressionEquivalence.committed_resource_stays_within_cap.
Print Assumptions ExpressionEquivalence.append_canonical_preserves_strict_order.
Print Assumptions ExpressionEquivalence.legacy_clone_result_is_unchanged.
