# 2026-09-13: first contact over VFIO

Host kernel 7.2.3-1-cachyos. Card bound with `scripts/bind-vfio.sh`.
Command: `sudo host/target/debug/phictl info` (run as root because the
`phi` group membership had not taken effect yet).

```
device        0000:2e:00.0
ids           8086:225d subsystem 8086:3c98
pci command   0x0006 (memory true bus master true)
aperture      16384 MiB mapped
postcode      0x00006330
spad2         0x040001c1: ready=true apic_id=224 download_addr=0x4000000
spad0         0x02e80000
spad1         0x00000000
spad2         0x040001c1
spad3         0x00000000
spad4         0x2800e6cf
spad5         0x00000000
spad6         0x02000403
spad7         0x00000373
spad8         0x00008086
spad9         0x3664fa13
spad10        0x00000000
spad11        0x000000ff
spad12        0x66400218
spad13        0x0000b120
spad14        0x00000000
spad15        0x00000000
```

## Readings

- VFIO path works end to end: group 30, both BARs mapped, memory decode
  and bus master already on (vfio-pci enables the device at open).
- `SPAD2` matches Intel's driver exactly: download address 64 MiB (the
  value the `mic_x100_load_ramdisk` comment calls typical), BSP APIC ID
  224 = 0xE0, which is thread 0 of core 56, the last core of 57.
- POST code raw `0x6330`. Interpreted as the ASCII pair Intel's table
  uses, that is `"0c"`, "Cache C code", an early bootstrap stage, which
  disagrees with the ready flag in `SPAD2`. Open: does the register stop
  updating after the early stages on this flash, or is it not the live
  POST register? The reset trace in the next record answers it.
- Scratchpads 0, 4, 6, 7, 8, 9, 11, 12, 13 hold non-zero values whose
  meaning is not documented in the sources used so far (`SPAD8 = 0x8086`
  is the vendor ID; the rest may be flash/SMC version words). To be
  decoded from `mpssd`/`micinfo` sources in `vendor/`.
