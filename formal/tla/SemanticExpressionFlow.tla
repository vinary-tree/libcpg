---- MODULE SemanticExpressionFlow ----
EXTENDS Naturals, TLC

(***************************************************************************
A bounded executable contract for composing proof-producing expression
equivalence with available/very-busy dataflow.  Equality is evidence about
terms; availability and anticipability are path facts.  The model therefore
cannot publish PRE/CSE candidates from equality alone and cannot turn an
incomplete semantic-diff run into a negative conclusion.
***************************************************************************)

Phases == {"Ready", "Done"}
ReplayModes == {"None", "Exact", "AlphaInvalid", "AlphaBijective"}
Evaluations == {"Unevaluable", "Equal", "Different"}
DiffStatuses == {"Unreported", "ProvenEquivalent", "ProvenDifferent",
                  "Limited", "Unknown"}

VARIABLES phase, admittedBoth, pureBoth, sameProfile, sameContext,
          replayMode, provenanceComplete, saturationComplete,
          dataflowComplete, withinBudget, availableAllPaths,
          dominantProducer, killFree, busyAllPaths, placementDominates,
          multipleOccurrences, evaluation, cseCandidate, preCandidate,
          diffStatus, pairLeft, pairRight, cpgDigest

vars == <<phase, admittedBoth, pureBoth, sameProfile, sameContext,
          replayMode, provenanceComplete, saturationComplete,
          dataflowComplete, withinBudget, availableAllPaths,
          dominantProducer, killFree, busyAllPaths, placementDominates,
          multipleOccurrences, evaluation, cseCandidate, preCandidate,
          diffStatus, pairLeft, pairRight, cpgDigest>>

ExactProof ==
    /\ admittedBoth
    /\ pureBoth
    /\ sameProfile
    /\ sameContext
    /\ replayMode = "Exact"
    /\ provenanceComplete

AlphaProof ==
    /\ admittedBoth
    /\ pureBoth
    /\ sameProfile
    /\ ~sameContext
    /\ replayMode = "AlphaBijective"
    /\ provenanceComplete

AnyProof == ExactProof \/ AlphaProof
EvaluatorDifferent == evaluation = "Different"
EvidenceLimited ==
    ~withinBudget \/ ~saturationComplete \/ ~provenanceComplete \/
    ~dataflowComplete

ExpectedCse ==
    ExactProof /\ dataflowComplete /\ availableAllPaths /\
    dominantProducer /\ killFree

ExpectedPre ==
    ExactProof /\ dataflowComplete /\ multipleOccurrences /\
    busyAllPaths /\ placementDominates

ExpectedDiff ==
    IF AnyProof
    THEN "ProvenEquivalent"
    ELSE IF EvaluatorDifferent
         THEN "ProvenDifferent"
         ELSE IF EvidenceLimited THEN "Limited" ELSE "Unknown"

Init ==
    /\ phase = "Ready"
    /\ admittedBoth \in BOOLEAN
    /\ pureBoth \in BOOLEAN
    /\ sameProfile \in BOOLEAN
    /\ sameContext \in BOOLEAN
    /\ replayMode \in ReplayModes
    /\ provenanceComplete \in BOOLEAN
    /\ saturationComplete \in BOOLEAN
    /\ dataflowComplete \in BOOLEAN
    /\ withinBudget \in BOOLEAN
    /\ availableAllPaths \in BOOLEAN
    /\ dominantProducer \in BOOLEAN
    /\ killFree \in BOOLEAN
    /\ busyAllPaths \in BOOLEAN
    /\ placementDominates \in BOOLEAN
    /\ multipleOccurrences \in BOOLEAN
    /\ evaluation \in Evaluations
    /\ ~(AnyProof /\ EvaluatorDifferent)
    /\ cseCandidate = FALSE
    /\ preCandidate = FALSE
    /\ diffStatus = "Unreported"
    /\ pairLeft = 1
    /\ pairRight = 2
    /\ cpgDigest = "immutable-cpg"

Analyze ==
    /\ phase = "Ready"
    /\ phase' = "Done"
    /\ cseCandidate' = ExpectedCse
    /\ preCandidate' = ExpectedPre
    /\ diffStatus' = ExpectedDiff
    /\ UNCHANGED <<admittedBoth, pureBoth, sameProfile, sameContext,
                    replayMode, provenanceComplete, saturationComplete,
                    dataflowComplete, withinBudget, availableAllPaths,
                    dominantProducer, killFree, busyAllPaths,
                    placementDominates, multipleOccurrences, evaluation,
                    pairLeft, pairRight, cpgDigest>>

Next == Analyze
Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ phase \in Phases
    /\ admittedBoth \in BOOLEAN
    /\ pureBoth \in BOOLEAN
    /\ sameProfile \in BOOLEAN
    /\ sameContext \in BOOLEAN
    /\ replayMode \in ReplayModes
    /\ provenanceComplete \in BOOLEAN
    /\ saturationComplete \in BOOLEAN
    /\ dataflowComplete \in BOOLEAN
    /\ withinBudget \in BOOLEAN
    /\ availableAllPaths \in BOOLEAN
    /\ dominantProducer \in BOOLEAN
    /\ killFree \in BOOLEAN
    /\ busyAllPaths \in BOOLEAN
    /\ placementDominates \in BOOLEAN
    /\ multipleOccurrences \in BOOLEAN
    /\ evaluation \in Evaluations
    /\ cseCandidate \in BOOLEAN
    /\ preCandidate \in BOOLEAN
    /\ diffStatus \in DiffStatuses
    /\ pairLeft \in Nat
    /\ pairRight \in Nat

CpgImmutable == cpgDigest = "immutable-cpg"
ProofEvaluatorConsistent == ~(AnyProof /\ EvaluatorDifferent)

CseRequiresExactProof == phase = "Done" /\ cseCandidate => ExactProof
CseRequiresCompleteDataflow ==
    phase = "Done" /\ cseCandidate => dataflowComplete
CseRequiresAvailability ==
    phase = "Done" /\ cseCandidate => availableAllPaths
CseRequiresDominance ==
    phase = "Done" /\ cseCandidate => dominantProducer
CseRequiresKillFreedom == phase = "Done" /\ cseCandidate => killFree
CsePairIsCanonical ==
    phase = "Done" /\ cseCandidate => pairLeft < pairRight

PreRequiresExactProof == phase = "Done" /\ preCandidate => ExactProof
PreRequiresCompleteDataflow ==
    phase = "Done" /\ preCandidate => dataflowComplete
PreRequiresMultipleOccurrences ==
    phase = "Done" /\ preCandidate => multipleOccurrences
PreRequiresAnticipability ==
    phase = "Done" /\ preCandidate => busyAllPaths
PreRequiresDominance ==
    phase = "Done" /\ preCandidate => placementDominates

EquivalentRequiresProof ==
    diffStatus = "ProvenEquivalent" => AnyProof
DifferentRequiresEvaluatorWitness ==
    diffStatus = "ProvenDifferent" => EvaluatorDifferent
LimitedHasNoDecisiveWitness ==
    diffStatus = "Limited" => ~AnyProof /\ ~EvaluatorDifferent
LimitedRequiresIncompleteEvidence ==
    diffStatus = "Limited" => EvidenceLimited
UnknownHasNoDecisiveWitness ==
    diffStatus = "Unknown" => ~AnyProof /\ ~EvaluatorDifferent
UnknownRequiresCompleteEvidence ==
    diffStatus = "Unknown" => ~EvidenceLimited

=============================================================================
