/* qjs_knc.c: the card's 512-bit vector unit, as a QuickJS module.
 *
 * Compiled into `qjs` rather than loaded as a shared object, for the same
 * reason as the Python module: the card's binaries are static against musl
 * and there is no dlopen. `import * as knc from "knc"` then works from any
 * script with no path and no loader.
 *
 * The kernels need 64-byte alignment and QuickJS's ArrayBuffers are
 * whatever malloc returns, so `knc.alloc` is the only way to get a buffer
 * they will accept: it hands back a real ArrayBuffer over memory this file
 * allocated, so `new Uint32Array(buf)` and the rest work normally on it.
 *
 * See qjs_knc.md.
 */
#include "quickjs.h"

#include <knc.h>

#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#ifndef countof
#define countof(x) (sizeof(x) / sizeof((x)[0]))
#endif

static void knc_free_buffer(JSRuntime *rt, void *opaque, void *ptr)
{
	(void)rt;
	(void)opaque;
	free(ptr);
}

/* An ArrayBuffer the kernels will accept. */
static JSValue js_knc_alloc(JSContext *ctx, JSValueConst this_val, int argc, JSValueConst *argv)
{
	int64_t n = 0;
	void *mem = NULL;
	JSValue ab;

	(void)this_val;
	(void)argc;
	if (JS_ToInt64(ctx, &n, argv[0]))
		return JS_EXCEPTION;
	if (n <= 0 || n > (int64_t)1 << 40)
		return JS_ThrowRangeError(ctx, "knc.alloc: size must be positive, not %lld", (long long)n);
	if (posix_memalign(&mem, 64, (size_t)n) != 0)
		return JS_ThrowOutOfMemory(ctx);
	memset(mem, 0, (size_t)n);

	ab = JS_NewArrayBuffer(ctx, mem, (size_t)n, knc_free_buffer, NULL, 0);
	if (JS_IsException(ab))
		free(mem);
	return ab;
}

/* An ArrayBuffer or any view over one. A typed array is accepted because
 * that is what a caller will have after `new Uint32Array(buf)`, and
 * refusing it would make every call site slice things by hand. */
static uint8_t *knc_bytes(JSContext *ctx, JSValueConst v, size_t *len)
{
	size_t size = 0, off = 0, blen = 0, bpe = 0;
	uint8_t *p = JS_GetArrayBuffer(ctx, &size, v);
	JSValue ab;

	if (p) {
		*len = size;
		return p;
	}
	JS_FreeValue(ctx, JS_GetException(ctx)); /* not an ArrayBuffer; try a view */

	ab = JS_GetTypedArrayBuffer(ctx, v, &off, &blen, &bpe);
	if (JS_IsException(ab))
		return NULL;
	p = JS_GetArrayBuffer(ctx, &size, ab);
	JS_FreeValue(ctx, ab);
	if (!p) {
		JS_FreeValue(ctx, JS_GetException(ctx));
		JS_ThrowTypeError(ctx, "knc: expected an ArrayBuffer or a view over one");
		return NULL;
	}
	*len = blen;
	return p + off;
}

static int knc_width(JSContext *ctx, JSValueConst v, uint32_t *bits)
{
	int64_t n = 0;

	if (JS_ToInt64(ctx, &n, v))
		return -1;
	if (n < 1 || n > 32) {
		JS_ThrowRangeError(ctx, "knc: bit width must be 1 to 32, not %lld", (long long)n);
		return -1;
	}
	*bits = (uint32_t)n;
	return 0;
}

static int knc_aligned(JSContext *ctx, const void *p, const char *what)
{
	if ((uintptr_t)p % 64 != 0) {
		JS_ThrowTypeError(ctx, "knc: %s is not 64-byte aligned; allocate it with knc.alloc", what);
		return -1;
	}
	return 0;
}

/* unpack(out, packed, bits) and pack(packed, values, bits) both walk as
 * many whole blocks as the two buffers hold and return the count, so the
 * per-call overhead is amortised over the whole buffer rather than paid
 * per block from JavaScript. */
