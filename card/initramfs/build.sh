#!/usr/bin/env bash
# build.sh: assemble the card's initramfs from the card busybox and the
# init script in this directory. Output: card/initramfs/build/initramfs.cpio.gz.
# See build.md.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
. "$here/../../toolchain/env.sh"
root="$phi_build/initramfs/root"
out="$phi_build/initramfs/initramfs.cpio.gz"
busybox="$phi_build/userland/busybox/busybox"
AUDIT="$phi_root/host/target/debug/phi-isa-audit"

[ -x "$busybox" ] || { echo "build.sh: $busybox missing; run card/userland/components/busybox.sh" >&2; exit 1; }
echo "== audit $busybox"
"$AUDIT" "$busybox"

rm -rf "$root"
mkdir -p "$root"/{bin,sbin,etc,proc,sys,dev,tmp,root}
cp "$busybox" "$root/bin/busybox"
# The card busybox runs on the host too (same instruction subset), so it can
# lay out its own applet links; relative targets keep the tree relocatable.
(cd "$root" && ./bin/busybox --install -s "$root/bin" > /dev/null 2>&1 || true)
find "$root/bin" -type l | while read -r l; do ln -sfn busybox "$l"; done
cp "$phi_root/card/initramfs/init" "$root/init"
chmod 755 "$root/init"
printf 'root:x:0:0:root:/root:/bin/sh\n' > "$root/etc/passwd"
printf 'root:x:0:\n' > "$root/etc/group"

# newc cpio, root-owned, gzip: the kernel config enables RD_GZIP.
(cd "$root" && bsdtar --format newc --uid 0 --gid 0 -cf - .) | gzip -9 > "$out"
ls -l "$out"
echo "applets: $(find "$root/bin" -type l | wc -l); boot with:"
echo "  phictl boot --kernel card/kernel/build/out/arch/x86/boot/bzImage --initrd $out --cmdline 'earlyprintk=phiring console=ttyPHI0 nosmp'"
