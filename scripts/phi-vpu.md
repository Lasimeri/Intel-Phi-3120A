# phi-vpu.sh: put the co-processor worker on the card and drive it

```
scripts/phi-vpu.sh [-c N] deploy        copy the sources to the card and build there
scripts/phi-vpu.sh [-c N] start [T]     start the worker with T threads (default 57)
scripts/phi-vpu.sh [-c N] stop
scripts/phi-vpu.sh [-c N] status        worker process on the card, control words on the host
scripts/phi-vpu.sh [-c N] log           the worker's output
scripts/phi-vpu.sh [-c N] poly [args]   run the host driver; deploys and starts first if needed
```

`-c N` picks the card (else `$PHI_CARD`, else 0). Each card has its own
host-memory window (`/dev/shm/phi-hostmem` for card 0, `phi-hostmem-N`
for card N) and its own SSH forward (`127.0.0.1:2222+N`), so each runs
its own worker, and the script reaches it over its own port with the
pinned host key; no `~/.ssh/config` stanza is needed. Needs the card up
(`phi -c N status`) with the native toolchain on its disk (`cc` builds
the worker on the card). The worker lives in `/opt/phi-vpu` on the card
(`PHI_VPU_DIR` to change), which is on the card's persistent disk, so a
deployed worker survives a reboot and only `start` is needed afterwards.

```
scripts/phi-vpu.sh poly --n 1048576 --threads 57 --repeat 5
```

is the one-line demonstration: the host hands a million-element AVX-512
kernel to the card, gets the answer back, and checks every lane against
its own FMA hardware.

`PHI_VPU_ARGS="-s 500 -i 1000" scripts/phi-vpu.sh start` passes worker
options (spin window, idle poll interval) through.

`start` reserves 2 MiB huge pages on the card first (`PHI_VPU_HUGEPAGES`,
512 by default: 1 GiB, enough for 128 M elements in and out), and
`stop` releases them. The worker takes its buffers from them, which
turns a request's transport from one block record per scattered 4 KiB
page into one per 512 KiB (`card/vpu/vpu_worker.md`, "Moving the
data"). A card that cannot give the whole reservation says so; the
worker then falls back to 4 KiB pages for buffers that do not fit.

## Two refusals

- **`start` refuses while `/dev/phiblk1` is a swap device on the card.**
  The window the worker uses is the same memory that backs that device,
  and the card's `init` puts swap on it at boot. Offloading over live
  swap would corrupt whichever side wrote second. Run
  `ssh phi swapoff /dev/phiblk1` first; `swapon` puts it back.
- **`stop` uses `pkill -f 'phi-vpu-worke[r]'`.** The bracket class keeps
  the pattern from matching the ssh command line that carries it, which
  is what happens with the plain name and kills the ssh session instead
  of the worker. The kill and the start are separate ssh calls for the
  same reason: a `pkill` in the same command line as `./phi-vpu-worker`
  matches its own shell and nothing starts, while the host still sees the
  dead worker's readiness word (the driver now clears that word and waits
  for a live worker to re-assert it, so this fails in five seconds with a
  message instead of waiting a minute for an answer).

## Backgrounding on the card

`start` runs the worker under `setsid`, with its output to
`worker.log`, and a `sleep 1` in the same ssh command. Without the sleep
the ssh session closes before the process has detached and takes it
along.

## Where the worker lives (2026-09-22)

`/opt/phi/vpu` on the card, inside the `/opt/phi` bind mount of the
persistent disk (`PHI_VPU_DIR` to change). The earlier `/opt/phi-vpu` was
in the initramfs and vanished at every reboot; `poly` redeployed it
silently, which is how nobody noticed. The worker now also carries the
seamless path's exec engine (`card/vpu/vpu_exec.c`), so a deploy copies
those sources too.
