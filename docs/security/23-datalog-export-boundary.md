# Datalog export security boundary

The exporter accepts an already constructed CPG and returns memory buffers. It
does not parse source, execute a rule, invoke a subprocess, or write a path.
This narrows the trusted boundary but does not make arbitrary Datalog safe.

## Threats and mitigations

| Threat | libcpg mitigation | Caller responsibility |
|---|---|---|
| Symbol injection into `.facts` | Emit RFC 4180 tab-separated fields: quote every symbol and double internal quotes, so literal tabs/newlines remain inside the field. | Require `rfc4180=true` and `delimiter="\t"`; preserve bytes and never reinterpret rows through a shell. |
| Lost parallel edges | Retain identity and multiplicity in `cpg_edge`. | Use the master relation when multiplicity matters. |
| Nondeterministic evidence | Sort stable facts and deduplicate set relations. | Keep stable node/edge identities across revisions if byte comparison is required. |
| Stack exhaustion from deep CPGs | Use iterative scans, sorts, and render cursors; validate at 100,001 nodes on 256 KiB. | Bound graph construction before export. |
| Memory or disk exhaustion | Output grows linearly in represented nodes, edges, labels, and rendered bytes; no hidden cache or file I/O. | Apply CPG-size, output-byte, filesystem-quota, engine-time, and engine-memory limits. |
| Malicious or divergent rules | No rule parser/evaluator exists in libcpg. | Validate caller-owned rules and sandbox/bound the external engine. |
| False negative from missing model evidence | Preserve only represented relations; do not fabricate calls/types/dependence. | Inspect frontend and resolver completeness before interpreting absence. |

The API is intentionally side-effect free. A consumer that writes
`SouffleFactFile::contents` should create a dedicated directory,
reject unexpected relation names, write atomically, and pass paths to the
Datalog engine without shell interpolation.

## Resource model

For $`n`$ nodes, $`m`$ edges, and
$`b`$ rendered bytes, owned exporter memory is
$`O(n+m+b)`$. There is no semantic result cap because truncating
an EDB could make a negative query appear proved. Callers that cannot admit the
complete CPG must reject the export before querying rather than silently
truncate it.

## References

1. Jordan, H., Scholz, B., Subotić, P. (2016). *Soufflé: On Synthesis of Program
   Analyzers.* CC. DOI:
   [10.1007/978-3-319-41540-6_23](https://doi.org/10.1007/978-3-319-41540-6_23).
