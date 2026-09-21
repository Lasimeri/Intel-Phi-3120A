// fls_roundtrip.cpp: does FastLanes actually run on the card, and what
// does the vector path buy in the real decode loop?
//
// CSV in, .fls out, then the .fls read back a vector at a time through
// RowgroupReader::get_chunk, which is the decode path and nothing else:
// no CSV formatting, no materialisation into a row group. That loop is
// timed twice on the same file in the same process, once with the MVEX
// unffor and once with FastLanes' scalar one, flipped by knc_fls_enabled.
//
// The loop still covers the whole expression interpreter, not just
// unffor, so the ratio here is necessarily smaller than the one
// fls_bench reports for unffor alone. That difference is the measurement:
// it says how much of a FastLanes decode is actually bit unpacking.
//
//   fls_roundtrip <csv_dir> <work_dir> [passes] [reps]
//
// csv_dir of "-" reuses the .fls already in work_dir and skips the encode.

#include "fastlanes.hpp"
#include "fls/footer/rowgroup_descriptor.hpp"
#include "fls/reader/rowgroup_reader.hpp"
#include <cstdio>
#include <cstdlib>
#include <ctime>
#include <iostream>

extern "C" int           knc_fls_enabled;
extern "C" unsigned long knc_fls_calls_vector;
extern "C" unsigned long knc_fls_calls_scalar;
extern "C" unsigned long knc_fls_calls_staged;

using namespace fastlanes; // NOLINT

namespace {

double now() {
	timespec ts {};
	clock_gettime(CLOCK_MONOTONIC, &ts);
	return static_cast<double>(ts.tv_sec) + static_cast<double>(ts.tv_nsec) * 1e-9;
}

// Exact, so the values-per-second figure is not an estimate from the file
// size and an assumed row width.
unsigned long count_rows(const path& csv) {
	std::FILE* f = std::fopen(csv.c_str(), "rb");
	if (f == nullptr) {
		return 0;
	}
	char          buf[65536];
	unsigned long n = 0;
	size_t        got = 0;
	while ((got = std::fread(buf, 1, sizeof buf, f)) > 0) {
		for (size_t i = 0; i < got; i++) {
			if (buf[i] == '\n') {
				n++;
			}
		}
	}
	std::fclose(f);
	return n;
}

// One full decode of every vector of every rowgroup. Returns seconds and
// sets values to the number of values decoded, so the two passes can be
// checked against each other.
double decode_pass(const path& fls_path, n_t& values, n_t& vectors, unsigned reps) {
	Connection con;
	const auto table = con.reset().read_fls(fls_path);

	values  = 0;
	vectors = 0;
	const double t = now();
	for (n_t rg = 0;; rg++) {
		up<RowgroupReader> reader;
		try {
			reader = (*table)[rg];
		} catch (...) {
			break;
		}
		if (!reader) {
			break;
		}
		const n_t n_vec = static_cast<n_t>(reader->get_descriptor().m_n_vec());
		if (vectors == 0) {
			std::fprintf(stderr, "   rowgroup %llu: %llu vectors, %llu columns\n", (unsigned long long)rg, (unsigned long long)n_vec, (unsigned long long)reader->get_descriptor().m_column_descriptors()->size());
		}
		const n_t n_col = reader->get_descriptor().m_column_descriptors()->size();
		// The example file is 59 vectors, which decodes in a few
		// milliseconds. Repeating the loop inside the timed region puts
		// the measurement well clear of the clock, and repeats it over
		// the same reader, which is what a large file would do anyway.
		for (unsigned r = 0; r < reps; r++) {
			for (n_t v = 0; v < n_vec; v++) {
				reader->get_chunk(v);
				vectors++;
				values += n_col * 1024;
			}
		}
		break; // one rowgroup in these files; see the note in fastlanes.md
	}
	return now() - t;
}

} // namespace

