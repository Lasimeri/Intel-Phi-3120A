# dropbear

SSH server, chosen over OpenSSH for a first login because it has no
hand-written SIMD assembly and links statically against musl without
patches. Built with `knc-cc`, `--disable-zlib` initially, host keys
generated on the host by `phictl` and injected into the initramfs so that
the fingerprint is stable across card reboots.

OpenSSH follows once the card is usable, built with OpenSSL `no-asm`.

## Build (2026-09-14)

`dropbear.sh` fetches dropbear 2025.88 (checksum recorded in
`toolchain/build/downloads/SHA256SUMS` on first use), configures it with
`CC=knc-cc`, `--host=x86_64-linux-musl`, no zlib, no utmp/wtmp/lastlog, no
syslog, and builds the static multi-call binary `dropbearmulti` (dropbear,
dropbearkey, dbclient, scp). The audit must be clean; it then generates
the card's ed25519 and RSA host keys with that binary on the host.

Post-quantum key exchange is disabled in `localoptions.h`
(`DROPBEAR_SNTRUP761 0`, `DROPBEAR_MLKEM768 0`): the reference sntrup761
code carries hand-written x86-64 inline assembly (the supercop
`crypto_int16` helpers) that uses `cmov`, 14 sites in
`crypto_kem_sntrup761_keypair`, which the audit rejects and the card would
fault on. The compiler is not involved: the same file compiled with
`-O0` still shows the `#APP` blocks. OpenSSH on the host negotiates
`curve25519-sha256` instead, so nothing is lost for a first login.

The initramfs build (`card/initramfs/build.sh`) installs the binary, the
applet links, the host keys under `/etc/dropbear` and the user's public
keys (`~/.ssh/id_ed25519.pub`, `id_ecdsa.pub`, `id_rsa.pub`, or
`PHI_SSH_PUBKEYS`) as root's `authorized_keys`; init starts `dropbear -s -E`
(public keys only, log to the console).
