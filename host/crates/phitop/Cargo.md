# phitop

The remote resource viewer for the card, run on the host. Depends on
`phi-rpc` for the `Stat` and `Traffic` messages, `clap` for the command
line, `anyhow` for errors and `libc` for the terminal; no TUI crate.
Built with the host workspace (`make build`), installed as
`host/target/debug/phitop`. Usage in `src/main.md` and
`docs/howto/monitoring.md`.
