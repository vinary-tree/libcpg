# Scientific validation 44 — Cryptographic API misuse

This validation plan separates implementation correctness from external
validity. **Implementation correctness** asks whether libcpg computes its
documented represented relations. **External validity** asks whether those
relations predict real deployed misuse; that requires library- and
environment-specific evaluation beyond this component.

## 1. Hypotheses

| ID | Hypothesis | Oracle |
|---|---|---|
| H1 | direct MD5/SHA-1 calls are classified | hand-computed call-path fixtures |
| H2 | ECB algorithm segments are classified | typed string-origin fixture |
| H3 | fixed IV/nonce values are candidates | literal-origin fixture |
| H4 | secure-random nonce provenance suppresses the fixed-value candidate | paired AES-GCM fixture |
| H5 | literal key material follows typed data flow | taint source-to-positional-sink witness |
| H6 | forbidden call order follows the FSM | post-finalization use fixture |
| H7 | public results are deterministic | 128 generated cap selections and repeated runs |
| H8 | input depth does not consume native stack | 100,000-definition chain on 64 KiB |
| H9 | partial work cannot become a clean result | zero call/origin bounds and delegated cap tests |
| H10 | parser integration preserves semantics | Rust Tree-sitter corpus |

## 2. Example-based differential structure

Each misuse has a paired clean interpretation:

- `AES/ECB/PKCS5Padding` versus an authenticated mode;
- `MD5::digest` versus an unclassified strong-hash path;
- a literal IV versus `OsRng::generate_nonce`;
- a literal key versus nonliteral material;
- use after `finalize` versus use while the object remains ready.

The fixtures assert exact sites and evidence variants. They also snapshot graph
node/edge counts before analysis and confirm the CPG remains unchanged.

## 3. Property campaign

The current property test samples `max_calls` and
`max_origin_nodes_per_argument` across bounded nonzero ranges, runs the same
graph and policy twice, and requires structural equality of the full report.
This checks canonicalization across local findings, delegated reports, issues,
and cap qualification.

Future corpus properties should permute insertion of semantically set-valued
DFG relations while preserving source ranges. Ordered call arguments must not
be permuted because parameter position is semantic.

## 4. Stack-safety experiment

The small-stack oracle constructs a literal followed by 100,000 identifier
definitions. It invokes the provenance machine on a 64-KiB thread stack and
requires the unique literal origin with no cap. This depth is far beyond a safe
native recursive traversal and directly tests the claimed heap-worklist
architecture.

The experiment covers construction, query, equality of ordered evidence, and
drop on the small-stack thread. It does not use a larger stack, a worker-process
escape, or a semantic depth cutoff.

## 5. Integration levels

1. Feature-free unit tests isolate rule and evidence semantics.
2. Serde tests round-trip policy and aggregate reports.
3. Feature-free integration tests consume only public exports.
4. `lang-rust` integration builds a real CPG from source and exercises all four
   parameter/call misuse families plus a clean random nonce.
5. The executable example verifies the documented minimal workflow.
6. Repository gates cover formatting, strict Clippy, rustdoc, links, diagrams,
   default/all-feature builds, and source-policy checks.

## 6. Interpretation limits

Passing these tests supports conformance to the documented static relation. It
does not establish recall/precision across ecosystems, entropy quality, nonce
uniqueness, runtime path feasibility, or resistance to side channels. A future
external-validity study should use versioned real-world crypto-misuse corpora,
reviewed ground truth, per-library policy versions, and confidence intervals.

## References

1. Krüger, S., Späth, J., Ali, K., Bodden, E., Mezini, M. (2019). *CrySL: An
   Extensible Approach to Validating the Correct Usage of Cryptographic APIs.*
   DOI: [10.1109/TSE.2019.2948910](https://doi.org/10.1109/TSE.2019.2948910).
2. Rahaman, S., et al. (2019). *CryptoGuard: High Precision Detection of
   Cryptographic Vulnerabilities in Massive-sized Java Projects.* DOI:
   [10.1145/3319535.3345659](https://doi.org/10.1145/3319535.3345659).
