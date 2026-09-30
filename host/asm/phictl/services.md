# services.S: the services' turns

`services_open` creates what exists before the card is up (the control
socket's listener, so a client can connect and wait). `services_step`
calls each requested service's step once per pass of the daemon loop
(`console.md`): the relay (`serve.S`), the disk image (`disk.S`), host
memory (`disk.S`), the forwarder (`forward.S`), the TAP bridge
(`net.S`). A service that was not asked for costs one compare per pass.
Each step returns whether it moved bytes, which is what keeps the loop
spinning instead of sleeping.
