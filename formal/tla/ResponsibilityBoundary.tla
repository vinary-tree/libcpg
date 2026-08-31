---- MODULE ResponsibilityBoundary ----
EXTENDS Naturals, TLC

(***************************************************************************
A bounded executable contract for the libcpg / Datalog / Replete boundary.
Positive semantic facts may cross into a caller-owned Datalog EDB only after
proof replay.  A tuple can never flow backward into equality authority.
Negative conclusions require both the equality search and the relational
fixed point to be complete.  The adapter copies canonical stable identities;
it shares no mutable engine or CPG state.
***************************************************************************)

Phases == {"Ready", "Done"}
Completions == {"Unreported", "Complete", "Limited"}

VARIABLES phase, replayedProof, equalityComplete, datalogComplete,
          stableSourceIdentity, canonicalInput, sharedMutableStateRequested,
          reverseDependencyRequested, embeddedRuntimeRequested,
          tuplePromotedToProof, internalClassIdRequested, factPublished,
          absenceAllowed, completion, architectureAccepted, canonicalOutput,
          internalClassIdEscaped, cpgDigest, reportDigest

vars == <<phase, replayedProof, equalityComplete, datalogComplete,
          stableSourceIdentity, canonicalInput, sharedMutableStateRequested,
          reverseDependencyRequested, embeddedRuntimeRequested,
          tuplePromotedToProof, internalClassIdRequested, factPublished,
          absenceAllowed, completion, architectureAccepted, canonicalOutput,
          internalClassIdEscaped, cpgDigest, reportDigest>>

ArchitectureSafe ==
    ~sharedMutableStateRequested /\
    ~reverseDependencyRequested /\
    ~embeddedRuntimeRequested /\
    ~tuplePromotedToProof /\
    ~internalClassIdRequested

ExpectedFact ==
    ArchitectureSafe /\ replayedProof /\ stableSourceIdentity /\ canonicalInput

ExpectedAbsence == ArchitectureSafe /\ equalityComplete /\ datalogComplete

ExpectedCompletion ==
    IF equalityComplete /\ datalogComplete THEN "Complete" ELSE "Limited"

Init ==
    /\ phase = "Ready"
    /\ replayedProof \in BOOLEAN
    /\ equalityComplete \in BOOLEAN
    /\ datalogComplete \in BOOLEAN
    /\ stableSourceIdentity \in BOOLEAN
    /\ canonicalInput \in BOOLEAN
    /\ sharedMutableStateRequested \in BOOLEAN
    /\ reverseDependencyRequested \in BOOLEAN
    /\ embeddedRuntimeRequested \in BOOLEAN
    /\ tuplePromotedToProof \in BOOLEAN
    /\ internalClassIdRequested \in BOOLEAN
    /\ factPublished = FALSE
    /\ absenceAllowed = FALSE
    /\ completion = "Unreported"
    /\ architectureAccepted = FALSE
    /\ canonicalOutput = FALSE
    /\ internalClassIdEscaped = FALSE
    /\ cpgDigest = "immutable-cpg"
    /\ reportDigest = "immutable-semantic-report"

Export ==
    /\ phase = "Ready"
    /\ phase' = "Done"
    /\ architectureAccepted' = ArchitectureSafe
    /\ factPublished' = ExpectedFact
    /\ absenceAllowed' = ExpectedAbsence
    /\ completion' = ExpectedCompletion
    /\ canonicalOutput' = ExpectedFact
    /\ internalClassIdEscaped' = FALSE
    /\ UNCHANGED <<replayedProof, equalityComplete, datalogComplete,
                    stableSourceIdentity, canonicalInput,
                    sharedMutableStateRequested, reverseDependencyRequested,
                    embeddedRuntimeRequested, tuplePromotedToProof,
                    internalClassIdRequested, cpgDigest, reportDigest>>

Next == Export
Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ phase \in Phases
    /\ replayedProof \in BOOLEAN
    /\ equalityComplete \in BOOLEAN
    /\ datalogComplete \in BOOLEAN
    /\ stableSourceIdentity \in BOOLEAN
    /\ canonicalInput \in BOOLEAN
    /\ sharedMutableStateRequested \in BOOLEAN
    /\ reverseDependencyRequested \in BOOLEAN
    /\ embeddedRuntimeRequested \in BOOLEAN
    /\ tuplePromotedToProof \in BOOLEAN
    /\ internalClassIdRequested \in BOOLEAN
    /\ factPublished \in BOOLEAN
    /\ absenceAllowed \in BOOLEAN
    /\ completion \in Completions
    /\ architectureAccepted \in BOOLEAN
    /\ canonicalOutput \in BOOLEAN
    /\ internalClassIdEscaped \in BOOLEAN

CpgImmutable == cpgDigest = "immutable-cpg"
ReportImmutable == reportDigest = "immutable-semantic-report"
NoInternalClassIdEscapes == ~internalClassIdEscaped

PublishedFactRequiresProof == factPublished => replayedProof
PublishedFactRequiresStableIdentity == factPublished => stableSourceIdentity
PublishedFactIsCanonical == factPublished => canonicalOutput /\ canonicalInput
PublishedFactRequiresSafeArchitecture == factPublished => ArchitectureSafe

ProofPositiveSurvivesLimits ==
    phase = "Done" /\ ArchitectureSafe /\ replayedProof /\
    stableSourceIdentity /\ canonicalInput => factPublished

AbsenceRequiresEqualityCompletion == absenceAllowed => equalityComplete
AbsenceRequiresDatalogCompletion == absenceAllowed => datalogComplete
AbsenceRequiresSafeArchitecture == absenceAllowed => ArchitectureSafe

CompletionIsHonest ==
    phase = "Done" =>
      (completion = "Complete" <=> equalityComplete /\ datalogComplete)

AcceptedHasNoSharedMutableState ==
    architectureAccepted => ~sharedMutableStateRequested
AcceptedHasAcyclicDependencies ==
    architectureAccepted => ~reverseDependencyRequested
AcceptedHasNoEmbeddedRuntime ==
    architectureAccepted => ~embeddedRuntimeRequested
AcceptedDoesNotPromoteTuplesToProofs ==
    architectureAccepted => ~tuplePromotedToProof
AcceptedHidesInternalClassIds ==
    architectureAccepted => ~internalClassIdRequested

=============================================================================
