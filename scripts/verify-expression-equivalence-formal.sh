#!/usr/bin/env bash
set -euo pipefail

repository_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repository_root"

mkdir -p target
build_dir=$(mktemp -d "$repository_root/target/formal-expression-equivalence.XXXXXX")
cleanup() {
    find "$build_dir" -depth -delete
}
trap cleanup EXIT

tla2sany formal/tla/ExpressionEquivalence.tla
tlc -cleanup -workers 1 \
    -metadir "$build_dir/tla" \
    -config formal/tla/ExpressionEquivalence.cfg \
    formal/tla/ExpressionEquivalence.tla

mkdir -p "$build_dir/rocq"
cp formal/rocq/ExpressionEquivalence.v "$build_dir/rocq/"
coqc -q \
    -Q "$build_dir/rocq" LibcpgFormal \
    "$build_dir/rocq/ExpressionEquivalence.v"

z3_output=$(z3 formal/z3/expression_equivalence.smt2)
printf '%s\n' "$z3_output"
if [[ $(grep -c '^unsat$' <<<"$z3_output") -ne 12 ]]; then
    echo 'Z3 did not refute every forbidden expression-equivalence state' >&2
    exit 1
fi
if grep -Eq '^(sat|unknown)$' <<<"$z3_output"; then
    echo 'Z3 found or could not decide a forbidden expression-equivalence state' >&2
    exit 1
fi

printf 'expression-equivalence formal verification passed (TLA+, Rocq, Z3)\n'
