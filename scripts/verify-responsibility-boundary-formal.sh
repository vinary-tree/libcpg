#!/usr/bin/env bash
set -euo pipefail

repository_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repository_root"

mkdir -p target
build_dir=$(mktemp -d "$repository_root/target/formal-responsibility-boundary.XXXXXX")
cleanup() {
    find "$build_dir" -depth -delete
}
trap cleanup EXIT

tla2sany formal/tla/ResponsibilityBoundary.tla
tlc -cleanup -workers 1 \
    -metadir "$build_dir/tla" \
    -config formal/tla/ResponsibilityBoundary.cfg \
    formal/tla/ResponsibilityBoundary.tla

mkdir -p "$build_dir/rocq"
cp formal/rocq/ResponsibilityBoundary.v "$build_dir/rocq/"
coqc -q \
    -Q "$build_dir/rocq" LibcpgFormal \
    "$build_dir/rocq/ResponsibilityBoundary.v"

z3_output=$(z3 formal/z3/responsibility_boundary.smt2)
printf '%s\n' "$z3_output"
if [[ $(rg -c '^unsat$' <<<"$z3_output") -ne 20 ]]; then
    echo 'Z3 did not refute every forbidden responsibility-boundary state' >&2
    exit 1
fi
if rg -q '^(sat|unknown)$' <<<"$z3_output"; then
    echo 'Z3 found or could not decide a forbidden responsibility-boundary state' >&2
    exit 1
fi

printf 'responsibility-boundary formal verification passed (TLA+, Rocq, Z3)\n'
