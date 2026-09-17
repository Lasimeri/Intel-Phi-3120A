# fetch.sh: sourced by toolchain/env.sh; defines phi_fetch for every script
# that downloads an upstream tarball (musl, busybox, dropbear, zlib, ncurses,
# CPython). Downloads are pinned: the SHA-256 of each file is listed in
# toolchain/SHA256SUMS (committed), and a file that is not listed or does
# not match stops the build. See fetch.md.
#
# phi_fetch NAME URL
#   Downloads URL to toolchain/build/downloads/NAME unless that file already
#   exists, then verifies it against toolchain/SHA256SUMS and sets
#   phi_fetched to the verified path. PHI_TOFU=1 records the checksum of an
#   unlisted file instead of failing (to add a new version: run once with
#   it, review the new line, commit it).
phi_fetch() {
    local name="$1" url="$2"
    local dl="$phi_root/toolchain/build/downloads"
    local sums="$phi_root/toolchain/SHA256SUMS"
    local file="$dl/$name" want have
    mkdir -p "$dl"
    [ -f "$sums" ] || { echo "fetch.sh: $sums missing" >&2; return 1; }
    want=$(awk -v n="$name" '$2 == n { print $1 }' "$sums")
    if [ -z "$want" ] && [ "${PHI_TOFU:-0}" != 1 ]; then
        echo "fetch.sh: $name is not listed in toolchain/SHA256SUMS; add its SHA-256 there, or set PHI_TOFU=1 to record the checksum of this first download" >&2
        return 1
    fi
    if [ ! -s "$file" ]; then
        echo "== fetching $url"
        if ! curl -fL --retry 3 -o "$file.part" "$url"; then
            rm -f "$file.part"
            echo "fetch.sh: download of $url failed" >&2
            return 1
        fi
        mv "$file.part" "$file"
    fi
    have=$(sha256sum "$file" | awk '{ print $1 }')
    if [ -z "$want" ]; then
        echo "$have  $name" >> "$sums"
        echo "== $name: SHA-256 $have recorded in toolchain/SHA256SUMS (PHI_TOFU=1); review and commit it"
    elif [ "$have" != "$want" ]; then
        echo "fetch.sh: SHA-256 mismatch for $file: have $have, want $want; delete the file to download it again, or change toolchain/SHA256SUMS if the pin moved on purpose" >&2
        return 1
    else
        echo "== $name: SHA-256 verified"
    fi
    phi_fetched="$file"
}
