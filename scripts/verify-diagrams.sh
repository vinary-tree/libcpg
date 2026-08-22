#!/usr/bin/env bash
set -euo pipefail

repository_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
verification_root="$repository_root/target/verification"
mkdir -p "$verification_root"
render_root=$(mktemp -d "$verification_root/diagram-repro.XXXXXX")

cleanup() {
    case "$render_root" in
        "$verification_root"/diagram-repro.*) rm -rf -- "$render_root" ;;
        *)
            echo "refusing to clean unexpected diagram directory: $render_root" >&2
            return 1
            ;;
    esac
}
trap cleanup EXIT

cd "$repository_root"

for command_name in plantuml dot cmp; do
    if ! command -v "$command_name" >/dev/null 2>&1; then
        echo "required diagram command is unavailable: $command_name" >&2
        exit 1
    fi
done

mapfile -t sources < <(
    rg --files docs/diagrams \
        | rg '\.(puml|dot)$' \
        | sort
)
mapfile -t committed < <(
    rg --files docs/diagrams \
        | rg '\.svg$' \
        | sort
)

if (( ${#sources[@]} == 0 )); then
    echo 'no diagram sources found' >&2
    exit 1
fi
if (( ${#sources[@]} != ${#committed[@]} )); then
    printf 'diagram source/SVG count mismatch: %d sources, %d SVGs\n' \
        "${#sources[@]}" "${#committed[@]}" >&2
    exit 1
fi

render_pass() {
    local pass=$1
    local pass_root="$render_root/$pass"
    mkdir -p "$pass_root"
    cp docs/diagrams/*.puml docs/diagrams/*.dot \
        docs/diagrams/_libcpg-theme.iuml "$pass_root/"

    (
        cd "$pass_root"
        # PlantUML shares renderer-global id generators between worker
        # threads. Serial rendering prevents cross-diagram scheduling from
        # changing otherwise equivalent SVG ids between the two passes.
        env JAVA_TOOL_OPTIONS=-Djava.awt.headless=true DISPLAY= \
            plantuml --threads 1 --svg --disable-metadata ./*.puml
        local source
        for source in ./*.dot; do
            dot -Tsvg "$source" -o "${source%.dot}.svg"
        done
    )
}

render_pass first
render_pass second

for source in "${sources[@]}"; do
    base=${source##*/}
    stem=${base%.*}
    first="$render_root/first/$stem.svg"
    second="$render_root/second/$stem.svg"
    checked_in="docs/diagrams/$stem.svg"

    if [[ ! -s "$first" || ! -s "$second" || ! -s "$checked_in" ]]; then
        echo "missing rendered diagram for source: $source" >&2
        exit 1
    fi
    if ! cmp -s "$first" "$second"; then
        echo "diagram render is not byte-reproducible: $source" >&2
        exit 1
    fi
    if ! cmp -s "$first" "$checked_in"; then
        echo "committed SVG is stale; render source again: $source" >&2
        exit 1
    fi
done

printf '%d diagram sources rendered headlessly twice with byte-identical committed SVGs\n' \
    "${#sources[@]}"
