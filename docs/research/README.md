# Research record

Everything here was gathered before design and is the evidence behind
`../decisions/`. Each file states its sources. Where a claim is an inference
rather than a documented fact it is labeled as such.

| File | Question answered |
| --- | --- |
| [isa-deletions.md](isa-deletions.md) | Which x86-64 instructions does Knights Corner not execute, and what does that break? |
| [os-limitations.md](os-limitations.md) | What does Intel say a "shrink-wrapped" OS must change to run on the card? |
| [memory-map.md](memory-map.md) | How do card and host see each other's memory? |
| [boot-protocol.md](boot-protocol.md) | What does the on-card bootstrap expect, and what did Intel's host driver write? |
| [abi-and-toolchain.md](abi-and-toolchain.md) | What ABI can 64-bit userland use without SSE, and which compilers can produce it? |
| [prior-art.md](prior-art.md) | Who has done what with this card since Intel dropped it? |
| [runtimes.md](runtimes.md) | Can gcc, tcc, Bun, Python actually run on the card? |
| [sources.md](sources.md) | Every URL and document number used |
