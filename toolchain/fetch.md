# toolchain/fetch.sh and toolchain/SHA256SUMS

Every upstream tarball the card build downloads (musl, busybox, dropbear,
zlib, ncurses, CPython) goes through `phi_fetch NAME URL`, defined here
and made available to every build script by `env.sh`:

1. If `toolchain/build/downloads/NAME` is missing, download `URL` to
   `NAME.part` (`curl -fL --retry 3`) and rename on success.
2. Compute the SHA-256 and compare it with the line for `NAME` in
   `toolchain/SHA256SUMS`, the committed pin list. A mismatch or an
   unlisted file stops the build with the two hashes in the message.
3. `phi_fetched` holds the verified path for the caller.

`toolchain/SHA256SUMS` is `sha256sum` format (two spaces). Where each pin
came from, checked 2026-09-17: musl 1.2.5, busybox 1.37.0, zlib 1.3.1 and
ncurses 6.5 are the values the projects publish for those releases;
Python 3.14.7 is the SHA-256 printed on python.org's release page;
dropbear 2025.88 is the value in Buildroot's `package/dropbear/dropbear.hash`
at its 2025.08 tag. All six equal the files that built every result in
`docs/results/`.

Adding or bumping a version: change the `PHI_*_VERSION` default in the
component script, run it once with `PHI_TOFU=1` (the checksum of that
first download is appended to `SHA256SUMS` and printed), verify the value
against the upstream announcement, and commit both changes together.
Without `PHI_TOFU=1` an unlisted file is refused, so a fresh clone can
never silently build from a tarball nobody has checked.

The LLVM and Linux sources are git checkouts, pinned by tag (LLVM also by
commit) in `toolchain/llvm/patches/SERIES` and `card/kernel/patches/SERIES`.
