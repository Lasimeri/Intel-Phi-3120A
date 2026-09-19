# net.rs: TAP bridge for the ring network channel

`phictl boot --net phi0` (and `phictl console --net phi0`) starts this
bridge on a second thread next to the console loop. It gives the host a
normal network interface to the card:

1. `open_tap` creates the TAP device with `TUNSETIFF` (`IFF_TAP |
   IFF_NO_PI`, so reads and writes are bare Ethernet frames). Root is
   required, which `phictl` already needs for VFIO.
2. `configure` runs `ip addr replace ADDR dev NAME` and `ip link set NAME up`
   (`--net-addr`, default `10.9.0.1/24`; the card's init script uses
   `10.9.0.2/24` on its `phi0`).
3. `bridge` attaches to the region's kind-2 channel (`phi_ring::Region`,
   `ChannelKind::Network`) once the header is valid, then loops at 1 kHz:
   a frame read from the TAP becomes one record (`u16` length, payload,
   padding to 4 bytes) in the host-to-card ring, written only when the ring
   has room for the whole record and dropped otherwise, like a full NIC
   queue; bytes drained from the card-to-host ring are reassembled into
   records and each frame is written to the TAP. A record with a zero or
   oversized length means the two sides disagree, and the pending bytes are
   discarded to resynchronise.

The card side is `arch/x86/kernel/knc_net.c` in the kernel series (patch
0021), polling at the same rate. Every 30 s the thread prints its counters
when they changed. Throughput is bounded by the aperture copies (8-byte
volatile accesses over PCIe) and the two polling intervals; it is meant for
SSH and file transfer, not bulk data. Interrupt signalling (SBOX doorbells)
would be the step beyond that; it is sketched in `docs/spec/ring-protocol.md`
under Signaling and has not been built.
