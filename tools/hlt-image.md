# tools/hlt-image.c

Makes a stub image for bisecting the card boot path: the real bzImage's
real-mode header and setup sectors, followed by one 512-byte sector at
the 32-bit entry point that does `cli`, stores 1 into the ring header's
`card_boot_flags` (card address `0x10000018` with the default ring base),
and halts. `syssize` in the header is rewritten to match.

Why: the first two `phictl boot` attempts reset the host with nothing
logged. `phictl boot --load-only` tests the aperture writes alone; this
image tests the boot interrupt and the bootstrap's hand-off without any
kernel of ours running. `phictl console` printing `card flags 0x1`
proves the entry code executed. The stub runs in the bootstrap's 32-bit
protected mode without paging, so it can only reach the low 4 GiB of
card memory; the DBOX POST register (at 32 GiB) is out of reach, hence
the flag in the ring instead.

```
tcc -run tools/hlt-image.c card/kernel/build/out/arch/x86/boot/bzImage card/kernel/build/hlt.img
sudo host/target/debug/phictl boot --kernel card/kernel/build/hlt.img --raw-cmdline --cmdline stub
```
