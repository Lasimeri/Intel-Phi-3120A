# tcp.S: a small TCP/IP stack

The Rust forwarder ran smoltcp (a full stack) over the ring's network
channel. This is the part of it the forwarder needs: Ethernet frames as
the channel's records (u16 length, frame, padding to 4), ARP for the
card's address (and answering the card's requests for ours), IPv4
without options or fragmentation (DF set, TTL 64), and TCP as an
active-open client.

**TCP, what is there**: the three-way handshake with an MSS option of
1460 (no window scaling, no SACK, no timestamps offered; the card may
send timestamps, which are skipped through the data offset); in-order
delivery into a 64 KiB receive buffer per connection, the advertised
window being that buffer's free space; segments that are not the next
expected are dropped and answered with an ACK restating the expectation
(the card retransmits from there); every received segment with data or a
FIN is acknowledged at once; sending from a 64 KiB send buffer within
the peer's window, at most 16 segments of up to 1460 bytes per pass; a
retransmission timer of 200 ms doubling to 3 s, resending one segment
from the first unacknowledged byte (or the SYN, or the FIN), given up
after 12 tries; the closes: the host side's end of file sends a FIN once
everything is acknowledged (FIN-WAIT-1, then 2, then closed on the
card's FIN; or LAST-ACK after the card's FIN); the card's FIN, once every
byte before it is delivered, shuts the host socket's write side
(CLOSE-WAIT); a reset from the card, or a host socket that fails, ends
the connection; TIME-WAIT is skipped, local ports run from 49152 up so
a port is not reused for 16383 connections.

**What is not there**: congestion control (the peer is one hop away on a
lossless ring; the 16-segment quota bounds the burst), window scaling,
SACK, delayed ACKs, out-of-order reassembly, path MTU, IPv6 (the card's
IPv6 multicast frames are seen and ignored), a persist timer for the
peer's zero window (Linux probes ours).

**Checksums**: the Internet checksum over 16-bit words read in memory
order, the complement stored in the same order, which is correct whatever
the byte order. The IP header's total length and checksum are written
after the TCP header is built (the SYN carries an option, so the header
length is not known before).

**The idle wait** (`tcp_fds`, called by `forward_fds`): each live
connection's host socket is watched for reading while the host has not
ended, the connection is established (or close-wait) and its send
buffer has room, and for writing while received bytes wait for it (the
last write met a full socket); a connection wanting neither is waiting
on the card, whose frames the daemon's passes read. The retransmission
timer (200 ms and up) and the ARP retry (500 ms) are checked by the
passes, which come at least every 2 ms (`console.md`).

`PHICTL_NET_TRACE=1` logs the first 400 frames (direction, length,
ethertype, and for TCP the ports, flags, sequence and acknowledgement
numbers) to standard error; the cap exists because an earlier uncapped
trace filled a tmpfs.

Verified on card 1 (`docs/results/2026-09-29-phictl-assembly.md`):
300000 random bytes to a listener on the card and back, byte-identical
(`tcpcat`, a 30-line tcc client, since the host has no netcat); ssh to
the card's dropbear, 16 MiB up and down byte-identical, two sessions at
once; the trace showing the handshake and bursts of 1514-byte segments.
