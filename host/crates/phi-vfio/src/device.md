# phi-vfio / device.rs

The device fd. Everything a host-side driver does to a PCI device goes
through it: BAR mmap, config space, interrupts, reset.

## Regions

VFIO exposes BAR0..BAR5 as regions 0..5, the option ROM as 6, and PCI config
space as 7, each with its own offset inside the device fd
(`VFIO_DEVICE_GET_REGION_INFO`). BARs with the MMAP flag are mmapped;
config space is read and written with `pread`/`pwrite`. On the Phi,
region 0 is the 16 GiB aperture and region 4 the 128 KiB MMIO block
(`phi-regs::sbox::{APER_BAR_INDEX, MMIO_BAR_INDEX}`).

## Config space

`vfio-pci` virtualizes the BAR registers and the MSI/MSI-X capabilities so
that userspace cannot move the device's windows; the COMMAND register's
memory-decode and bus-master bits pass through. `enable_memory_and_bus_master`
sets both; recent kernels already enable the device when the fd is opened,
so the write is usually a no-op, and the returned value shows the final
state for logging.

## Interrupts

`set_irq_eventfds` builds the variable-length `struct vfio_irq_set` by hand:
a 20-byte header followed by one `i32` eventfd per vector. Passing MSI-X
index 2 with N eventfds makes the kernel allocate N MSI-X vectors and route
each to its eventfd. `disable_irqs` tears the mode down.

Nothing calls this yet. Every channel on the ring polls (`docs/spec/
ring-protocol.md`, Signaling), so the card has never raised an interrupt
at the host. The code is here because the ioctl shape is the part that is
easy to get wrong, and it is unit-tested against the header layout.

## Reset

`VFIO_DEVICE_RESET` is a last resort. The card's own `RGCR` reset (phi-hw)
returns it to the bootstrap without touching the PCIe link, which is what
Intel's driver used. A bus reset also works because the card is the sole
function on bus 2e, but it retrains the link and takes longer.
