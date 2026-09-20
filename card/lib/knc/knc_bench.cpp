// knc_bench.cpp: what libknc reaches at every bit width, and the C++ layer
// exercised while doing it.
//
// Three things at once, because they need the same scaffolding: it proves
// knc.hpp compiles and works with the card's libc++, it produces the
// pack and unpack throughput table for all 32 widths, and it scales that
// across threads, which is the only axis on which this card has ever beaten
// its host.
//
//   c++ -std=c++17 -O2 -o knc_bench knc_bench.cpp -lknc -lpthread
//   ./knc_bench [threads] [blocks-per-thread] [reps] [only-this-width]
//
// `reps` matters. Creating 228 threads costs more than a few milliseconds
// of work, and the pool is created inside the timed region, so a low rep
// count measures thread creation rather than the kernels. The default is
// chosen so one measurement runs for a few hundred milliseconds at one
// thread; raise it when raising the thread count.
//
// See knc.md.
#include <knc.hpp>

#include <atomic>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <thread>
#include <vector>

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

// The scalar baseline a codec would otherwise run: one contiguous
// bitstream, one value at a time.
void unpack_stream(int *out, const unsigned *p, unsigned bits)
{
	unsigned m = lowmask(bits);

	for (int i = 0; i < KNC_BLOCK; i++) {
		unsigned bit = static_cast<unsigned>(i) * bits, w = bit / 32, s = bit % 32;
		unsigned val = p[w] >> s;

		if (s + bits > 32)
			val |= p[w + 1] << (32 - s);
		out[i] = static_cast<int>(val & m);
	}
}

// One thread's working set, sized so the whole thing is far past any cache.
struct Worker {
	knc::aligned_buffer<int> values;
	knc::aligned_buffer<int> out;
	knc::aligned_buffer<unsigned> packed;
	// The frame of reference and the delta seed are one value per lane,
	// and they are kernel arguments, so they need the same alignment as
	// everything else.
	knc::aligned_buffer<int> base;
	std::size_t blocks;

	explicit Worker(std::size_t n)
	    : values(KNC_BLOCK), out(n * KNC_BLOCK), packed(n * KNC_BLOCK),
	      base(knc::lanes), blocks(n)
	{
		for (std::size_t i = 0; i < knc::lanes; i++)
			base[i] = 0;
	}
};

// Runs `body` on `threads` workers and returns values per second in total.
//
// The workers are created first and spin on a flag, and only then does the
// clock start. Creating 228 threads takes about 140 ms, which is not the
// problem by itself; the problem is the skew. Threads created early begin
// work while later ones are still being made, so on a short run they finish
// and exit before the rest have started, fewer of them ever contend for
// memory at once, and the reported rate is too high. Measured that way the
// same configuration read 17.2 G values/s over 256 reps and 11.7 over 1024,
// declining monotonically with run length at a constant 1100 MHz clock,
// which is the signature of the skew rather than of throttling.
template <typename F>
double timed(std::vector<Worker> &w, unsigned threads, std::size_t reps, F body)
{
	std::vector<std::thread> pool;
	std::atomic<bool> go{false};
	std::atomic<unsigned> ready{0};

	pool.reserve(threads);
	for (unsigned t = 0; t < threads; t++)
		pool.emplace_back([&, t] {
			ready.fetch_add(1, std::memory_order_release);
			while (!go.load(std::memory_order_acquire))
				;
			for (std::size_t r = 0; r < reps; r++)
				body(w[t]);
		});
	while (ready.load(std::memory_order_acquire) < threads)
		;

	double t0 = now_s();
	go.store(true, std::memory_order_release);
	for (auto &th : pool)
		th.join();
	double secs = now_s() - t0;

	double total = static_cast<double>(threads) * reps * w[0].blocks * KNC_BLOCK;
	return total / secs / 1e6;
}

} // namespace

