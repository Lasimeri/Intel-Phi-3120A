# phi-vpu.sh: put the co-processor worker on the card and drive it

```
scripts/phi-vpu.sh deploy        copy the sources to the card and build there
scripts/phi-vpu.sh start [N]     start the worker with N threads (default 57)
scripts/phi-vpu.sh stop
scripts/phi-vpu.sh status        worker process on the card, control words on the host
scripts/phi-vpu.sh log           the worker's output
scripts/phi-vpu.sh poly [args]   run the host driver; deploys and starts first if needed
```

Needs the card up (`phi status`) and reachable as `ssh phi`. The worker
lives in `/opt/phi-vpu` on the card (`PHI_VPU_DIR` to change), which is
on the card's persistent disk, so a deployed worker survives a reboot
and only `start` is needed afterwards.

```
scripts/phi-vpu.sh poly --n 1048576 --threads 57 --repeat 5
```

is the one-line demonstration: the host hands a million-element AVX-512
kernel to the card, gets the answer back, and checks every lane against
its own FMA hardware.

`PHI_VPU_ARGS="-s 500 -i 1000" scripts/phi-vpu.sh start` passes worker
options (spin window, idle poll interval) through.

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
