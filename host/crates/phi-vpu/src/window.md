# window.rs

The shared window as the host sees it: `/dev/shm/phi-hostmem[-N]`, the
file the daemon pinned and mapped for card N, opened read-write and
mapped whole. `Window::read` and `write` are volatile accesses to the
control words the worker polls; `put` and `get` move bulk bytes.
`wait_ready` clears the readiness word and waits for a live worker to
re-assert it (a dead worker's stale word would otherwise satisfy a plain
check); `submit` rings the doorbell and waits for the echoed sequence
number. Moved out of `main.rs` on 2026-09-22 so that `libphi512`'s
seamless path (`phi512/src/offload.rs`) uses the same code as the
explicit driver. `Window` is `Send` (one sits behind a mutex there);
the pointer is to shared memory and every access through it is volatile
or a plain copy.

`ptr` (2026-09-22) hands out a raw pointer into the window so the
seamless path can copy the program's memory straight into a fetch slot
with `process_vm_readv` and apply a write-back straight from a slot,
one copy instead of two.
