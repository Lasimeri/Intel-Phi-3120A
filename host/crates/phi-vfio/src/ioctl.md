# phi-vfio / ioctl.rs

Transcription of the parts of `/usr/include/linux/vfio.h` this project
uses. The header is the source of truth; if a constant here disagrees with
it, the header wins and this file is wrong.

## How the numbers were derived

`_IO(';', 100 + n)` with x86-64's `_IOC` encoding (`dir<<30 | size<<16 |
type<<8 | nr`) and `dir = size = 0` is simply `0x3B00 | (100 + n)`. The test
pins three of them: `VFIO_GET_API_VERSION = 0x3B64`,
`VFIO_GROUP_GET_DEVICE_FD = 0x3B6A`, `VFIO_IOMMU_UNMAP_DMA = 0x3B72`.

## Struct layouts

Every struct is `#[repr(C)]` with the same field order as the header. `u64`
fields in the header are `__aligned_u64`, which on x86-64 has 8-byte
alignment, the same as Rust's `u64`; the size tests would catch a mismatch.
`vfio_irq_set` has a trailing flexible array, so only its 20-byte header is
a struct; callers build the full buffer by hand (`device.rs`).

## Verified against

`linux-api-headers` as installed on this host on 2026-09-13 (kernel 7.x
headers). The grep used:

```
grep -nE '^#define VFIO_(GET_API_VERSION|...)' /usr/include/linux/vfio.h
awk '/^struct vfio_region_info \{/,/^\};/' /usr/include/linux/vfio.h
```
