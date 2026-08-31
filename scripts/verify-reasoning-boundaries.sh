#!/usr/bin/env bash
set -euo pipefail

repository_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
verification_root="$repository_root/target/verification"
mkdir -p "$verification_root"
scratch=$(mktemp -d "$verification_root/reasoning-boundaries.XXXXXX")
trap 'rm -rf "$scratch"' EXIT
cd "$repository_root"

fail_if_present() {
    local pattern=$1
    local path=$2
    local message=$3
    if rg -n "$pattern" "$path" >"$scratch/matches"; then
        cat "$scratch/matches" >&2
        echo "$message" >&2
        return 1
    fi
}

cargo tree -p libcpg --edges normal --prefix none >"$scratch/libcpg-tree"
cargo tree -p replete --edges normal --prefix none >"$scratch/replete-tree"

if ! rg -q '^replete v0\.1\.0 ' "$scratch/libcpg-tree"; then
    echo "libcpg must have exactly one visible Replete dependency" >&2
    exit 1
fi
if [[ $(rg -c '^replete v0\.1\.0 ' "$scratch/libcpg-tree") -ne 1 ]]; then
    echo "libcpg dependency graph contains more than one Replete package" >&2
    exit 1
fi

fail_if_present '^(egg|egglog|souffle)([ -]|$)' "$scratch/libcpg-tree" \
    "libcpg must not embed a second equality or Datalog runtime"
fail_if_present '^libcpg ' "$scratch/replete-tree" \
    "Replete must remain independent from libcpg"
fail_if_present 'replete::|ExpressionEquivalence|SemanticExpression' \
    src/analysis/datalog.rs \
    "the ordinary Datalog exporter must not depend on semantic equality"
fail_if_present 'DatalogFacts|SouffleFactFile|export_semantic_datalog' \
    src/analysis/expression_equivalence.rs \
    "the equality engine must not depend on Datalog export"
fail_if_present 'use replete|replete::|CodePropertyGraph|CpgNode|CpgEdge' \
    src/analysis/semantic_datalog.rs \
    "the bridge must consume owned report evidence, not engine or CPG internals"
fail_if_present 'Arc<|Rc<|Mutex<|RwLock<|RefCell<|UnsafeCell<|static mut' \
    src/analysis/semantic_datalog.rs \
    "the bridge must not introduce shared mutable reasoning state"

echo "reasoning-engine dependency and source boundaries passed"
