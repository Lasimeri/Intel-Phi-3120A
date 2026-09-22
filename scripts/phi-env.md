# phi-env.sh: which card, and what it owns

Sourced by every script that talks to a card (`phi.sh`, `phi-up.sh`,
`phi-down.sh`, `phi-run.sh`, `phi-boot.sh`, `phi-wait-vfio.sh`,
`phi-vpu.sh`). `phi_env "$@"` eats a leading `-c N` / `--card N` (else
`$PHI_CARD`, else 0), asks `phictl cards --plain` which PCI address that
index is, and derives the rest:

| variable | card 0 | card N |
| --- | --- | --- |
| `PHI_SOCK` | `<runtime>/phictl/control.sock` | `<runtime>/phictl/N/control.sock` |
| `PHI_PORT` | 2222 | 2222 + N |
| `PHI_HOSTMEM_FILE` | `/dev/shm/phi-hostmem` | `/dev/shm/phi-hostmem-N` |
| `PHI_HOST_IP`, `PHI_CARD_IP` | 10.9.0.1, 10.9.0.2 | 10.9.N.1, 10.9.N.2 |
| `PHI_HOSTNAME` | phi | phiN |
| `PHI_UNIT` | `phi@0.service` | `phi@N.service` |

Disk image and host-memory size come from the same line of
`~/.config/phi/cards` (card 0 also honours the old `PHI_DISK` and
`PHI_HOST_MEM`); host memory defaults to 6G.

The derivation is deliberately duplicated from
`host/crates/phi-vfio/src/cards.rs` rather than shelling out for each
value, and the two are kept in step by `phictl cards` being the only
source of the index-to-address mapping: a script can never think card 1
is a different device than the daemon does. Indices run 0 to 15.
