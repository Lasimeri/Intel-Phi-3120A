// fls_compress.cpp: how fast does FastLanes compress, in MB/s?
//
// Builds for the host and for the card from the same source and links
// against nothing of this project's, so the two numbers are comparable and
// the card's can be read as a ratio rather than in isolation.
//
// Three denominators, because a columnar compressor has three defensible
// ones and quoting only the flattering one is how compression benchmarks
// lie:
//
//   csv      the bytes handed to read_csv. Includes the text encoding, so
//            it overstates throughput against a binary source.
//   logical  what the compressor is handed: for a fixed width column
//            its width times the row count, for a string column the
//            bytes of its own fields. This is the honest headline,
//            and the schema is what says which is which.
//   out      the bytes written, for the ratio.
//
// read_csv and to_fls are timed apart because parsing is not compression.
//
//   fls_compress <csv_dir> <out.fls> [scheme]
//
// scheme, if given, forces a single encoding instead of letting the
// wizard search: "ffor", "delta", "rle", "uncompressed". Use it to
// separate the cost of the search from the cost of the encoding.

#include "fastlanes.hpp"
#include "fls/io/file.hpp"
#include "fls/json/nlohmann/json.hpp"
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <ctime>
#include <iostream>
#include <string>
#include <vector>

// ---- self-sampling profiler -----------------------------------------
//
// The card has no perf and no gdb, so the only way to find out where a
// 163 second run spends its time is for the program to sample its own
// program counter. ITIMER_PROF charges CPU time rather than wall time,
// so time blocked in a syscall is not misattributed to whatever ran
// last before it.
//
// Off unless FLS_PROF names an output file. A stopped itimer costs
// nothing, so this can stay in the shipped binary.
//
//   FLS_PROF=/tmp/prof.bin fls_compress <csv_dir> <out.fls> [scheme]
//
// Writes a header line to stderr giving the runtime address of a known
// symbol, so a position independent load can be un-slid, followed by a
// flat array of 64-bit program counters.

#include <atomic>
#include <csignal>
#include <sys/time.h>
#include <ucontext.h>

extern "C" void fls_prof_anchor(void) {}

namespace {

constexpr size_t PROF_MAX = 8u << 20; // 8M samples, 64 MB, ~2.3 hours at 1 kHz

unsigned long*           prof_pc   = nullptr;
std::atomic<size_t>      prof_n    = 0;
std::atomic<size_t>      prof_lost = 0;

void prof_tick(int /*sig*/, siginfo_t* /*si*/, void* uc) {
	const size_t i = prof_n.fetch_add(1, std::memory_order_relaxed);
	if (i >= PROF_MAX) {
		prof_lost.fetch_add(1, std::memory_order_relaxed);
		return;
	}
	prof_pc[i] = static_cast<unsigned long>(
	    static_cast<ucontext_t*>(uc)->uc_mcontext.gregs[REG_RIP]);
}

bool prof_on() {
	return std::getenv("FLS_PROF") != nullptr;
}

void prof_start() {
	if (!prof_on()) {
		return;
	}
	prof_pc = static_cast<unsigned long*>(std::calloc(PROF_MAX, sizeof(unsigned long)));
	if (prof_pc == nullptr) {
		std::fprintf(stderr, "prof: out of memory\n");
		return;
	}
	struct sigaction sa {};
	std::memset(&sa, 0, sizeof sa);
	sa.sa_sigaction = prof_tick;
	sa.sa_flags     = SA_SIGINFO | SA_RESTART;
	sigaction(SIGPROF, &sa, nullptr);

	itimerval it {};
	it.it_interval.tv_sec  = 0;
	it.it_interval.tv_usec = 1000; // 1 kHz
	it.it_value            = it.it_interval;
	if (setitimer(ITIMER_PROF, &it, nullptr) != 0) {
		std::fprintf(stderr, "prof: setitimer failed\n");
	}
}

void prof_stop() {
	if (!prof_on() || prof_pc == nullptr) {
		return;
	}
	itimerval off {};
	setitimer(ITIMER_PROF, &off, nullptr);
	const char* path = std::getenv("FLS_PROF");
	std::FILE*  f    = std::fopen(path, "wb");
	if (f == nullptr) {
		std::fprintf(stderr, "prof: cannot write %s\n", path);
		return;
	}
	std::fwrite(prof_pc, sizeof(unsigned long), prof_n.load(), f);
	std::fclose(f);
	std::fprintf(stderr,
	             "prof: %zu samples, %zu lost, anchor fls_prof_anchor at %p, wrote %s\n",
	             static_cast<size_t>(prof_n),
	             static_cast<size_t>(prof_lost),
	             reinterpret_cast<void*>(&fls_prof_anchor),
	             path);
}

} // namespace

using namespace fastlanes; // NOLINT

