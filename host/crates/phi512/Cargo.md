# phi512 manifest

A `cdylib`, because the way an unmodified program gets this is
`LD_PRELOAD`: the loader maps it, its constructor installs the handler, and
the program runs without knowing anything happened. Also an `rlib` so the
tests can drive the emulator directly instead of through a signal.

`iced-x86` decodes the faulting instruction. `libc` is for `sigaction`,
`mprotect` and the `ucontext` layout.
