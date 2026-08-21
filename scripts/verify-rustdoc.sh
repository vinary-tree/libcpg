#!/usr/bin/env bash
set -euo pipefail

repository_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repository_root"

export RUSTDOCFLAGS="${RUSTDOCFLAGS:+$RUSTDOCFLAGS }-D warnings"

echo '==> rustdoc-default'
cargo doc --no-deps

echo '==> rustdoc-all-features'
cargo doc --all-features --no-deps

echo '==> doctest-default'
cargo test --doc

echo '==> doctest-all-features'
cargo test --all-features --doc

echo 'default and all-feature rustdoc/doctest gates passed with warnings denied'
