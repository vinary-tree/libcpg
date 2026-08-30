#!/usr/bin/env bash
set -euo pipefail

repository_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repository_root"

mkdir -p target
build_dir=$(mktemp -d "$repository_root/target/formal-semantic-expression-flow.XXXXXX")
cleanup() {
    find "$build_dir" -depth -delete
}
trap cleanup EXIT

tla2sany formal/tla/SemanticExpressionFlow.tla
tlc -cleanup -workers 1 \
    -metadir "$build_dir/tla" \
    -config formal/tla/SemanticExpressionFlow.cfg \
    formal/tla/SemanticExpressionFlow.tla

mkdir -p "$build_dir/rocq"
cp formal/rocq/SemanticExpressionFlow.v "$build_dir/rocq/"
coqc -q \
    -Q "$build_dir/rocq" LibcpgFormal \
    "$build_dir/rocq/SemanticExpressionFlow.v"

z3_output=$(z3 formal/z3/semantic_expression_flow.smt2)
printf '%s\n' "$z3_output"
if [[ $(rg -c '^unsat$' <<<"$z3_output") -ne 24 ]]; then
    echo 'Z3 did not refute every forbidden semantic-expression-flow state' >&2
    exit 1
fi
if rg -q '^(sat|unknown)$' <<<"$z3_output"; then
    echo 'Z3 found or could not decide a forbidden semantic-expression-flow state' >&2
    exit 1
fi

printf 'semantic-expression-flow formal verification passed (TLA+, Rocq, Z3)\n'
