#!/usr/bin/env bash
set -euo pipefail

repository_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repository_root"

mkdir -p target
build_dir=$(mktemp -d "$repository_root/target/formal-expression-ir.XXXXXX")
cleanup() {
    find "$build_dir" -depth -delete
}
trap cleanup EXIT

tla2sany formal/tla/ExpressionIr.tla
tlc -cleanup -workers 1 \
    -metadir "$build_dir/tla" \
    -config formal/tla/ExpressionIr.cfg \
    formal/tla/ExpressionIr.tla

mkdir -p "$build_dir/rocq"
cp formal/rocq/ExpressionIr.v "$build_dir/rocq/"
coqc -q \
    -Q "$build_dir/rocq" LibcpgFormal \
    "$build_dir/rocq/ExpressionIr.v"

z3_output=$(z3 formal/z3/expression_ir.smt2)
printf '%s\n' "$z3_output"
if [[ $(grep -c '^unsat$' <<<"$z3_output") -ne 6 ]]; then
    echo 'Z3 did not refute every forbidden rewrite-certificate state' >&2
    exit 1
fi
if grep -Eq '^(sat|unknown)$' <<<"$z3_output"; then
    echo 'Z3 found or could not decide a forbidden rewrite-certificate state' >&2
    exit 1
fi

printf 'expression IR formal verification passed (TLA+, Rocq, Z3)\n'
