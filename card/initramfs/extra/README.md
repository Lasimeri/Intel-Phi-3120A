# card/initramfs/extra

Files placed here are copied into the initramfs at the same paths
(`extra/opt/hello` becomes `/opt/hello` on the card) by
`card/initramfs/build.sh`, which audits every ELF file with `phi-isa-audit`
first. The directory is git-ignored apart from this file: it holds build
products, not sources. See `docs/howto/build-and-run.md`.
