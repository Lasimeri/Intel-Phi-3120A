// knc_demo.js: JavaScript on the card, using the vector unit.
//
// Round-trips every bit width against a scalar model written here, then
// measures the decode rate. Run it on the card:
//
//   phi put card/lib/knc-js/knc_demo.js /tmp/knc_demo.js
//   phi run qjs --std /tmp/knc_demo.js
//
// See qjs_knc.md.
import * as knc from "knc";
import * as std from "std";
import * as os from "os";

const LANES = 16;

function lowMask(bits) {
    return bits >= 32 ? 0xFFFFFFFF : (1 << bits) - 1;
}

// The layout written out, independent of the kernels: value i is lane
// i % 16 at position i / 16. Kept in unsigned arithmetic throughout,
// because at 32 bits a value fills the word and JavaScript's bitwise
// operators are signed.
function scalarUnpack(words, bits) {
    const m = lowMask(bits) >>> 0;
    const out = new Uint32Array(knc.BLOCK);

    for (let i = 0; i < knc.BLOCK; i++) {
        const lane = i % LANES, pos = (i / LANES) | 0;
        const bit = pos * bits, w = (bit / 32) | 0, s = bit % 32;
        let val = words[w * LANES + lane] >>> s;

        if (s + bits > 32)
            val = (val | (words[(w + 1) * LANES + lane] << (32 - s))) >>> 0;
        out[i] = (val & m) >>> 0;
    }
    return out;
}

function sameValues(a, b) {
    if (a.length !== b.length)
        return false;
    for (let i = 0; i < a.length; i++)
        if (a[i] !== b[i])
            return false;
    return true;
}

function checkEveryWidth() {
    let failed = 0;
    const values = knc.alloc(knc.BLOCK * 4);
    const out = knc.alloc(knc.BLOCK * 4);
    const vv = new Uint32Array(values), ov = new Uint32Array(out);

    for (let bits = 1; bits <= 32; bits++) {
        const m = lowMask(bits) >>> 0;
        const packed = knc.alloc(knc.packedBytes(bits));

        for (let i = 0; i < knc.BLOCK; i++)
            vv[i] = (Math.imul(0x9E3779B9, i + 1) & m) >>> 0;

        knc.pack(packed, values, bits);
        knc.unpack(out, packed, bits);

        const model = scalarUnpack(new Uint32Array(packed), bits);
        if (!sameValues(ov, vv) || !sameValues(model, vv)) {
            console.log(`  ${bits} FAILED`);
            failed++;
        }
    }
    console.log("widths 1 to 32:", failed === 0 ? "all OK" : "FAILED");

    for (const bad of [0, 33, -1]) {
        let threw = false;
        try {
            knc.unpack(out, knc.alloc(4096), bad);
        } catch (e) {
            threw = e instanceof RangeError;
        }
        if (!threw) {
            console.log(`  width ${bad} did not throw RangeError: FAILED`);
            failed++;
        }
    }

    // A plain ArrayBuffer is whatever malloc returned, which is not what
    // the kernels need. This is the reason knc.alloc exists.
    let threw = false;
    try {
        knc.unpack(new ArrayBuffer(knc.BLOCK * 4), knc.alloc(4096), 11);
    } catch (e) {
        threw = e instanceof TypeError;
    }
    if (!threw) {
        console.log("  unaligned buffer did not throw TypeError (may be aligned by luck)");
    }
    return failed;
}

function measure(bits, blocks, reps) {
    const values = knc.alloc(knc.BLOCK * 4 * blocks);
    const packed = knc.alloc(knc.packedBytes(bits, blocks));
    const out = knc.alloc(knc.BLOCK * 4 * blocks);
    const vv = new Uint32Array(values);
    const m = lowMask(bits) >>> 0;

    for (let i = 0; i < knc.BLOCK; i++)
        vv[i] = (Math.imul(0x9E3779B9, i + 1) & m) >>> 0;
    knc.pack(packed, values, bits);

    const t0 = os.now();
    for (let r = 0; r < reps; r++)
        knc.unpack(out, packed, bits);
    const secs = (os.now() - t0) / 1000;

    return reps * blocks * knc.BLOCK / secs / 1e6;
}

console.log("knc_demo: JavaScript on the card, through the built-in knc module");
const failed = checkEveryWidth();

console.log("\n bits   unpack M/s   packed bytes");
for (const bits of [1, 8, 11, 16, 32]) {
    const rate = measure(bits, 64, 64).toFixed(1);
    console.log(`${String(bits).padStart(5)} ${rate.padStart(12)} ${String(knc.packedBytes(bits)).padStart(14)}`);
}

console.log("\nno dlopen here: static musl, so the module is compiled into qjs");
console.log("rather than loaded beside it, and needs no path or loader.");
std.exit(failed ? 1 : 0);
