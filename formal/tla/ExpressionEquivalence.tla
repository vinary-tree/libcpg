---- MODULE ExpressionEquivalence ----
EXTENDS Naturals, Sequences, FiniteSets, TLC

(***************************************************************************
A bounded executable model of libcpg's exact expression-equivalence adapter.
It separates semantic admission, equality growth, proof availability, terminal
completion, and deterministic public ordering.  The model deliberately keeps
the CPG and legacy clone result as immutable observations.
***************************************************************************)

Candidates == 0..4
Modes == {"PureLive", "IdentityOnly", "Unknown", "Persisted"}
Statuses == {
    "Running", "Saturated", "WorkLimited", "NodeLimited", "MatchLimited",
    "Cancelled", "Malformed", "ProofLimited"
}
ForcedStops == {"None", "Match", "Cancel", "Malformed"}
Outcomes == {"Equivalent", "NotEquivalent", "Inconclusive", "Unsupported"}

VARIABLES status, pending, equalities, proofPairs, output, work, nodes,
          proofFacts, provenanceComplete, blockedMode, maxWork, maxNodes,
          maxProofFacts, forcedStop, cpgDigest, legacyCloneDigest

CanonicalPair(left, right) ==
    IF left <= right THEN <<left, right>> ELSE <<right, left>>

CandidatePairs ==
    {<<left, right>> \in Candidates \X Candidates : left < right}

IdentityPairs == {<<candidate, candidate>> : candidate \in Candidates}
LawPair == <<0, 1>>

ScopeOf(candidate) == IF candidate = 2 THEN 1 ELSE 0
ProfileOf(candidate) == IF candidate = 3 THEN 1 ELSE 0
ModeOf(candidate) == IF candidate = 4 THEN blockedMode ELSE "PureLive"
Admitted(candidate) == ModeOf(candidate) = "PureLive"
SameBoundary(left, right) ==
    /\ ScopeOf(left) = ScopeOf(right)
    /\ ProfileOf(left) = ProfileOf(right)

SeqRange(sequence) == {sequence[index] : index \in DOMAIN sequence}
PairRank(pair) == pair[1] * Cardinality(Candidates) + pair[2]

vars == <<status, pending, equalities, proofPairs, output, work, nodes,
          proofFacts, provenanceComplete, blockedMode, maxWork, maxNodes,
          maxProofFacts, forcedStop, cpgDigest, legacyCloneDigest>>

Equivalent(left, right) == CanonicalPair(left, right) \in equalities
Replayable(left, right) == CanonicalPair(left, right) \in proofPairs

QueryOutcome(left, right) ==
    IF ~Admitted(left) \/ ~Admitted(right) \/ ~SameBoundary(left, right)
    THEN "Unsupported"
    ELSE IF Equivalent(left, right)
         THEN IF provenanceComplete /\ Replayable(left, right)
              THEN "Equivalent"
              ELSE "Inconclusive"
         ELSE IF status = "Saturated" /\ provenanceComplete
              THEN "NotEquivalent"
              ELSE "Inconclusive"

PositivePairs ==
    {pair \in CandidatePairs : QueryOutcome(pair[1], pair[2]) = "Equivalent"}

Init ==
    /\ status = "Running"
    /\ pending = {LawPair}
    /\ equalities = IdentityPairs
    /\ proofPairs = IdentityPairs
    /\ output = <<>>
    /\ work = 0
    /\ nodes = Cardinality(Candidates)
    /\ proofFacts = 0
    /\ provenanceComplete = TRUE
    /\ blockedMode \in Modes
    /\ maxWork \in 0..1
    /\ maxNodes \in Cardinality(Candidates)..(Cardinality(Candidates) + 1)
    /\ maxProofFacts \in 0..1
    /\ forcedStop \in ForcedStops
    /\ cpgDigest = "immutable-cpg"
    /\ legacyCloneDigest = "unchanged-type-1-2-3"

StopForced ==
    /\ status = "Running"
    /\ forcedStop # "None"
    /\ status' = CASE forcedStop = "Match" -> "MatchLimited"
                    [] forcedStop = "Cancel" -> "Cancelled"
                    [] forcedStop = "Malformed" -> "Malformed"
    /\ pending' = {}
    /\ UNCHANGED <<equalities, proofPairs, output, work, nodes, proofFacts,
                    provenanceComplete, blockedMode, maxWork, maxNodes,
                    maxProofFacts, forcedStop, cpgDigest, legacyCloneDigest>>

StopWork ==
    /\ status = "Running"
    /\ forcedStop = "None"
    /\ pending # {}
    /\ work >= maxWork
    /\ status' = "WorkLimited"
    /\ pending' = {}
    /\ UNCHANGED <<equalities, proofPairs, output, work, nodes, proofFacts,
                    provenanceComplete, blockedMode, maxWork, maxNodes,
                    maxProofFacts, forcedStop, cpgDigest, legacyCloneDigest>>

