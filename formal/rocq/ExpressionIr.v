From Stdlib Require Import Bool.Bool.
From Stdlib Require Import Lists.List.
From Stdlib Require Import Arith.PeanoNat.
Import ListNotations.

Module ExpressionIr.

Inductive RewriteLaw : Type :=
| AdditiveIdentity
| MultiplicativeIdentity
| Commutativity
| Associativity
| ConstantFold
| BooleanShortCircuit.

Inductive EvidenceMode : Type := Pure | IdentityOnly | Unknown.
Inductive Completion : Type :=
| Complete
| WorkLimited
| NodeLimited
| MalformedFacts.

Record SemanticProfile : Type := {
  purity_effects_known : bool;
  evaluation_order_known : bool;
  integer_semantics_known : bool;
  floating_point_semantics_known : bool;
  exception_semantics_known : bool;
  short_circuit_semantics_known : bool;
  alias_memory_semantics_known : bool;
  call_semantics_known : bool;
  volatile_atomic_semantics_known : bool;
  binding_scope_semantics_known : bool;
  rewrite_allowed : RewriteLaw -> bool
}.

Definition profile_completeb (profile : SemanticProfile) : bool :=
  purity_effects_known profile &&
  evaluation_order_known profile &&
  integer_semantics_known profile &&
  floating_point_semantics_known profile &&
  exception_semantics_known profile &&
  short_circuit_semantics_known profile &&
  alias_memory_semantics_known profile &&
  call_semantics_known profile &&
  volatile_atomic_semantics_known profile &&
  binding_scope_semantics_known profile.

Definition rewrite_eligibleb
    (profile : SemanticProfile)
    (mode : EvidenceMode)
    (law : RewriteLaw)
    (completion : Completion) : bool :=
  match completion, mode with
  | Complete, Pure => profile_completeb profile && rewrite_allowed profile law
  | _, _ => false
  end.

Theorem rewrite_eligible_sound :
  forall profile mode law completion,
    rewrite_eligibleb profile mode law completion = true ->
    completion = Complete /\
    mode = Pure /\
    profile_completeb profile = true /\
    rewrite_allowed profile law = true.
Proof.
  intros profile mode law completion eligible.
  destruct completion, mode; simpl in eligible; try discriminate.
  apply andb_true_iff in eligible as [complete_profile allowed].
  repeat split; assumption || reflexivity.
Qed.

Theorem incomplete_profile_denies_rewrite :
  forall profile mode law completion,
    profile_completeb profile = false ->
    rewrite_eligibleb profile mode law completion = false.
Proof.
  intros profile mode law completion incomplete.
  destruct completion, mode; simpl; try reflexivity.
  rewrite incomplete.
  reflexivity.
Qed.

Theorem identity_only_denies_rewrite :
  forall profile law completion,
    rewrite_eligibleb profile IdentityOnly law completion = false.
Proof. intros; destruct completion; reflexivity. Qed.

Theorem unknown_denies_rewrite :
  forall profile law completion,
    rewrite_eligibleb profile Unknown law completion = false.
Proof. intros; destruct completion; reflexivity. Qed.

Theorem incomplete_lowering_denies_rewrite :
  forall profile mode law completion,
    completion <> Complete ->
    rewrite_eligibleb profile mode law completion = false.
Proof.
  intros profile mode law completion incomplete.
  destruct completion; try contradiction; destruct mode; reflexivity.
Qed.

Definition FlatNode : Type := list nat.

Inductive FlatTape : list FlatNode -> Prop :=
| flat_empty : FlatTape []
| flat_append : forall tape children,
    FlatTape tape ->
    Forall (fun child => child < length tape) children ->
    FlatTape (tape ++ [children]).

Theorem append_preserves_backward_references :
  forall tape children,
    FlatTape tape ->
    Forall (fun child => child < length tape) children ->
    FlatTape (tape ++ [children]).
Proof. exact flat_append. Qed.

Inductive LimitDecision : Type := Advance | StopWork | StopNodes.

Definition decide_limits
    (work nodes max_work max_nodes : nat) : LimitDecision :=
  if work <? max_work then
    if nodes <? max_nodes then Advance else StopNodes
  else StopWork.

Theorem advance_has_both_capacities :
  forall work nodes max_work max_nodes,
    decide_limits work nodes max_work max_nodes = Advance ->
    work < max_work /\ nodes < max_nodes.
Proof.
  intros work nodes max_work max_nodes decision.
  unfold decide_limits in decision.
  destruct (work <? max_work) eqn:work_test; try discriminate.
  destruct (nodes <? max_nodes) eqn:node_test; try discriminate.
  apply Nat.ltb_lt in work_test.
  apply Nat.ltb_lt in node_test.
  split; assumption.
Qed.

Theorem exhausted_work_precedes_node_limit :
  forall work nodes max_work max_nodes,
    max_work <= work ->
    decide_limits work nodes max_work max_nodes = StopWork.
Proof.
  intros work nodes max_work max_nodes exhausted.
  unfold decide_limits.
  apply Nat.ltb_ge in exhausted.
  rewrite exhausted.
  reflexivity.
Qed.

End ExpressionIr.

Print Assumptions ExpressionIr.rewrite_eligible_sound.
Print Assumptions ExpressionIr.incomplete_profile_denies_rewrite.
Print Assumptions ExpressionIr.append_preserves_backward_references.
Print Assumptions ExpressionIr.advance_has_both_capacities.
