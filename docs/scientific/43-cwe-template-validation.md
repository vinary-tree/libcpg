# Scientific validation 43 — Declarative CWE templates

This ledger makes the declarative Common Weakness Enumeration (CWE) analysis
falsifiable. An **oracle** is an independently stated expected result. A
**property-based test** generates many inputs and checks an invariant or
independent model. A **benchmark** measures performance only after its
correctness oracle passes.

## 1. Hypotheses

| ID | Falsifiable hypothesis |
|---|---|
| H1 | The shipped registry contains exactly CWE-22, 78, 79, 89, 190, 416, 476, 502, 787, and 798 with nonempty roles and structural alternatives. |
| H2 | An unneutralized represented source-to-SQL-sink flow produces exactly one CWE-89 candidate; inserting a recognized parameterizer removes it. |
| H3 | CWE-22, 78, 79, and 502 obey the same vulnerable-versus-neutralized relation under their own role catalogs. |
| H4 | CWE-190 reports an interval outside the exact represented fixed-width integer domain and does not report a contained interval. |
| H5 | Missing and contradictory integer types remain distinguishable incomplete-evidence states. |
| H6 | CWE-476 projects a maybe-null flow only at a typed dereference. |
| H7 | CWE-416 projects only represented post-free use events and retains the underlying typestate witness. |
| H8 | CWE-787 compares the represented index interval with the represented array length and does not report an in-range write. |
| H9 | CWE-798 retains typed literal provenance and credential role without copying the literal value into the candidate. |
| H10 | Invalid duplicate/malformed templates fail before analysis, and a cap-plus-one structural observation makes the report incomplete. |
| H11 | Template permutation and graph-relation insertion permutation do not change the public report. |
| H12 | A 100,000-level expression completes on a 64-KiB native stack. |
| H13 | The feature-free public API and the Rust Tree-sitter pipeline distinguish vulnerable and sanitized SQL flows without graph mutation. |
| H14 | Template/config/report serde round trips preserve exact equality. |

## 2. Example-based unit oracles

`src/analysis/security/cwe_tests.rs` contains twelve feature-free tests:

| Oracle family | Expected observation |
|---|---|
| registry | exact ten-number set and nonempty declarative roles/shapes |
| SQL injection | one direct candidate, zero parameterized candidates, unchanged graph size |
| command/path/XSS/deserialization | one unneutralized and zero neutralized candidates for each CWE |
| integer overflow | exact interval `[128,128]` exceeds `i8`; `[127,127]` does not |
| integer-type ambiguity | two represented types produce `AmbiguousIntegerType`, no candidate, incomplete report |
| null dereference | one maybe-null dereference with typed evidence |
| use after free | only the post-free event is projected; the clean protocol has none |
| out-of-bounds write | represented out-of-range index reports; in-range index does not |
| hard-coded credential | literal `NodeId` and role are retained; secret bytes are absent from formatted candidate evidence |
| invalid/capped requests | duplicate identity fails; structural cap records the first omitted observation |
| stack depth | depth 100,000 completes on a 64-KiB thread stack |

Under `serde`, a thirteenth test round-trips the complete registry,
configuration, and a qualified analysis through JSON.

## 3. Property-based determinism oracle

The property campaign generates 128 cases. Each case varies unrelated graph
relations and template order, then compares the complete report against the
canonical construction. Equality includes:

- evaluated template ordering;
- structural embeddings and dense mappings;
- candidates, evidence, scores, origins, and witnesses;
- delegated taint/null/typestate reports;
- issue ordering and completeness.

This is stronger than comparing only candidate counts. A hidden dependence on
edge insertion order, hash iteration, or solver visitation order changes the
report and fails the property.

## 4. Public integration oracles

`tests/cwe_templates_integration.rs` has two downstream-crate tests.

The feature-free fixture builds one direct and one parameterized SQL flow using
only public CPG APIs. Running all ten templates must yield exactly one CWE-89
candidate at the direct sink, every witness node must resolve in the input CPG,
the report must be complete, and node/edge counts must remain unchanged.

With `lang-rust`, a Rust source corpus is parsed by `TreeSitterCpgBuilder`. The
corpus contains `vulnerable` and `safe` functions; only the first may own a
CWE-89 candidate. This crosses source parsing, typed graph construction, DFG
evidence, structural matching, semantic taint, candidate projection, and public
report interpretation.

## 5. Small-stack oracle

The depth oracle constructs a 100,000-level unary expression below one
interval-backed arithmetic root and launches analysis on a thread whose native
stack is 64 KiB. It requires successful join and an empty candidate set.

The test falsifies native recursion in:

- CWE structural orchestration;
- containing-function ownership and AST-parent discovery;
- interval expression evaluation;
- report ordering and teardown on the tested path.

Passing does not prove a universal memory bound. It demonstrates that native
call depth is independent of this adversarial AST depth and that the work is
held in explicit heap structures.

