#!/usr/bin/env bash
set -euo pipefail

repository_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repository_root"

proof=formal/rocq/ImportOverlayEdgeIdentity.v
[[ -f "$proof" && ! -L "$proof" ]] || {
    printf 'missing or symlinked import-overlay proof: %s\n' "$proof" >&2
    exit 1
}

# Treat the named propositions as a closed, reviewed inventory.  In
# particular the legacy kind-only Exports case may not silently disappear.
expected=(
    adjacent_duplicate_refines_projection
    adjacent_kind_refines_projection
    append_if_new_kind_has_identity
    append_if_new_kind_idempotent
    append_if_new_has_identity
    append_if_new_idempotent
)
mapfile -t actual < <(rg '^(Theorem|Lemma) [A-Za-z_][A-Za-z_0-9]*' "$proof" | awk '{print $2}')
[[ "${actual[*]}" == "${expected[*]}" ]] || {
    printf 'import-overlay proposition inventory drifted\n' >&2
    printf 'expected: %s\nactual:   %s\n' "${expected[*]}" "${actual[*]}" >&2
    exit 1
}
if rg -n '(^|[^[:alnum:]_])(Axiom|Admitted|admit)([^[:alnum:]_]|$)' "$proof"; then
    printf 'import-overlay proof contains an unproved assumption\n' >&2
    exit 1
fi

mkdir -p target/verification
build_dir=$(mktemp -d "$repository_root/target/verification/import-overlay-formal.XXXXXXXX")
cleanup() {
    find "$build_dir" -depth -delete
}
trap cleanup EXIT

source_digest=$(sha256sum "$proof" | awk '{print $1}')
cp "$proof" "$build_dir/ImportOverlayEdgeIdentity.v"
cmp -s "$proof" "$build_dir/ImportOverlayEdgeIdentity.v" || {
    printf 'import-overlay proof changed during capture\n' >&2
    exit 1
}
coqc -q -Q "$build_dir" LibcpgFormal "$build_dir/ImportOverlayEdgeIdentity.v"
coqchk -silent -Q "$build_dir" LibcpgFormal LibcpgFormal.ImportOverlayEdgeIdentity
[[ "$(sha256sum "$proof" | awk '{print $1}')" == "$source_digest" ]] || {
    printf 'import-overlay proof changed during verification\n' >&2
    exit 1
}

printf 'import-overlay Rocq proof passed: 6 propositions, source sha256 %s\n' "$source_digest"
