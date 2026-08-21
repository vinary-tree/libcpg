#!/usr/bin/env bash
set -euo pipefail

repository_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repository_root"

resolve=false
case "${1:-}" in
    '') ;;
    --resolve) resolve=true ;;
    *)
        echo 'usage: scripts/verify-dois.sh [--resolve]' >&2
        exit 2
        ;;
esac

mapfile -t doi_urls < <(
    rg --no-filename --only-matching \
        'https://doi\.org/10\.[0-9]{4,9}/[^[:space:]<>]+' \
        README.md docs --glob '*.md' \
        | sed -E 's/[),.;:]+$//' \
        | sort -u
)

if (( ${#doi_urls[@]} == 0 )); then
    echo 'documentation contains no DOI links' >&2
    exit 1
fi

if rg -n 'http://(dx\.)?doi\.org|https://dx\.doi\.org' \
    README.md docs --glob '*.md'; then
    echo 'DOI links must use the canonical https://doi.org/ resolver' >&2
    exit 1
fi

if [[ "$resolve" == false ]]; then
    printf '%d unique canonical DOI links passed syntax policy\n' "${#doi_urls[@]}"
    exit 0
fi

failed=0
for url in "${doi_urls[@]}"; do
    status=$(curl --silent --show-error --output /dev/null \
        --write-out '%{http_code}' --max-redirs 0 \
        --user-agent 'libcpg-documentation-audit/1.0' "$url")
    case "$status" in
        200|301|302|303|307|308) ;;
        *)
            printf '%s %s\n' "$status" "$url" >&2
            failed=1
            ;;
    esac
done

if (( failed != 0 )); then
    echo 'one or more DOI identifiers did not resolve' >&2
    exit 1
fi

printf '%d unique DOI identifiers resolved through doi.org\n' "${#doi_urls[@]}"
