(* Import/export overlay edge identity and direct-adjacency refinement.
   This finite-list model describes the observable edge sequence, not a
   particular petgraph or CSR representation.  The actual overlay domain
   emits only unlabeled Exports edges. *)
From Stdlib Require Import Bool.Bool Lists.List Arith.PeanoNat.
Import ListNotations.

Record Edge := {
  edge_source : nat;
  edge_target : nat;
  edge_kind : nat;
  edge_label : option nat
}.

Definition label_eqb (a b : option nat) : bool :=
  match a, b with
  | None, None => true
  | Some x, Some y => Nat.eqb x y
  | _, _ => false
  end.

Definition endpoints (source target : nat) (edge : Edge) : bool :=
  Nat.eqb (edge_source edge) source && Nat.eqb (edge_target edge) target.

Definition exact_identity (source target kind : nat) (label : option nat)
    (edge : Edge) : bool :=
  endpoints source target edge && Nat.eqb (edge_kind edge) kind &&
    label_eqb (edge_label edge) label.

(* CSR projection filters by endpoints before checking kind and label. *)
Definition projected_duplicate (source target kind : nat)
    (label : option nat) (edges : list Edge) : bool :=
  existsb (fun edge => Nat.eqb (edge_kind edge) kind &&
                         label_eqb (edge_label edge) label)
          (filter (endpoints source target) edges).

(* The native adjacency iterator filters the same directed endpoints. *)
Definition adjacent_duplicate (source target kind : nat)
    (label : option nat) (edges : list Edge) : bool :=
  existsb (exact_identity source target kind label) edges.

Theorem adjacent_duplicate_refines_projection :
  forall source target kind label edges,
    adjacent_duplicate source target kind label edges =
    projected_duplicate source target kind label edges.
Proof.
  intros source target kind label edges.
  induction edges as [|edge rest IH]; simpl; auto.
  unfold exact_identity, projected_duplicate in *; simpl in *.
  destruct (endpoints source target edge); simpl; rewrite ?IH;
    destruct (Nat.eqb (edge_kind edge) kind &&
              label_eqb (edge_label edge) label); reflexivity.
Qed.

(* The existing Exports insertion deliberately uses kind-only identity.  It
   emits an unlabeled edge, but a preexisting labeled Exports edge suppresses
   insertion too; changing that rule would not be observationally equivalent
   on arbitrary hand-built graphs. *)
Definition projected_kind_duplicate (source target kind : nat)
    (edges : list Edge) : bool :=
  existsb (fun edge => Nat.eqb (edge_kind edge) kind)
          (filter (endpoints source target) edges).

Definition adjacent_kind_duplicate (source target kind : nat)
    (edges : list Edge) : bool :=
  existsb (fun edge => endpoints source target edge &&
                         Nat.eqb (edge_kind edge) kind) edges.

Theorem adjacent_kind_refines_projection :
  forall source target kind edges,
    adjacent_kind_duplicate source target kind edges =
    projected_kind_duplicate source target kind edges.
Proof.
  intros source target kind edges.
  induction edges as [|edge rest IH]; simpl; auto.
  unfold projected_kind_duplicate in *; simpl in *.
  destruct (endpoints source target edge); simpl; rewrite ?IH;
    destruct (Nat.eqb (edge_kind edge) kind); reflexivity.
Qed.

Definition append_if_new_kind (source target kind : nat)
    (edges : list Edge) : list Edge :=
  if adjacent_kind_duplicate source target kind edges then edges
  else edges ++ [{| edge_source := source; edge_target := target;
                   edge_kind := kind; edge_label := None |}].

Lemma append_if_new_kind_has_identity :
  forall source target kind edges,
    adjacent_kind_duplicate source target kind
      (append_if_new_kind source target kind edges) = true.
Proof.
  intros source target kind edges.
  unfold append_if_new_kind.
  destruct (adjacent_kind_duplicate source target kind edges) eqn:Hseen.
  - exact Hseen.
  - unfold adjacent_kind_duplicate, endpoints.
    rewrite existsb_app. simpl. repeat rewrite Nat.eqb_refl.
    now rewrite Bool.orb_true_r.
Qed.

Theorem append_if_new_kind_idempotent :
  forall source target kind edges,
    append_if_new_kind source target kind
      (append_if_new_kind source target kind edges) =
    append_if_new_kind source target kind edges.
Proof.
  intros source target kind edges.
  unfold append_if_new_kind at 1.
  rewrite append_if_new_kind_has_identity.
  reflexivity.
Qed.

Definition append_if_new (source target kind : nat) (label : option nat)
    (edges : list Edge) : list Edge :=
  if adjacent_duplicate source target kind label edges then edges
  else edges ++ [{| edge_source := source; edge_target := target;
                   edge_kind := kind; edge_label := label |}].

Lemma append_if_new_has_identity :
  forall source target kind label edges,
    adjacent_duplicate source target kind label
      (append_if_new source target kind label edges) = true.
Proof.
  intros source target kind label edges.
  unfold append_if_new.
  destruct (adjacent_duplicate source target kind label edges) eqn:Hseen.
  - exact Hseen.
  - unfold adjacent_duplicate, exact_identity, endpoints.
    rewrite existsb_app. simpl. repeat rewrite Nat.eqb_refl.
    destruct label; simpl; repeat rewrite Nat.eqb_refl;
      now rewrite Bool.orb_true_r.
Qed.

Theorem append_if_new_idempotent :
  forall source target kind label edges,
    append_if_new source target kind label
      (append_if_new source target kind label edges) =
    append_if_new source target kind label edges.
Proof.
  intros source target kind label edges.
  unfold append_if_new at 1.
  rewrite append_if_new_has_identity.
  reflexivity.
Qed.
