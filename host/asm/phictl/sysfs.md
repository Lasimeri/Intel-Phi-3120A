# sysfs.S: PCI addresses, sysfs, the cards list

- `normalize_bdf`: `[dddd:]bb:dd.f`, hex, device at most 31, function at
  most 7, lowercased into `dddd:bb:dd.f` (the rules of
  `host/crates/phi-vfio/src/sysfs.rs`).
- `device_exists`, `ids_of`, `is_phi` (8086:225d), `iommu_group_of` (the
  `iommu_group` link's basename), `driver_of` (the `driver` link's
  basename), `read_attr`: under `/sys/bus/pci/devices/<bdf>/`
  (`Documentation/ABI/testing/sysfs-bus-pci`).
- `find_phis`: every Phi on the bus, sorted by address.
- `cards_list`: `$XDG_CONFIG_HOME/phi/cards` (else `~/.config/phi/cards`)
  when it exists, one card per line (address, optional disk image,
  optional host memory size, `#` comments), the line order being the
  card index; else the enumerated cards by address. A card listed but
  absent keeps its index. Duplicates, junk and more than 16 cards are
  errors that exit with the line number. Presence from sysfs.
- `resolve_card`, `card_entry`, `index_from_env` (`PHI_CARD`),
  `socket_path_for` (`PHICTL_SOCKET`, else `/run/phictl[/N]/control.sock`
  for root, else `$XDG_RUNTIME_DIR/phictl[/N]/control.sock`, else
  `/tmp/phictl-UID/...`), `hostmem_path` (`/dev/shm/phi-hostmem[-N]`):
  the per-card names of `host/crates/phi-vfio/src/cards.rs`.

`phictl cards` and `phictl cards --plain` are compared with the Rust
tool's output byte for byte (`docs/results/2026-09-29-phictl-assembly.md`).
