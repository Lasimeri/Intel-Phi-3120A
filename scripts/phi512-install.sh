#!/usr/bin/env bash
# phi512-install.sh: put the AVX-512 layer on this system.
#
#   scripts/phi512-install.sh              # PATH wrapper only (default, safe)
#   scripts/phi512-install.sh --system     # every process, via /etc/ld.so.preload
#   scripts/phi512-install.sh --uninstall  # remove both
#   scripts/phi512-install.sh --status
#
# The default installs a `phi512` command and the shared library, and
# changes nothing about how other programs start. `--system` is the
# seamless option and is opt-in on purpose: see the warning it prints.
#
# See phi512-install.md.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)

LIB=/usr/lib/libphi512.so
PRELOAD=/etc/ld.so.preload
BIN=/usr/local/bin/phi512
bold=$'\033[1m'; green=$'\033[32m'; yellow=$'\033[33m'; red=$'\033[31m'; off=$'\033[0m'
[ -t 1 ] || { bold=; green=; yellow=; red=; off=; }

mode=wrapper
case "${1:-}" in
    --system) mode=system ;;
    --uninstall) mode=uninstall ;;
    --status) mode=status ;;
    "") ;;
    *) echo "$0: unknown option $1" >&2; exit 2 ;;
esac

as_root() { if [ "$(id -u)" -eq 0 ]; then "$@"; else sudo "$@"; fi; }

cmd_status() {
    printf "%-28s " "shared library"
    [ -f "$LIB" ] && echo "${green}installed${off} ($LIB)" || echo "not installed"
    printf "%-28s " "phi512 command"
    [ -x "$BIN" ] && echo "${green}installed${off} ($BIN)" || echo "not installed"
    printf "%-28s " "system-wide preload"
    if [ -f "$PRELOAD" ] && grep -qF "$LIB" "$PRELOAD" 2>/dev/null; then
        echo "${yellow}ACTIVE${off} (every process; $PRELOAD)"
    else
        echo "off"
    fi
    printf "%-28s " "this host has AVX-512"
    grep -qw avx512f /proc/cpuinfo && echo "yes (the layer is unnecessary)" || echo "no"
}

# Every way this can be turned off again, printed where someone who needs
# it will see it. This matters because /etc/ld.so.preload is read for
# setuid binaries too, so a broken library takes sudo with it.
print_recovery() {
    cat <<RECOVERY

${bold}If anything goes wrong, any one of these undoes it:${off}
  PHI512_DISABLE=1 <command>        turn the layer off for one command, no root
  sudo rm $PRELOAD    turn it off for the system
  if sudo itself is broken: boot with ${bold}init=/bin/sh${off} on the kernel
  command line, then: mount -o remount,rw / && rm $PRELOAD
RECOVERY
}

# Run a set of ordinary programs with the library forced in. Nothing here
# uses AVX-512: the point is that loading the library changes nothing for
# programs that do not.
self_test() {
    local lib="$1" failed=0
    echo "checking that ordinary programs are unaffected..."
    for prog in "/bin/true" "/bin/echo phi512" "/usr/bin/id -u" "/usr/bin/sudo --version"; do
        # shellcheck disable=SC2086
        if LD_PRELOAD="$lib" $prog >/dev/null 2>&1; then
            printf "  %-24s ${green}ok${off}\n" "$(basename "${prog%% *}")"
        else
            printf "  %-24s ${red}FAILED${off}\n" "$(basename "${prog%% *}")"
            failed=1
        fi
    done
    return $failed
}

case "$mode" in
status) cmd_status; exit 0 ;;

uninstall)
    if [ -f "$PRELOAD" ]; then
        as_root sed -i "\\|$LIB|d" "$PRELOAD"
        [ -s "$PRELOAD" ] || as_root rm -f "$PRELOAD"
        echo "removed the system-wide preload"
    fi
    as_root rm -f "$LIB" "$BIN"
    echo "${green}uninstalled${off}. Ordinary programs are unaffected either way."
    exit 0
    ;;
esac

# --- build and install the library ------------------------------------
lib_src="$root/host/target/release/libphi512.so"
if [ ! -f "$lib_src" ]; then
    echo "building the release library..."
    (cd "$root/host" && cargo build --release -p phi512 -q)
fi
[ -f "$lib_src" ] || { echo "$0: $lib_src was not built" >&2; exit 1; }

# The library is tested from its build location first, so a bad build
# never reaches /usr/lib.
if ! self_test "$lib_src"; then
    echo "${red}the library breaks ordinary programs; nothing was installed${off}" >&2
    exit 1
fi

as_root install -Dm755 "$lib_src" "$LIB"
as_root install -Dm755 "$here/phi512.sh" "$BIN"

echo "${green}installed${off} $LIB and $BIN"

if [ "$mode" = wrapper ]; then
    cat <<DONE

Run a program that needs AVX-512 with:
  ${bold}phi512 ./my-program${off}

To make it automatic for every process instead:
  ${bold}$0 --system${off}
DONE
    exit 0
fi

# --- system-wide ------------------------------------------------------
cat <<WARNING

${yellow}${bold}About to make this apply to every process on the system.${off}
$PRELOAD is read for setuid binaries too, so a fault in this
library would take ${bold}sudo${off} with it. It has just been checked against
ordinary programs, and it will be checked again immediately after.
WARNING
print_recovery
printf "\ncontinue? [y/N] "
read -r reply </dev/tty || reply=n
case "$reply" in [Yy]*) ;; *) echo "nothing changed."; exit 0 ;; esac

if [ -f "$PRELOAD" ]; then
    as_root cp -a "$PRELOAD" "$PRELOAD.before-phi512"
    echo "kept the previous $PRELOAD as $PRELOAD.before-phi512"
fi
if ! grep -qF "$LIB" "$PRELOAD" 2>/dev/null; then
    echo "$LIB" | as_root tee -a "$PRELOAD" >/dev/null
fi

# Verify with the file actually in place, and undo it at once if the
# system is not healthy. This is the check that matters: the earlier one
# used LD_PRELOAD, which setuid binaries ignore.
echo "verifying with the preload live..."
ok=1
for prog in "/bin/true" "/usr/bin/id -u" "/usr/bin/sudo --version"; do
    # shellcheck disable=SC2086
    $prog >/dev/null 2>&1 || { printf "  %-24s ${red}FAILED${off}\n" "$(basename "${prog%% *}")"; ok=0; }
done
if [ "$ok" != 1 ]; then
    as_root sed -i "\\|$LIB|d" "$PRELOAD"
    [ -s "$PRELOAD" ] || as_root rm -f "$PRELOAD"
    echo "${red}the system was not healthy with the preload active, so it has been removed again.${off}" >&2
    exit 1
fi

echo "${green}${bold}active system-wide.${off} Any program needing AVX-512 now runs here."
print_recovery
