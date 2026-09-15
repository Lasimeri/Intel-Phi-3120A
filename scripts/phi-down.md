# phi-down.sh

Halts the card started by `phi-up.sh` (a `poweroff -f` through the agent,
which the platform layer answers with POST "KH") and ends the `phictl boot`
process, which resets the card when it closes the VFIO device; removes the
pid file and the socket. Safe to run when nothing is up.
