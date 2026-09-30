# console.S: the daemon loop and the console

**One thread.** The Rust daemon ran the console, the control socket, the
block service, the host memory service and the forwarder each on its own
thread, each sleeping on its own schedule (1 ms for the console and the
relay, 200 us for the block services after a 20 ms spin). Here `daemon`
gives every service a turn per pass (`services.S`); a pass in which any
service moved bytes runs again at once, and after 20 ms without movement
the loop sleeps 200 us between passes. One core at most, and no lock
anywhere: every buffer has one owner.

What this changes: a blocking call in one service stalls the others for
its duration. The block service's `pread`/`pwrite` of the image and its
`fdatasync` on a flush are the only blocking calls (the DMA copies are
polled); a flush can take tens of milliseconds, during which the rpc
relay and the console wait. The Rust daemon's relay kept running through
a flush. Not measured as a problem: `phi run`, `put`, `get`, phitop and
ssh all ran while `/data` was being written at 195 MB/s
(`docs/results/2026-09-29-phictl-assembly.md`).

**The console.** The region is opened (retried every 500 ms until its
header is valid, the console tailing POST codes meanwhile); each pass
pops the console ring into standard output, logs a POST change with the
time since the daemon started, the watched word (`--watch`) as a
POST-style code, and the card's flags word; standard input, when it is
readable, goes into the console's host-to-card ring whole (the Rust
daemon read lines on a thread; under systemd there is no input and the
descriptor reads end of file once, after which it is left alone).

**wait_init_seen**: the card's kernel has reached init: POST "K7", or
bit 0 of the region's flags word. Every service starts relaying only
then, as the Rust services waited for "K7" (they returned quietly on
"KH" or "KP"; here the services simply never start).

`phictl console` runs the loop on a card this process opened without
booting it; since opening resets the card, it only ever tails a
bootstrap, as the Rust command did.
