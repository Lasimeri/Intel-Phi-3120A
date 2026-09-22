# phi-run.sh

Runs one command on a card through its control socket, from the user's
shell: `scripts/phi-run.sh [-c N] CMD [ARGS...]`. Stdin, stdout, stderr
and the exit status are relayed by `phictl exec`. `-c N` (else
`$PHI_CARD`, else 0) picks the card; `phi-env.sh` turns that into the
socket. `phi -c N run` is the same thing with the CLI's checks in front.
