# knc.cmake: CMake toolchain file for the card (knc64-x87 ABI).
#
# Point any CMake project at this with
#   cmake -DCMAKE_TOOLCHAIN_FILE=toolchain/cmake/knc.cmake
# and it cross-builds for the Xeon Phi 3120A through knc-cc and knc-c++,
# which carry the whole -mno-* set and the sysroot. See knc.md.
set(CMAKE_SYSTEM_NAME Linux)

# Deliberately not "x86_64". Projects branch on CMAKE_SYSTEM_PROCESSOR to
# add -march=native or -mavx512dq, and both are wrong here: native would
# describe the host, and AVX-512 is a different 512-bit encoding this card
# does not have. "knc" falls through those branches to "no flags", which is
# correct, because knc-cc already specifies the architecture.
set(CMAKE_SYSTEM_PROCESSOR knc)

set(_knc_root "${CMAKE_CURRENT_LIST_DIR}/../..")
set(CMAKE_C_COMPILER "${_knc_root}/toolchain/clang/knc-cc")
set(CMAKE_CXX_COMPILER "${_knc_root}/toolchain/clang/knc-c++")

if (DEFINED ENV{PHI_SYSROOT})
    set(CMAKE_SYSROOT "$ENV{PHI_SYSROOT}")
else ()
    set(CMAKE_SYSROOT "${_knc_root}/toolchain/build/sysroot")
endif ()

# Compile the probe programs to a static library rather than an executable.
# knc-cc does not add -static (the kernel build needs it not to), so a probe
# that links would produce a dynamic binary, and the card has no dynamic
# loader; CMake would still call it a success, but every later link test
# would be answering a question about the wrong kind of output.
set(CMAKE_TRY_COMPILE_TARGET_TYPE STATIC_LIBRARY)

# Find headers and libraries in the sysroot, never on the host.
set(CMAKE_FIND_ROOT_PATH "${CMAKE_SYSROOT}")
set(CMAKE_FIND_ROOT_PATH_MODE_PROGRAM NEVER)
set(CMAKE_FIND_ROOT_PATH_MODE_LIBRARY ONLY)
set(CMAKE_FIND_ROOT_PATH_MODE_INCLUDE ONLY)
set(CMAKE_FIND_ROOT_PATH_MODE_PACKAGE ONLY)

# Everything on the card is statically linked.
set(CMAKE_EXE_LINKER_FLAGS_INIT "-static")
set(BUILD_SHARED_LIBS OFF CACHE BOOL "the card has no dynamic loader" FORCE)

# No loop on this card can be vectorised by the compiler: LLVM has no
# Knights Corner vector backend, so `#pragma clang loop vectorize(enable)`
# always fails its transformation. That is an advisory warning, but a
# project built with -Werror turns it into a hard error, and the project is
# not wrong to do so on a machine where the pragma means something.
# FastLanes hits this in ALP's encoder; its own CMakeLists already makes the
# same exception for CI runners without AVX-512. Here it is permanent.
set(CMAKE_CXX_FLAGS_INIT "-Wno-pass-failed")
set(CMAKE_C_FLAGS_INIT "-Wno-pass-failed")
