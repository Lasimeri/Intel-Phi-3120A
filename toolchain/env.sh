# env.sh: sourced by every toolchain build script.
# Third-party build systems (musl's configure, autotools, kernel Kbuild)
# split unquoted $CC and $DESTDIR on whitespace, and this repository's
# directory name contains spaces. The scripts therefore work through a
# space-free symlink to the repository root, created here on first use.
# Usage: . "$(dirname "$0")/../env.sh"   (sets phi_root, PATH, CC/CXX)
_env_here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
_env_real=$(cd "$_env_here/.." && pwd)
case "$_env_real" in
    *[[:space:]]*)
        _link="${XDG_CACHE_HOME:-$HOME/.cache}/intel-phi-3120a"
        mkdir -p "$(dirname "$_link")"
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
export PHI_LLVM="${PHI_LLVM:-$phi_root/toolchain/build/llvm}"
export PHI_SYSROOT="${PHI_SYSROOT:-$phi_root/toolchain/build/sysroot}"
export PATH="$phi_root/toolchain/clang:$PHI_LLVM/bin:$PATH"
export CC=knc-cc
export CXX=knc-c++
