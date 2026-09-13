/* SPDX-License-Identifier: GPL-2.0-only */
/*
 * phi_ring.h: wire layout of the host/card ring transport.
 * Mirror of host/crates/phi-ring/src/layout.rs; both follow
 * docs/spec/ring-protocol.md. tools/ring-layout-check.c prints the C
 * offsets so they can be compared with the Rust constants.
 */
#ifndef PHI_RING_H
#define PHI_RING_H

#ifdef __KERNEL__
#include <linux/types.h>
#else
#include <stdint.h>
typedef uint8_t u8;
typedef uint32_t u32;
typedef uint64_t u64;
#endif

#define PHI_REGION_MAGIC	0x52494850u	/* 'P','H','I','R' little-endian */
#define PHI_RING_MAGIC		0x474E4952u	/* 'R','I','N','G' */
#define PHI_RING_VERSION	1u

#define PHI_CHANNEL_CONSOLE	1u
#define PHI_CHANNEL_NETWORK	2u

#define PHI_CARD_FLAG_INIT_REACHED	1ull

/* One 64-byte line. Host writes everything except card_boot_flags. */
struct phi_region_hdr {
	u32 magic;
	u32 version;
	u32 region_size;
	u32 channel_count;
	u64 host_epoch_ns;
	u64 card_boot_flags;
	u8  pad[64 - 32];
};

/* One 64-byte line per channel, starting at offset 64. */
struct phi_channel_desc {
	u32 kind;
	u32 flags;
	u32 h2c_offset;
	u32 h2c_size;
	u32 c2h_offset;
	u32 c2h_size;
	u8  pad[64 - 24];
};

/*
 * Ring header: magic and size on the first line, head alone on the second,
 * tail alone on the third; data follows at offset 192.
 */
struct phi_ring {
	u32 magic;
	u32 size;
	u8  pad0[56];
	u32 head;
	u8  pad1[60];
	u32 tail;
	u8  pad2[60];
	u8  data[];
};

#define PHI_RING_HDR_SIZE	192u

#if defined(__STDC_VERSION__) && __STDC_VERSION__ >= 201112L
_Static_assert(sizeof(struct phi_region_hdr) == 64, "region header is one line");
_Static_assert(sizeof(struct phi_channel_desc) == 64, "channel desc is one line");
_Static_assert(sizeof(struct phi_ring) == PHI_RING_HDR_SIZE, "ring header is three lines");
#endif

#endif /* PHI_RING_H */
