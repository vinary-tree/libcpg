#!/usr/bin/env bash
set -euo pipefail

repository_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repository_root"

failed=0

if rg -n -F -e '</content>' -e '</invoke>' docs --glob '*.md'; then
    echo 'documentation contains leaked tool-protocol markup' >&2
    failed=1
fi

mapfile -t diagram_sources < <(
    rg --files docs/diagrams \
        | rg '\.(puml|dot)$' \
        | sort
)
mapfile -t rendered_diagrams < <(
    rg --files docs/diagrams \
        | rg '\.svg$' \
        | sort
)

for source in "${diagram_sources[@]}"; do
    rendered="${source%.*}.svg"
    base=${source##*/}
    if [[ ! -s "$rendered" ]]; then
        echo "missing rendered diagram: $rendered (source: $source)" >&2
        failed=1
    fi
    if ! rg -q -F "\`$base\`" docs/diagrams/README.md; then
        echo "diagram source is absent from the catalog: $source" >&2
        failed=1
    fi
    if [[ "$source" == *.puml ]] \
        && ! rg -q -F '!include _libcpg-theme.iuml' "$source"; then
        echo "PlantUML source omits the shared theme: $source" >&2
        failed=1
    fi
    if ! rg -q '#[0-9A-Fa-f]{6}' "$source"; then
        echo "diagram source omits explicit semantic colour: $source" >&2
        failed=1
    fi
done

for rendered in "${rendered_diagrams[@]}"; do
    puml="${rendered%.svg}.puml"
    dot="${rendered%.svg}.dot"
    if [[ ! -s "$puml" && ! -s "$dot" ]]; then
        echo "rendered diagram has no source: $rendered" >&2
        failed=1
    fi
    if (( $(stat --format='%s' "$rendered") < 1024 )); then
        echo "rendered diagram smaller than 1 KiB: $rendered" >&2
        failed=1
    fi
done

if (( ${#diagram_sources[@]} != ${#rendered_diagrams[@]} )); then
    printf 'diagram source/SVG count mismatch: %d sources, %d SVGs\n' \
        "${#diagram_sources[@]}" "${#rendered_diagrams[@]}" >&2
    failed=1
fi

mapfile -t duplicate_catalog_entries < <(
    rg --no-filename --only-matching '\`[^`]+\.(puml|dot)\`' \
        docs/diagrams/README.md \
        | tr -d '\`' \
        | sort \
        | uniq -d
)
if (( ${#duplicate_catalog_entries[@]} > 0 )); then
    echo 'diagram catalog contains duplicate source entries' >&2
    printf '%s\n' "${duplicate_catalog_entries[@]}" >&2
    failed=1
fi

if rg -l -F -e 'contains errors' -e 'syntax error' -e 'deprecated' \
    -e 'Please use CSS' docs/diagrams/*.svg; then
    echo 'diagram render contains an error or deprecation marker' >&2
    failed=1
fi

# GitHub math must use the invertible $`...`$ form. Perl lookarounds avoid
# treating compliant delimiters as bare dollar-math.
mapfile -t markdown_files < <(rg --files docs --glob '*.md' | sort)
if perl -ne '
    if (/^\s*```/) {
        $fenced = !$fenced;
        next;
    }
    next if $fenced;
    $line = $_;
    $line =~ s/``.*?``//g;
    $line =~ s/\$`[^`]*`\$//g;
    $line =~ s/`[^`]*`//g;
    if ($line =~ /\$\$/ || $line =~ /(?<!\\)\$(?!\s)[^\$\n]+?(?<!\\)\$/) {
        print "$ARGV:$.:$_";
        $bad = 1;
    }
    END { exit($bad ? 1 : 0) }
' "${markdown_files[@]}"; then
    :
else
    echo 'invalid Markdown math delimiter; use $`...`$ or a fenced math block' >&2
    failed=1
fi

stale_claims=(
    'reserved `Throw`/`Catch` nodes'
    'reserved exception evidence'
    'reserved exception topology'
    'reserved scope relations'
    'emits ten of the fourteen'
)
for claim in "${stale_claims[@]}"; do
    if rg -n -F "$claim" README.md docs --glob '*.md'; then
        echo "documentation contains stale implemented-surface claim: $claim" >&2
        failed=1
    fi
done

if ! ./scripts/verify-dois.sh; then
    failed=1
fi

if ! lychee --offline --include-fragments --no-progress README.md docs; then
    echo 'documentation contains a broken local link or fragment' >&2
    failed=1
fi

if ! ./scripts/verify-diagrams.sh; then
    failed=1
fi

if (( failed != 0 )); then
    exit 1
fi

printf 'documentation structure, %d diagrams, DOI syntax, links, anchors, and math delimiters passed\n' \
    "${#diagram_sources[@]}"
