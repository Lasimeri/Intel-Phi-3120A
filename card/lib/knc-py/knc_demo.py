#!/usr/bin/env python3
"""knc_demo.py: Python on the card, using the vector unit.

Round-trips every bit width against a scalar model written here, then
measures the decode rate. Run it on the card:

    phi put card/lib/knc-py/knc_demo.py /tmp/knc_demo.py
    phi run python3 /tmp/knc_demo.py

See kncmodule.md.
"""
import sys
import time

import knc

LANES = 16


def low_mask(bits):
    return 0xFFFFFFFF if bits >= 32 else (1 << bits) - 1


def scalar_unpack(packed_words, bits):
    """The layout written out, independent of the kernels: value i is lane
    i % 16 at position i // 16."""
    m = low_mask(bits)
    out = [0] * knc.BLOCK
    for i in range(knc.BLOCK):
        lane, pos = i % LANES, i // LANES
        bit = pos * bits
        w, s = bit // 32, bit % 32
        val = packed_words[w * LANES + lane] >> s
        if s + bits > 32:
            val |= packed_words[(w + 1) * LANES + lane] << (32 - s)
        out[i] = val & m
    return out


def check_every_width():
    failed = 0
    values = knc.Buffer(knc.BLOCK * 4)
    out = knc.Buffer(knc.BLOCK * 4)
    # Unsigned: at 32 bits a value fills the word, which "i" would reject.
    vv, ov = memoryview(values).cast("I"), memoryview(out).cast("I")

    for bits in range(1, 33):
        m = low_mask(bits)
        packed = knc.Buffer(knc.packed_bytes(bits))
        for i in range(knc.BLOCK):
            vv[i] = (0x9E3779B9 * (i + 1)) & m

        knc.pack(packed, values, bits)
        knc.unpack(out, packed, bits)

        words = memoryview(packed).cast("I")
        if list(ov) != list(vv) or scalar_unpack(words, bits) != list(vv):
            print(f"  {bits:2} FAILED")
            failed += 1
    print("widths 1 to 32:", "all OK" if failed == 0 else "FAILED")

    for bad in (0, 33, -1):
        try:
            knc.unpack(out, knc.Buffer(4096), bad)
        except ValueError:
            pass
        else:
            print(f"  width {bad} did not raise: FAILED")
            failed += 1

    try:
        knc.unpack(bytearray(knc.BLOCK * 4), knc.Buffer(4096), 11)
    except ValueError:
        pass
    else:
        print("  unaligned output did not raise: FAILED")
        failed += 1

    return failed


def check_cascades():
    """Frame of reference and delta at every width, through the packer.

    Both need the residue to fit in the width unsigned, so the values are
    built from residues rather than the other way round.
    """
    failed = 0
    base = knc.Buffer(knc.LANES * 4)
    bv = memoryview(base).cast("I")
    for i in range(knc.LANES):
        bv[i] = (0x5BF03635 * (i + 1)) & 0xFFFFFFFF

    values = knc.Buffer(knc.BLOCK * 4)
    staged = knc.Buffer(knc.BLOCK * 4)
    out = knc.Buffer(knc.BLOCK * 4)
    vv, ov = memoryview(values).cast("I"), memoryview(out).cast("I")

    for bits in range(1, 33):
        m = low_mask(bits)
        packed = knc.Buffer(knc.packed_bytes(bits))

        for i in range(knc.BLOCK):
            vv[i] = ((0x9E3779B9 * (i + 1)) & m) + bv[i % knc.LANES] & 0xFFFFFFFF
        knc.encode_for(staged, values, base)
        knc.pack(packed, staged, bits)
        knc.unpack_for(out, packed, bits, base)
        if list(ov) != list(vv):
            print(f"  FOR {bits} FAILED")
            failed += 1

        for i in range(knc.BLOCK):
            prev = bv[i] if i < knc.LANES else vv[i - knc.LANES]
            vv[i] = (prev + ((0x9E3779B9 * (i + 1)) & m)) & 0xFFFFFFFF
        knc.encode_delta(staged, values, base)
        knc.pack(packed, staged, bits)
        knc.unpack_delta(out, packed, bits, base)
        if list(ov) != list(vv):
            print(f"  DELTA {bits} FAILED")
            failed += 1

    print("frame of reference and delta, widths 1 to 32:",
          "all OK" if failed == 0 else "FAILED")
    return failed


def measure(bits, blocks=64, reps=64):
    values = knc.Buffer(knc.BLOCK * 4 * blocks)
    packed = knc.Buffer(knc.packed_bytes(bits, blocks))
    out = knc.Buffer(knc.BLOCK * 4 * blocks)
    m = low_mask(bits)

    vv = memoryview(values).cast("I")
    for i in range(knc.BLOCK):
        vv[i] = (0x9E3779B9 * (i + 1)) & m
    knc.pack(packed, values, bits)

    t0 = time.perf_counter()
    for _ in range(reps):
        knc.unpack(out, packed, bits)
    secs = time.perf_counter() - t0
    return reps * blocks * knc.BLOCK / secs / 1e6


def main():
    print("knc_demo: Python on the card, through the built-in knc module")
    print(knc.__doc__.splitlines()[0])
    failed = check_every_width()
    failed += check_cascades()

    print(f"\n{'bits':>5} {'unpack M/s':>12} {'packed bytes':>14}")
    for bits in (1, 8, 11, 16, 32):
        print(f"{bits:5} {measure(bits):12.1f} {knc.packed_bytes(bits):14}")

    # One call unpacks every whole block both buffers hold, so the Python
    # loop is out of the way entirely. The number above is what the card
    # does, not what the interpreter does.
    print("\nno _ctypes here: static musl has no dlopen, so this module is")
    print("built into the interpreter rather than loaded beside it.")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
