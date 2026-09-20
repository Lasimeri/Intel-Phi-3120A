// fls_check.cpp: does the MVEX unffor produce exactly what FastLanes'
// scalar unffor produces?
//
// The reference is not a model written here. It is FastLanes' own
// generated code, called by the name the component script renamed it to,
// so the two functions run on the same input in the same process and the
// comparison is a memcmp of 4096 bytes. A layout misunderstanding on my
// part cannot hide in it.
//
// Four sweeps, in order of how likely each was to be wrong:
//
//   1. every bit width 0 to 32 at every 4-byte input offset 0 to 60, both
//      through the kernel directly and through the patched dispatcher;
//   2. every byte offset 0 to 63 at five widths, which is the dispatcher's
//      staging path, since the kernels cannot take an input below 4-byte
//      alignment at all;
//   3. the four base-pointer offsets, which is where a general protection
//      fault was found on 2026-09-20;
//   4. an output that is not 64-byte aligned, which must fall back rather
//      than fault.
//
// Built and run by card/userland/components/fastlanes.sh.

#include <cstdint>
#include <initializer_list>
#include <cstdio>
#include <cstring>
#include <cstdlib>

extern "C" void knc_fls_unffor(const unsigned* in, unsigned* out, unsigned bw, const unsigned* base);

namespace fastlanes::generated::unffor::fallback::scalar {
void unffor_scalar(const uint32_t* in, uint32_t* out, uint8_t bw, const uint32_t* base);
void unffor(const uint32_t* in, uint32_t* out, uint8_t bw, const uint32_t* base);
} // namespace fastlanes::generated::unffor::fallback::scalar

namespace {

constexpr unsigned VALUES     = 1024;
constexpr unsigned MAX_BYTES  = 32 * 128; // bw = 32
constexpr unsigned SLACK      = 128;      // room for the alignment offset

uint32_t next(uint32_t& s) {
	s ^= s << 13;
	s ^= s >> 17;
	s ^= s << 5;
	return s;
}

// The first lane that differs, or -1.
int first_diff(const uint32_t* a, const uint32_t* b) {
	for (unsigned i = 0; i < VALUES; i++) {
		if (a[i] != b[i]) {
			return static_cast<int>(i);
		}
	}
	return -1;
}

} // namespace

