// fls_bench.cpp: FastLanes' scalar unffor against the MVEX one, on the
// card, at every bit width.
//
// Both functions come out of the same FastLanes build; the scalar one is
// the generated switch under the name the component script gave it, so
// this is not a comparison against a stand-in. Per width it reports values
// per second for each and the ratio.
//
// Three columns: FastLanes' scalar unffor, the MVEX kernel on an input it
// can take directly, and the dispatcher on one it has to stage first.
//
// One thread. The 228-thread picture for bit unpacking is already known
// and is a bandwidth story rather than an instruction story
// (docs/results/2026-09-20-bitunpack.md): FOR costs 20 percent in L1 and
// nothing at full occupancy. What is unknown here is the single-thread
// instruction-level win on FastLanes' own layout, which is what decides
// whether routing the hot path is worth anything.
//
// Built and run by card/userland/components/fastlanes.sh.
//
//   fls_bench [blocks] [reps]

#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <ctime>

extern "C" void knc_fls_unffor(const unsigned* in, unsigned* out, unsigned bw, const unsigned* base);

namespace fastlanes::generated::unffor::fallback::scalar {
void unffor_scalar(const uint32_t* in, uint32_t* out, uint8_t bw, const uint32_t* base);
void unffor(const uint32_t* in, uint32_t* out, uint8_t bw, const uint32_t* base);
} // namespace fastlanes::generated::unffor::fallback::scalar

namespace {

constexpr unsigned VALUES = 1024;

double now() {
	timespec ts {};
	clock_gettime(CLOCK_MONOTONIC, &ts);
	return static_cast<double>(ts.tv_sec) + static_cast<double>(ts.tv_nsec) * 1e-9;
}

uint32_t next(uint32_t& s) {
	s ^= s << 13;
	s ^= s >> 17;
	s ^= s << 5;
	return s;
}

} // namespace

int main(int argc, char** argv) {
	const unsigned blocks = argc > 1 ? static_cast<unsigned>(std::atoi(argv[1])) : 64;
	const unsigned reps   = argc > 2 ? static_cast<unsigned>(std::atoi(argv[2])) : 200;

	// One input arena big enough for the widest width, and one output
	// block. The output is reused so the working set stays in cache and
	// the measurement is of instructions, not of memory.
	unsigned char* in  = nullptr;
	uint32_t*      out = nullptr;
	if (posix_memalign(reinterpret_cast<void**>(&in), 64, static_cast<size_t>(blocks) * 32 * 128 + 128)
	    || posix_memalign(reinterpret_cast<void**>(&out), 64, VALUES * sizeof(uint32_t))) {
		perror("posix_memalign");
		return 2;
	}
	uint32_t seed = 0x2545F491u;
	for (size_t i = 0; i < static_cast<size_t>(blocks) * 32 * 128 + 128; i += 4) {
		const uint32_t v = next(seed);
		std::memcpy(in + i, &v, sizeof v);
	}
	const uint32_t base_v[1] = {1000000u};

	// FastLanes hands over an arbitrary byte offset, and which kind it is
	// decides which path runs, so both are measured. A multiple of four
	// goes straight into the kernels. Anything else is staged through an
	// aligned buffer by the dispatcher, because vloadunpackld is #GP below
	// element granularity, and on the example file that is half the calls.
	constexpr size_t SKEW        = 8;
	constexpr size_t SKEW_STAGED = 9;

	std::printf("FastLanes unffor, 32-bit values, one thread, %u blocks x %u reps\n", blocks, reps);
	std::printf("%3s  %12s  %12s  %7s  %12s  %7s\n", "bw", "scalar M/s", "mvex M/s", "ratio", "staged M/s", "ratio");

	namespace fls = fastlanes::generated::unffor::fallback::scalar;

	for (unsigned bw = 0; bw <= 32; bw++) {
		const size_t stride = static_cast<size_t>(bw) * 128;
		double       best_s = 0.0, best_m = 0.0, best_g = 0.0;

		for (int pass = 0; pass < 3; pass++) {
			double t = now();
			for (unsigned r = 0; r < reps; r++) {
				for (unsigned b = 0; b < blocks; b++) {
					fls::unffor_scalar(reinterpret_cast<const uint32_t*>(in + SKEW + b * stride),
					                   out,
					                   static_cast<uint8_t>(bw),
					                   base_v);
				}
			}
			const double vs = static_cast<double>(reps) * blocks * VALUES / (now() - t);
			if (vs > best_s) {
				best_s = vs;
			}

			t = now();
			for (unsigned r = 0; r < reps; r++) {
				for (unsigned b = 0; b < blocks; b++) {
					knc_fls_unffor(reinterpret_cast<const unsigned*>(in + SKEW + b * stride),
					               reinterpret_cast<unsigned*>(out),
					               bw,
					               base_v);
				}
			}
			const double vm = static_cast<double>(reps) * blocks * VALUES / (now() - t);
			if (vm > best_m) {
				best_m = vm;
			}

			// Through the dispatcher, at an offset it has to stage.
			t = now();
			for (unsigned r = 0; r < reps; r++) {
				for (unsigned b = 0; b < blocks; b++) {
					fls::unffor(reinterpret_cast<const uint32_t*>(in + SKEW_STAGED + b * stride),
					            out,
					            static_cast<uint8_t>(bw),
					            base_v);
				}
			}
			const double vg = static_cast<double>(reps) * blocks * VALUES / (now() - t);
			if (vg > best_g) {
				best_g = vg;
			}
		}
		std::printf("%3u  %12.1f  %12.1f  %6.2fx  %12.1f  %6.2fx\n",
		            bw,
		            best_s / 1e6,
		            best_m / 1e6,
		            best_m / best_s,
		            best_g / 1e6,
		            best_g / best_s);
	}

	std::free(in);
	std::free(out);
	return 0;
}
