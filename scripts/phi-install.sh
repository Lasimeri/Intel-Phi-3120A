#!/usr/bin/env bash
# phi-install.sh: one command from a fresh Arch Linux install to a running
# card. Every stage checks whether it is already done before doing anything,
# so this can be rerun at any time and after any failure; it picks up where
# it stopped. Nothing here reimplements a build: each stage calls the same
# script a person would have called by hand.
#
#   scripts/phi-install.sh              run every pending stage, in order
#   scripts/phi-install.sh status       what is done, what is not, what it makes
#   scripts/phi-install.sh run STAGE    force one stage, done or not
#   scripts/phi-install.sh list         stage ids
#
# Options: --full also builds the card-native toolchain (phase P7, slow and
# only needed to compile on the card), --dry-run prints what would run,
# --yes does not stop to confirm the long builds.
#
# See phi-install.md.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)
cd "$root"

full=0; dry=0; assume_yes=0
args=()
while [ $# -gt 0 ]; do
    case "$1" in
        --full) full=1; shift ;;
        --dry-run) dry=1; shift ;;
        --yes|-y) assume_yes=1; shift ;;
        *) args+=("$1"); shift ;;
    esac
done
set -- "${args[@]:-}"

bold=$'\033[1m'; dim=$'\033[2m'; green=$'\033[32m'; yellow=$'\033[33m'; red=$'\033[31m'; off=$'\033[0m'
[ -t 1 ] || { bold=; dim=; green=; yellow=; red=; off=; }

# Stage order is dependency order. Each id has a check_ and a run_ function
# and one line of description below.
stages=(packages card vfio host-tools llvm sysroot llvm-dylib rust-std userland sshkey agent kernel initramfs disk cli autoboot)
stages_full=(libcxx llvm-card clang-pkg)

describe() {
    case "$1" in
        packages)   echo "host packages, the phi group, the udev rule, memlock" ;;
        card)       echo "the card is enumerated and its link is healthy" ;;
        vfio)       echo "vfio-pci owns the card" ;;
        host-tools) echo "phictl, phitop, phi-isa-audit, the MVEX encoder" ;;
        llvm)       echo "the patched clang and lld that can target this card" ;;
        sysroot)    echo "musl, compiler-rt and libunwind for the card" ;;
        llvm-dylib) echo "libLLVM.so for rustc (needed to build card Rust)" ;;
        rust-std)   echo "core, alloc and std cross-built for the card" ;;
        userland)   echo "static busybox and dropbear" ;;
        sshkey)     echo "the ssh key the card will trust" ;;
        agent)      echo "phi-agent, the card end of the control channel" ;;
        kernel)     echo "the patched Linux kernel for the card" ;;
        initramfs)  echo "the boot image: init, busybox, dropbear, agent" ;;
        disk)       echo "a persistent disk image the card mounts on /data" ;;
        cli)        echo "phi, phictl and phitop on PATH" ;;
        autoboot)   echo "a user service that runs the card at boot" ;;
        libcxx)     echo "libc++ and libc++abi for the card" ;;
        llvm-card)  echo "clang and lld built to run ON the card" ;;
        clang-pkg)  echo "the native toolchain package pushed to the card" ;;
    esac
}

produces() {
    case "$1" in
        packages)   echo "group phi, /etc/udev/rules.d" ;;
        card)       echo "(a check, produces nothing)" ;;
        vfio)       echo "the card bound to vfio-pci" ;;
        host-tools) echo "host/target/debug/phictl" ;;
        llvm)       echo "toolchain/build/llvm/bin/clang" ;;
        sysroot)    echo "toolchain/build/sysroot/usr/lib/libc.a" ;;
        llvm-dylib) echo "toolchain/build/llvm-dylib/lib/libLLVM*.so" ;;
        rust-std)   echo "card Rust std in the rustc sysroot" ;;
        userland)   echo "card/userland/build/busybox/busybox" ;;
        sshkey)     echo "$HOME/.ssh/phi_ed25519" ;;
        agent)      echo "card/agent/target/x86_64-knc-linux-musl/release/phi-agent" ;;
        kernel)     echo "card/kernel/build/out/arch/x86/boot/bzImage" ;;
        initramfs)  echo "card/initramfs/build/initramfs.cpio.gz" ;;
        disk)       echo "${PHI_DISK:-\$PHI_DISK (unset)}" ;;
        cli)        echo "$HOME/.local/bin/phi" ;;
        autoboot)   echo "${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user/phi.service" ;;
        libcxx)     echo "libc++.a in the sysroot" ;;
        llvm-card)  echo "toolchain/build/llvm-build-card" ;;
        clang-pkg)  echo "card/userland/build/clang/phi-clang.tar.gz" ;;
    esac
}

