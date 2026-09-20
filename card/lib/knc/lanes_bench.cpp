// lanes_bench.cpp: the comparison libknc has to win to mean anything.
//
// FastLanes' claim is that the lane-interleaved layout lets an *ordinary
// scalar loop* auto-vectorise, with no intrinsics, on whatever SIMD the
// machine has. That is true on the host, where the compiler targets AVX2.
// It is false on the card, whose vector unit no compiler can emit
// (docs/research/compression-on-knc.md), which is why libknc exists.
//
// So this file contains no vector code and no libknc: just the two scalar
// unpackers, compiled wherever it is pointed. Run it on the host with the
// compiler's full SIMD, and on the card, and compare both with the
// knc_bench numbers. Identical source on all three, so the only variable
// is the machine and the compiler.
//
//   host   c++ -O3 -march=native -o lanes_bench lanes_bench.cpp -lpthread
//   card   knc-cc -x c++ -std=c++17 -O3 -o lanes_bench lanes_bench.cpp -lpthread
//
//   ./lanes_bench [threads] [blocks-per-thread] [reps] [bits]
//
// See knc.md.
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <thread>
#include <vector>

#define BLOCK 1024
#define LANES 16

namespace {

double now_s()
{
	using clock = std::chrono::steady_clock;
	return std::chrono::duration<double>(clock::now().time_since_epoch()).count();
}

unsigned lowmask(unsigned bits)
{
	return bits >= 32 ? 0xFFFFFFFFu : (1u << bits) - 1u;
}

// The ordinary layout: one contiguous bitstream, value i at bit i*bits.
// Sixteen consecutive values need sixteen different shift counts, so this
// does not vectorise anywhere.
void unpack_stream(int *out, const unsigned *p, unsigned bits)
{
	unsigned m = lowmask(bits);

	for (int i = 0; i < BLOCK; i++) {
		unsigned bit = static_cast<unsigned>(i) * bits, w = bit / 32, s = bit % 32;
		unsigned val = p[w] >> s;

		if (s + bits > 32)
			val |= p[w + 1] << (32 - s);
		out[i] = static_cast<int>(val & m);
	}
}

// The FastLanes layout, written the way FastLanes writes it: the inner loop
// runs over the sixteen lanes with one shift count, which is the shape a
// vectoriser can take. On a machine with SIMD the compiler turns this into
// vector code by itself.
void unpack_lanes(int *out, const unsigned *p, unsigned bits)
{
	unsigned m = lowmask(bits);

	for (int pos = 0; pos < BLOCK / LANES; pos++) {
		unsigned bit = static_cast<unsigned>(pos) * bits, w = bit / 32, s = bit % 32;

		if (s + bits > 32) {
			for (int lane = 0; lane < LANES; lane++)
				out[pos * LANES + lane] = static_cast<int>(
					((p[w * LANES + lane] >> s) | (p[(w + 1) * LANES + lane] << (32 - s))) & m);
		} else {
			for (int lane = 0; lane < LANES; lane++)
				out[pos * LANES + lane] = static_cast<int>((p[w * LANES + lane] >> s) & m);
		}
	}
}

struct Worker {
	std::vector<int> out;
	std::vector<unsigned> packed;
	std::size_t blocks;

	explicit Worker(std::size_t n) : out(n * BLOCK), packed(n * BLOCK), blocks(n) {}
};

template <typename F>
double timed(std::vector<Worker> &w, unsigned threads, std::size_t reps, F body)
{
	std::vector<std::thread> pool;
	double t0 = now_s();

	pool.reserve(threads);
	for (unsigned t = 0; t < threads; t++)
		pool.emplace_back([&, t] {
			for (std::size_t r = 0; r < reps; r++)
				body(w[t]);
		});
	for (auto &th : pool)
		th.join();

	double secs = now_s() - t0;
	return static_cast<double>(threads) * reps * w[0].blocks * BLOCK / secs / 1e6;
}

} // namespace

int main(int argc, char **argv)
{
	unsigned threads = argc > 1 ? static_cast<unsigned>(std::strtoul(argv[1], nullptr, 10)) : 1;
	std::size_t blocks = argc > 2 ? static_cast<std::size_t>(std::strtoul(argv[2], nullptr, 10)) : 64;
	std::size_t reps = argc > 3 ? static_cast<std::size_t>(std::strtoul(argv[3], nullptr, 10)) : 128;
	unsigned only = argc > 4 ? static_cast<unsigned>(std::strtoul(argv[4], nullptr, 10)) : 0;

	if (threads < 1)
		threads = 1;

	std::printf("lanes_bench: %u thread(s), %zu blocks each, %zu reps\n", threads, blocks, reps);
	std::printf("%5s %14s %14s %10s\n", "bits", "stream M/s", "lanes M/s", "lanes/stream");

	std::vector<Worker> w;
	w.reserve(threads);
	for (unsigned t = 0; t < threads; t++)
		w.emplace_back(blocks);

	for (unsigned bits = 1; bits <= 32; bits++) {
		if (only != 0 && bits != only)
			continue;

		std::size_t words = static_cast<std::size_t>(BLOCK) * bits / 32;

		for (auto &x : w)
			for (std::size_t i = 0; i < x.packed.size(); i++)
				x.packed[i] = 0x9E3779B9u * static_cast<unsigned>(i + 1);

		double st = timed(w, threads, reps, [&](Worker &x) {
			for (std::size_t b = 0; b < x.blocks; b++)
				unpack_stream(x.out.data() + b * BLOCK, x.packed.data() + b * words, bits);
		});
		double ln = timed(w, threads, reps, [&](Worker &x) {
			for (std::size_t b = 0; b < x.blocks; b++)
				unpack_lanes(x.out.data() + b * BLOCK, x.packed.data() + b * words, bits);
		});

		std::printf("%5u %14.1f %14.1f %9.1fx\n", bits, st, ln, ln / st);
	}
	return 0;
}
