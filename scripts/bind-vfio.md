# bind-vfio.sh

Hands the Xeon Phi cards to `vfio-pci`, or takes them back.

```
sudo scripts/bind-vfio.sh              # bind every 8086:225d device
sudo scripts/bind-vfio.sh bind BDF     # one card
sudo scripts/bind-vfio.sh unbind [BDF|all]
```

For each card: unbind whatever driver holds it, set `driver_override` to
`vfio-pci`, reprobe, and check that `vfio-pci` took it; then print the
IOMMU group and its `/dev/vfio/<group>` node, which the udev rule from
`setup-arch.sh` makes group `phi` writable.

This is the by-hand path. After `sudo scripts/setup-arch.sh` the modprobe
option `options vfio-pci ids=8086:225d` claims every card at boot without
it (`docs/results/2026-09-19-boot-without-login.md`), which is also how a
second card added later comes up bound with nothing typed: the ids match
is per device, not per card.