# Stages that need root. They are run with sudo, and only these.
needs_root() { case "$1" in packages|vfio) return 0 ;; *) return 1 ;; esac; }

# Long stages get a confirmation unless --yes, because an unattended hour is
# worth asking about.
is_long() { case "$1" in llvm|llvm-dylib|llvm-card) return 0 ;; *) return 1 ;; esac; }

bdf() { lspci -Dn -d 8086:225d 2>/dev/null | awk '{print $1; exit}'; }

check() {
    case "$1" in
        packages)   getent group phi >/dev/null 2>&1 && command -v clang >/dev/null && command -v tcc >/dev/null ;;
        card)       [ -n "$(bdf)" ] ;;
        vfio)       local b; b=$(bdf); [ -n "$b" ] && [ "$(basename "$(readlink -f "/sys/bus/pci/devices/$b/driver" 2>/dev/null)" 2>/dev/null)" = vfio-pci ] ;;
        host-tools) [ -x host/target/debug/phictl ] ;;
        llvm)       [ -x toolchain/build/llvm/bin/clang ] ;;
        sysroot)    [ -f toolchain/build/sysroot/usr/lib/libc.a ] ;;
        llvm-dylib) compgen -G "toolchain/build/llvm-dylib/lib/libLLVM*.so*" >/dev/null 2>&1 ;;
        rust-std)   compgen -G "$(rustc --print sysroot 2>/dev/null)/lib/rustlib/x86_64-knc-linux-musl/lib/libcore-*.rlib" >/dev/null 2>&1 \
                    || [ -x card/agent/target/x86_64-knc-linux-musl/release/phi-agent ] ;;
        userland)   [ -x card/userland/build/busybox/busybox ] && compgen -G "card/userland/build/dropbear/*dropbear*" >/dev/null 2>&1 ;;
        sshkey)     [ -f "$HOME/.ssh/phi_ed25519" ] ;;
        agent)      [ -x card/agent/target/x86_64-knc-linux-musl/release/phi-agent ] ;;
        kernel)     [ -s card/kernel/build/out/arch/x86/boot/bzImage ] ;;
        initramfs)  [ -s card/initramfs/build/initramfs.cpio.gz ] ;;
        disk)       [ -n "${PHI_DISK:-}" ] && [ -f "${PHI_DISK}" ] ;;
        cli)        [ -x "$HOME/.local/bin/phi" ] ;;
        autoboot)   [ -f "${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user/phi.service" ] ;;
        libcxx)     [ -f toolchain/build/sysroot/usr/lib/libc++.a ] ;;
        llvm-card)  [ -d toolchain/build/llvm-build-card ] ;;
        clang-pkg)  [ -f card/userland/build/clang/phi-clang.tar.gz ] ;;
        *)          return 1 ;;
    esac
}

run_stage() {
    case "$1" in
        packages)   sudo scripts/setup-arch.sh ;;
        card)       scripts/verify-card.sh ;;
        vfio)       sudo scripts/bind-vfio.sh ;;
        host-tools) make build ;;
        llvm)       toolchain/llvm/build.sh all ;;
        sysroot)    toolchain/musl/build.sh && toolchain/compiler-rt/build.sh && toolchain/libunwind/build.sh ;;
        llvm-dylib) PHI_LLVM_VARIANT=dylib toolchain/llvm/build.sh all ;;
        rust-std)   toolchain/rust/gen-target.sh && toolchain/rust/build-std.sh ;;
        userland)   card/userland/components/busybox.sh && card/userland/components/dropbear.sh ;;
        sshkey)     ssh-keygen -t ed25519 -N '' -f "$HOME/.ssh/phi_ed25519" ;;
        agent)      card/agent/build.sh ;;
        kernel)     card/kernel/build.sh all ;;
        initramfs)  card/initramfs/build.sh ;;
        disk)       [ -n "${PHI_DISK:-}" ] || { echo "phi-install: set PHI_DISK to where the card's disk image should live" >&2; return 1; }
                    scripts/phi-disk.sh create "$PHI_DISK" "${PHI_DISK_SIZE:-64G}" ;;
        cli)        scripts/phi.sh install-cli ;;
        autoboot)   scripts/phi-autoboot.sh install "${PHI_DISK:-}" "${PHI_HOST_MEM:-6G}" ;;
        libcxx)     toolchain/libcxx/build.sh ;;
        llvm-card)  PHI_LLVM_VARIANT=card toolchain/llvm/build.sh configure && PHI_LLVM_VARIANT=card toolchain/llvm/build.sh build ;;
        clang-pkg)  card/userland/components/clang.sh ;;
        *)          echo "phi-install: no such stage: $1" >&2; return 1 ;;
    esac
}