static JSValue js_knc_codec(JSContext *ctx, JSValueConst this_val, int argc, JSValueConst *argv, int unpacking)
{
	size_t dst_len = 0, src_len = 0;
	uint8_t *dst, *src;
	uint32_t bits;
	int64_t per_block, blocks, i;

	(void)this_val;
	(void)argc;
	if (knc_width(ctx, argv[2], &bits))
		return JS_EXCEPTION;
	dst = knc_bytes(ctx, argv[0], &dst_len);
	if (!dst)
		return JS_EXCEPTION;
	src = knc_bytes(ctx, argv[1], &src_len);
	if (!src)
		return JS_EXCEPTION;
	if (knc_aligned(ctx, dst, unpacking ? "out" : "packed")
	    || knc_aligned(ctx, src, unpacking ? "packed" : "values"))
		return JS_EXCEPTION;

	per_block = (int64_t)bits * KNC_BLOCK / 8;
	{
		int64_t values_bytes = KNC_BLOCK * 4;
		int64_t a = unpacking ? (int64_t)dst_len / values_bytes : (int64_t)dst_len / per_block;
		int64_t b = unpacking ? (int64_t)src_len / per_block : (int64_t)src_len / values_bytes;

		blocks = a < b ? a : b;
	}
	for (i = 0; i < blocks; i++) {
		if (unpacking)
			knc_unpack((int *)(dst + i * KNC_BLOCK * 4), src + i * per_block, bits);
		else
			knc_pack(dst + i * per_block, (const int *)(src + i * KNC_BLOCK * 4), bits);
	}
	return JS_NewInt64(ctx, blocks);
}

static JSValue js_knc_unpack(JSContext *ctx, JSValueConst this_val, int argc, JSValueConst *argv)
{
	return js_knc_codec(ctx, this_val, argc, argv, 1);
}

static JSValue js_knc_pack(JSContext *ctx, JSValueConst this_val, int argc, JSValueConst *argv)
{
	return js_knc_codec(ctx, this_val, argc, argv, 0);
}

static JSValue js_knc_memcpy64(JSContext *ctx, JSValueConst this_val, int argc, JSValueConst *argv)
{
	size_t dst_len = 0, src_len = 0;
	uint8_t *dst, *src;
	size_t blocks;

	(void)this_val;
	(void)argc;
	dst = knc_bytes(ctx, argv[0], &dst_len);
	if (!dst)
		return JS_EXCEPTION;
	src = knc_bytes(ctx, argv[1], &src_len);
	if (!src)
		return JS_EXCEPTION;
	if (knc_aligned(ctx, dst, "dst") || knc_aligned(ctx, src, "src"))
		return JS_EXCEPTION;

	blocks = (dst_len < src_len ? dst_len : src_len) / 64;
	knc_memcpy64(dst, src, blocks);
	return JS_NewInt64(ctx, (int64_t)(blocks * 64));
}

static JSValue js_knc_packed_bytes(JSContext *ctx, JSValueConst this_val, int argc, JSValueConst *argv)
{
	uint32_t bits;
	int64_t blocks = 1;

	(void)this_val;
	if (knc_width(ctx, argv[0], &bits))
		return JS_EXCEPTION;
	if (argc > 1 && JS_ToInt64(ctx, &blocks, argv[1]))
		return JS_EXCEPTION;
	return JS_NewInt64(ctx, (int64_t)bits * KNC_BLOCK / 8 * blocks);
}

static const JSCFunctionListEntry js_knc_funcs[] = {
	JS_CFUNC_DEF("alloc", 1, js_knc_alloc),
	JS_CFUNC_DEF("unpack", 3, js_knc_unpack),
	JS_CFUNC_DEF("pack", 3, js_knc_pack),
	JS_CFUNC_DEF("memcpy64", 2, js_knc_memcpy64),
	JS_CFUNC_DEF("packedBytes", 2, js_knc_packed_bytes),
	JS_PROP_INT32_DEF("BLOCK", KNC_BLOCK, JS_PROP_CONFIGURABLE),
	JS_PROP_INT32_DEF("ALIGN", 64, JS_PROP_CONFIGURABLE),
};

static int js_knc_init(JSContext *ctx, JSModuleDef *m)
{
	return JS_SetModuleExportList(ctx, m, js_knc_funcs, countof(js_knc_funcs));
}

JSModuleDef *js_init_module_knc(JSContext *ctx, const char *module_name)
{
	JSModuleDef *m = JS_NewCModule(ctx, module_name, js_knc_init);

	if (!m)
		return NULL;
	JS_AddModuleExportList(ctx, m, js_knc_funcs, countof(js_knc_funcs));
	return m;
}