int main(int argc, char **argv)
{
	unsigned threads = argc > 1 ? static_cast<unsigned>(std::strtoul(argv[1], nullptr, 10)) : 1;
	std::size_t blocks = argc > 2 ? static_cast<std::size_t>(std::strtoul(argv[2], nullptr, 10)) : 256;
	std::size_t reps = argc > 3 ? static_cast<std::size_t>(std::strtoul(argv[3], nullptr, 10)) : 64;
	unsigned only = argc > 4 ? static_cast<unsigned>(std::strtoul(argv[4], nullptr, 10)) : 0;

	if (threads < 1)
		threads = 1;
	if (reps < 1)
		reps = 1;

	std::printf("libknc: %u thread(s), %zu blocks each, %zu reps, %d values per block\n",
		    threads, blocks, reps, KNC_BLOCK);
	{
		// Reported for context only: thread creation is outside the timed
		// region now, and all workers are at the start line before the
		// clock starts.
		double t0 = now_s();
		std::vector<std::thread> pool;

		pool.reserve(threads);
		for (unsigned i = 0; i < threads; i++)
			pool.emplace_back([] {});
		for (auto &th : pool)
			th.join();
		std::printf("spawning %u threads costs %.1f ms, outside the timed region\n",
			    threads, (now_s() - t0) * 1e3);
	}
	std::printf("%5s %11s %11s %11s %11s %10s %8s\n", "bits",
		    "unpack", "for", "delta", "pack", "scalar", "vs scalar");

	std::vector<Worker> w;
	w.reserve(threads);
	for (unsigned t = 0; t < threads; t++)
		w.emplace_back(blocks);

	for (unsigned bits = 1; bits <= 32; bits++) {
		if (only != 0 && bits != only)
			continue;

		knc::codec c(bits);
		unsigned m = lowmask(bits);
		std::size_t words = static_cast<std::size_t>(KNC_BLOCK) * bits / 32;

		for (auto &x : w) {
			for (int i = 0; i < KNC_BLOCK; i++)
				x.values[i] = static_cast<int>((0x9E3779B9u * static_cast<unsigned>(i + 1)) & m);
			for (std::size_t b = 0; b < blocks; b++)
				c.pack(x.packed.data() + b * words, x.values.data());
		}

		double un = timed(w, threads, reps, [&](Worker &x) {
			for (std::size_t b = 0; b < x.blocks; b++)
				c.unpack(x.out.data() + b * KNC_BLOCK, x.packed.data() + b * words);
		});
		double pk = timed(w, threads, reps, [&](Worker &x) {
			for (std::size_t b = 0; b < x.blocks; b++)
				c.pack(x.packed.data() + b * words, x.out.data() + b * KNC_BLOCK);
		});
		double fo = timed(w, threads, reps, [&](Worker &x) {
			for (std::size_t b = 0; b < x.blocks; b++)
				c.unpack_for(x.out.data() + b * KNC_BLOCK, x.packed.data() + b * words,
					     x.base.data());
		});
		// The delta chain is 64 dependent adds per lane and cannot be
		// pipelined: the chain is the algorithm. This is the number that
		// says whether that matters on an in-order core.
		double de = timed(w, threads, reps, [&](Worker &x) {
			for (std::size_t b = 0; b < x.blocks; b++)
				c.unpack_delta(x.out.data() + b * KNC_BLOCK, x.packed.data() + b * words,
					       x.base.data());
		});
		double sc = timed(w, threads, reps, [&](Worker &x) {
			for (std::size_t b = 0; b < x.blocks; b++)
				unpack_stream(x.out.data() + b * KNC_BLOCK, x.packed.data() + b * words, bits);
		});

		std::printf("%5u %11.1f %11.1f %11.1f %11.1f %10.1f %7.1fx\n",
			    bits, un, fo, de, pk, sc, un / sc);
	}

	// The out-of-range width throws from the C++ layer rather than doing
	// nothing, which is the one behaviour knc.hpp adds over knc.h.
	try {
		knc::codec bad(33);
		std::printf("\nknc::codec(33) did not throw: FAILED\n");
		return 1;
	} catch (const std::out_of_range &) {
		std::printf("\nknc::codec(33) throws, as it should\n");
	}
	return 0;
}
