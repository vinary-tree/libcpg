# MeTTa grammar source provenance

This package contains generated tree-sitter C sources so it can be resolved and
built from an immutable Git revision without depending on any developer's
filesystem layout. It does not change the MeTTa grammar or the MeTTa compiler.

The snapshot was copied byte-for-byte from
`F1R3FLY-io/MeTTa-Compiler`, commit
`5b2635dc7510e90b0a7ea7a0666027a07de60d84`, directory
`tree-sitter-metta/src/`. The upstream repository remains the grammar's design
authority; this package is its version-agnostic distribution boundary.

## Integrity invariants

The following invariants define a faithful snapshot:

1. `parser.c`, `scanner.c`, and `tree_sitter/parser.h` match the recorded
   upstream commit byte-for-byte.
2. `cargo test -p tree-sitter-metta` loads the generated language and parses a
   representative MeTTa rule without an error node.
3. Every node-kind constant documented as present resolves in the compiled
   grammar, while compatibility-only constants remain absent.
4. Every consumer resolves this package from the same immutable libcpg Git
   revision, preventing duplicate `tree_sitter_metta` C symbols.

The recorded SHA-256 digests are:

| Path | SHA-256 |
|---|---|
| `src/parser.c` | `2726ea79bc44aa5dc287451b730addcb92cd0e4f016d1d873bc181836f00bc46` |
| `src/scanner.c` | `71e015389eff7441293cfdd06983f813196df37cf791a4ee1971107573b03248` |
| `src/tree_sitter/parser.h` | `180b893c8734778fd32f372dfbc27bd6ad1cd2221f26150b31256ff6716320d2` |

To audit an update, check out the recorded upstream commit and compare both
directories recursively before changing the commit and digest table. Then run
the package tests and every downstream integration gate. Do not regenerate or
edit generated C sources in this repository.
