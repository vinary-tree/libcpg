---- MODULE ExpressionEquivalence ----
EXTENDS Naturals, Sequences, FiniteSets, TLC

(***************************************************************************
A bounded executable model of libcpg's exact expression-equivalence adapter.
It separates semantic admission, equality growth, proof availability, terminal
completion, and deterministic public ordering.  The model deliberately keeps
the CPG and legacy clone result as immutable observations.
***************************************************************************)

Candidates == 0..5
Modes == {"PureLive", "IdentityOnly", "Unknown", "Persisted"}
Statuses == {
    "Running", "Saturated", "WorkLimited", "NodeLimited", "MatchLimited",
    "IterationLimited", "Cancelled", "Malformed", "ProofLimited"
}
ForcedStops == {"None", "Match", "Cancel", "Malformed"}
Outcomes == {
    "Equivalent", "AlphaClone", "NotEquivalent", "Inconclusive", "Unsupported"
}

VARIABLES status, pending, equalities, proofPairs, output, work, nodes,
          matches, iterations, proofFacts, provenanceComplete, blockedMode,
          maxWork, maxNodes, maxMatches, maxIterations, maxProofFacts,
          forcedStop, cpgDigest, legacyCloneDigest

CanonicalPair(left, right) ==
    IF left <= right THEN <<left, right>> ELSE <<right, left>>

CandidatePairs ==
    {<<left, right>> \in Candidates \X Candidates : left < right}

IdentityPairs == {<<candidate, candidate>> : candidate \in Candidates}
ClosedEvaluationLawPair == <<0, 5>>
AlphaLawPair == <<0, 1>>
LawPairs == {ClosedEvaluationLawPair, AlphaLawPair}
CertifiedLawPair(pair) ==
    pair = ClosedEvaluationLawPair \/ pair = AlphaLawPair

ScopeOf(candidate) == IF candidate = 2 THEN 1 ELSE 0
ProfileOf(candidate) == IF candidate = 3 THEN 1 ELSE 0
ModeOf(candidate) == IF candidate = 4 THEN blockedMode ELSE "PureLive"
BindingNamespaceOf(candidate) == IF candidate = 1 THEN 1 ELSE 0
AlphaWitness(left, right) == CanonicalPair(left, right) = AlphaLawPair
Admitted(candidate) == ModeOf(candidate) = "PureLive"
SameBoundary(left, right) ==
    /\ ScopeOf(left) = ScopeOf(right)
    /\ ProfileOf(left) = ProfileOf(right)
SameBindingContext(left, right) ==
    BindingNamespaceOf(left) = BindingNamespaceOf(right)
SupportedPair(left, right) ==
    /\ Admitted(left)
    /\ Admitted(right)
    /\ SameBoundary(left, right)
    /\ (SameBindingContext(left, right) \/ AlphaWitness(left, right))

SeqRange(sequence) == {sequence[index] : index \in DOMAIN sequence}
PairRank(pair) == pair[1] * Cardinality(Candidates) + pair[2]

vars == <<status, pending, equalities, proofPairs, output, work, nodes,
          matches, iterations, proofFacts, provenanceComplete, blockedMode,
          maxWork, maxNodes, maxMatches, maxIterations, maxProofFacts,
          forcedStop, cpgDigest, legacyCloneDigest>>

Equivalent(left, right) == CanonicalPair(left, right) \in equalities
Replayable(left, right) == CanonicalPair(left, right) \in proofPairs

QueryOutcome(left, right) ==
    IF ~SupportedPair(left, right)
    THEN "Unsupported"
    ELSE IF Equivalent(left, right)
         THEN IF provenanceComplete /\ Replayable(left, right)
              THEN IF SameBindingContext(left, right)
                   THEN "Equivalent"
                   ELSE "AlphaClone"
              ELSE "Inconclusive"
         ELSE IF status = "Saturated" /\ provenanceComplete
              THEN "NotEquivalent"
              ELSE "Inconclusive"

PositivePairs ==
    {pair \in CandidatePairs :
        QueryOutcome(pair[1], pair[2]) \in {"Equivalent", "AlphaClone"}}

