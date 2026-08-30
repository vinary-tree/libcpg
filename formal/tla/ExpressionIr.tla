---- MODULE ExpressionIr ----
EXTENDS Naturals, Sequences, FiniteSets, TLC

(***************************************************************************
A bounded executable model of the libcpg expression-fact lowering machine.
The model varies semantic completeness, rewrite permission, node modes, graph
shape, and exact work/node limits across initial states.
***************************************************************************)

Facts == 0..2
Root == 2
Modes == {"Pure", "IdentityOnly", "Unknown"}
Shapes == {"Tree", "Shared", "SelfCycle"}
Statuses == {"Running", "Complete", "WorkLimited", "NodeLimited", "Malformed"}
Phases == {"Enter", "Exit"}
Provenances == {"LiveLowering", "PersistedIdentityOnly"}

ChildrenOf(shapeValue, fact) ==
    CASE shapeValue = "Tree" ->
             CASE fact = 2 -> <<0, 1>> [] OTHER -> <<>>
      [] shapeValue = "Shared" ->
             CASE fact = 2 -> <<1, 1>>
               [] fact = 1 -> <<0>>
               [] OTHER -> <<>>
      [] shapeValue = "SelfCycle" ->
             CASE fact = 2 -> <<2>> [] OTHER -> <<>>

SeqRange(sequence) == {sequence[index] : index \in DOMAIN sequence}
EnterFrame(fact) == [fact |-> fact, phase |-> "Enter"]
ExitFrame(fact) == [fact |-> fact, phase |-> "Exit"]
EnterFrames(children) ==
    [index \in DOMAIN children |-> EnterFrame(children[index])]

VARIABLES status, pending, active, tape, safe, certified, work,
          profileComplete, lawAllowed, mode, shape, maxWork, maxNodes,
          provenance

vars == <<status, pending, active, tape, safe, certified, work,
          profileComplete, lawAllowed, mode, shape, maxWork, maxNodes,
          provenance>>

Children(fact) == ChildrenOf(shape, fact)
Mapped == SeqRange(tape)
Positions(fact) == {index \in DOMAIN tape : tape[index] = fact}
Position(fact) == CHOOSE index \in Positions(fact) : TRUE
Top == Head(pending)
FrameType(frame) == /\ frame.fact \in Facts
                    /\ frame.phase \in Phases
ChildrenSafe(fact) == \A child \in SeqRange(Children(fact)) : child \in safe
NodeSafe(fact) == /\ profileComplete
                  /\ mode[fact] = "Pure"
                  /\ ChildrenSafe(fact)
CanRewrite == /\ status = "Complete"
              /\ provenance = "LiveLowering"
              /\ Root \in certified

Init ==
    /\ status = "Running"
    /\ pending = <<EnterFrame(Root)>>
    /\ active = {}
    /\ tape = <<>>
    /\ safe = {}
    /\ certified = {}
    /\ work = 0
    /\ profileComplete \in BOOLEAN
    /\ lawAllowed \in BOOLEAN
    /\ mode \in [Facts -> Modes]
    /\ shape \in Shapes
    /\ maxWork \in 0..8
    /\ maxNodes \in 0..Cardinality(Facts)
    /\ provenance = "LiveLowering"

CompleteLowering ==
    /\ status = "Running"
    /\ pending = <<>>
    /\ status' = "Complete"
    /\ UNCHANGED <<pending, active, tape, safe, certified, work,
                    profileComplete, lawAllowed, mode, shape, maxWork, maxNodes,
                    provenance>>

StopForWorkLimit ==
    /\ status = "Running"
    /\ pending # <<>>
    /\ work >= maxWork
    /\ status' = "WorkLimited"
    /\ pending' = <<>>
    /\ active' = {}
    /\ UNCHANGED <<tape, safe, certified, work, profileComplete,
                    lawAllowed, mode, shape, maxWork, maxNodes, provenance>>

RejectCycle ==
    /\ status = "Running"
    /\ pending # <<>>
    /\ work < maxWork
    /\ Top.phase = "Enter"
    /\ Top.fact \in active
    /\ status' = "Malformed"
    /\ pending' = <<>>
    /\ active' = {}
    /\ work' = work + 1
    /\ UNCHANGED <<tape, safe, certified, profileComplete,
                    lawAllowed, mode, shape, maxWork, maxNodes, provenance>>

ReuseMapped ==
    /\ status = "Running"
    /\ pending # <<>>
    /\ work < maxWork
    /\ Top.phase = "Enter"
    /\ Top.fact \notin active
    /\ Top.fact \in Mapped
    /\ pending' = Tail(pending)
    /\ work' = work + 1
    /\ UNCHANGED <<status, active, tape, safe, certified,
                    profileComplete, lawAllowed, mode, shape, maxWork, maxNodes,
                    provenance>>

