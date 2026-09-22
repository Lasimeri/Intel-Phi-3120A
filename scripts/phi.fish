# Completions for the `phi` command (scripts/phi.sh). Installed to
# ~/.config/fish/completions/phi.fish by `phi install-cli`. See phi.md.

set -l phi_cmds cards up down restart status run sh put get top sensors traffic console log disk vpu ssh-config install-cli help

complete -c phi -f
complete -c phi -s c -l card -d "card index, 0 to 15" -x -a "(phictl cards --plain 2>/dev/null | awk '{print \$1\"\t\"\$2}')"
complete -c phi -n "not __fish_seen_subcommand_from $phi_cmds" -a cards -d "every card: index, address, link, state"
complete -c phi -n "not __fish_seen_subcommand_from $phi_cmds" -a up -d "boot the card and serve it"
complete -c phi -n "not __fish_seen_subcommand_from $phi_cmds" -a down -d "power off and release the card"
complete -c phi -n "not __fish_seen_subcommand_from $phi_cmds" -a restart -d "down then up"
complete -c phi -n "not __fish_seen_subcommand_from $phi_cmds" -a status -d "unit, card, storage, access surface"
complete -c phi -n "not __fish_seen_subcommand_from $phi_cmds" -a run -d "run a command on the card"
complete -c phi -n "not __fish_seen_subcommand_from $phi_cmds" -a sh -d "interactive shell on the card"
complete -c phi -n "not __fish_seen_subcommand_from $phi_cmds" -a put -d "copy a file to the card"
complete -c phi -n "not __fish_seen_subcommand_from $phi_cmds" -a get -d "copy a file from the card"
complete -c phi -n "not __fish_seen_subcommand_from $phi_cmds" -a top -d "live viewer"
complete -c phi -n "not __fish_seen_subcommand_from $phi_cmds" -a sensors -d "temperatures, voltage, clock"
complete -c phi -n "not __fish_seen_subcommand_from $phi_cmds" -a traffic -d "PCIe bytes by path and direction"
complete -c phi -n "not __fish_seen_subcommand_from $phi_cmds" -a console -d "follow the card console"
complete -c phi -n "not __fish_seen_subcommand_from $phi_cmds" -a log -d "the daemon journal"
complete -c phi -n "not __fish_seen_subcommand_from $phi_cmds" -a disk -d "manage a disk image"
complete -c phi -n "not __fish_seen_subcommand_from $phi_cmds" -a vpu -d "the AVX-512 co-processor worker on this card"
complete -c phi -n "not __fish_seen_subcommand_from $phi_cmds" -a ssh-config -d "a ~/.ssh/config stanza per card"
complete -c phi -n "not __fish_seen_subcommand_from $phi_cmds" -a install-cli -d "symlink the CLI and completions"
complete -c phi -n "not __fish_seen_subcommand_from $phi_cmds" -a help -d "usage"

# `phi cards init [DIR]` writes ~/.config/phi/cards.
complete -c phi -n "__fish_seen_subcommand_from cards" -a init -d "write ~/.config/phi/cards from the bus"

# `phi put` takes a host path then a card path; `phi get` the reverse.
complete -c phi -n "__fish_seen_subcommand_from put" -F
complete -c phi -n "__fish_seen_subcommand_from get" -F
complete -c phi -n "__fish_seen_subcommand_from disk" -a "create check usage" -d "disk image operation"

# `phi top` options, from phitop --help.
complete -c phi -n "__fish_seen_subcommand_from top" -s i -l interval -d "seconds between samples" -x
complete -c phi -n "__fish_seen_subcommand_from top" -s b -l batch -d "print N plain frames and exit" -x
complete -c phi -n "__fish_seen_subcommand_from top" -s k -l kthreads -d "show kernel threads"

# `phi up all`, `phi down all`; `phi vpu` takes the worker script's verbs.
complete -c phi -n "__fish_seen_subcommand_from up down" -a all -d "every card"
complete -c phi -n "__fish_seen_subcommand_from vpu" -a "deploy start stop status log poly" -d "worker operation"
complete -c phi -n "__fish_seen_subcommand_from ssh-config" -l apply -d "append the missing stanzas"