Init ==
    /\ status = "Running"
    /\ pending = LawPairs
    /\ equalities = IdentityPairs
    /\ proofPairs = IdentityPairs
    /\ output = <<>>
    /\ work = 0
    /\ nodes = Cardinality(Candidates)
    /\ matches = 0
    /\ iterations = 0
    /\ proofFacts = 0
    /\ provenanceComplete = TRUE
    /\ blockedMode \in Modes
    /\ maxWork \in 0..2
    /\ maxNodes \in Cardinality(Candidates)..(Cardinality(Candidates) + 2)
    /\ maxMatches \in 0..2
    /\ maxIterations \in 0..2
    /\ maxProofFacts \in 0..2
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
    /\ UNCHANGED <<equalities, proofPairs, output, work, nodes, matches,
                    iterations, proofFacts, provenanceComplete, blockedMode,
                    maxWork, maxNodes, maxMatches, maxIterations,
                    maxProofFacts, forcedStop, cpgDigest, legacyCloneDigest>>

StopWork ==
    /\ status = "Running"
    /\ forcedStop = "None"
    /\ pending # {}
    /\ work >= maxWork
    /\ status' = "WorkLimited"
    /\ pending' = {}
    /\ UNCHANGED <<equalities, proofPairs, output, work, nodes, matches,
                    iterations, proofFacts, provenanceComplete, blockedMode,
                    maxWork, maxNodes, maxMatches, maxIterations,
                    maxProofFacts, forcedStop, cpgDigest, legacyCloneDigest>>

StopNodes ==
    /\ status = "Running"
    /\ forcedStop = "None"
    /\ pending # {}
    /\ work < maxWork
    /\ nodes >= maxNodes
    /\ status' = "NodeLimited"
    /\ pending' = {}
    /\ UNCHANGED <<equalities, proofPairs, output, work, nodes, matches,
                    iterations, proofFacts, provenanceComplete, blockedMode,
                    maxWork, maxNodes, maxMatches, maxIterations,
                    maxProofFacts, forcedStop, cpgDigest, legacyCloneDigest>>

StopMatches ==
    /\ status = "Running"
    /\ forcedStop = "None"
    /\ pending # {}
    /\ work < maxWork
    /\ nodes < maxNodes
    /\ matches >= maxMatches
    /\ status' = "MatchLimited"
    /\ pending' = {}
    /\ UNCHANGED <<equalities, proofPairs, output, work, nodes, matches,
                    iterations, proofFacts, provenanceComplete, blockedMode,
                    maxWork, maxNodes, maxMatches, maxIterations,
                    maxProofFacts, forcedStop, cpgDigest, legacyCloneDigest>>

StopIterations ==
    /\ status = "Running"
    /\ forcedStop = "None"
    /\ pending # {}
    /\ work < maxWork
    /\ nodes < maxNodes
    /\ matches < maxMatches
    /\ iterations >= maxIterations
    /\ status' = "IterationLimited"
    /\ pending' = {}
    /\ UNCHANGED <<equalities, proofPairs, output, work, nodes, matches,
                    iterations, proofFacts, provenanceComplete, blockedMode,
                    maxWork, maxNodes, maxMatches, maxIterations,
                    maxProofFacts, forcedStop, cpgDigest, legacyCloneDigest>>

ApplyLaw ==
    /\ status = "Running"
    /\ forcedStop = "None"
    /\ work < maxWork
    /\ nodes < maxNodes
    /\ matches < maxMatches
    /\ iterations < maxIterations
    /\ \E lawPair \in pending :
        /\ pending' = pending \ {lawPair}
        /\ equalities' = equalities \cup {lawPair}
        /\ IF proofFacts < maxProofFacts
              THEN /\ proofPairs' = proofPairs \cup {lawPair}
                   /\ proofFacts' = proofFacts + 1
                   /\ provenanceComplete' = provenanceComplete
              ELSE /\ proofPairs' = proofPairs
                   /\ proofFacts' = proofFacts
                   /\ provenanceComplete' = FALSE
    /\ work' = work + 1
    /\ nodes' = nodes + 1
    /\ matches' = matches + 1
    /\ iterations' = iterations + 1
    /\ UNCHANGED <<status, output, blockedMode, maxWork, maxNodes,
                    maxMatches, maxIterations, maxProofFacts, forcedStop,
                    cpgDigest, legacyCloneDigest>>

