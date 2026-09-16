#!/usr/bin/env bash
# phi-disk.sh: manage the card's persistent disk image, a sparse file the
# host serves to the card as /dev/phiblk0 (phictl boot --disk, kernel patch
# 0025). ext4 inside; the card mounts it on /data at boot.
#   scripts/phi-disk.sh create PATH SIZE     # e.g. create /mnt/1TB-NVMe/phi/disk.img 256G
#   scripts/phi-disk.sh check PATH           # read-only e2fsck (card must be down)
#   scripts/phi-disk.sh usage PATH           # apparent and allocated size
# See phi-disk.md.
set -euo pipefail
cmd="${1:-}"; path="${2:-}"
case "$cmd" in
    create)
        size="${3:-}"
        [ -n "$path" ] && [ -n "$size" ] || { echo "usage: $0 create PATH SIZE" >&2; exit 2; }
        [ -e "$path" ] && { echo "$0: $path exists; refusing to overwrite" >&2; exit 1; }
        mkdir -p "$(dirname "$path")"
        truncate -s "$size" "$path"
        chmod 600 "$path"
        mkfs.ext4 -F -q -L phidata -E lazy_itable_init=1,lazy_journal_init=1 "$path"
        echo "$0: created $path ($size, sparse, ext4 'phidata', mode 0600)"
        ;;
    check)
        [ -n "$path" ] || { echo "usage: $0 check PATH" >&2; exit 2; }
        if pgrep -f "^(sudo )?\S*phictl boot " > /dev/null; then
            echo "$0: the card is up; a check while it is mounted is not meaningful (scripts/phi-down.sh first)" >&2
            exit 1
        fi
        e2fsck -fn "$path"
        ;;
    usage)
        [ -n "$path" ] || { echo "usage: $0 usage PATH" >&2; exit 2; }
        ls -la --block-size=M "$path"
        du -h "$path"
        ;;
    *)
        echo "usage: $0 create PATH SIZE | check PATH | usage PATH" >&2
        exit 2
        ;;
esac