StopNodes ==
    /\ status = "Running"
    /\ forcedStop = "None"
    /\ pending # {}
    /\ work < maxWork
    /\ nodes >= maxNodes
    /\ status' = "NodeLimited"
    /\ pending' = {}
    /\ UNCHANGED <<equalities, proofPairs, output, work, nodes, proofFacts,
                    provenanceComplete, blockedMode, maxWork, maxNodes,
                    maxProofFacts, forcedStop, cpgDigest, legacyCloneDigest>>

ApplyLaw ==
    /\ status = "Running"
    /\ forcedStop = "None"
    /\ LawPair \in pending
    /\ work < maxWork
    /\ nodes < maxNodes
    /\ pending' = pending \ {LawPair}
    /\ equalities' = equalities \cup {LawPair}
    /\ work' = work + 1
    /\ nodes' = nodes + 1
    /\ IF proofFacts < maxProofFacts
          THEN /\ proofPairs' = proofPairs \cup {LawPair}
               /\ proofFacts' = proofFacts + 1
               /\ provenanceComplete' = provenanceComplete
          ELSE /\ proofPairs' = proofPairs
               /\ proofFacts' = proofFacts
               /\ provenanceComplete' = FALSE
    /\ UNCHANGED <<status, output, blockedMode, maxWork, maxNodes,
                    maxProofFacts, forcedStop, cpgDigest, legacyCloneDigest>>

Finish ==
    /\ status = "Running"
    /\ pending = {}
    /\ status' = IF provenanceComplete THEN "Saturated" ELSE "ProofLimited"
    /\ UNCHANGED <<pending, equalities, proofPairs, output, work, nodes,
                    proofFacts, provenanceComplete, blockedMode, maxWork,
                    maxNodes, maxProofFacts, forcedStop, cpgDigest,
                    legacyCloneDigest>>

EmitNext ==
    LET remaining == PositivePairs \ SeqRange(output)
        next == CHOOSE pair \in remaining :
                    \A other \in remaining : PairRank(pair) <= PairRank(other)
    IN
    /\ status # "Running"
    /\ remaining # {}
    /\ output' = Append(output, next)
    /\ UNCHANGED <<status, pending, equalities, proofPairs, work, nodes,
                    proofFacts, provenanceComplete, blockedMode, maxWork,
                    maxNodes, maxProofFacts, forcedStop, cpgDigest,
                    legacyCloneDigest>>

Next == StopForced \/ StopWork \/ StopNodes \/ ApplyLaw \/ Finish \/ EmitNext
Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ status \in Statuses
    /\ pending \subseteq {LawPair}
    /\ equalities \subseteq CandidatePairs \cup IdentityPairs
    /\ proofPairs \subseteq CandidatePairs \cup IdentityPairs
    /\ SeqRange(output) \subseteq CandidatePairs
    /\ blockedMode \in Modes
    /\ work \in Nat
    /\ nodes \in Nat
    /\ proofFacts \in Nat
    /\ maxWork \in Nat
    /\ maxNodes \in Nat
    /\ maxProofFacts \in Nat
    /\ forcedStop \in ForcedStops

CpgImmutable == cpgDigest = "immutable-cpg"
LegacyCloneImmutable == legacyCloneDigest = "unchanged-type-1-2-3"

EqualityConfinement ==
    \A pair \in equalities :
        pair[1] # pair[2] =>
            /\ Admitted(pair[1])
            /\ Admitted(pair[2])
            /\ SameBoundary(pair[1], pair[2])

EvidenceSound == proofPairs \subseteq equalities

PositiveHasReplayableProof ==
    \A pair \in CandidatePairs :
        QueryOutcome(pair[1], pair[2]) = "Equivalent" =>
            /\ Equivalent(pair[1], pair[2])
            /\ Replayable(pair[1], pair[2])
            /\ provenanceComplete

NegativeRequiresCompleteSaturation ==
    \A pair \in CandidatePairs :
        QueryOutcome(pair[1], pair[2]) = "NotEquivalent" =>
            /\ status = "Saturated"
            /\ provenanceComplete
            /\ ~Equivalent(pair[1], pair[2])

LimitedNeverReportsAbsence ==
    status \in {"WorkLimited", "NodeLimited", "MatchLimited", "Cancelled",
                 "Malformed", "ProofLimited"} =>
        \A pair \in CandidatePairs :
            QueryOutcome(pair[1], pair[2]) # "NotEquivalent"

UnsupportedNeverReportsAbsence ==
    \A pair \in CandidatePairs :
        (~Admitted(pair[1]) \/ ~Admitted(pair[2]) \/
         ~SameBoundary(pair[1], pair[2])) =>
            QueryOutcome(pair[1], pair[2]) = "Unsupported"

OutputContainsOnlyProvedPairs == SeqRange(output) \subseteq PositivePairs
OutputHasNoDuplicates == Len(output) = Cardinality(SeqRange(output))
OutputIsCanonical == \A pair \in SeqRange(output) : pair[1] < pair[2]
OutputIsStrictlySorted ==
    \A left, right \in DOMAIN output :
        left < right => PairRank(output[left]) < PairRank(output[right])

CapConservation ==
    /\ work <= maxWork
    /\ nodes <= maxNodes
    /\ proofFacts <= maxProofFacts

=============================================================================
