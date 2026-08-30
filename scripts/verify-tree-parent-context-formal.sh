#!/usr/bin/env bash
set -euo pipefail

repository_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repository_root"

mkdir -p target
build_dir=$(mktemp -d "$repository_root/target/formal-tree-parent.XXXXXX")
cleanup() {
    find "$build_dir" -depth -delete
}
trap cleanup EXIT

tla2sany formal/tla/TreeParentContext.tla
tlc -cleanup -workers 1 \
    -metadir "$build_dir/tla" \
    -config formal/tla/TreeParentContext.cfg \
    formal/tla/TreeParentContext.tla

mkdir -p "$build_dir/rocq"
cp formal/rocq/TreeParentContext.v "$build_dir/rocq/"
coqc -q \
    -Q "$build_dir/rocq" LibcpgFormal \
    "$build_dir/rocq/TreeParentContext.v"

z3_output=$(z3 formal/z3/tree_parent_context.smt2)
printf '%s\n' "$z3_output"
if [[ $(grep -c '^unsat$' <<<"$z3_output") -ne 4 ]]; then
    echo 'Z3 did not refute every forbidden parent-context state' >&2
    exit 1
fi
if grep -Eq '^(sat|unknown)$' <<<"$z3_output"; then
    echo 'Z3 found or could not decide a forbidden parent-context state' >&2
    exit 1
fi

printf 'tree parent-context formal verification passed (TLA+, Rocq, Z3)\n'
