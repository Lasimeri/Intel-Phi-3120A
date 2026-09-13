# dropbear

SSH server, chosen over OpenSSH for a first login because it has no
hand-written SIMD assembly and links statically against musl without
patches. Built with `knc-cc`, `--disable-zlib` initially, host keys
generated on the host by `phictl` and injected into the initramfs so that
the fingerprint is stable across card reboots.

OpenSSH follows once the card is usable, built with OpenSSL `no-asm`.
