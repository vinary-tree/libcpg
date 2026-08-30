---- MODULE TreeParentContext ----
EXTENDS Integers, Sequences, FiniteSets, TLC

(***************************************************************************
A bounded model of the preorder AST work machine carrying both the direct raw
tree-sitter parent and the nearest included CPG parent. Node 1 may be a
transparent syntax node, so the two contexts must remain distinct.
***************************************************************************)

Nodes == 0..3
Null == -1

ParentOf(node) ==
    CASE node = 0 -> Null
      [] node = 1 -> 0
      [] node \in {2, 3} -> 1

Children(node) ==
    CASE node = 0 -> <<1>>
      [] node = 1 -> <<2, 3>>
      [] OTHER -> <<>>

NearestIncludedParent(node, included) ==
    CASE node = 0 -> Null
      [] node = 1 -> IF 0 \in included THEN 0 ELSE Null
      [] node \in {2, 3} ->
           IF 1 \in included THEN 1
           ELSE IF 0 \in included THEN 0 ELSE Null

Frame(node, rawParent, effectiveParent) ==
    [node |-> node, raw |-> rawParent, effective |-> effectiveParent]

ChildFrames(node, effectiveParent) ==
    [index \in DOMAIN Children(node) |->
        Frame(Children(node)[index], node, effectiveParent)]

VARIABLES pending, visited, mapped, mappedParent, included

vars == <<pending, visited, mapped, mappedParent, included>>

Init ==
    /\ pending = <<Frame(0, Null, Null)>>
    /\ visited = <<>>
    /\ mapped = {}
    /\ mappedParent = [node \in Nodes |-> Null]
    /\ included \in SUBSET Nodes

Step ==
    /\ pending # <<>>
    /\ LET frame == Head(pending)
           childEffective == IF frame.node \in included
                             THEN frame.node ELSE frame.effective
       IN /\ pending' = ChildFrames(frame.node, childEffective)
                           \o Tail(pending)
          /\ visited' = Append(visited, frame.node)
          /\ mapped' = IF frame.node \in included
                         THEN mapped \cup {frame.node} ELSE mapped
          /\ mappedParent' = IF frame.node \in included
                               THEN [mappedParent EXCEPT
                                      ![frame.node] = frame.effective]
                               ELSE mappedParent
    /\ UNCHANGED included

Done ==
    /\ pending = <<>>
    /\ UNCHANGED vars

Next == Step \/ Done
Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ \A frame \in {pending[index] : index \in DOMAIN pending} :
          /\ frame.node \in Nodes
          /\ frame.raw \in Nodes \cup {Null}
          /\ frame.effective \in Nodes \cup {Null}
    /\ {visited[index] : index \in DOMAIN visited} \subseteq Nodes
    /\ mapped \subseteq Nodes
    /\ mappedParent \in [Nodes -> Nodes \cup {Null}]
    /\ included \subseteq Nodes

RawContextExact ==
    \A frame \in {pending[index] : index \in DOMAIN pending} :
        frame.raw = ParentOf(frame.node)

EffectiveContextExact ==
    \A frame \in {pending[index] : index \in DOMAIN pending} :
        frame.effective = NearestIncludedParent(frame.node, included)

MappedParentExact ==
    \A node \in mapped :
        mappedParent[node] = NearestIncludedParent(node, included)

VisitedOnce ==
    Len(visited) = Cardinality({visited[index] : index \in DOMAIN visited})

MappedOnlyVisitedIncluded ==
    mapped \subseteq included \cap {visited[index] : index \in DOMAIN visited}

TerminalVisitsAll == pending = <<>> =>
    {visited[index] : index \in DOMAIN visited} = Nodes

=============================================================================
