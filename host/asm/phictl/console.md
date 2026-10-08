# console.S: the daemon loop and the console

**One thread.** The Rust daemon ran the console, the control socket, the
block service, the host memory service and the forwarder each on its own
thread, each sleeping on its own schedule (1 ms for the console and the
relay, 200 us for the block services after a 20 ms spin). Here `daemon`
gives every service a turn per pass (`services.S`). One core at most,
and no lock anywhere: every buffer has one owner.

**When it waits.** A pass in which any service moved bytes runs again at
once, and so does every pass for the next 200 us (`SPIN_US`). After that
the loop waits between passes for one eighth of the time since the last
movement (`BACKOFF_SHIFT`), at most 2 ms (`IDLE_MAX_US`): a card event
that comes `t` after the last one is seen at most `t / 8` late, and an
idle stretch costs a logarithmic number of passes before the 2 ms
rhythm. The wait is a `ppoll` on the descriptors the next pass would act
on, so whatever arrives from the host side ends it at once: the control
socket's listener and every client the relay would read
(`serve_fds`), the forwarder's listener and each connection's host
socket, for reading while its send buffer has room and for writing while
received bytes wait for it (`forward_fds`, `tcp_fds`), the TAP device
(`net_fds`), standard input while it is open (`console_fds`). The card
side has no descriptor: its rings are read by the passes. If a
descriptor ends a wait and the pass after it moves nothing (readiness no
service consumes), the next wait is timed only and the one after it
watches the descriptors again, so a mismatch costs half the sleep, never
a spin. The thread's timer slack is 10 us (the default 50 us would more
than double the shortest waits).

Until 2026-10-08 the loop was meant to spin for 20 ms after movement and
then sleep 200 us between passes, but it never slept: `pick_holder`
(`serve.S`) returned the last client slot's offset (`0x780`) instead of
0 whenever no session was open and nobody waited, so every pass counted
as movement. Each daemon used a whole core and read the card about
600 000 times a second through the aperture
(`docs/results/2026-10-08-phictl-idle.md`).

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
