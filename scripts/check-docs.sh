#!/usr/bin/env bash
# check-docs.sh: enforce repository documentation rules.
#   1. Every code file has a sibling Markdown file with the same stem.
#   2. No em dash (U+2014) or en dash (U+2013) anywhere in tracked text.
# Exit code 1 on any violation. See check-docs.md.
set -uo pipefail
cd "$(dirname "$0")/.."
fail=0

# Rule 1: sibling docs. Code file extensions and the directories that hold code.
code_dirs=(host/crates card toolchain tools scripts)
while IFS= read -r f; do
    stem="${f%.*}"
    if [ ! -f "$stem.md" ]; then
        echo "missing sibling doc: $stem.md (for $f)"
        fail=1
    fi
done < <(find "${code_dirs[@]}" -type f \
    \( -name '*.rs' -o -name '*.c' -o -name '*.h' -o -name '*.S' -o -name '*.sh' -o -name '*.json' -o -name '*.config' \) \
    -not -path '*/target/*' -not -path '*/build/*' -not -path '*/vendor/*' 2>/dev/null)

# Rule 2: no em/en dashes. Search everything git tracks or would track.
if git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    files=$(git ls-files --cached --others --exclude-standard)
else
    files=$(find . -type f -not -path './.git/*' -not -path './vendor/*' -not -path './host/target/*')
fi
if [ -n "$files" ]; then
    hits=$(echo "$files" | xargs -d '\n' grep -nP '[\x{2013}\x{2014}]' 2>/dev/null || true)
    if [ -n "$hits" ]; then
        echo "em/en dash found:"
        echo "$hits"
        fail=1
    fi
fi

if [ $fail -eq 0 ]; then echo "check-docs.sh: ok"; fi
exit $fail
