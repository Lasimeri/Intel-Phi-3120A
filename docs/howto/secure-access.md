# Access to the card, and what protects it

The card is a PCIe device owned by whoever holds its VFIO group. Nothing
on it is reachable from the network; every path goes through the process
that booted it (the daemon), on this host. What exists and what limits it:

| path | who can use it | mechanism |
| --- | --- | --- |
| VFIO device `/dev/vfio/<group>` | group `phi` (udev rule from `setup-arch.sh`) | one opener at a time; `vfio-pci` resets the card at open (measured 2026-09-14, `host/crates/phi-hw/src/card.md`) and at close (`scripts/phi-down.md`), so nothing survives on the card between holders |
| Control socket (`phictl exec/put/get/status/sensors/traffic`, `phitop`) | the booting user, and root | `$XDG_RUNTIME_DIR/phictl/control.sock`, directory 0700, socket 0600, every connection checked with `SO_PEERCRED` (`host/crates/phictl/src/serve.md`); at most 16 connections, one card session at a time (ADR 0009) |
| SSH through the forwarder | holders of an authorized private key | dropbear on the card, public keys only (`-s`), root only; the forwarder listens on 127.0.0.1:2222 and nothing else (`host/crates/phictl/src/forward.md`), so the host's loopback is the only way in |
| SSH over `phi0` (root boot with `--net`) | as above | 10.9.0.2, reachable only through the host's TAP device |
| The disk image | the owner | on this machine `/mnt/1TB-NVMe/phi/disk.img`, mode 0600, opened read/write by the booting process only (`scripts/phi-disk.md`) |
| Host memory for the card | the owner | `/dev/shm/phi-hostmem`, mode 0600, pinned by the daemon; a host program that maps it shares memory with the card (`docs/spec/ring-protocol.md`, host memory) |

Keys. The initramfs carries the card's dropbear host keys (generated
once by `card/userland/components/dropbear.sh`) and root's
`authorized_keys`, assembled from `~/.ssh/phi_ed25519.pub` and the user's
own public keys at build time (`PHI_SSH_PUBKEYS` overrides the list); the
image file is mode 0600 since 2026-09-16. `~/.ssh/phi_ed25519` has no
passphrase (chosen so that scripts and this assistant can use it);
`ssh-keygen -p -f ~/.ssh/phi_ed25519` adds one at the cost of an agent or a
prompt. The client stanzas in `~/.ssh/config` (`Host phi-fwd`, `Host phi`,
`docs/reproducibility.md` step 12) use that key only (`IdentitiesOnly yes`),
and `phi-fwd` pins the card's ed25519 host key in `~/.ssh/known_hosts_phi`
with `StrictHostKeyChecking yes`, so a process squatting on 127.0.0.1:2222
cannot pose as the card. Re-pin after a dropbear key regeneration:
`ssh-keyscan -p 2222 -t ed25519 127.0.0.1 >> ~/.ssh/known_hosts_phi`.

Autoboot. `scripts/phi-autoboot.sh install DISK [HOSTMEM]` runs the boot as
a systemd user service at login; it inherits every limit above because it
is the same process. Stopping the service halts the card cleanly first
(`poweroff -f` through the socket), then the VFIO release resets it.

What this does not do: encrypt the disk image (the host filesystem's
protection is what there is), isolate the card from a root user on the
host (root can open the VFIO group and read card memory), or survive a
host reboot without the service starting again at login. The agent runs as
root on a card the host resets at will, so it is not a privilege the host
lacks (ADR 0008).
