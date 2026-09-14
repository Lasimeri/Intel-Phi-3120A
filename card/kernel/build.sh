#!/usr/bin/env bash
# build.sh: the card kernel. A pinned mainline tag plus the patch series in
# patches/SERIES, configured from config/knc.config on top of
# x86_64_defconfig, compiled with the project's patched clang through the
# knc-cc wrapper, and audited with phi-isa-audit. See build.md.
#
# Usage: card/kernel/build.sh [fetch|patch|configure|build|audit|all]
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
. "$here/../../toolchain/env.sh"
# env.sh exports CC=knc-cc for third-party userland builds. The kernel gets
# its compilers from the make variables below; keep the environment clean.
unset CC CXX

TAG="${PHI_KERNEL_TAG:-$(sed -n 's/^# Linux tag: //p' "$here/patches/SERIES")}"
SRC="${PHI_KERNEL_SRC:-$phi_build/kernel/linux}"
OUT="${PHI_KERNEL_OUT:-$phi_build/kernel/out}"
JOBS="${PHI_JOBS:-$(nproc)}"
step="${1:-all}"
# Space-free alias paths for everything make and merge_config.sh see.
frag="$phi_root/card/kernel/config/knc.config"
patches="$phi_root/card/kernel/patches"
AUDIT="$phi_root/host/target/debug/phi-isa-audit"

# LLVM=1 takes ld.lld, llvm-ar, llvm-nm, llvm-objcopy and friends from PATH,
# where env.sh put the patched install first. CC is the wrapper so that
# every C and assembly file, the decompressor included (its Makefile
# replaces KBUILD_CFLAGS), gets the deletion-list flags. Host tools must
# use the distro compiler: the patched clang defaults to the musl target.
MAKE=(make -C "$SRC" O="$OUT" LLVM=1 LLVM_IAS=1 CC=knc-cc
      HOSTCC=/usr/bin/clang HOSTCXX=/usr/bin/clang++
      HOSTLD=/usr/bin/ld.lld HOSTAR=/usr/bin/llvm-ar
      KBUILD_BUILD_USER=phi KBUILD_BUILD_HOST=intel-phi-3120a)

fetch() {
    if [ ! -d "$SRC/.git" ]; then
        echo "== fetching linux $TAG (shallow)"
        git clone --depth 1 --branch "$TAG" \
            https://git.kernel.org/pub/scm/linux/kernel/git/stable/linux.git "$SRC"
    else
        echo "== have linux at $(git -C "$SRC" describe --tags --always)"
    fi
}

patch_tree() {
    echo "== resetting to $TAG and applying $patches/SERIES"
    git -C "$SRC" reset -q --hard "$TAG" && git -C "$SRC" clean -qfd
    while IFS= read -r p; do
        case "$p" in ''|'#'*) continue ;; esac
        echo "== $p"
        git -C "$SRC" -c user.name="Intel Phi 3120A build" -c user.email="build@intel-phi-3120a.invalid" \
            am -q "$patches/$p"
    done < "$patches/SERIES"
    git -C "$SRC" log --oneline "$TAG"..HEAD | tac
}

configure() {
    echo "== x86_64_defconfig + $frag into $OUT"
    mkdir -p "$OUT"
    "${MAKE[@]}" x86_64_defconfig
    (cd "$SRC" && KCONFIG_CONFIG="$OUT/.config" scripts/kconfig/merge_config.sh -m -O "$OUT" "$OUT/.config" "$frag")
    "${MAKE[@]}" olddefconfig
    if ! grep -q '^CONFIG_X86_KNC=y' "$OUT/.config"; then
        echo "configure: CONFIG_X86_KNC is not set; the patch series is not applied (run: $0 patch)" >&2
        exit 1
    fi
    # Every option the fragment asked for must have survived olddefconfig.
    local bad=0
    while IFS= read -r line; do
        case "$line" in ''|'#'*) continue ;; esac
        local sym="${line%%=*}" val="${line#*=}"
        if [ "$val" = n ]; then
            grep -q "^$sym=" "$OUT/.config" && { echo "configure: $sym should be off"; bad=1; }
        else
            grep -qx "$line" "$OUT/.config" || { echo "configure: $line not in .config"; bad=1; }
        fi
    done < "$frag"
    [ "$bad" = 0 ] || { echo "configure: fragment not fully honored (see above)" >&2; exit 1; }
    echo "== configuration matches the fragment"
}

build() {
    echo "== building bzImage with $JOBS jobs ($(knc-cc --version | head -1))"
    "${MAKE[@]}" -j"$JOBS" bzImage
    ls -l "$OUT/vmlinux" "$OUT/arch/x86/boot/bzImage"
}

audit() {
    echo "== audit vmlinux (.altinstr_replacement is skipped by the tool)"
    # Two documented exceptions, both instruction-level false positives that
    # the static audit cannot see through:
    # - FSGSBASE: rdgsbase/wrgsbase in paranoid_entry/exit and the NMI path
    #   sit behind ALTERNATIVE "jmp over", "", X86_FEATURE_FSGSBASE, so they
    #   are jumped over unless CPUID reports FSGSBASE.
    # - CMPXCHG16B: arch_cmpxchg128() is inlined only where the caller first
    #   tests system_has_cmpxchg128() (CPUID CX16), such as the SLUB freelist;
    #   the per-CPU variant is an alternative with an emulation fallback.
    # - MONITOR/MWAIT: mwait_idle and mwait_play_dead, behind X86_FEATURE_MWAIT.
    # - XSAVE/XSAVES/AMX_TILE: fpu/xstate paths, behind X86_FEATURE_XSAVE and
    #   AMX_TILE (fpu__init_system_xstate returns early without XSAVE).
    # - INVPCID: flush_tlb_*, behind static_cpu_has(X86_FEATURE_INVPCID).
    # - SERIALIZE: sync_core(), behind static_cpu_has(X86_FEATURE_SERIALIZE).
    # - WAITPKG: delay_halt_tpause, selected only with X86_FEATURE_WAITPKG.
    # - RDRAND/RDSEED: x86_init_rdrand and arch_get_random_*, behind CPUID.
    # - MMX: one emms under X86_BUG_FXSAVE_LEAK, an AMD-only erratum.
    # - IN/OUT: vmware_hypercall_slow, reached only when VMware is detected.
    "$AUDIT" --ignore FSGSBASE --ignore CMPXCHG16B --ignore MONITOR --ignore MWAIT \
        --ignore XSAVE --ignore XSAVES --ignore AMX_TILE --ignore INVPCID \
        --ignore SERIALIZE --ignore WAITPKG --ignore RDRAND --ignore RDSEED \
        --ignore MMX --ignore "IN/INS (port I/O)" --ignore "OUT/OUTS (port I/O)" \
        "$OUT/vmlinux"
}

case "$step" in
    fetch) fetch ;;
    patch) fetch; patch_tree ;;
    configure) fetch; configure ;;
    build) build ;;
    audit) audit ;;
    all) fetch; patch_tree; configure; build; audit ;;
    *) echo "usage: build.sh [fetch|patch|configure|build|audit|all]" >&2; exit 2 ;;
esac
