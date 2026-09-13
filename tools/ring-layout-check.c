/*
 * ring-layout-check.c: verify that the C ring layout matches the Rust one.
 * Run: tcc -run tools/ring-layout-check.c   (or `make layout-check`)
 * Exit status 0 when every offset matches the constants documented in
 * host/crates/phi-ring/src/layout.rs; 1 otherwise.
 */
#include <stddef.h>
#include <stdio.h>

#include "../card/drivers/phinet/include/phi_ring.h"

static int fails;

static void check(const char *name, size_t have, size_t want)
{
	if (have != want) {
		printf("MISMATCH %-28s C=%zu rust=%zu\n", name, have, want);
		fails = 1;
	} else {
		printf("ok       %-28s %zu\n", name, have);
	}
}

int main(void)
{
	/* Values on the right are the Rust constants (layout.rs). */
	check("sizeof(phi_region_hdr)", sizeof(struct phi_region_hdr), 64);
	check("region_hdr.magic", offsetof(struct phi_region_hdr, magic), 0);
	check("region_hdr.version", offsetof(struct phi_region_hdr, version), 4);
	check("region_hdr.region_size", offsetof(struct phi_region_hdr, region_size), 8);
	check("region_hdr.channel_count", offsetof(struct phi_region_hdr, channel_count), 12);
	check("region_hdr.host_epoch_ns", offsetof(struct phi_region_hdr, host_epoch_ns), 16);
	check("region_hdr.card_boot_flags", offsetof(struct phi_region_hdr, card_boot_flags), 24);

	check("sizeof(phi_channel_desc)", sizeof(struct phi_channel_desc), 64);
	check("channel_desc.kind", offsetof(struct phi_channel_desc, kind), 0);
	check("channel_desc.flags", offsetof(struct phi_channel_desc, flags), 4);
	check("channel_desc.h2c_offset", offsetof(struct phi_channel_desc, h2c_offset), 8);
	check("channel_desc.h2c_size", offsetof(struct phi_channel_desc, h2c_size), 12);
	check("channel_desc.c2h_offset", offsetof(struct phi_channel_desc, c2h_offset), 16);
	check("channel_desc.c2h_size", offsetof(struct phi_channel_desc, c2h_size), 20);

	check("sizeof(phi_ring) (header)", sizeof(struct phi_ring), 192);
	check("ring.magic", offsetof(struct phi_ring, magic), 0);
	check("ring.size", offsetof(struct phi_ring, size), 4);
	check("ring.head", offsetof(struct phi_ring, head), 64);
	check("ring.tail", offsetof(struct phi_ring, tail), 128);
	check("ring.data", offsetof(struct phi_ring, data), 192);

	check("PHI_REGION_MAGIC", PHI_REGION_MAGIC, 0x52494850u);
	check("PHI_RING_MAGIC", PHI_RING_MAGIC, 0x474E4952u);
	return fails;
}
