# tests/agent.rs: the card agent against phi-rpc

The card agent (`card/agent`) is x86-64 assembly and no longer shares this
crate's source, so this test is what keeps the two ends of the rpc channel
in agreement. It builds the agent with `card/agent/build.sh --out` into
Cargo's test directory (so `make check` always tests the current sources),
runs it on the host, and talks to it over a Unix socket pair given as its
standard input (`--device -`). Every frame sent is encoded by `Msg::encode`
and every reply decoded by `Decoder`, so a byte of disagreement on the wire
fails a decode here rather than on a card.

The host can run the agent because its instructions are a subset of the
host's (`passes_the_knc_audit` holds it to that with `phi-isa-audit`) and
its system calls are the x86-64 Linux ones the card's kernel has too.

| test | what |
| --- | --- |
| `passes_the_knc_audit` | the binary has no instruction Knights Corner lacks, and no suspect encodings |
| `ping_and_startup_line` | `Pong { "0.2.0" }`; the startup line on standard error |
| `exec_relays_streams_and_status` | stdout, stderr, the exit status; 128 + 9 for a killed shell |
| `exec_input_by_path_lookup` | `Stdin` frames into `cat` found through `PATH` |
| `exec_large_input_and_output` | 3 MiB each way through `cat`, written from a second thread as the daemon's relay does, no output frame over 32 KiB |
| `exec_failures_are_errors` | the texts Rust's `Command` gave: missing program, empty argv, NUL byte, bad working directory, not executable |
| `exec_cwd_and_environment` | `cwd`; the environment deduplicated (last value wins) and sorted by key; musl's default `PATH` |
| `stat_is_answered_during_a_command` | a `StatReply` while a command runs |
| `background_children_do_not_hold_the_session` | `Exit` within the 300 ms grace although a background child keeps the pipes, and the agent free afterwards |
| `put_and_get_round_trip` | 100 000 bytes out and back, a `Stat` mid-put, mode 0640 exactly |
| `file_failures_are_errors` | missing directory, a directory read as a file, a NUL in a path; the rest of a failed put dropped silently |
| `stray_and_bad_frames` | a stray host-bound frame answered with `Error`; stray input dropped; an unknown tag, a short body, a non-UTF-8 string and bad lengths logged and survived |
| `stat_from_a_fixture` | a whole `StatReply` against a `--root` tree, field by field, over six samples (kernel threads re-read every fourth) |
| `stat_without_hwmon_or_files` | an empty tree gives `Stat::default()` |
| `stat_of_this_host` | the host's CPU count, and the agent listing itself |

Needs GNU `as` and `ld` on the host (binutils, installed with the base
system on Arch) and `/bin/sh`, `/bin/pwd`, `/usr/bin/env`, `cat`; nothing
of the card.
