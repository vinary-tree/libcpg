From Stdlib Require Import Lists.List.
From Stdlib Require Import Bool.Bool.
Import ListNotations.

Module TreeParentContext.

Record Frame : Type := {
  node : nat;
  raw_parent : option nat;
  effective_parent : option nat
}.

Definition child_frame (parent : Frame) (child : nat) : Frame :=
  {| node := child;
     raw_parent := Some (node parent);
     effective_parent := effective_parent parent |}.

Theorem scheduled_child_has_direct_raw_parent :
  forall parent child,
    raw_parent (child_frame parent child) = Some (node parent).
Proof. reflexivity. Qed.

Definition next_effective
    (included : bool) (current : option nat) (current_node : nat)
    : option nat :=
  if included then Some current_node else current.

Theorem transparent_node_preserves_effective_parent :
  forall current current_node,
    next_effective false current current_node = current.
Proof. reflexivity. Qed.

Theorem included_node_becomes_effective_parent :
  forall current current_node,
    next_effective true current current_node = Some current_node.
Proof. reflexivity. Qed.

Inductive OwnershipClass : Type := Move | Borrow | Neither.

Definition classify_with_parent
    (is_reference parent_is_assignment child_is_value : bool)
    : OwnershipClass :=
  if is_reference then Borrow
  else if andb parent_is_assignment child_is_value then Move
  else Neither.

Theorem carried_parent_is_observationally_equivalent :
  forall is_reference carried_assignment oracle_assignment child_is_value,
    carried_assignment = oracle_assignment ->
    classify_with_parent is_reference carried_assignment child_is_value =
    classify_with_parent is_reference oracle_assignment child_is_value.
Proof. intros; subst; reflexivity. Qed.

End TreeParentContext.

Print Assumptions TreeParentContext.scheduled_child_has_direct_raw_parent.
Print Assumptions TreeParentContext.transparent_node_preserves_effective_parent.
Print Assumptions TreeParentContext.included_node_becomes_effective_parent.
Print Assumptions TreeParentContext.carried_parent_is_observationally_equivalent.
