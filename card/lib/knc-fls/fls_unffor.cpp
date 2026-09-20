// fls_unffor.cpp: the FastLanes 32-bit unffor dispatcher, rerouted to the
// card's vector unit.
//
// card/userland/components/fastlanes.sh appends this file to the end of
// FastLanes' own src/alp/src/fastlanes_gen_unffor.cpp, after renaming that
// file's uint32_t dispatcher to unffor_scalar. Appending rather than
// editing in place means no CMake change is needed to compile it, and the
// scalar dispatcher stays reachable by name, which is what the equivalence
// check in fls_check.cpp compares against.
//
// Only the 32-bit width is rerouted. Knights Corner has no byte or word
// integer vector instruction at all, and its entire 64-bit integer vector
// set is vfixupnanpd, vpandnq, vpandq, vpblendmq, vporq and vpxorq (ISA
// reference 327364-001, appendix D.1.8): no add, no shift. The 64, 16 and
// 8 bit instantiations therefore stay on FastLanes' scalar code, and this
// file does not touch them.

#include <cstdint>
#include <cstring>
#include <knc.h>

// Defined by FastLanes' fls/common/restrict.hpp, which the file this is
// appended to has already included. Defined here too so the file reads and
// parses on its own.
#ifndef FLS_RESTRICT
#define FLS_RESTRICT __restrict
#endif

extern "C" {
// Lets one binary decode a file both ways, so an end-to-end measurement
// compares the same build against itself rather than two builds against
// each other. Read once per 1024 values, next to a branch that is there
// anyway, so it costs nothing measurable. Default is the vector path.
int knc_fls_enabled = 1;

// How many 32-bit unffor calls each path took. Without these, an
// end-to-end number cannot be read: FastLanes narrows physical types by
// magnitude and cascades several encodings, so a column may never reach
// this function at all, and the difference between "the vector path is
// not worth much" and "the vector path ran three times" is invisible.
// One non-atomic increment per 1024 values. `staged` counts the subset of
// `vector` whose input had to be copied to a 4-byte boundary first.
//
// Not atomic, so they assume a single-threaded decode, which is what
// fls_roundtrip does. Under concurrent decodes they undercount; they are
// a diagnostic, and nothing reads them to make a decision.
unsigned long knc_fls_calls_vector = 0;
unsigned long knc_fls_calls_scalar = 0;
unsigned long knc_fls_calls_staged = 0;
}

namespace fastlanes::generated::unffor::fallback::scalar {

// Renamed from unffor by the component script; still the generated switch
// over 33 widths.
void unffor_scalar(const uint32_t* FLS_RESTRICT a_in_p,
                   uint32_t* FLS_RESTRICT       a_out_p,
                   uint8_t                      bw,
                   const uint32_t* FLS_RESTRICT a_base_p);

void unffor(const uint32_t* FLS_RESTRICT a_in_p,
            uint32_t* FLS_RESTRICT       a_out_p,
            uint8_t                      bw,
            const uint32_t* FLS_RESTRICT a_base_p) {
	// Two preconditions the kernels cannot check for themselves, tested
	// once per 1024 values on a branch that always goes the same way. A
	// third, the base pointer, is handled rather than tested; see below.
	//
	// The output must be 64-byte aligned because the kernels store with
	// vmovaps. Every FastLanes buffer that reaches here is declared
	// alignas(64) (unffored_data in dec_unffor_opr, and unffor_arr,
	// unffor_right_arr and unffor_left_arr in the ALP operators), so the
	// branch is here to keep a future upstream buffer from turning into a
	// fault rather than a fallback.
	//
	// The input is never 64-byte aligned in practice: a bitpacked segment
	// starts at an arbitrary offset inside the file buffer, measured at 8,
	// 48 and 60 modulo 64 on 2026-09-20. The kernels take that in their
	// stride, because every load is the unaligned pair. What they cannot
	// take is an offset that is not a multiple of four, which
	// vloadunpackld answers with #GP (ISA reference 327364-001,
	// VLOADUNPACKLD, "Exceptions"), and which about half the columns of a
	// real file have. Those are staged through an aligned buffer below
	// rather than sent to the scalar path.
	//
	// The kernel reads up to 60 bytes past the input. That needs no
	// padding: those bytes are in the same 64-byte line as the last byte
	// of the input, and a line never crosses a page.
	const auto out_bits = reinterpret_cast<uintptr_t>(a_out_p);
	const auto in_bits  = reinterpret_cast<uintptr_t>(a_in_p);

	if (knc_fls_enabled != 0 && bw <= 32 && (out_bits & 63U) == 0U) {
		// The base is copied through a local rather than passed straight
		// down. A base segment holds four bytes per vector but starts at
		// an arbitrary byte offset in the file, so a_base_p can be an odd
		// address; `*(a_base_p)` in FastLanes' scalar code is a legal
		// misaligned scalar load, while the kernel's vpbroadcastd is #GP
		// on anything but a 4-byte boundary (ISA reference 327364-001,
		// VPBROADCASTD, "Exceptions"). Found as a general protection fault
		// at the first instruction of knc_fls_unffor_b11 on 2026-09-20.
		//
		// memcpy, not a dereference: reading a uint32_t through a pointer
		// that is not 4-byte aligned is undefined in C++ whatever x86
		// does with it. The local is 4-byte aligned by definition, and
		// one load plus one store per 1024 values is not measurable.
		uint32_t base_value;
		std::memcpy(&base_value, a_base_p, sizeof base_value);

		knc_fls_calls_vector++;
		if ((in_bits & 3U) == 0U) {
			knc_fls_unffor(a_in_p, a_out_p, bw, &base_value);
			return;
		}

		// An input that is not even 4-byte aligned, which happens for
		// about half the columns in a real file, is staged rather than
		// given up on. The copy is of the packed bytes, at most 4096 of
		// them and usually far fewer, against 4096 bytes of output.
		// Measured on the card on 2026-09-20: 59 of 118 u32 unffor calls
		// take this path, and at bw = 11 it runs at 3.24x the scalar code
		// against 6.61x for the direct path, so it is worth half the win
		// rather than none of it.
		//
		// The exception is bw = 32, where the kernel is already a copy
		// and staging makes it two: 0.98x, a wash. Not special-cased,
		// because the branch would cost more than the width is worth.
		//
		// Sixteen values of slack because the kernel's load pair reads
		// up to 60 bytes past the packed data.
		knc_fls_calls_staged++;
		alignas(64) uint32_t scratch[KNC_FLS_BLOCK + 16];
		std::memcpy(scratch, a_in_p, static_cast<size_t>(bw) * 128U);
		knc_fls_unffor(scratch, a_out_p, bw, &base_value);
		return;
	}
	knc_fls_calls_scalar++;
	unffor_scalar(a_in_p, a_out_p, bw, a_base_p);
}

} // namespace fastlanes::generated::unffor::fallback::scalar
