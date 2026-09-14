/*
 * hlt-image.c: build a stub "kernel" image from a real bzImage for bisecting
 * the card boot path. The real-mode header and setup sectors are copied
 * unchanged (the bootstrap parses them), and the 32-bit entry point that
 * follows them is replaced by a few bytes of code that set bit 0 of the ring
 * region header's card_boot_flags (offset 24 of the region at 0x2000000,
 * where phictl formats it) and halt. If the host survives the boot interrupt
 * and phictl console reports "card flags 0x1", the bootstrap handed off to
 * the image and everything up to that point is exonerated. See hlt-image.md.
 *
 * Usage: tcc -run tools/hlt-image.c IN.bzImage OUT.img
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define OFF_SETUP_SECTS 0x1F1
#define OFF_SYSSIZE 0x1F4
#define OFF_BOOT_FLAG 0x1FE

static const unsigned char entry[] = {
	0xFA,                         /* cli */
	0xB8, 0x01, 0x00, 0x00, 0x00, /* mov eax, 1 */
	0xA3, 0x18, 0x00, 0x00, 0x02, /* mov [0x02000018], eax: card_boot_flags */
	0xF4,                         /* hlt */
	0xEB, 0xFD                    /* jmp back to hlt */
};

int main(int argc, char **argv)
{
	FILE *in, *out;
	unsigned char *buf;
	long len, setup_len;
	unsigned sects, syssize;
	unsigned char pad[512];

	if (argc != 3) {
		fprintf(stderr, "usage: hlt-image IN.bzImage OUT.img\n");
		return 2;
	}
	in = fopen(argv[1], "rb");
	if (!in) { perror(argv[1]); return 1; }
	fseek(in, 0, SEEK_END);
	len = ftell(in);
	fseek(in, 0, SEEK_SET);
	buf = malloc(len);
	if (!buf || fread(buf, 1, len, in) != (size_t)len) { perror("read"); return 1; }
	fclose(in);
	if (len < 0x300 || buf[OFF_BOOT_FLAG] != 0x55 || buf[OFF_BOOT_FLAG + 1] != 0xAA) {
		fprintf(stderr, "%s: not a bzImage (boot flag)\n", argv[1]);
		return 1;
	}
	sects = buf[OFF_SETUP_SECTS] ? buf[OFF_SETUP_SECTS] : 4;
	setup_len = (long)(sects + 1) * 512;
	/* One 512-byte sector of protected-mode "kernel": 32 paragraphs. */
	syssize = 512 / 16;
	buf[OFF_SYSSIZE] = syssize & 0xFF;
	buf[OFF_SYSSIZE + 1] = (syssize >> 8) & 0xFF;
	buf[OFF_SYSSIZE + 2] = 0;
	buf[OFF_SYSSIZE + 3] = 0;
	memset(pad, 0xF4, sizeof(pad)); /* hlt everywhere else */
	memcpy(pad, entry, sizeof(entry));

	out = fopen(argv[2], "wb");
	if (!out) { perror(argv[2]); return 1; }
	fwrite(buf, 1, setup_len, out);
	fwrite(pad, 1, sizeof(pad), out);
	fclose(out);
	printf("%s: %u setup sectors kept (%ld bytes), 512-byte hlt entry appended, syssize %u paragraphs\n",
	       argv[2], sects, setup_len, syssize);
	return 0;
}
