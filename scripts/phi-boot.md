# phi-boot.sh: boot one card, with everything it owns

The single place the `phictl boot` command line for a card is assembled.
`phi@N.service` runs it in the foreground as `phi-boot.sh -c N`;
`phi-up.sh` runs the same script in the background. A card booted by hand
and a card booted at host boot are therefore identical: same socket, same
forward port, same disk, same host memory, same subnet, same hostname.

What card N gets (from `phi-env.sh`):

- `--card N`, so the daemon opens the right PCI device
- `--serve` on that card's control socket
- `--forward 2222+N:22` and `--net-addr 10.9.N.1/24`, the host's end of
  that card's ring network
- kernel command line `PHI_CARD_ADDR=10.9.N.2/24 PHI_HOSTNAME=phiN`,
  which the card's init reads from its environment (the kernel passes
  unknown `name=value` parameters to init that way)
- `--disk` from the card's line in `~/.config/phi/cards`, if any
- `--host-mem` from the same line, default 6G; `none` to give it nothing

`PHI_CMDLINE_EXTRA` is appended to the kernel command line, as before.
Extra arguments go to `phictl boot` verbatim.
