---- MODULE SemanticKeyAdapter ----
EXTENDS Naturals, Sequences

\* A small executable model of libcpg's adapter to Replete's fallible
\* SemanticWriter. Bytes accumulate only in private storage. The adapter
\* publishes the complete canonical sequence or preserves the prior public
\* value when the effective capacity refuses a write.

Canonical == <<11, 22, 33>>
Outcomes == {"Running", "Complete", "Refused"}

VARIABLES capacity, cursor, private, published, prior, outcome

vars == <<capacity, cursor, private, published, prior, outcome>>

Init ==
    /\ capacity \in 0..Len(Canonical)
    /\ cursor = 1
    /\ private = <<>>
    /\ prior \in {<<>>, <<99>>}
    /\ published = prior
    /\ outcome = "Running"

Write ==
    /\ outcome = "Running"
    /\ cursor <= Len(Canonical)
    /\ IF Len(private) + 1 <= capacity
          THEN /\ private' = Append(private, Canonical[cursor])
               /\ cursor' = cursor + 1
               /\ UNCHANGED <<capacity, published, prior, outcome>>
          ELSE /\ outcome' = "Refused"
               /\ UNCHANGED <<capacity, cursor, private, published, prior>>

Commit ==
    /\ outcome = "Running"
    /\ cursor = Len(Canonical) + 1
    /\ published' = private
    /\ outcome' = "Complete"
    /\ UNCHANGED <<capacity, cursor, private, prior>>

Next == Write \/ Commit

Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

TypeOK ==
    /\ capacity \in Nat
    /\ cursor \in 1..(Len(Canonical) + 1)
    /\ private \in Seq(Nat)
    /\ published \in Seq(Nat)
    /\ prior \in Seq(Nat)
    /\ outcome \in Outcomes

PrivateIsCanonicalPrefix ==
    private = SubSeq(Canonical, 1, Len(private))

PrivateWithinCapacity == Len(private) <= capacity

NoPartialPublication ==
    outcome \in {"Running", "Refused"} => published = prior

CompletePublishesExactlyCanonical ==
    outcome = "Complete" => published = Canonical

RefusalPreservesPrior ==
    outcome = "Refused" => published = prior

Termination == <>(outcome # "Running")

====
