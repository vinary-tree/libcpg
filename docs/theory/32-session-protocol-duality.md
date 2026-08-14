# Theory 32 — Advisory session-protocol duality

A **session type** describes the order and payload types of communications on
one channel endpoint. **Duality** means that the two endpoints agree step by
step: whenever one sends, the other receives, and both describe the same
payload type. Honda, Vasconcelos, and Kubo established this behavioral typing
view for structured communication [[1]](#references); Caires and Pfenning later
connected binary session fidelity with propositions-as-types [[2]](#references).

libcpg implements a deliberately smaller object: an immutable **session
sketch** inferred from normalized Code Property Graph (CPG) evidence. It is an
advisory conformance check, not a type system, operational semantics, or
deadlock-freedom proof.

![Typed events become endpoint traces and a three-valued advisory result](../diagrams/session-duality.svg)

## 1. Evidence vocabulary

The `rholang` feature enables the analysis because the current frontend with
behavioral message metadata is Rholang Mode B. A channel identity is the
`Variable` node reached from an event-local channel identifier through its
definition, reaching-definition, or reference evidence. No source spelling is
compared.

For channel $`c`$ and top-level process $`p`$, the ordered endpoint trace is:

```math
\sigma(c,p)=a_0\,a_1\cdots a_{n-1}.
```

Each action is either a send $`!\vec{\tau}`$ or receive
$`?\vec{\tau}`$, where $`\vec{\tau}`$ is the source-ordered vector of shallow
payload types. Literal nodes produce categories such as `Integer`, `String`,
and `Array`. Explicit `TypeInfo` produces a normalized primitive category or a
named type. A payload expression or untyped binder that the CPG cannot classify
produces `Unknown`; uncertainty is retained rather than guessed away.

`Send` and `SendSync` have send direction. `Recv`, `RecvPersistent`, and `Peek`
have receive direction, but only `Recv` is linear. Persistent receives remain
available and peeks do not consume, so ordinary linear binary duality is not a
sound model for either.

## 2. Binary linear duality

Suppose exactly two processes $`p`$ and $`q`$ use $`c`$. Their sketches are
proven conformant only when their action counts are equal and every position
satisfies all three clauses:

```math
|\sigma(c,p)|=|\sigma(c,q)|
\quad\land\quad
\forall i:\;
d_i^p\ne d_i^q
\;\land\;
\vec{\tau}_i^p=\vec{\tau}_i^q
\;\land\;
\operatorname{linear}(a_i^p,a_i^q).
```

Here $`d_i^p`$ is the direction at position $`i`$ and
$`\operatorname{linear}`$ excludes persistent and peeking receives. Equality
of payload vectors includes arity and position: `(Int, String)` differs from
`(String, Int)`.

The result lattice has three user-facing values:

| Result | Meaning |
|---|---|
| `Conformant` | exactly two linear traces are opposite and have equal known payload types |
| `Nonconformant` | evidence proves a length, direction, arity, or known-type mismatch |
| `Inconclusive` | endpoint count, receive mode, or payload typing is insufficient |

A concrete mismatch dominates uncertainty. For example, two sends with unknown
payloads are nonconformant because the direction conflict is already proven.

## 3. Literate algorithm

The implementation tells the evidence story in the same order a reviewer would
audit it:

```text
Extract normalized channel events for each top-level process.
For each resolved send or receive event:
    Retain its ChannelId, process, MsgKind, site, and source range.
    Walk only the event-local AST with a cycle guard.
    Exclude the resolved channel operand and nested event/function boundaries.
    Infer shallow payload types; preserve missing evidence as Unknown.
Group actions by ChannelId and process in ordered maps.
For each channel:
    If the process count is not two, return Inconclusive with evidence.
    Compare the two action vectors position by position.
    Record every concrete mismatch and every uncertainty.
    Return Nonconformant if any concrete mismatch exists;
    otherwise return Inconclusive if uncertainty exists;
    otherwise return Conformant.
```

The output is out of graph. It cannot become stale-looking source topology and
cannot affect later AST, CFG, or DFG traversals.

## 4. Determinism and complexity

Let $`E_c`$ be the number of channel events and $`T_e`$ the number of AST nodes
visited beneath event $`e`$. Ordered maps and stable site sorting yield
deterministic results. The analysis costs:

```math
O\!\left(\sum_e T_e + E_c\log E_c\right)
```

time and $`O(E_c+T_{\max})`$ transient/output space, where $`T_{\max}`$ is the
largest event-local traversal. Cycle guards terminate malformed AST overlays.
All reported process and site identifiers come from the input CPG.

## 5. Research horizon, not implemented claims

The 2025 research horizon is richer than this checker. Mordido and Pérez extend
context-free session types with priorities for deadlock freedom, recursion, and
polymorphism [[3]](#references). Jaramillo and Pérez compare priority-based and
linear-logic-based deadlock-free session-process disciplines [[4]](#references).
libcpg cites these developments to state the gap clearly: it does not infer
recursive/context-free protocols, priorities, polymorphism, global types,
progress, or deadlock freedom. Those require an operational and type-theoretic
model beyond ordered shallow CPG sketches.

## References

1. Honda, K., Vasconcelos, V. T., Kubo, M. (1998). *Language Primitives and
   Type Discipline for Structured Communication-Based Programming.* ESOP 1998.
   DOI: [10.1007/BFb0053567](https://doi.org/10.1007/BFb0053567).
2. Caires, L., Pfenning, F. (2010). *Session Types as Intuitionistic Linear
   Propositions.* CONCUR 2010. DOI:
   [10.1007/978-3-642-15375-4_16](https://doi.org/10.1007/978-3-642-15375-4_16).
3. Mordido, A., Pérez, J. A. (2025). *Deadlock-free Context-free Session
   Types.* DOI:
   [10.48550/arXiv.2506.20356](https://doi.org/10.48550/arXiv.2506.20356).
4. Jaramillo, J. C., Pérez, J. A. (2025). *Contrasting Deadlock-Free Session
   Processes.* ECOOP 2025. DOI:
   [10.4230/LIPIcs.ECOOP.2025.17](https://doi.org/10.4230/LIPIcs.ECOOP.2025.17).
