# exec.S: one command

## Spawning, as Rust's Command did

The Rust agent ran `Command::new(argv[0]).args(..).env_clear().envs(env)
.stdin/stdout/stderr(piped).current_dir(cwd).spawn()`. This file does what
that does on Linux with musl (Rust's `library/std/src/sys/process/unix`):

1. **The environment.** Exactly the entries sent, one per key, the last
   value for a key winning, in key order (Rust keeps them in a `BTreeMap`,
   so `env` on the card prints them sorted): a stable merge sort of the
   entries, then the last of each run of equal keys.
2. **NUL bytes.** A NUL in the program, an argument, the working directory
   or a kept environment entry fails the spawn with "nul byte found in
   provided data", before anything is created.
3. **Pipes.** Four `pipe2(O_CLOEXEC)`: standard input, output, error, and
   an exec-error pipe. Standard descriptors 0 to 2 are always open
   (`agent.S` gives closed ones `/dev/null`, as Rust's runtime does), so
   no pipe lands on them.
4. **fork**, then in the child: `dup2` the pipes onto 0, 1, 2; `chdir`;
   an empty signal mask and `SIGPIPE` back to its default (the agent
   ignores it, as Rust's runtime does, and an ignored disposition would
   survive `execve`); then musl's `execvp` (`src/process/execvp.c`,
   `__execvpe`) over the child's own environment: a name containing `/` is
   run as it is; otherwise each `PATH` entry is tried in order (musl's
   default `/usr/local/bin:/bin:/usr/bin` when the environment has no
   `PATH`), `EACCES` is remembered, `ENOENT` and `ENOTDIR` move on, any
   other error stops. A failure at any step writes its errno to the
   exec-error pipe and exits 1.
5. **The parent** closes the child's ends and reads the exec-error pipe:
   end of file means the exec happened (the pipe is close-on-exec);
   four bytes are the child's errno, answered as `Error("<program>: <why>
   (os error N)")` with musl's message text (`errno.md`), after reaping
   the child.

## The command loop

Input: the pipe is non-blocking, and what the command has not taken yet
waits in `in_buf`. While more than `INPUT_BACKLOG` (1 MiB) waits, the
device is not read at all, so the backlog builds in the ring and the host's
buffers rather than in card memory; `Stat` requests behind it wait too,
exactly as in the Rust agent. After `StdinEof` the pipe is closed once
drained; a pipe that breaks (the command closed its input or exited)
drops what is queued. `in_buf` holds the backlog plus whatever the
decoder had already cut when reading stopped (under `RXCAP`), so 4 MiB.

Each pass:

1. `wait4(WNOHANG)`: the command has exited, go to the end.
2. Write what the input pipe takes.
3. A frame already buffered is handled at once (`Stdin` queued,
   `StdinEof` noted, `Stat` answered, anything else logged as "dropping
   <name> during a command" and dropped: an `Error` would tell the daemon
   the session is over while the command runs on), then the output pipes
   are looked at without waiting.
4. Otherwise `poll` for up to `CHILD_POLL_MS` (20 ms) on the device (unless
   the backlog is full), the input pipe (while it has something queued),
   the two output pipes and a pidfd of the child (`pidfd_open`; without
   one the 20 ms bound still holds). Output is read `CHUNK` (32 KiB) at a
   time and sent as one frame; end of file or an error closes that pipe.

## The end

The input pipe is closed; the output pipes get `OUTPUT_GRACE_MS` (300 ms)
to reach end of file; then `Exit` goes out and the output pipes are closed.
A process the command left in the background (a daemon) may keep the
pipes open: its later output belongs to no session and is lost, as with
the Rust agent, which abandoned its pump threads after the same grace.
The exit code is the status, or 128 plus the signal (Rust's
`ExitStatus::code` and `signal`). `in_buf`'s pages are then handed back
(`madvise(MADV_DONTNEED)`), so a large input does not stay resident.

## Testing

`host/crates/phi-rpc/tests/agent.rs`: streams and status, a signal,
`PATH` lookup, `cwd`, the environment's order and deduplication, musl's
default path, every spawn failure text (empty argv, NUL, missing program,
bad working directory, not executable), 3 MiB each way through `cat`
with the input written from another thread as the relay does, `Stat`
during a command, a background child holding the pipes.
