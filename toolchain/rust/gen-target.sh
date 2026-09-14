#!/usr/bin/env bash
# gen-target.sh: regenerate x86_64-knc-linux-musl.json from the installed
# rustc's own x86_64-unknown-linux-musl spec, so field names and types always
# match the compiler in use, then apply the card-specific edits. See gen-target.md.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
out="$here/x86_64-knc-linux-musl.json"
# Every feature on the Knights Corner deletion list (docs/research/isa-deletions.md),
# plus nopl (multi-byte NOP, unverified on KNC) and cx16; x87 stays on.
features="-mmx,-sse,-sse2,-sse3,-ssse3,-sse4.1,-sse4.2,-sse4a,-avx,-avx2,-fma,-f16c,-cmov,-nopl,-movbe,-popcnt,-lzcnt,-bmi,-bmi2,-xsave,-cx16,-prfchw,-clflushopt,-rdrnd,-rdseed,-adx,-fsgsbase,-pclmul,-aes,-sha,+x87"
RUSTC_BOOTSTRAP=1 rustc -Zunstable-options --print target-spec-json --target x86_64-unknown-linux-musl \
    | jq --arg f "$features" '.vendor = "knc" | .features = $f | .cpu = "x86-64" | ."max-atomic-width" = 64 | ."crt-static-default" = true' \
    > "$out.tmp"
mv "$out.tmp" "$out"
echo "wrote $out from $(rustc --version)"
grep -E '"(vendor|features|cpu|max-atomic-width|target-pointer-width)"' "$out"
