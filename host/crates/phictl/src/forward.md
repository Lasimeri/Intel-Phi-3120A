# forward.rs: TCP to the card without root

`phictl boot --forward 2222:22` (also on `console`) makes phictl the host
end of the ring network itself: a smoltcp interface with MAC
`02:50:48:49:00:01` and address `10.9.0.1/24` whose frames go through the
ring's Ethernet channel, exactly what the card's `phi0` expects, but with
no TAP device and therefore no root. A listener on `127.0.0.1:2222`
accepts host connections; each becomes a smoltcp TCP connection to
`10.9.0.2:22` and bytes are shuttled in both directions from one 1 kHz
loop. `ssh -p 2222 root@localhost` reaches the card's dropbear from an
unprivileged boot; `scripts/phi-up.sh --ssh` sets this up.

Limits: TCP only, host-initiated, one port pair per option (`--forward`
may be repeated later if needed); throughput is the ring's. `--net` and
`--forward` are mutually exclusive: both would consume the same ring.
`RingDevice` implements smoltcp's `Device` over the `phi-ring`
producer/consumer pair with the same record framing as the bridge.
