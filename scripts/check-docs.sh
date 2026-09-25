#!/usr/bin/env bash
# check-docs.sh: enforce repository documentation rules.
#   1. Every code file has a sibling Markdown file with the same stem.
#   2. No em dash (U+2014) or en dash (U+2013) anywhere in tracked text.
#   3. Every relative link in a Markdown file points at a file that exists.
# Exit code 1 on any violation. The same script in every repository of the
# family (Intel-Phi-3120A, Intel-Phi-AVX512, Intel-Phi-Jev, Mechanical-Jev);
# only `code_dirs` differs. See check-docs.md.
set -uo pipefail
cd "$(dirname "$0")/.."
fail=0

# The directories (or files) that hold code, in this repository.
code_dirs=(host/crates card toolchain tools scripts)

# Rule 1: sibling docs. A code directory that does not exist is an error,
# so a stale entry cannot pass silently.
for d in "${code_dirs[@]}"; do
    if [ ! -e "$d" ]; then
        echo "check-docs.sh: code_dirs names $d, which does not exist"
        fail=1
    fi
done
while IFS= read -r f; do
    stem="${f%.*}"
    if [ ! -f "$stem.md" ]; then
        echo "missing sibling doc: $stem.md (for $f)"
        fail=1
    fi
done < <(find "${code_dirs[@]}" -type f \
    \( -name '*.rs' -o -name '*.c' -o -name '*.h' -o -name '*.S' -o -name '*.sh' -o -name '*.json' -o -name '*.config' \) \
    -not -path '*/target/*' -not -path '*/build/*' -not -path '*/vendor/*' -not -path '*/tokenizers/*' 2>/dev/null)

# Rule 2: no em/en dashes. Search everything git tracks or would track.
if git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    files=$(git ls-files --cached --others --exclude-standard)
else
    files=$(find . -type f -not -path './.git/*' -not -path '*/vendor/*' -not -path '*/target/*' | sed 's|^\./||')
fi
if [ -n "$files" ]; then
    hits=$(echo "$files" | xargs -d '\n' grep -nP '[\x{2013}\x{2014}]' 2>/dev/null || true)
    if [ -n "$hits" ]; then
        echo "em/en dash found:"
        echo "$hits"
        fail=1
    fi
fi

# Rule 3: every relative Markdown link points at a file that exists. Targets
# come from `](target)` and `](target#fragment)`; URLs are skipped.
while IFS= read -r f; do
    [ -n "$f" ] || continue
    dir=$(dirname "$f")
    for target in $(grep -oP '\]\(\K[^)#[:space:]]+(?=[#)])' "$f" 2>/dev/null | grep -vE '^[a-z]+:' || true); do
        if [ ! -e "$dir/$target" ]; then
            echo "broken link in $f: $target"
            fail=1
        fi
    done
done < <(echo "$files" | grep '\.md$')

if [ $fail -eq 0 ]; then echo "check-docs.sh: ok"; fi
exit $fail
