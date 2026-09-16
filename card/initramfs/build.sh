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

# dropbear: the static multi-call binary, its applet links, the host keys
# generated on the host by card/userland/components/dropbear.sh (stable
# fingerprint across card boots), and the user's public keys for root.
# Password logins are off (dropbear -s), so without a key nothing can log in.
# phi-agent: the card end of phictl exec/put/get/status over /dev/phirpc
# (card/agent/build.sh; kernel patch 0022).
agent="$phi_build/userland/agent/phi-agent"
if [ -x "$agent" ]; then
	cp "$agent" "$root/bin/phi-agent"
	chmod 755 "$root/bin/phi-agent"
	echo "phi-agent: installed"
else
	echo "phi-agent: not built (card/agent/build.sh); phictl exec will not work with this image"
fi

dropbear="$phi_build/userland/dropbear"
if [ -x "$dropbear/dropbearmulti" ]; then
	cp "$dropbear/dropbearmulti" "$root/bin/dropbearmulti"
	for a in dropbear dropbearkey dbclient scp; do ln -sfn dropbearmulti "$root/bin/$a"; done
	ln -sfn dbclient "$root/bin/ssh"
	mkdir -p "$root/etc/dropbear" "$root/root/.ssh"
	cp "$dropbear"/keys/dropbear_*_host_key "$root/etc/dropbear/"
	chmod 600 "$root/etc/dropbear"/*
	auth="$root/root/.ssh/authorized_keys"
	: > "$auth"
	for k in ${PHI_SSH_PUBKEYS:-"$HOME"/.ssh/phi_ed25519.pub "$HOME"/.ssh/id_ed25519.pub "$HOME"/.ssh/id_ecdsa.pub "$HOME"/.ssh/id_rsa.pub}; do
		[ -r "$k" ] && cat "$k" >> "$auth"
	done
	chmod 700 "$root/root" "$root/root/.ssh"; chmod 600 "$auth"
	echo "dropbear: $(wc -l < "$auth") authorized key(s) for root"
else
	echo "dropbear: not built (card/userland/components/dropbear.sh); no SSH server in this image"
fi

# newc cpio, root-owned, gzip: the kernel config enables RD_GZIP.
(cd "$root" && bsdtar --format newc --uid 0 --gid 0 -cf - .) | gzip -9 > "$out"
# The image holds the card's SSH host private keys and root's authorized keys.
chmod 600 "$out"
# The image holds the card's SSH host private keys and root's authorized keys.
chmod 600 "$out"
ls -l "$out"
echo "applets: $(find "$root/bin" -type l | wc -l); boot with:"
echo "  phictl boot --kernel card/kernel/build/out/arch/x86/boot/bzImage --initrd $out --cmdline 'earlyprintk=phiring console=ttyPHI0'"