## 6. Benchmark method and observation

`benches/cwe_templates.rs` builds corpora with 20 and 100 independent endpoints.
Each endpoint contains one represented request-body origin and one SQL sink.
Before Criterion times anything, the benchmark requires:

```math
\operatorname{complete}(R)=\mathrm{true}
```

and:

```math
|\{c\in R.\mathrm{candidates}\mid c.\mathrm{cwe}=89\}|=N
```

where $`N`$ is the endpoint population. It also requires the total candidate
count to equal $`N`$, guarding against unintended cross-template matches.

On the local verification host on 2026-08-20, an optimized run observed:

| Population | Median interval | Throughput interval |
|---:|---:|---:|
| 20 | 969.67–970.59 µs | 123.64–123.75 thousand represented nodes/s |
| 100 | 4.8616–4.8764 ms | 123.04–123.41 thousand represented nodes/s |

These are controlled observations, not universal service-level objectives. CPU,
toolchain, allocator, build flags, graph shape, and load affect the result. The
approximately stable throughput is consistent with linear scaling on this
fixture; it does not prove asymptotic complexity for arbitrary VF2 patterns.

## 7. Traceability matrix

| Hypothesis | Primary oracle |
|---|---|
| H1 | `shipped_registry_is_exactly_the_ten_required_cwes` |
| H2 | `cwe_89_sql_injection_and_sanitized_oracle` |
| H3 | `taint_backed_command_path_xss_and_deserialization_oracles` |
| H4 | `cwe_190_uses_interval_result_and_fixed_width_type` |
| H5 | `cwe_190_reports_contradictory_represented_integer_types` plus issue variants |
| H6 | `cwe_476_projects_maybe_null_dereference` |
| H7 | `cwe_416_projects_only_post_free_use_violation` |
| H8 | `cwe_787_compares_index_interval_with_represented_array_length` |
| H9 | `cwe_798_reports_literal_provenance_without_secret_value` |
| H10 | `invalid_templates_and_structural_caps_fail_explicitly` |
| H11 | `reports_are_deterministic_under_relation_and_template_permutation` |
| H12 | `depth_100_000_interval_template_is_safe_on_64_kib_stack` |
| H13 | both tests in `cwe_templates_integration.rs` |
| H14 | `templates_config_and_qualified_report_round_trip` |

## 8. Reproduction commands

```bash
cargo fmt --all -- --check
cargo test --no-default-features analysis::security::cwe::tests
cargo test --no-default-features --features serde \
  analysis::security::cwe::tests
cargo test --no-default-features --test cwe_templates_integration
cargo test --no-default-features --features lang-rust \
  --test cwe_templates_integration
cargo run --no-default-features --example cwe_templates
cargo clippy --no-default-features --all-targets -- -D warnings
cargo bench --no-default-features --bench cwe_templates
```

The repository verification script additionally checks rustdoc, snippets,
links, diagram source/render parity, features, and the broader regression
matrix. `pgmcp bug-gate` supplies the scoped defect gate after verification.

## 9. Limits of inference

The experiments validate implementation semantics over represented CPG
evidence. They do not establish real-world precision/recall across languages,
frameworks, or projects; exploitability; absence of unmodeled vulnerabilities;
or completeness of any frontend. The built-in name catalogs are transparent
starting policies and should be extended with project-specific tests.

Yamaguchi et al. motivate graph-shaped vulnerability modeling
[[1]](#references). MoCQ studies generated vulnerability-pattern languages
[[2]](#references), while LLMxCPG studies CPG-selected context for a different
learning-based pipeline [[3]](#references). Those works motivate the design
space but do not serve as correctness evidence for libcpg; the oracles above do.

See [ADR-0048](../design/0048-declarative-qualified-cwe-templates.md),
[Theory 38](../theory/38-declarative-cwe-query-semantics.md), and
[Security 13](../security/13-cwe-template-analysis.md).

## References

1. Yamaguchi, F., Golde, N., Arp, D., Rieck, K. (2014). *Modeling and
   Discovering Vulnerabilities with Code Property Graphs.* DOI:
   [10.1109/SP.2014.44](https://doi.org/10.1109/SP.2014.44).
2. Li, P., Yao, S., Korich, J. S., Luo, C., Yu, J., Cao, Y., Yang, J. (2025).
   *Neuro-symbolic Static Analysis with LLM-generated Vulnerability Patterns.*
   DOI: [10.48550/arXiv.2504.16057](https://doi.org/10.48550/arXiv.2504.16057).
3. Lekssays, A., Mouhcine, H., Tran, K., Yu, T., Khalil, I. (2025).
   *LLMxCPG: Context-Aware Vulnerability Detection Through Code Property
   Graph-Guided Large Language Models.* DOI:
   [10.48550/arXiv.2507.16585](https://doi.org/10.48550/arXiv.2507.16585).
