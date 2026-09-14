#!/usr/bin/env bash
# gen-target.sh: regenerate x86_64-knc-linux-musl.json from the installed
# rustc's own x86_64-unknown-linux-musl spec, so field names and types always
# match the compiler in use, then apply the card-specific edits. See gen-target.md.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
out="$here/x86_64-knc-linux-musl.json"
# The Knights Corner deletion list (docs/research/isa-deletions.md) in Rust
# feature names (rustc maps bmi1/cmpxchg16b/rdrand/pclmulqdq to LLVM's
# bmi/cx16/rdrnd/pclmul; names rustc does not know, such as cmov, nopl and
# mmx, pass through to LLVM unchanged). x87 stays on.
#
# The SSE tree is switched off with the single entry "-sse". rustc derives
# "-sse2 ... -avx512*" from it by reverse implication. Listing "-sse2"
# literally is rejected by the target spec consistency check (the x86-64
# hard-float ABI requires sse2 in rustc's model); the check only inspects
# names written in this string, so the derived form passes. rustc 1.98 then
# warns once per crate that sse2 "must be enabled", a future-incompatibility
# notice (rust-lang/rust#116344). See x86_64-knc-linux-musl.md and ADR 0007.
features="-sse,-mmx,-cmov,-nopl,-movbe,-popcnt,-lzcnt,-bmi1,-bmi2,-xsave,-cmpxchg16b,-prfchw,-clflushopt,-rdrand,-rdseed,-adx,-fsgsbase,-pclmulqdq,-aes,-sha,+x87"
RUSTC_BOOTSTRAP=1 rustc -Zunstable-options --print target-spec-json --target x86_64-unknown-linux-musl \
    | jq --arg f "$features" '.vendor = "knc" | .features = $f | .cpu = "x86-64" | ."max-atomic-width" = 64 | ."crt-static-default" = true | ."panic-strategy" = "abort" | ."crt-objects-fallback" = "false" | del(."pre-link-objects-fallback") | del(."post-link-objects-fallback") | ."position-independent-executables" = false | ."static-position-independent-executables" = false | ."relocation-model" = "static"' \
    > "$out.tmp"
mv "$out.tmp" "$out"
echo "wrote $out from $(rustc --version)"
grep -E '"(vendor|features|cpu|max-atomic-width|target-pointer-width|panic-strategy)"' "$out"
