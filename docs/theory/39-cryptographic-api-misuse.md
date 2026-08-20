# Theory 39 — Cryptographic API-misuse analysis

This chapter defines libcpg's represented-domain semantics for cryptographic
API misuse. A **finite-state machine (FSM)** describes legal call sequences. A
**parameter rule** identifies meaningful argument positions. A **provenance
relation** identifies where their values came from. A finding is advisory
evidence over a Code Property Graph (CPG), not a runtime security verdict.

## 1. Policy domain

Let $`G=(V,E,\lambda_V,\lambda_E)`$ be a finite typed CPG. A cryptographic
policy is:

```math
\mathcal C=(A,H,M,N,K,R,W,B)
```

Here $`A`$ is a deterministic property automaton; $`H`$ is a finite set of
weak-hash callee predicates; $`M`$, $`N`$, and $`K`$ are positional mode,
IV/nonce, and key rules; $`R`$ recognizes secure-random calls; and $`W`$ and
$`B`$ are normalized weak-hash and forbidden-mode segment sets.

A positional rule $`r=(n,P,J)`$ has stable name $`n`$, callee predicates $`P`$,
and zero-based argument positions $`J`$. It applies to call $`c`$ and argument
$`j`$ when:

```math
\operatorname{applies}(r,c,j)
=
\left(\bigvee_{p\in P}p(\operatorname{callee}(c))\right)\land(j\in J)
```

## 2. Call-order language

The property automaton is $`A=(Q,\Sigma,\delta,q_e,Q_a,\alpha)`$, with states
$`Q`$, call-event alphabet $`\Sigma`$, partial transition function $`\delta`$,
absorbing error state $`q_e`$, accepting states $`Q_a`$, and allocation-event
map $`\alpha`$. A missing ordinary transition enters $`q_e`$.

The typestate engine associates each allocation site with an abstract object,
propagates possible states through the canonical per-function CFG, and uses DFG
and receiver evidence to bind later events. A forbidden call-order finding is:

```math
F_{\mathrm{order}}
=
\{(o,c,q,e,w)\mid \delta(q,e)=q_e\}
```

where $`w`$ is a canonical allocation-to-event witness.

## 3. Literal-origin taint for keys

A categorical literal source rule maps typed literal nodes into an open taint
class without inspecting their value. For the built-in key policy, strings and
array literals enter class `hardcoded-crypto-material`. Key parameter rules
compile to named taint sinks.

Let $`L`$ be those literal origins and $`K_r`$ the governed sink arguments for
rule $`r`$. The hardcoded-key relation is:

```math
F_{\mathrm{key}}
=
\{(l,c,j,w)\mid l\in L\land(c,j)\in K_r\land l\leadsto_{\mathrm{df}}(c,j)\}
```

The reachability relation includes represented AST, DFG, reaching-definition,
call, and summary transfers. The finding retains $`l`$ and $`w`$, not the
literal value.

## 4. Mode and hash constraints

String constraints split a typed string literal on non-alphanumeric boundaries
and compare lowercase segments. Consequently `AES/ECB/PKCS5Padding` contains
the forbidden segment `ecb`, while an unrelated identifier that merely embeds
those letters does not.

For argument $`a_j`$, let $`O(a_j)`$ be the finite set of literal origins found
by the explicit provenance machine and $`S(l)`$ the normalized segment set of
string literal $`l`$. Then:

```math
F_{\mathrm{ecb}}
=
\{(c,j,l)\mid \operatorname{applies}(r,c,j)
\land l\in O(a_j)\land S(l)\cap B\neq\varnothing\}
```

Weak-hash parameter evidence substitutes $`W`$ for $`B`$. Weak-hash callee
patterns additionally report direct MD5 and SHA-1 entry points.

## 5. Predictable IV and nonce evidence

Let $`C(a_j)`$ be call origins found for an IV or nonce argument, and let
$`R(c)`$ mean that a secure-random source pattern recognizes call $`c`$. The
built-in candidate relation is conservative but explicit:

```math
F_{\mathrm{iv}}
=
\{(c,j,O(a_j))\mid \operatorname{applies}(r,c,j)
\land O(a_j)\neq\varnothing
\land \neg\exists x\in C(a_j):R(x)\}
```

This relation says that fixed typed material reaches the governed position and
no represented secure-random origin discharges it. It does not estimate entropy
or prove nonce reuse across runtime executions.

## 6. Explicit provenance machine

The origin walk is a pushdown-style work machine whose continuation state lives
in `VecDeque`, not the native stack:

```text
TRACE-ORIGINS(cpg, root, cap):
    queue := [root]
    visited := empty ordered set
    literals := empty ordered set
    calls := empty ordered map

    while queue is not empty:
        node := pop-front(queue)
        if node was visited: continue
        if size(visited) = cap: return CAPPED(literals, calls)
        mark node visited

        if node is a typed literal: record node
        else if node is a call:
            reconstruct its typed callee path; record it
            enqueue call arguments in canonical order
        else if node is an identifier:
            enqueue its bound and reaching definitions
        else:
            enqueue its AST operands in canonical order

        enqueue incoming typed data-flow predecessors in canonical order

    return COMPLETE(literals, calls)
```

Each node is processed at most once per argument query. If $`V_q`$ and $`E_q`$
are the visited provenance subgraph, ordered-set operations give a conservative
bound of $`O((|V_q|+|E_q|)\log|V_q|)`$ time and $`O(|V_q|)`$ heap space. Native
call depth is constant with respect to $`|V_q|`$.

## 7. Determinism and qualification

Calls are ordered by source range and stable `NodeId`. Rules use ordered sets;
origins, issues, taint witnesses, and typestate witnesses are canonicalized.
Finding keys are `(site, kind, argument, rule)`. Repeated runs therefore return
the same result for the same represented graph and policy.

No finding is a probability. No empty finding vector is a clean claim unless
`CryptoAnalysis::is_complete()` is true. See [ADR-0049](../design/0049-composed-cryptographic-api-misuse.md),
the [component contract](../components/graph/crypto-misuse-analysis.md), and
[Security 14](../security/14-cryptographic-api-misuse.md).

## References

1. Krüger, S., Späth, J., Ali, K., Bodden, E., Mezini, M. (2019). *CrySL: An
   Extensible Approach to Validating the Correct Usage of Cryptographic APIs.*
   DOI: [10.1109/TSE.2019.2948910](https://doi.org/10.1109/TSE.2019.2948910).
2. Rahaman, S., Xiao, Y., Afrose, S., Shaon, F., Tian, K., Frantz, M.,
   Kantarcioglu, M., Yao, D. (2019). *CryptoGuard: High Precision Detection of
   Cryptographic Vulnerabilities in Massive-sized Java Projects.* DOI:
   [10.1145/3319535.3345659](https://doi.org/10.1145/3319535.3345659).
