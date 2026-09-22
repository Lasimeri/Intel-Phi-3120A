# phi-vpu

Host side of the AVX-512 co-processor: a binary that hands work to the
card's vector units through the shared host-memory window and checks the
answer. Depends on `libc` for the mapping, `clap` for the command line,
and `anyhow` for errors. The card side is `card/vpu/`.
