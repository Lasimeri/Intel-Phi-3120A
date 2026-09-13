# card/userland

Per-component notes for what runs on the card. Each file records: upstream
version pinned, configure flags, the ABI-sensitive spots (anything that
touches float passing or contains assembly), the audit result, and the
smoke test. Build scripts are added as each component lands.

| Component | Phase | Language | Note |
| --- | --- | --- | --- |
| [musl](components/musl.md) | P4 | C | libc, static by default |
| [busybox](components/busybox.md) | P4 | C | shell and utilities |
| [init (Rust)](components/init.md) | P4 | Rust | PID 1, ring readiness flag, service supervision |
| [dropbear](components/dropbear.md) | P5 | C | SSH server |
| [clang](components/clang.md) | P7 | C++ | first native compiler |
| [gcc](components/gcc.md) | P8 | C | needs the knc64-x87 backend amendments |
| [tcc](components/tcc.md) | P8 | C | x87 float backend |
| [QuickJS](components/quickjs.md) | P9 | C | JavaScript |
| [CPython](components/cpython.md) | P9 | C | Python 3.13+ with libffi patch |
