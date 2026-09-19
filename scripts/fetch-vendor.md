# fetch-vendor.sh

Downloads reference material into `vendor/`, which git ignores. Nothing in
the build reads `vendor/`; it exists so a reader can check every citation in
`docs/` against the primary source on their own disk.

## Contents fetched

| Path | Source | Size |
| --- | --- | --- |
| `docs/isa-reference-327364-001.pdf` | intel.com | 2.4 MB |
| `docs/ssdg-328207-002.pdf` | kib.kiev.ua mirror | 2.8 MB |
| `docs/datasheet-328209.pdf` | intel.com | small |
| `docs/k1om-psabi-1.0.pdf` | community.intel.com attachment | 0.5 MB |
| `mpss-3.8.6/*.tar` | archive.org item `intel-mpss-3.8.6` | 300 MB + k1om + src |
| `mpss-3.8.6/mpss-modules-3.8.6/` | extracted from the source RPM inside the host tarball with `bsdtar` | small |
| `linux-5.9-mic/mic/` | GitHub tag v5.9 tarball, extracted subtree | 180 MB download, small result |
| `solros/phi-kernel/` | GitHub, sparse checkout | a full 2.6.38 tree |

## Integrity

Trust on first use: the first download records `sha256sum` into
`vendor/SHA256SUMS`; later runs verify against it and abort on mismatch.
If you want to seed known-good hashes from another machine, copy that file
before running the script. Intel never published hashes for MPSS; the
Revival Project recorded `fce922dd...` for its local MPSS 3.8.6 archive
without naming which tarball, so it is not used here.

## Re-running

Idempotent: existing files are kept and re-verified. Delete a file to
re-download it.
