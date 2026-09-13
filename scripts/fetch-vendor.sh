#!/usr/bin/env bash
# fetch-vendor.sh: download reference material into vendor/ (git-ignored).
# Trust-on-first-use: the SHA-256 of every file is recorded in
# vendor/SHA256SUMS on first download and verified afterwards.
# See fetch-vendor.md.
set -euo pipefail
cd "$(dirname "$0")/.."
V=vendor
mkdir -p "$V/docs" "$V/mpss-3.8.6" "$V/linux-5.9-mic"
SUMS="$V/SHA256SUMS"
touch "$SUMS"

fetch() { # fetch <relative path under vendor> <url>
    local rel="$1" url="$2" path="$V/$1"
    if [ -s "$path" ]; then
        echo "have   $rel"
    else
        echo "fetch  $rel"
        curl -fL --retry 3 -o "$path.part" "$url"
        mv "$path.part" "$path"
    fi
    local sum; sum=$(sha256sum "$path" | awk '{print $1}')
    if grep -q " $rel\$" "$SUMS"; then
        local want; want=$(grep " $rel\$" "$SUMS" | awk '{print $1}')
        if [ "$sum" != "$want" ]; then
            echo "SHA-256 MISMATCH for $rel: have $sum want $want" >&2
            exit 1
        fi
    else
        echo "$sum  $rel" >> "$SUMS"
    fi
}

# Intel documents
fetch docs/isa-reference-327364-001.pdf \
    "https://www.intel.com/content/dam/develop/external/us/en/documents/327364001en.pdf"
fetch docs/ssdg-328207-002.pdf \
    "https://kib.kiev.ua/x86docs/Intel/Knights/328207-002.pdf"
fetch docs/datasheet-328209.pdf \
    "https://www.intel.com/content/dam/www/public/us/en/documents/datasheets/xeon-phi-coprocessor-datasheet.pdf"
fetch docs/k1om-psabi-1.0.pdf \
    "https://community.intel.com/cipcp26785/attachments/cipcp26785/software-archive/5984/2/k1om-psabi-1.0.pdf"

# MPSS 3.8.6 from the Internet Archive item intel-mpss-3.8.6
IA="https://archive.org/download/intel-mpss-3.8.6"
fetch mpss-3.8.6/mpss-3.8.6-linux.tar "$IA/mpss-3.8.6-linux.tar"
fetch mpss-3.8.6/mpss-3.8.6-k1om.tar  "$IA/mpss-3.8.6-k1om.tar"
fetch mpss-3.8.6/mpss-src-3.8.6.tar   "$IA/mpss-src-3.8.6.tar"

# Mainline v5.9 mic driver (last version before removal)
if [ ! -d "$V/linux-5.9-mic/mic" ]; then
    echo "fetch  linux-5.9 drivers/misc/mic"
    curl -fL --retry 3 -o "$V/linux-5.9.tar.gz" \
        "https://github.com/torvalds/linux/archive/refs/tags/v5.9.tar.gz"
    tar -xzf "$V/linux-5.9.tar.gz" -C "$V/linux-5.9-mic" --strip-components=3 \
        "linux-5.9/drivers/misc/mic" "linux-5.9/Documentation/misc-devices/mic" 2>/dev/null || \
    tar -xzf "$V/linux-5.9.tar.gz" -C "$V/linux-5.9-mic" --strip-components=3 "linux-5.9/drivers/misc/mic"
    rm -f "$V/linux-5.9.tar.gz"
else
    echo "have   linux-5.9 drivers/misc/mic"
fi

# Intel's k1om kernel tree inside the solros repository (sparse checkout)
if [ ! -d "$V/solros/phi-kernel" ]; then
    echo "fetch  solros phi-kernel (sparse)"
    git clone --depth 1 --filter=blob:none --sparse \
        https://github.com/cosmoss-jigu/solros "$V/solros"
    git -C "$V/solros" sparse-checkout set phi-kernel
else
    echo "have   solros phi-kernel"
fi

echo "fetch-vendor.sh: done. Extract MPSS tarballs by hand as needed; see vendor/README.md"
