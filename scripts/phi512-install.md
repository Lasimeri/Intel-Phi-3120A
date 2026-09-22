# phi512-install.sh

Put the AVX-512 layer on this system.

```sh
scripts/phi512-install.sh              # a phi512 command, changes nothing else
scripts/phi512-install.sh --system     # every process, automatically
scripts/phi512-install.sh --status
scripts/phi512-install.sh --uninstall
```

## The two modes, and why the safe one is the default

**Wrapper mode** installs `/usr/lib/libphi512.so` and a `phi512` command.
Programs start exactly as they did; a program that needs AVX-512 is run as
`phi512 ./the-program`. Nothing else on the machine changes.

**System mode** adds the library to `/etc/ld.so.preload`, so the loader maps
it into every dynamically linked process. Any program that executes an
AVX-512 instruction then works, with no wrapper and no knowledge that
anything is unusual. That is the seamless option, and it is opt-in because
of what the file is.

## What `/etc/ld.so.preload` actually means

It is not `LD_PRELOAD`. The environment variable is ignored for setuid
binaries; the file is not. **A fault in a library listed there takes `sudo`
with it**, and with `sudo` goes the ordinary way of undoing the change.

So the installer does this, in this order:

1. Runs `true`, `echo`, `id` and `sudo --version` with the library forced
   in, from its build location, before anything is copied anywhere. A
   library that breaks ordinary programs never reaches `/usr/lib`.
2. Copies the library and the command into place.
3. Prints what the change is and every way to undo it, and asks.
4. Keeps any existing `/etc/ld.so.preload` as `.before-phi512`.
5. Writes the file, then **runs those programs again with the preload
   live**, which is the check that matters, because step 1 used
   `LD_PRELOAD` and setuid binaries ignore that.
6. If anything is unhealthy, removes the entry immediately and reports,
   rather than leaving a broken system behind.

## Getting out of it

Any one of these is enough, in increasing order of desperation:

| | |
| --- | --- |
| `PHI512_DISABLE=1 <command>` | turns the layer off for one command. No root, no files touched. The library checks this before it does anything at all. |
| `scripts/phi512-install.sh --uninstall` | removes the preload entry, the library and the command |
| `sudo rm /etc/ld.so.preload` | turns it off for the system |
| boot with `init=/bin/sh`, then `mount -o remount,rw / && rm /etc/ld.so.preload` | for the case where `sudo` itself will not run |

## What the library does when it is loaded into something that does not need it

Nothing worth measuring. It checks `PHI512_DISABLE`, asks CPUID whether
this host has AVX-512 already (and returns immediately if it does),
installs a `SIGILL` handler, and returns. A process that never executes an
AVX-512 instruction never enters it again.

Every failure path in that sequence returns quietly and leaves the process
exactly as it would have been without the library. It never aborts and
never prevents a program from starting, because it may be inside every
process on the machine including the ones needed to repair it.

## What this does not do

It does not make the machine *report* AVX-512. Software that asks CPUID
before using a feature will still see a CPU without it and take its other
path, which on this host means AVX2 and is the faster answer anyway.
Software that simply uses AVX-512, because it was built that way, works.

That is a deliberate split and `docs/research/avx512-transparency.md`
explains it: CPUID cannot be made to fault on this processor, and
advertising the feature would send glibc's `memcpy` and `strlen` down
AVX-512 paths, which are short, constant, and would each become a fault.
Succeeding at advertisement would make the machine slower at its most
common work.
