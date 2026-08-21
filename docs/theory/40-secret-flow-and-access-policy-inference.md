# Theory 40 — Secret flow and access-policy inference

This chapter defines libcpg's represented relations for embedded-secret flow
and missing authentication/authorization guards. **Authentication (authN)**
establishes an identity. **Authorization (authZ)** decides whether that identity
may perform an operation. A **secret candidate** is a typed string literal that
meets a configured heuristic; it is not necessarily a live credential.

## 1. Typed graph and policies

Let $`G=(V,E,\lambda_V,\lambda_E)`$ be a finite Code Property Graph (CPG). A
secret policy is $`\mathcal S=(R,H,n_0,h_0)`$, where $`R`$ contains positional
sink rules, $`H`$ contains typed binding-name patterns, $`n_0`$ is the minimum
byte length, and $`h_0`$ is the minimum entropy threshold.

An access policy is $`\mathcal A=(M,Q,\tau,N,\rho)`$, where $`M`$ recognizes
state-mutating APIs, $`Q`$ recognizes access-guard APIs, $`\tau`$ is an exact
minimum support fraction, $`N`$ is the minimum population, and $`\rho`$ selects
whole-program or call-neighborhood peers.

## 2. Shannon-entropy qualification

For string bytes $`s`$, let $`B(s)`$ be the byte values occurring in $`s`$ and
$`p_b`$ the relative frequency of byte $`b`$. Shannon entropy per byte is:

```math
H(s)=-\sum_{b\in B(s)}p_b\log_2 p_b
```

The implementation stores $`\operatorname{round}(1000H(s))`$ as an integer in
$`[0,8000]`$. Floating point is confined to this measurement; ordering,
threshold comparison, evidence, and serialization use integers.

A literal $`l`$ with bytes $`s_l`$ passes the entropy branch when:

```math
Q_H(l)=(|s_l|\ge n_0)\land(1000H(s_l)\ge h_0)
```

Entropy estimates unpredictability of the observed byte distribution. It does
not establish secrecy, validity, randomness of generation, or current service
acceptance.

## 3. Typed binding-name qualification

The structural context machine walks from a literal toward its binding
boundary. It examines typed node names on the parent chain, the left-hand
subtree of a containing assignment, and immediate DFG predecessors/successors.
It stops at function/module/root boundaries and at an explicit node cap.

Let $`C(l)`$ be those represented context nodes and $`name(v)`$ a typed name.
The name branch is:

```math
Q_N(l)=\exists v\in C(l),q\in H:q(name(v))
```

Overall qualification is $`Q(l)=Q_H(l)\lor Q_N(l)`$. The finding records only
the matched policy pattern and context node, never $`name(v)`$ or $`s_l`$.

```text
QUALIFY-LITERAL(cpg, literal, policy, cap):
    entropy := SHANNON-MILLIBITS(typed_string(literal))
    if length and entropy cross policy thresholds:
        return HIGH_ENTROPY(length, entropy)

    current := literal
    visited := empty ordered set
    while current stays inside one binding context:
        consume one explicit budget unit
        if typed_name(current) matches a nonempty hint:
            return NAME_HINT(hint, current)
        if current enters an assignment:
            scan its left-hand subtree with a heap queue
        current := typed AST parent(current)

    inspect immediate typed DFG neighbors
    return NOT_QUALIFIED or CAPPED
```

The continuation state is data in a loop or `VecDeque`, so native stack depth
is constant with respect to $`|C(l)|`$.

## 4. Secret source-to-sink relation

All string literals enter a detector-local open taint class. Sink rule
$`r=(name,family,P,J)`$ applies when a typed callee path matches some $`p\in P`$
and either $`J`$ is absent or the zero-based argument $`j\in J`$.

Let $`l\leadsto_G(c,j)`$ mean the shared IFDS-style taint engine produced a
canonical typed witness from literal $`l`$ to argument $`j`$ of call $`c`$.
The finding relation is:

```math
F_S=\{(l,c,j,r,w)\mid Q(l)\land r(c,j)\land l\leadsto_G(c,j)\}
```

The built-in families are authentication, network transmission, and
cryptographic key consumption. Qualification alone and sink matching alone are
insufficient; the concrete flow witness is required.

## 5. Exact missing-guard population

For mutation site $`c`$, let $`api(c)`$ be its exact `ApiIdentity`, $`P(c)`$ its
configured comparable peer population, and $`checks(c)`$ its normalized flat
guard fingerprints. A fingerprint abstracts local identifier spellings while
retaining operators, literals, members, and called API identities.

For fingerprint $`q`$, exact support is:

```math
\operatorname{support}(q,c)
=
\frac{|\{p\in P(c)\mid q\in checks(p)\}|}{|P(c)|}
```

No floating-point comparison is used. With threshold $`\tau=a/b`$, support
$`x/y`$ is accepted by cross multiplication:

```math
xb\ge ay
```

Let $`access(q)`$ mean a typed call/member/attribute/macro retained by $`q`$
matches an authN/authZ guard pattern. The access finding relation is:

```math
F_A=\{(c,q)\mid c\in M\land |P(c)|\ge N
\land q\notin checks(c)\land access(q)
\land \operatorname{support}(q,c)\ge\tau\}
```

Thus a common quota or null check is not relabeled as authorization merely
because peers use it.

## 6. Determinism, complexity, and interpretation

Literals and calls use source-range/`NodeId` order. Sets and maps are ordered;
fingerprints are flat canonical tapes; findings use stable structural keys.

If $`L`$ literals each inspect at most $`K`$ context nodes, detector-local J3
work is $`O(LK\log K+|F_T|\log|F_T|)`$ beyond delegated taint cost, where
$`F_T`$ is the taint-flow set. J4 adds a linear scan over normalized fingerprint
tapes beyond the missing-check engine. Heap space is bounded by the retained
qualification/finding maps and the largest explicit context queue.

Neither detector produces a probability, exploitability judgment, or proof of
access control. An empty result supports a represented-policy no-candidate
statement only when `is_complete()` is true.

See [ADR-0050](../design/0050-composed-secret-and-access-policy-analysis.md),
the [component contract](../components/graph/security-policy-analysis.md), and
[Security 15](../security/15-secret-and-access-policy-analysis.md).

## References

1. Shannon, C. E. (1948). *A Mathematical Theory of Communication.* Part I DOI:
   [10.1002/j.1538-7305.1948.tb01338.x](https://doi.org/10.1002/j.1538-7305.1948.tb01338.x);
   Part II DOI:
   [10.1002/j.1538-7305.1948.tb00917.x](https://doi.org/10.1002/j.1538-7305.1948.tb00917.x).
2. Meli, M., McNiece, M. R., Reaves, B. (2019). *How Bad Can It Git?
   Characterizing Secret Leakage in Public GitHub Repositories.* DOI:
   [10.14722/ndss.2019.23418](https://doi.org/10.14722/ndss.2019.23418).
3. Yamaguchi, F., Wressnegger, C., Gascon, H., Rieck, K. (2013). *Chucky:
   Exposing Missing Checks in Source Code for Vulnerability Discovery.* DOI:
   [10.1145/2508859.2516665](https://doi.org/10.1145/2508859.2516665).
4. Yamaguchi, F., Golde, N., Arp, D., Rieck, K. (2014). *Modeling and
   Discovering Vulnerabilities with Code Property Graphs.* DOI:
   [10.1109/SP.2014.44](https://doi.org/10.1109/SP.2014.44).
