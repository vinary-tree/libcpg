From Stdlib Require Import List Arith Lia.
Import ListNotations.

Module SemanticKeyAdapter.

Inductive EncodeResult (A : Type) : Type :=
| Complete (bytes : list A)
| Refused.

Arguments Complete {A} _.
Arguments Refused {A}.

Definition encode_bounded {A : Type}
    (canonical : list A) (capacity : nat) : EncodeResult A :=
  if Nat.leb (length canonical) capacity
  then Complete canonical
  else Refused.

Definition publish {A : Type}
    (prior : list A) (result : EncodeResult A) : list A :=
  match result with
  | Complete bytes => bytes
  | Refused => prior
  end.

Theorem complete_is_exact :
  forall (A : Type) (canonical bytes : list A) capacity,
    encode_bounded canonical capacity = Complete bytes ->
    bytes = canonical.
Proof.
  intros A canonical bytes capacity H.
  unfold encode_bounded in H.
  destruct (Nat.leb (length canonical) capacity); inversion H; reflexivity.
Qed.

Theorem complete_requires_capacity :
  forall (A : Type) (canonical bytes : list A) capacity,
    encode_bounded canonical capacity = Complete bytes ->
    length canonical <= capacity.
Proof.
  intros A canonical bytes capacity H.
  unfold encode_bounded in H.
  destruct (Nat.leb (length canonical) capacity) eqn:Hfits.
  - apply Nat.leb_le. exact Hfits.
  - discriminate.
Qed.

Theorem refusal_preserves_prior :
  forall (A : Type) (canonical prior : list A) capacity,
    encode_bounded canonical capacity = Refused ->
    publish prior (encode_bounded canonical capacity) = prior.
Proof.
  intros A canonical prior capacity H.
  rewrite H. reflexivity.
Qed.

Theorem sufficient_capacity_completes :
  forall (A : Type) (canonical : list A) capacity,
    length canonical <= capacity ->
    encode_bounded canonical capacity = Complete canonical.
Proof.
  intros A canonical capacity H.
  unfold encode_bounded.
  assert (Nat.leb (length canonical) capacity = true) as Hfits.
  { apply Nat.leb_le. exact H. }
  rewrite Hfits. reflexivity.
Qed.

Theorem equal_completed_bytes_imply_equal_canonical_bytes :
  forall (A : Type) (left right bytes : list A) left_capacity right_capacity,
    encode_bounded left left_capacity = Complete bytes ->
    encode_bounded right right_capacity = Complete bytes ->
    left = right.
Proof.
  intros A left right bytes left_capacity right_capacity Hleft Hright.
  pose proof (complete_is_exact A left bytes left_capacity Hleft) as Eleft.
  pose proof (complete_is_exact A right bytes right_capacity Hright) as Eright.
  congruence.
Qed.

End SemanticKeyAdapter.

Print Assumptions SemanticKeyAdapter.complete_is_exact.
Print Assumptions SemanticKeyAdapter.complete_requires_capacity.
Print Assumptions SemanticKeyAdapter.refusal_preserves_prior.
Print Assumptions SemanticKeyAdapter.sufficient_capacity_completes.
Print Assumptions SemanticKeyAdapter.equal_completed_bytes_imply_equal_canonical_bytes.