all_stages() { printf '%s\n' "${stages[@]}"; [ "$full" = 1 ] && printf '%s\n' "${stages_full[@]}"; return 0; }

cmd_status() {
    local done_n=0 todo_n=0
    printf "%s%-12s %-6s %-52s %s%s\n" "$bold" STAGE STATE "WHAT IT IS" "PRODUCES" "$off"
    while read -r s; do
        if check "$s" 2>/dev/null; then
            printf "%-12s ${green}%-6s${off} %-52s ${dim}%s${off}\n" "$s" done "$(describe "$s")" "$(produces "$s")"
            done_n=$((done_n + 1))
        else
            printf "%-12s ${yellow}%-6s${off} %-52s ${dim}%s${off}\n" "$s" todo "$(describe "$s")" "$(produces "$s")"
            todo_n=$((todo_n + 1))
        fi
    done < <(all_stages)
    echo
    if [ "$todo_n" -eq 0 ]; then
        echo "${green}everything is installed${off}. Try: phi status, phi run nproc"
    else
        echo "$done_n done, $todo_n to go. Run ${bold}scripts/phi-install.sh${off} to continue."
    fi
    [ "$full" = 1 ] || echo "${dim}(--full adds the card-native toolchain: compiling on the card itself)${off}"
}

cmd_install() {
    local todo=() s
    while read -r s; do check "$s" 2>/dev/null || todo+=("$s"); done < <(all_stages)

    if [ "${#todo[@]}" -eq 0 ]; then
        echo "${green}Nothing to do: every stage is already installed.${off}"
        echo "Try: phi status, phi run nproc"
        return 0
    fi

    echo "${bold}${#todo[@]} stage(s) to run:${off} ${todo[*]}"
    if [ "$dry" = 1 ]; then
        for s in "${todo[@]}"; do echo "  would run: $s  ($(describe "$s"))"; done
        return 0
    fi

    # Ask for the password once, up front, instead of halfway through an hour
    # of building.
    for s in "${todo[@]}"; do
        if needs_root "$s"; then
            echo "${dim}Some stages need root ($s). Asking for sudo now so the rest runs unattended.${off}"
            sudo -v
            break
        fi
    done

    local start_all; start_all=$(date +%s)
    for s in "${todo[@]}"; do
        if is_long "$s" && [ "$assume_yes" = 0 ]; then
            echo
            echo "${yellow}$s takes a long time${off} ($(describe "$s"); the LLVM builds are 15 to 55 minutes)."
            printf "run it now? [Y/n] "
            read -r reply </dev/tty || reply=y
            case "$reply" in [Nn]*) echo "stopping. Rerun when ready; finished stages will not repeat."; return 0 ;; esac
        fi
        echo
        echo "${bold}== $s ==${off} $(describe "$s")"
        local t0; t0=$(date +%s)
        if ! run_stage "$s"; then
            echo
            echo "${red}stage $s failed.${off} Fix the cause and rerun scripts/phi-install.sh;"
            echo "the stages that already finished will not run again."
            return 1
        fi
        if ! check "$s" 2>/dev/null; then
            echo
            echo "${red}stage $s reported success but did not produce $(produces "$s").${off}"
            return 1
        fi
        echo "${green}== $s done${off} in $(( $(date +%s) - t0 ))s"
    done
    echo
    echo "${green}${bold}Installed${off} in $(( $(date +%s) - start_all ))s."
    echo "Next: ${bold}phi up${off} then ${bold}phi run nproc${off} (expect 228)."
}

case "${1:-install}" in
    ""|install) cmd_install ;;
    status)     cmd_status ;;
    list)       all_stages ;;
    run)        [ -n "${2:-}" ] || { echo "usage: $0 run STAGE" >&2; exit 2; }
                echo "${bold}== $2 ==${off} $(describe "$2")"; run_stage "$2" ;;
    -h|--help)  sed -n '2,17p' "$0" | sed 's/^# \{0,1\}//' ;;
    *)          echo "$0: unknown command ${1}; try status, list, run STAGE" >&2; exit 2 ;;
esac