int main(int argc, char** argv) {
	if (argc < 3) {
		std::cerr << "usage: fls_roundtrip <csv_dir> <work_dir> [passes] [reps]\n";
		return 2;
	}
	std::setvbuf(stdout, nullptr, _IONBF, 0); // the card is slow; see progress as it goes
	const int      passes = argc > 3 ? std::atoi(argv[3]) : 3;
	const unsigned reps   = argc > 4 ? static_cast<unsigned>(std::atoi(argv[4])) : 200;
	try {
		const path dir_path = argv[1];
		const path work     = argv[2];
		const path fls_path = work / "data.fls";

		// A csv_dir of "-" reuses the .fls already in work_dir and skips
		// the encode, which costs minutes on this card and says nothing
		// about the decode. Use it to time decoding of a file that is
		// already there.
		const bool reuse = (dir_path == "-");
		if (reuse && !exists(fls_path)) {
			std::cerr << "fls_roundtrip: no " << fls_path << " to reuse\n";
			return 2;
		}
		if (!reuse && exists(fls_path)) {
			fs::remove(fls_path);
		}

		// Split, because the two halves answer different questions.
		// read_csv is text parsing and says nothing about compression;
		// to_fls is the compressor: statistics, scheme selection, the
		// transposes, ffor and the bit packing. Only the second is what
		// anyone means by "compression speed", and it is entirely scalar,
		// because this port reroutes unffor and nothing on the encode
		// side (encoding_operator.cpp:300 still calls
		// generated::ffor::fallback::scalar::ffor).
		if (!reuse) {
			double     t   = now();
			const auto con = connect();
			con->read_csv(dir_path);
			const double parse_s = now() - t;

			t = now();
			con->to_fls(fls_path);
			const double compress_s = now() - t;

			const auto csv_bytes = file_size(dir_path / "data.csv");
			const auto fls_bytes = file_size(fls_path);
			const auto rows      = static_cast<double>(count_rows(dir_path / "data.csv"));

			std::printf("read_csv (parse)     %8.3f s  %8.2f MiB/s of CSV\n",
			            parse_s,
			            static_cast<double>(csv_bytes) / parse_s / (1024 * 1024));
			std::printf("to_fls (compress)    %8.3f s  %8.2f M values/s  %8.2f MiB/s in\n",
			            compress_s,
			            rows * 3.0 / compress_s / 1e6,
			            static_cast<double>(csv_bytes) / compress_s / (1024 * 1024));
			std::printf("ratio                %8.2fx  %ld bytes from %ld\n",
			            static_cast<double>(csv_bytes) / static_cast<double>(fls_bytes),
			            static_cast<long>(fls_bytes),
			            static_cast<long>(csv_bytes));
		} else {
			std::printf("reusing              %s  %ld bytes\n",
			            fls_path.c_str(),
			            static_cast<long>(file_size(fls_path)));
		}

		n_t values = 0, vectors = 0, check = 0;
		decode_pass(fls_path, values, vectors, reps); // warm the page cache

		double best_m = 1e30, best_s = 1e30;
		for (int p = 0; p < passes; p++) {
			knc_fls_enabled = 1;
			const double m = decode_pass(fls_path, values, vectors, reps);
			knc_fls_enabled = 0;
			const double s = decode_pass(fls_path, check, vectors, reps);
			knc_fls_enabled = 1;
			if (m < best_m) {
				best_m = m;
			}
			if (s < best_s) {
				best_s = s;
			}
		}
		if (values != check) {
			std::printf("MISMATCH: %llu values with mvex, %llu with scalar\n",
			            static_cast<unsigned long long>(values),
			            static_cast<unsigned long long>(check));
			return 1;
		}

		std::printf("decoded              %llu values in %llu vectors, best of %d\n",
		            static_cast<unsigned long long>(values),
		            static_cast<unsigned long long>(vectors),
		            passes);
		std::printf("decode, scalar       %8.3f s  %8.1f M values/s\n",
		            best_s,
		            static_cast<double>(values) / best_s / 1e6);
		std::printf("decode, mvex         %8.3f s  %8.1f M values/s\n",
		            best_m,
		            static_cast<double>(values) / best_m / 1e6);
		std::printf("end to end           %8.2fx\n", best_s / best_m);
		// One more pass, counted rather than timed, so the numbers above
		// can be read. A column FastLanes narrowed to u16 or u8, or
		// encoded some other way, never reaches the 32-bit unffor at all,
		// and an end-to-end ratio means nothing without knowing how many
		// of the columns did.
		{
			n_t a = 0, b = 0;
			knc_fls_enabled      = 1;
			knc_fls_calls_vector = 0;
			knc_fls_calls_scalar = 0;
			knc_fls_calls_staged = 0;
			decode_pass(fls_path, a, b, 1);
			std::printf("unffor u32, one pass %lu on the vector unit (%lu of them staged), %lu fell back, over %llu vectors\n",
			            knc_fls_calls_vector,
			            knc_fls_calls_staged,
			            knc_fls_calls_scalar,
			            static_cast<unsigned long long>(b));
		}
		return 0;
	} catch (const std::exception& ex) {
		std::cerr << "fls_roundtrip: " << ex.what() << "\n";
		return 1;
	}
}
