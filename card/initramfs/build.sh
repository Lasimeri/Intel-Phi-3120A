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
# A filesystem layout a Linux program expects to find: /bin and /sbin hold
# the busybox links, /usr mirrors them for anything with a hard-coded path,
# /etc and /var are mount points for the persistent copies on the disk, and
# /lib/phi/etc-skel is the image's own copy of /etc that init installs.
mkdir -p "$root"/{bin,sbin,etc,proc,sys,dev,run,tmp,var,root,home,opt,mnt,media,srv}
mkdir -p "$root"/usr/{bin,sbin,lib} "$root"/lib/phi/etc-skel
cp "$busybox" "$root/bin/busybox"
# The card busybox runs on the host too (same instruction subset), so it can
# lay out its own applet links; relative targets keep the tree relocatable.
(cd "$root" && ./bin/busybox --install -s "$root/bin" > /dev/null 2>&1 || true)
find "$root/bin" -type l | while read -r l; do ln -sfn busybox "$l"; done
cp "$phi_root/card/initramfs/init" "$root/init"
chmod 755 "$root/init"
# The /etc skeleton. init installs it over /etc on every boot: passwd, group,
# shells, os-release and the dropbear keys are copied every time, so the image
# stays authoritative for who may log in; hostname, hosts, profile, fstab, TZ
# and resolv.conf are seeded only when absent, so an edit made on the card
# survives on the persistent disk.
skel="$root/lib/phi/etc-skel"
printf 'root:x:0:0:root:/root:/bin/sh\n' > "$skel/passwd"
printf 'root:x:0:\n' > "$skel/group"
printf '/bin/sh\n' > "$skel/shells"
printf '%s\n' "phi" > "$skel/hostname"
cat > "$skel/hosts" <<'HOSTS'
127.0.0.1	localhost
::1		localhost
10.9.0.2	phi
10.9.0.1	host
HOSTS
# UTC, as a server usually is. busybox reads a POSIX TZ string from this file
# (there is no zoneinfo database on the card): edit it to e.g.
# CST6CDT,M3.2.0,M11.1.0 for US Central and the change persists on the disk.
printf 'UTC0\n' > "$skel/TZ"
# The card has no route off phi0 unless the host bridges a TAP, so there is
# nothing to resolve by default. Left empty and editable on purpose.
printf '# no nameserver: the card reaches only the host, over phi0\n' > "$skel/resolv.conf"
cat > "$skel/os-release" <<OSREL
NAME="Intel Phi 3120A"
ID=phi
PRETTY_NAME="Intel Phi 3120A (Knights Corner), mainline Linux"
VERSION_ID="$(date +%Y-%m-%d)"
HOME_URL="https://github.com/Lasimeri/Intel-Phi-3120A"
OSREL
# fstab documents what init mounts. busybox mount reads it, so `mount /data`
# after a manual umount works, but init does not use it: the waits for the
# block devices to appear have to happen in order.
cat > "$skel/fstab" <<'FSTAB'
# The card's root is the initramfs; everything below is mounted by /init.
# <device>	<mount point>	<type>		<options>		<dump> <pass>
proc		/proc		proc		defaults		0 0
sysfs		/sys		sysfs		defaults		0 0
devtmpfs	/dev		devtmpfs	defaults		0 0
devpts		/dev/pts	devpts		defaults		0 0
tmpfs		/tmp		tmpfs		mode=1777		0 0
tmpfs		/run		tmpfs		mode=0755,size=64m	0 0
tmpfs		/dev/shm	tmpfs		mode=1777		0 0
/dev/phiblk0	/data		ext4		noatime			0 0
/dev/phiblk1	none		swap		sw			0 0
# /etc /var /opt/phi /root /home are bind mounts from /data of the same name.
FSTAB
cat > "$skel/profile" <<'PROFILE'
# /etc/profile: read by every login shell (the console shell that init starts
# with -l, and every dropbear session).
PATH=/opt/phi/bin:/bin:/sbin:/usr/bin:/usr/sbin
export PATH
[ -r /etc/TZ ] && TZ=$(cat /etc/TZ) && export TZ
export PAGER=less
export EDITOR=vi
# The card's own clang is on the disk under /opt/phi; nothing else provides cc.
if [ -x /opt/phi/bin/cc ]; then
	export CC=/opt/phi/bin/cc
	export CXX=/opt/phi/bin/c++
fi
PS1="$(hostname):\w\$ "
export PS1
umask 022
PROFILE
# Plain files first: the glob must not see a directory, because chmod 644
# on one strips the traverse bit and everything under it becomes
# unreachable (which is exactly what happened the first time).
chmod 644 "$skel"/*
# Configuration that is a directory rather than a file. fastfetch looks for
# /etc/fastfetch/config.jsonc; the file is versioned in the repository
# because it describes the card, not the machine that built the image.
mkdir -p "$skel/fastfetch"
cp "$phi_root/card/initramfs/etc-skel/fastfetch.jsonc" "$skel/fastfetch/config.jsonc"
chmod 755 "$skel/fastfetch"
chmod 644 "$skel/fastfetch/config.jsonc"


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
	# Into the skeleton, not into /etc and /root directly: init copies both
	# over the persistent copies on every boot, so a rebuilt image decides
	# who can log in even after the card has been running for weeks.
	mkdir -p "$skel/dropbear" "$skel/root-ssh"
	cp "$dropbear"/keys/dropbear_*_host_key "$skel/dropbear/"
	chmod 700 "$skel/dropbear"; chmod 600 "$skel/dropbear"/*
	auth="$skel/root-ssh/authorized_keys"
	: > "$auth"
	for k in ${PHI_SSH_PUBKEYS:-"$HOME"/.ssh/phi_ed25519.pub "$HOME"/.ssh/id_ed25519.pub "$HOME"/.ssh/id_ecdsa.pub "$HOME"/.ssh/id_rsa.pub}; do
		[ -r "$k" ] && cat "$k" >> "$auth"
	done
	chmod 700 "$skel/root-ssh"; chmod 600 "$auth"
	echo "dropbear: $(wc -l < "$auth") authorized key(s) for root"
else
	echo "dropbear: not built (card/userland/components/dropbear.sh); no SSH server in this image"
fi

# newc cpio, root-owned, gzip: the kernel config enables RD_GZIP.
(cd "$root" && bsdtar --format newc --uid 0 --gid 0 -cf - .) | gzip -9 > "$out"
# The image holds the card's SSH host private keys and root's authorized keys.
chmod 600 "$out"
ls -l "$out"
echo "applets: $(find "$root/bin" -type l | wc -l); boot with:"
echo "  phictl boot --kernel card/kernel/build/out/arch/x86/boot/bzImage --initrd $out --cmdline 'earlyprintk=phiring console=ttyPHI0'"