EnterFresh ==
    /\ status = "Running"
    /\ pending # <<>>
    /\ work < maxWork
    /\ Top.phase = "Enter"
    /\ Top.fact \notin active
    /\ Top.fact \notin Mapped
    /\ pending' = EnterFrames(Children(Top.fact))
                   \o <<ExitFrame(Top.fact)>>
                   \o Tail(pending)
    /\ active' = active \cup {Top.fact}
    /\ work' = work + 1
    /\ UNCHANGED <<status, tape, safe, certified, profileComplete,
                    lawAllowed, mode, shape, maxWork, maxNodes, provenance>>

StopForNodeLimit ==
    /\ status = "Running"
    /\ pending # <<>>
    /\ work < maxWork
    /\ Top.phase = "Exit"
    /\ Len(tape) >= maxNodes
    /\ status' = "NodeLimited"
    /\ pending' = <<>>
    /\ active' = {}
    /\ UNCHANGED <<tape, safe, certified, work, profileComplete,
                    lawAllowed, mode, shape, maxWork, maxNodes, provenance>>

ExitFresh ==
    LET fact == Top.fact IN
    /\ status = "Running"
    /\ pending # <<>>
    /\ work < maxWork
    /\ Top.phase = "Exit"
    /\ Len(tape) < maxNodes
    /\ fact \in active
    /\ fact \notin Mapped
    /\ \A child \in SeqRange(Children(fact)) : child \in Mapped
    /\ pending' = Tail(pending)
    /\ active' = active \ {fact}
    /\ tape' = Append(tape, fact)
    /\ safe' = IF NodeSafe(fact) THEN safe \cup {fact} ELSE safe
    /\ certified' = IF NodeSafe(fact) /\ lawAllowed
                      THEN certified \cup {fact}
                      ELSE certified
    /\ work' = work + 1
    /\ UNCHANGED <<status, profileComplete, lawAllowed, mode, shape,
                    maxWork, maxNodes, provenance>>

PersistArtifact ==
    /\ status # "Running"
    /\ provenance = "LiveLowering"
    /\ provenance' = "PersistedIdentityOnly"
    /\ UNCHANGED <<status, pending, active, tape, safe, certified, work,
                    profileComplete, lawAllowed, mode, shape, maxWork, maxNodes>>

Next == CompleteLowering
     \/ StopForWorkLimit
     \/ RejectCycle
     \/ ReuseMapped
     \/ EnterFresh
     \/ StopForNodeLimit
     \/ ExitFresh
     \/ PersistArtifact

Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ status \in Statuses
    /\ \A frame \in SeqRange(pending) : FrameType(frame)
    /\ active \subseteq Facts
    /\ SeqRange(tape) \subseteq Facts
    /\ safe \subseteq Facts
    /\ certified \subseteq Facts
    /\ work \in Nat
    /\ profileComplete \in BOOLEAN
    /\ lawAllowed \in BOOLEAN
    /\ mode \in [Facts -> Modes]
    /\ shape \in Shapes
    /\ maxWork \in Nat
    /\ maxNodes \in Nat
    /\ provenance \in Provenances

MapMatchesTape ==
    Len(tape) = Cardinality(Mapped)

BackwardReferences ==
    \A fact \in Mapped :
        \A child \in SeqRange(Children(fact)) :
            /\ child \in Mapped
            /\ Position(child) < Position(fact)

SafeEvidenceSound ==
    \A fact \in safe :
        /\ fact \in Mapped
        /\ profileComplete
        /\ mode[fact] = "Pure"
        /\ \A child \in SeqRange(Children(fact)) : child \in safe

CertificateSound ==
    /\ certified \subseteq safe
    /\ certified # {} => lawAllowed

PublicRewriteSound ==
    CanRewrite =>
        /\ status = "Complete"
        /\ provenance = "LiveLowering"
        /\ profileComplete
        /\ lawAllowed
        /\ Root \in safe
        /\ mode[Root] = "Pure"

IncompleteDeniesRewrite == status # "Complete" => ~CanRewrite
IdentityDeniesRewrite == mode[Root] # "Pure" => ~CanRewrite
PersistedDeniesRewrite == provenance = "PersistedIdentityOnly" => ~CanRewrite
CompleteHasRoot == status = "Complete" => Root \in Mapped
TerminalQuiescent == status # "Running" => /\ pending = <<>> /\ active = {}
WorkLimitHonest == status = "WorkLimited" => work >= maxWork
NodeLimitHonest == status = "NodeLimited" => Len(tape) >= maxNodes

=============================================================================
