#!/usr/bin/env bash
set -euo pipefail

repository_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repository_root"

failed=0

if rg -n -F -e '</content>' -e '</invoke>' docs --glob '*.md'; then
    echo 'documentation contains leaked tool-protocol markup' >&2
    failed=1
fi

while IFS= read -r source; do
    rendered="${source%.*}.svg"
    if [[ ! -s "$rendered" ]]; then
        echo "missing rendered diagram: $rendered (source: $source)" >&2
        failed=1
    fi
done < <(find docs/diagrams -maxdepth 1 -type f \( -name '*.puml' -o -name '*.dot' \) -print | sort)

mapfile -t undersized_diagrams < <(
    find docs/diagrams -maxdepth 1 -type f -name '*.svg' -size -1024c -print | sort
)
if (( ${#undersized_diagrams[@]} > 0 )); then
    echo "rendered diagram smaller than 1 KiB" >&2
    printf '%s\n' "${undersized_diagrams[@]}" >&2
    failed=1
fi

if rg -l -F -e 'contains errors' -e 'syntax error' -e 'deprecated' -e 'Please use CSS' docs/diagrams/*.svg; then
    echo "diagram render contains an error or deprecation marker" >&2
    failed=1
fi

# GitHub math must use the invertible $`...`$ form. Perl lookarounds avoid
# treating compliant delimiters as bare dollar-math.
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
' $(find docs -type f -name '*.md' -print | sort); then
    :
else
    echo 'invalid Markdown math delimiter; use $`...`$ or a fenced math block' >&2
    failed=1
fi

if [[ "$failed" -ne 0 ]]; then
    exit 1
fi

echo "documentation structure, diagram, and math-delimiter checks passed"
