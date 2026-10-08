# services.S: the services' turns

`services_open` creates what exists before the card is up (the control
socket's listener, so a client can connect and wait). `services_step`
calls each requested service's step once per pass of the daemon loop
(`console.md`): the relay (`serve.S`), the disk image (`disk.S`), host
memory (`disk.S`), the forwarder (`forward.S`), the TAP bridge
(`net.S`). A service that was not asked for costs one compare per pass.
Each step returns whether it moved bytes, which is what keeps the loop
spinning instead of waiting; a step that reports movement when nothing
moved keeps the daemon on a whole core (the `pick_holder` defect found
on 2026-10-08, `docs/results/2026-10-08-phictl-idle.md`).

`services_fds` gives the daemon's idle wait the descriptors the same
services would act on in their next turn: the relay's (`serve_fds`),
the forwarder's (`forward_fds`), the bridge's (`net_fds`). The block
services have none; what they wait for is the card's.
