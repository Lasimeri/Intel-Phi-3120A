/* fls-example-csv.c: write the CSV that card/userland/components/fastlanes.sh
 * packages as the round trip's example dataset.
 *
 * Three integer columns, all chosen so FastLanes stores them as its 32-bit
 * physical type, which is the only one Knights Corner can decode on the
 * vector unit. Magnitude decides the physical type, not bit width: a
 * column whose values fit in 16 bits is narrowed to u16, and the card has
 * no 16-bit integer vector instruction at all (ISA reference 327364-001,
 * appendix D.1). Every column here is therefore above 65535, while the
 * residue after frame of reference is 11, 17 and 5 bits.
 *
 * That makes the measurement a best case for the vector path, and saying
 * so is the point: a column of small integers would not reach it.
 *
 *   tcc -run tools/fls-example-csv.c 60000 > data.csv
 */
#include <stdio.h>
#include <stdlib.h>

int main(int argc, char **argv)
{
	long rows = (argc > 1) ? atol(argv[1]) : 60000;
	unsigned int s = 12345u;
	long i;

	for (i = 0; i < rows; i++) {
		unsigned a, b, c;

		s = s * 1103515245u + 12345u;
		a = 100000u + ((s >> 16) & 0x7FFu);      /* bw 11 after FOR */
		s = s * 1103515245u + 12345u;
		b = 7000000u + ((s >> 16) & 0x1FFFFu);   /* bw 17 */
		s = s * 1103515245u + 12345u;
		c = 200000u + ((s >> 16) & 0x1Fu);       /* bw 5  */
		printf("%u|%u|%u\n", a, b, c);
	}
	return 0;
}
