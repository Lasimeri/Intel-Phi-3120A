// knc.hpp: the same vector kernels as knc.h, with C++ types.
//
// Header-only, C++17, no allocation of its own, no exceptions on the fast
// path. It adds the two things the C API cannot express: that a block is
// exactly knc::block_values values wide, and that the packed side is a
// byte buffer whose length depends on the bit width. See knc.md.
//
//   c++ -std=c++17 -O2 prog.cpp -lknc
//
// The buffers must be 64-byte aligned, as in C. knc::aligned_buffer is
// provided for callers that do not already have one.
#ifndef KNC_HPP
#define KNC_HPP

#include "knc.h"

#include <cstddef>
#include <cstdlib>
#include <memory>
#include <stdexcept>

namespace knc {

inline constexpr std::size_t block_values = KNC_BLOCK;

/// Bytes one packed block occupies at this width.
inline constexpr std::size_t packed_bytes(unsigned bits) noexcept
{
	return block_values * bits / 8;
}

/// Throws rather than silently doing nothing, which is what the C entry
/// point does for an out-of-range width.
inline void check_width(unsigned bits)
{
	if (bits < 1 || bits > 32)
		throw std::out_of_range("knc: bit width must be 1 to 32");
}

/// One block of values into one packed block, and back.
inline void pack(void *packed, const int *values, unsigned bits)
{
	check_width(bits);
	knc_pack(packed, values, bits);
}

inline void unpack(int *out, const void *packed, unsigned bits)
{
	check_width(bits);
	knc_unpack(out, packed, bits);
}

/// A kernel bound to one width, for a loop that does not want the dispatch.
class codec {
public:
	explicit codec(unsigned bits) : bits_(bits)
	{
		check_width(bits);
	}

	void pack(void *packed, const int *values) const noexcept
	{
		knc_pack_table[bits_](static_cast<unsigned *>(packed), values);
	}

	void unpack(int *out, const void *packed) const noexcept
	{
		knc_unpack_table[bits_](out, static_cast<const unsigned *>(packed));
	}

	/// Frame of reference: add `base[lane]` to every value as it is
	/// decoded. `base` is `lanes` values, 64-byte aligned.
	void unpack_for(int *out, const void *packed, const int *base) const noexcept
	{
		knc_unpack_for_table[bits_](out, static_cast<const unsigned *>(packed), base);
	}

	/// Delta: the running sum along positions within each lane, starting
	/// from `base[lane]`.
	void unpack_delta(int *out, const void *packed, const int *base) const noexcept
	{
		knc_unpack_delta_table[bits_](out, static_cast<const unsigned *>(packed), base);
	}

	unsigned bits() const noexcept { return bits_; }
	std::size_t packed_bytes() const noexcept { return knc::packed_bytes(bits_); }

private:
	unsigned bits_;
};

/// 64-byte aligned storage, which every kernel here requires. Not a
/// container: the kernels write whole blocks and bounds are the caller's.
template <typename T> class aligned_buffer {
public:
	explicit aligned_buffer(std::size_t count) : count_(count)
	{
		void *p = nullptr;
		if (posix_memalign(&p, 64, count * sizeof(T)) != 0)
			throw std::bad_alloc();
		ptr_.reset(static_cast<T *>(p));
	}

	T *data() noexcept { return ptr_.get(); }
	const T *data() const noexcept { return ptr_.get(); }
	std::size_t size() const noexcept { return count_; }
	T &operator[](std::size_t i) noexcept { return ptr_.get()[i]; }
	const T &operator[](std::size_t i) const noexcept { return ptr_.get()[i]; }

private:
	struct free_deleter {
		void operator()(T *p) const noexcept { std::free(p); }
	};

	std::unique_ptr<T, free_deleter> ptr_;
	std::size_t count_;
};

/// Lanes in the layout: a base vector is this many values.
inline constexpr std::size_t lanes = KNC_LANES;

/// The encode side of the two cascaded transforms. Feed the output to
/// `pack`. Both require the residue to fit in the bit width, unsigned;
/// `knc.h` says what happens when it does not.
inline void encode_for(int *out, const int *values, const int *base) noexcept
{
	knc_encode_for(out, values, base);
}

inline void encode_delta(int *out, const int *values, const int *base) noexcept
{
	knc_encode_delta(out, values, base);
}

/// Copy whole 64-byte blocks between aligned pointers.
inline void *memcpy64(void *dst, const void *src, std::size_t blocks) noexcept
{
	return knc_memcpy64(dst, src, blocks);
}

} // namespace knc

#endif // KNC_HPP
