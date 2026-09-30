# postcode.S: POST codes

The register `POSTCODE` holds two ASCII characters in its low two bytes,
low byte first: `"12"` is `0x3231` (Intel's POST code table, MPSS 2.1
readme; confirmed on this card 2026-09-13, `docs/results/2026-09-13-first-contact.md`).

- `post_text`: the two characters when both are printable ASCII, else
  the raw value as `0x%08x` (a garbage read shows up that way).
- `post_is_ready`: `"12"`, the bootstrap waiting for an image.
- `post_describe`: Intel's table plus this project's kernel marks (`K`
  plus a character, `S` and `A` for the SMP bring-up, from
  `card/kernel/patches`, `asm/knc.h`), matched case-insensitively; an
  empty string for an unknown code.

The table is the one in `host/crates/phi-regs/src/postcode.rs`, entry
for entry.