namespace {

double now() {
	timespec ts {};
	clock_gettime(CLOCK_MONOTONIC, &ts);
	return static_cast<double>(ts.tv_sec) + static_cast<double>(ts.tv_nsec) * 1e-9;
}


// Columns, counted from the first data line's separator count. Good
// enough: the schema next to it is what actually types them, and this is
// only used to size the logical denominator.

// What the compressor is actually handed, in bytes, per column type.
// Strings have no fixed width, so a string column contributes the bytes
// of its own fields rather than a nominal width; for every other type
// this is the width of the physical column FastLanes materialises.
unsigned fixed_width(const std::string& t) {
	if (t == "BIGINT" || t == "TIMESTAMP" || t == "DOUBLE" || t == "DATETIME") {
		return 8;
	}
	if (t == "INT" || t == "INTEGER" || t == "DATE" || t == "FLOAT" || t == "MEDIUMINT") {
		return 4;
	}
	if (t == "SMALLINT") {
		return 2;
	}
	if (t == "BOOLEAN" || t == "BIT") {
		return 1;
	}
	return 0; // string-like: measured below
}

struct Shape {
	unsigned long rows;
	unsigned long cols;
	double        logical;
};

// One pass, counting rows and the bytes of every string column. The
// schema is what types the columns, so the type list comes from there
// and not from guessing at the text.
Shape measure(const path& csv, const path& schema_path) {
	Shape shape {0, 0, 0.0};

	std::vector<unsigned> width;
	{
		const auto           text = File::read(schema_path);
		const nlohmann::json j    = nlohmann::json::parse(text);
		for (const auto& c : j.at("columns")) {
			width.push_back(fixed_width(c.at("type").get<std::string>()));
		}
	}
	shape.cols = width.size();
	if (shape.cols == 0) {
		return shape;
	}

	std::FILE* f = std::fopen(csv.c_str(), "rb");
	if (f == nullptr) {
		return shape;
	}
	std::vector<double> str_bytes(width.size(), 0.0);
	char                buf[1 << 16];
	size_t              got   = 0;
	size_t              col   = 0;
	double              field = 0;
	while ((got = std::fread(buf, 1, sizeof buf, f)) > 0) {
		for (size_t i = 0; i < got; i++) {
			const char ch = buf[i];
			if (ch == '|' || ch == '\n') {
				if (col < width.size() && width[col] == 0) {
					str_bytes[col] += field;
				}
				field = 0;
				col++;
				if (ch == '\n') {
					shape.rows++;
					col = 0;
				}
			} else if (ch != '\r') {
				field += 1;
			}
		}
	}
	std::fclose(f);

	for (size_t c = 0; c < width.size(); c++) {
		shape.logical += width[c] == 0 ? str_bytes[c] : static_cast<double>(width[c]) * static_cast<double>(shape.rows);
	}
	return shape;
}

const char* MB = "MB/s";

void report(const char* what, double bytes, double seconds) {
	std::printf("  %-24s %10.3f MB   %8.3f s   %9.3f %s\n",
	            what,
	            bytes / 1e6,
	            seconds,
	            bytes / seconds / 1e6,
	            MB);
}

} // namespace

int main(int argc, char** argv) {
	if (argc < 3) {
		std::cerr << "usage: fls_compress <csv_dir> <out.fls> [scheme]\n";
		return 2;
	}
	std::setvbuf(stdout, nullptr, _IONBF, 0);
	try {
		const path        dir_path = argv[1];
		const path        fls_path = argv[2];
		const std::string scheme   = argc > 3 ? argv[3] : "";

		if (exists(fls_path)) {
			fs::remove(fls_path);
		}

		const auto csv      = dir_path / "data.csv";
		const auto csv_size = static_cast<double>(file_size(csv));
		const auto shape    = measure(csv, dir_path / "schema.json");
		const auto rows     = static_cast<double>(shape.rows);
		const auto cols     = static_cast<double>(shape.cols);
		const auto logical  = shape.logical;

		const auto con = connect();

		if (!scheme.empty()) {
			vector<OperatorToken> pool;
			if (scheme == "ffor") {
				pool = {OperatorToken::EXP_FFOR_I32};
			} else if (scheme == "delta") {
				pool = {OperatorToken::EXP_DELTA_I32};
			} else if (scheme == "rle") {
				pool = {OperatorToken::EXP_RLE_I32_U16};
			} else if (scheme == "uncompressed") {
				pool = {OperatorToken::EXP_UNCOMPRESSED_I32};
			} else {
				std::cerr << "unknown scheme: " << scheme << "\n";
				return 2;
			}
			con->force_schema_pool(pool);
			std::printf("forced scheme        %s (no wizard search)\n", scheme.c_str());
		}

		double t = now();
		con->read_csv(dir_path);
		const double parse_s = now() - t;

		t = now();
		prof_start();
		con->to_fls(fls_path);
		const double compress_s = now() - t;
		prof_stop();

		const auto out_size = static_cast<double>(file_size(fls_path));

		std::printf("rows %.0f  columns %.0f\n", rows, cols);
		std::printf("parse:\n");
		report("csv in", csv_size, parse_s);
		std::printf("compress:\n");
		report("csv in", csv_size, compress_s);
		report("logical in", logical, compress_s);
		report("fls out", out_size, compress_s);
		std::printf("  %-24s %10.2fx  (%.0f -> %.0f bytes)\n",
		            "ratio vs logical",
		            logical / out_size,
		            logical,
		            out_size);
		return 0;
	} catch (const std::exception& ex) {
		std::cerr << "fls_compress: " << ex.what() << "\n";
		return 1;
	}
}