int main() {
	unsigned char* raw    = nullptr;
	uint32_t*      ref    = nullptr;
	uint32_t*      got    = nullptr;
	uint32_t*      viadis = nullptr;
	uint32_t       base_v[16];

	// posix_memalign for every buffer: a stack array is 16-byte aligned at
	// best and the kernels store with vmovaps.
	if (posix_memalign(reinterpret_cast<void**>(&raw), 64, MAX_BYTES + SLACK)
	    || posix_memalign(reinterpret_cast<void**>(&ref), 64, VALUES * sizeof(uint32_t))
	    || posix_memalign(reinterpret_cast<void**>(&got), 64, VALUES * sizeof(uint32_t))
	    || posix_memalign(reinterpret_cast<void**>(&viadis), 64, VALUES * sizeof(uint32_t))) {
		perror("posix_memalign");
		return 2;
	}

	// Unbuffered: a wrong encoding faults rather than mismatching, so the
	// useful signal is how far the partial output got. Reverting the
	// base-pointer fix in fls_unffor.cpp makes this program exit 139 at
	// the base-offset sweep rather than report a failure; verified on the
	// card on 2026-09-20.
	std::setvbuf(stdout, nullptr, _IONBF, 0);

	uint32_t seed = 0x9E3779B9u;
	int      failed = 0, checks = 0;

	for (unsigned bw = 0; bw <= 32; bw++) {
		for (unsigned delta = 0; delta <= 60; delta += 4) {
			const auto* in = reinterpret_cast<const uint32_t*>(raw + delta);

			for (unsigned i = 0; i < MAX_BYTES + SLACK; i += 4) {
				const uint32_t v = next(seed);
				std::memcpy(raw + i, &v, sizeof v);
			}
			base_v[0] = next(seed);

			std::memset(ref, 0xA5, VALUES * sizeof(uint32_t));
			std::memset(got, 0x5A, VALUES * sizeof(uint32_t));
			std::memset(viadis, 0x3C, VALUES * sizeof(uint32_t));

			namespace fls = fastlanes::generated::unffor::fallback::scalar;
			fls::unffor_scalar(in, ref, static_cast<uint8_t>(bw), base_v);
			knc_fls_unffor(reinterpret_cast<const unsigned*>(in), reinterpret_cast<unsigned*>(got), bw, base_v);
			// The patched dispatcher, which is what FastLanes actually
			// calls: it must reach the same place.
			fls::unffor(in, viadis, static_cast<uint8_t>(bw), base_v);

			checks += 2;
			const int d1 = first_diff(ref, got);
			const int d2 = first_diff(ref, viadis);
			if (d1 >= 0) {
				failed++;
				std::printf("bw=%2u delta=%2u  kernel differs at value %d: scalar %08x, mvex %08x\n",
				            bw, delta, d1, ref[d1], got[d1]);
			}
			if (d2 >= 0) {
				failed++;
				std::printf("bw=%2u delta=%2u  dispatcher differs at value %d: scalar %08x, dispatched %08x\n",
				            bw, delta, d2, ref[d2], viadis[d2]);
			}
		}
	}

	// Inputs that are not even 4-byte aligned. The dispatcher stages these
	// through an aligned buffer; the kernels themselves cannot take them,
	// because vloadunpackld is #GP below element granularity. Every byte
	// offset 0 to 63, at four widths that between them cover a value
	// inside one word, one that straddles, one that is a whole word, and
	// the degenerate zero.
	{
		namespace fls = fastlanes::generated::unffor::fallback::scalar;

		for (unsigned bw : {0U, 5U, 11U, 17U, 32U}) {
			for (unsigned delta = 0; delta < 64; delta++) {
				for (unsigned i = 0; i < MAX_BYTES + SLACK; i += 4) {
					const uint32_t v = next(seed);
					std::memcpy(raw + i, &v, sizeof v);
				}
				base_v[0] = next(seed);
				const auto* in = reinterpret_cast<const uint32_t*>(raw + delta);

				fls::unffor_scalar(in, ref, static_cast<uint8_t>(bw), base_v);
				fls::unffor(in, got, static_cast<uint8_t>(bw), base_v);

				checks++;
				const int d = first_diff(ref, got);
				if (d >= 0) {
					failed++;
					std::printf("bw=%2u byte offset %2u: differs at value %d: scalar %08x, dispatched %08x\n",
					            bw, delta, d, ref[d], got[d]);
				}
			}
		}
	}

	// A misaligned base pointer. This is not hypothetical: a FastLanes
	// base segment holds four bytes per vector but starts at an arbitrary
	// byte offset in the file, so a_base_p is routinely not 4-byte
	// aligned, and vpbroadcastd is #GP on such an address. The first
	// version of the dispatcher passed it straight through and took a
	// general protection fault at the first instruction of
	// knc_fls_unffor_b11 the first time a real file was decoded.
	{
		namespace fls = fastlanes::generated::unffor::fallback::scalar;
		alignas(64) unsigned char base_raw[16];

		for (unsigned off = 0; off < 4; off++) {
			const uint32_t v = next(seed);
			std::memcpy(base_raw + off, &v, sizeof v);
			const auto* skewed_base = reinterpret_cast<const uint32_t*>(base_raw + off);

			for (unsigned i = 0; i < MAX_BYTES + SLACK; i += 4) {
				const uint32_t w = next(seed);
				std::memcpy(raw + i, &w, sizeof w);
			}
			fls::unffor_scalar(reinterpret_cast<const uint32_t*>(raw), ref, 11, skewed_base);
			fls::unffor(reinterpret_cast<const uint32_t*>(raw), got, 11, skewed_base);

			checks++;
			const int d = first_diff(ref, got);
			if (d >= 0) {
				failed++;
				std::printf("base offset %u: differs at value %d: scalar %08x, dispatched %08x\n",
				            off, d, ref[d], got[d]);
			}
		}
	}

	// The dispatcher must fall back, not fault, when the output is not
	// 64-byte aligned. FastLanes' own buffers always are, so this checks
	// the guard rather than a path anything currently takes.
	//
	// Its own buffer, 64 bytes over: writing 1024 values from four bytes
	// into a 4096-byte allocation runs off the end of it, which showed up
	// as a crash inside free() rather than as a wrong answer.
	{
		namespace fls  = fastlanes::generated::unffor::fallback::scalar;
		unsigned char* skewbuf = nullptr;
		if (posix_memalign(reinterpret_cast<void**>(&skewbuf), 64, VALUES * sizeof(uint32_t) + 64)) {
			perror("posix_memalign");
			return 2;
		}
		auto* skewed = reinterpret_cast<uint32_t*>(skewbuf + 4);
		fls::unffor_scalar(reinterpret_cast<const uint32_t*>(raw), ref, 11, base_v);
		fls::unffor(reinterpret_cast<const uint32_t*>(raw), skewed, 11, base_v);
		checks++;
		if (std::memcmp(ref, skewed, VALUES * sizeof(uint32_t)) != 0) {
			failed++;
			std::printf("unaligned output: the dispatcher did not fall back correctly\n");
		}
		std::free(skewbuf);
	}

	std::printf("fls_check: %d of %d comparisons failed\n", failed, checks);
	std::free(raw);
	std::free(ref);
	std::free(got);
	std::free(viadis);
	return failed ? 1 : 0;
}
