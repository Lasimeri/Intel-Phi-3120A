# phi-run.sh

`scripts/phi-run.sh CMD ARGS...` runs a command on the card started by
`phi-up.sh`: a thin wrapper that points `phictl exec` at the socket in the
user's runtime directory. Standard input, output, error and the exit status
are relayed.
