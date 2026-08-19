#!/usr/bin/env bash
set -euo pipefail

repository_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
verification_root="$repository_root/target/verification"
mkdir -p "$verification_root"
log_root=$(mktemp -d "$verification_root/run.XXXXXX")
trap 'rm -rf "$log_root"' EXIT
cd "$repository_root"

run_gate() {
    local name=$1
    shift
    echo "==> $name"
    if ! "$@" >"$log_root/$name.log" 2>&1; then
        cat "$log_root/$name.log" >&2
        return 1
    fi
    tail -n 12 "$log_root/$name.log"
}

run_gate fmt cargo fmt --check
run_gate test-default cargo test
run_gate test-maximal cargo test --features 'full rholang metta'
run_gate test-all-features cargo test --all-features
run_gate test-examples cargo test --all-features --examples
run_gate clippy cargo clippy --all-targets --all-features -- -D warnings
run_gate rustdoc env RUSTDOCFLAGS=-Dwarnings cargo doc --all-features --no-deps
run_gate docs ./scripts/verify-docs.sh
run_gate diff-check git diff --check

echo "all libcpg verification gates passed"
