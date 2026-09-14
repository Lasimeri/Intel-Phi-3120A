#!/usr/bin/env bash
# build.sh: assemble the card's initramfs from the card busybox, the init
# script in this directory and anything placed under card/initramfs/extra/.
# Output: card/initramfs/build/initramfs.cpio.gz. See build.md.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
. "$here/../../toolchain/env.sh"
root="$phi_build/initramfs/root"
out="$phi_build/initramfs/initramfs.cpio.gz"
busybox="$phi_build/userland/busybox/busybox"
extra="$phi_root/card/initramfs/extra"
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

# Extra files: the tree under card/initramfs/extra/ lands at the same paths
# on the card (extra/opt/hello becomes /opt/hello). Every ELF file in it is
# audited first, so a binary built with the wrong flags never reaches the
# card. The directory is git-ignored except for its README.
if [ -d "$extra" ]; then
	n=0
	while IFS= read -r f; do
		if [ "$(head -c 4 "$f" 2>/dev/null | tr -d '\0')" = $'\x7fELF' ]; then
			echo "== audit $f"
			"$AUDIT" "$f"
		fi
		n=$((n + 1))
	done < <(find "$extra" -type f ! -name README.md)
	(cd "$extra" && find . -type f ! -name README.md -print0 | cpio -0 -pdm --quiet "$root")
	echo "extra files: $n"
fi

# newc cpio, root-owned, gzip: the kernel config enables RD_GZIP.
(cd "$root" && bsdtar --format newc --uid 0 --gid 0 -cf - .) | gzip -9 > "$out"
ls -l "$out"
echo "applets: $(find "$root/bin" -type l | wc -l); boot with:"
echo "  phictl boot --kernel card/kernel/build/out/arch/x86/boot/bzImage --initrd $out --cmdline 'earlyprintk=phiring console=ttyPHI0'"
