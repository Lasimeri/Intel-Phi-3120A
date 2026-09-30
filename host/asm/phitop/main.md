# main.S: phitop in assembly

A resource viewer for the cards in the spirit of glances, run on the
host: every interval it asks each card's daemon (`phictl boot --serve`)
for a `Stat` sample, which the card's agent answers from /proc and
sysfs, and for the daemon's own PCIe byte counters (`Traffic`), then
draws the load of every hardware thread on a core grid, the die
temperatures, memory and swap, PCIe rates by path and direction, the
card's disk and network rates, and its processes. Until 2026-09-29 it
was the Rust crate `host/crates/phitop`; this is the same program in
assembly, sharing `text.S`, `sysfs.S` and `wire.S` with `../phictl`.

| file | what |
| --- | --- |
| `main.S` | options, the slots (one per card), sampling over the control socket, the batch and interactive loops, the keys |
| `model.S` | a `StatReply` and a `TrafficReply` into a snapshot; two snapshots into loads, cores, rates and shares |
| `view.S` | one frame as text with ANSI colours |
| `term.S` | raw keys, the screen size, the alternate screen, restoring on exit or a signal |
| `defs.inc` | the record layouts on top of phictl's constants |

**Options**: `-c`/`--card N`, `-i`/`--interval SECONDS` (decimal, 0.2 to
60), `-b`/`--batch FRAMES` (plain text, 132 by 32, no terminal control),
`--socket PATH`, `-k`/`--kthreads`. With one card named (`-c`, `PHI_CARD`,
`--socket`, `PHICTL_SOCKET`) that card fills the screen; with none, every
card in the cards list gets its own block, sampled on its own socket and
drawn from its own model, so one card's load never colours another's.
A card's socket is the root daemon's when phitop runs as root or a
daemon answers there, else the card's socket in the user's runtime
directory.

**Sampling**: one request per fresh connection, so the daemon can
interleave it with whatever session another client holds (ADR 0009); a
`Stat` waits up to 5 s, a `Traffic` 2 s. A failure keeps the model and
sets the slot's error, shown in the status (red in colour) or on the
header line of the card's block.

**Keys**: `q` or Ctrl-C quit, `c`/`m` sort by CPU or memory, `k` kernel
threads, `+`/`-` the interval by half a second, `n`/`p` one card, `a`
all. SIGINT and SIGTERM end the loop through a flag the handler sets;
the terminal is restored on every exit.

Verified: `phitop --batch` frames for cards 0 and 1 against the Rust
phitop's (`docs/results/2026-09-29-phictl-assembly.md`, phitop section).