Finish ==
    /\ status = "Running"
    /\ pending = {}
    /\ status' = IF provenanceComplete THEN "Saturated" ELSE "ProofLimited"
    /\ UNCHANGED <<pending, equalities, proofPairs, output, work, nodes,
                    matches, iterations, proofFacts, provenanceComplete,
                    blockedMode, maxWork, maxNodes, maxMatches, maxIterations,
                    maxProofFacts, forcedStop, cpgDigest, legacyCloneDigest>>

EmitNext ==
    LET remaining == PositivePairs \ SeqRange(output)
        next == CHOOSE pair \in remaining :
                    \A other \in remaining : PairRank(pair) <= PairRank(other)
    IN
    /\ status # "Running"
    /\ remaining # {}
    /\ output' = Append(output, next)
    /\ UNCHANGED <<status, pending, equalities, proofPairs, work, nodes,
                    matches, iterations, proofFacts, provenanceComplete,
                    blockedMode, maxWork, maxNodes, maxMatches, maxIterations,
                    maxProofFacts, forcedStop, cpgDigest, legacyCloneDigest>>

Next == StopForced \/ StopWork \/ StopNodes \/ StopMatches \/ StopIterations \/
        ApplyLaw \/ Finish \/ EmitNext
Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ status \in Statuses
    /\ pending \subseteq LawPairs
    /\ equalities \subseteq CandidatePairs \cup IdentityPairs
    /\ proofPairs \subseteq CandidatePairs \cup IdentityPairs
    /\ SeqRange(output) \subseteq CandidatePairs
    /\ blockedMode \in Modes
    /\ work \in Nat
    /\ nodes \in Nat
    /\ matches \in Nat
    /\ iterations \in Nat
    /\ proofFacts \in Nat
    /\ maxWork \in Nat
    /\ maxNodes \in Nat
    /\ maxMatches \in Nat
    /\ maxIterations \in Nat
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
            /\ (SameBindingContext(pair[1], pair[2]) \/
                AlphaWitness(pair[1], pair[2]))

EvidenceSound == proofPairs \subseteq equalities

ReplayEvidenceCertified ==
    \A pair \in proofPairs \ IdentityPairs : CertifiedLawPair(pair)

PositiveHasReplayableProof ==
    \A pair \in CandidatePairs :
        QueryOutcome(pair[1], pair[2]) \in {"Equivalent", "AlphaClone"} =>
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
    status \in {"WorkLimited", "NodeLimited", "MatchLimited",
                 "IterationLimited", "Cancelled", "Malformed", "ProofLimited"} =>
        \A pair \in CandidatePairs :
            QueryOutcome(pair[1], pair[2]) # "NotEquivalent"

UnsupportedNeverReportsAbsence ==
    \A pair \in CandidatePairs :
        ~SupportedPair(pair[1], pair[2]) =>
            QueryOutcome(pair[1], pair[2]) = "Unsupported"

ExactEqualityRetainsBindingContext ==
    \A pair \in CandidatePairs :
        QueryOutcome(pair[1], pair[2]) = "Equivalent" =>
            SameBindingContext(pair[1], pair[2])

AlphaCloneIsQualified ==
    \A pair \in CandidatePairs :
        QueryOutcome(pair[1], pair[2]) = "AlphaClone" =>
            /\ ~SameBindingContext(pair[1], pair[2])
            /\ AlphaWitness(pair[1], pair[2])

OutputContainsOnlyProvedPairs == SeqRange(output) \subseteq PositivePairs
OutputHasNoDuplicates == Len(output) = Cardinality(SeqRange(output))
OutputIsCanonical == \A pair \in SeqRange(output) : pair[1] < pair[2]
OutputIsStrictlySorted ==
    \A left, right \in DOMAIN output :
        left < right => PairRank(output[left]) < PairRank(output[right])

SaturatedIsFixedPoint == status = "Saturated" => pending = {}

CapConservation ==
    /\ work <= maxWork
    /\ nodes <= maxNodes
    /\ matches <= maxMatches
    /\ iterations <= maxIterations
    /\ proofFacts <= maxProofFacts

=============================================================================
