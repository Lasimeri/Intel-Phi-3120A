# env.sh: sourced by every toolchain and card build script.
#
# Two path problems are solved here, both caused by the repository living in
# a directory whose name contains spaces ("Intel Phi 3120A"):
#
# 1. Third-party build systems expand $CC, $DESTDIR and friends unquoted, so
#    scripts hand them a space-free symlink to the repository root
#    (~/.cache/intel-phi-3120a) instead of the real path.
# 2. make derives CURDIR from getcwd(), which returns the real path even when
#    entered through a symlink, so the build trees themselves must live at a
#    space-free real location. toolchain/build, card/kernel/build,
#    card/userland/build and card/initramfs/build are symlinks into
#    ~/.cache/intel-phi-3120a-build/, created here on first use.
#
# Usage: . "$(dirname "$0")/../env.sh"   (sets phi_root, phi_build, PATH, CC/CXX)
_env_here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
_env_real=$(cd "$_env_here/.." && pwd)
_cache="${XDG_CACHE_HOME:-$HOME/.cache}"

# Space-free alias of the repository root.
case "$_env_real" in
    *[[:space:]]*)
        _link="$_cache/intel-phi-3120a"
        mkdir -p "$_cache"
        if [ ! -L "$_link" ] || [ "$(readlink "$_link")" != "$_env_real" ]; then
            ln -sfn "$_env_real" "$_link"
        fi
        phi_root="$_link"
        ;;
    *)
        phi_root="$_env_real"
        ;;
esac
export phi_root

# Real, space-free build trees behind symlinks in the repository.
phi_build="${PHI_BUILD_ROOT:-$_cache/intel-phi-3120a-build}"
export phi_build
for _pair in "toolchain/build:toolchain" "card/kernel/build:kernel" "card/userland/build:userland" "card/initramfs/build:initramfs"; do
    _rel="$_env_real/${_pair%%:*}"; _name="${_pair##*:}"
    mkdir -p "$phi_build/$_name"
    if [ -d "$_rel" ] && [ ! -L "$_rel" ]; then
        # A real directory from before this scheme: migrate its contents.
        (shopt -s dotglob nullglob; mv "$_rel"/* "$phi_build/$_name"/ 2>/dev/null || true)
        rmdir "$_rel"
    fi
    [ -L "$_rel" ] || ln -sfn "$phi_build/$_name" "$_rel"
done

export PHI_LLVM="${PHI_LLVM:-$phi_root/toolchain/build/llvm}"
export PHI_SYSROOT="${PHI_SYSROOT:-$phi_root/toolchain/build/sysroot}"
export PATH="$phi_root/toolchain/clang:$PHI_LLVM/bin:$PATH"
export CC=knc-cc
export CXX=knc-c++
