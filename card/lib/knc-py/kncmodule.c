/* kncmodule.c: the card's 512-bit vector unit, from Python.
 *
 * Built *into* the interpreter, not loaded beside it. The card's CPython is
 * static against musl, which has no dlopen, so there are no extension
 * modules to import and `_ctypes` is absent for the same reason: it could
 * never load a shared library here even with libffi present. A built-in
 * module is not a workaround for that, it is the only route.
 *
 * card/userland/components/cpython.sh builds it in:
 *   PHI_PYTHON_EXTRA_SRC=card/lib/knc-py PHI_PYTHON_SETUP=card/lib/knc-py/Setup.local
 *
 * See kncmodule.md.
 */
#define PY_SSIZE_T_CLEAN
#include <Python.h>

#include <knc.h>

#include <stdlib.h>
#include <string.h>

/* ------------------------------------------------------------ knc.Buffer */

/* Python's allocator gives 16-byte alignment. Every kernel here loads and
 * stores whole 64-byte vectors and has no unaligned form, so a bytes or a
 * bytearray is not a valid argument and this type is how a caller gets one
 * that is. It exposes the buffer protocol, so `memoryview(buf)` and
 * `buf[:] = data` work and NumPy would see it if NumPy were here. */
typedef struct {
	PyObject_HEAD
	void *mem;
	Py_ssize_t len;
	Py_ssize_t exports;
} BufferObject;

static PyObject *Buffer_new(PyTypeObject *type, PyObject *args, PyObject *kwds)
{
	static char *kw[] = { "nbytes", NULL };
	Py_ssize_t n = 0;
	BufferObject *self;
	void *mem = NULL;

	if (!PyArg_ParseTupleAndKeywords(args, kwds, "n", kw, &n))
		return NULL;
	if (n <= 0)
		return PyErr_Format(PyExc_ValueError, "knc.Buffer: nbytes must be positive, not %zd", n);
	if (posix_memalign(&mem, 64, (size_t)n) != 0)
		return PyErr_NoMemory();
	memset(mem, 0, (size_t)n);

	self = (BufferObject *)type->tp_alloc(type, 0);
	if (!self) {
		free(mem);
		return NULL;
	}
	self->mem = mem;
	self->len = n;
	self->exports = 0;
	return (PyObject *)self;
}

static void Buffer_dealloc(BufferObject *self)
{
	free(self->mem);
	Py_TYPE(self)->tp_free((PyObject *)self);
}

static int Buffer_getbuffer(BufferObject *self, Py_buffer *view, int flags)
{
	/* Always writable: an object that can be written reports readonly = 0
	 * whatever the caller asked for, the way bytearray does. Deriving it
	 * from PyBUF_WRITABLE instead makes plain memoryview(buf) read-only. */
	if (PyBuffer_FillInfo(view, (PyObject *)self, self->mem, self->len, 0, flags) < 0)
		return -1;
	self->exports++;
	return 0;
}

static void Buffer_releasebuffer(BufferObject *self, Py_buffer *view)
{
	(void)view;
	self->exports--;
}

static Py_ssize_t Buffer_length(BufferObject *self)
{
	return self->len;
}

static PyObject *Buffer_repr(BufferObject *self)
{
	return PyUnicode_FromFormat("<knc.Buffer %zd bytes at 64-byte alignment>", self->len);
}

static PyBufferProcs Buffer_as_buffer = {
	.bf_getbuffer = (getbufferproc)Buffer_getbuffer,
	.bf_releasebuffer = (releasebufferproc)Buffer_releasebuffer,
};

static PySequenceMethods Buffer_as_sequence = {
	.sq_length = (lenfunc)Buffer_length,
};

static PyTypeObject BufferType = {
	PyVarObject_HEAD_INIT(NULL, 0)
	.tp_name = "knc.Buffer",
	.tp_basicsize = sizeof(BufferObject),
	.tp_dealloc = (destructor)Buffer_dealloc,
	.tp_repr = (reprfunc)Buffer_repr,
	.tp_as_sequence = &Buffer_as_sequence,
	.tp_as_buffer = &Buffer_as_buffer,
	.tp_flags = Py_TPFLAGS_DEFAULT,
	.tp_doc = PyDoc_STR("Buffer(nbytes): 64-byte aligned zeroed memory, which every kernel here requires."),
	.tp_new = Buffer_new,
};

/* ------------------------------------------------------------- the kernels */

static int check_width(int bits)
{
	if (bits < 1 || bits > 32) {
		PyErr_Format(PyExc_ValueError, "knc: bit width must be 1 to 32, not %d", bits);
		return -1;
	}
	return 0;
}

static int check_aligned(const Py_buffer *b, const char *what)
{
	if ((uintptr_t)b->buf % 64 != 0) {
		PyErr_Format(PyExc_ValueError,
			     "knc: %s is not 64-byte aligned; use knc.Buffer", what);
		return -1;
	}
	return 0;
}

PyDoc_STRVAR(unpack_doc,
"unpack(out, packed, bits) -> int\n\
\n\
Unpack as many whole blocks as both buffers hold, and return how many.\n\
`out` takes 4096 bytes per block, `packed` takes bits*128. Both must be\n\
64-byte aligned: use knc.Buffer.\n\
\n\
The packed layout is not a contiguous bitstream. Value i of a block lives\n\
in lane i%16 at position i//16, which is what makes it vectorisable; see\n\
card/lib/knc/knc.md.");

static PyObject *knc_unpack_py(PyObject *self, PyObject *args)
{
	Py_buffer out, packed;
	int bits;
	Py_ssize_t blocks, i;

	(void)self;
	if (!PyArg_ParseTuple(args, "w*y*i", &out, &packed, &bits))
		return NULL;

	PyObject *result = NULL;
	if (check_width(bits) < 0 || check_aligned(&out, "out") < 0 || check_aligned(&packed, "packed") < 0)
		goto done;

	{
		Py_ssize_t per_block = (Py_ssize_t)bits * KNC_BLOCK / 8;
		Py_ssize_t a = out.len / ((Py_ssize_t)KNC_BLOCK * 4);
		Py_ssize_t b = packed.len / per_block;

		blocks = a < b ? a : b;
		Py_BEGIN_ALLOW_THREADS
		for (i = 0; i < blocks; i++)
			knc_unpack((int *)out.buf + i * KNC_BLOCK,
				   (const char *)packed.buf + i * per_block, (unsigned)bits);
		Py_END_ALLOW_THREADS
		result = PyLong_FromSsize_t(blocks);
	}
done:
	PyBuffer_Release(&out);
	PyBuffer_Release(&packed);
	return result;
}

PyDoc_STRVAR(pack_doc,
"pack(packed, values, bits) -> int\n\
\n\
The inverse of unpack. Values wider than `bits` are truncated rather than\n\
corrupting their neighbours. Returns the number of blocks packed.");

static PyObject *knc_pack_py(PyObject *self, PyObject *args)
{
	Py_buffer packed, values;
	int bits;
	Py_ssize_t blocks, i;

	(void)self;
	if (!PyArg_ParseTuple(args, "w*y*i", &packed, &values, &bits))
		return NULL;

	PyObject *result = NULL;
	if (check_width(bits) < 0 || check_aligned(&packed, "packed") < 0 || check_aligned(&values, "values") < 0)
		goto done;

	{
		Py_ssize_t per_block = (Py_ssize_t)bits * KNC_BLOCK / 8;
		Py_ssize_t a = values.len / ((Py_ssize_t)KNC_BLOCK * 4);
		Py_ssize_t b = packed.len / per_block;

		blocks = a < b ? a : b;
		Py_BEGIN_ALLOW_THREADS
		for (i = 0; i < blocks; i++)
			knc_pack((char *)packed.buf + i * per_block,
				 (const int *)values.buf + i * KNC_BLOCK, (unsigned)bits);
		Py_END_ALLOW_THREADS
		result = PyLong_FromSsize_t(blocks);
	}
done:
	PyBuffer_Release(&packed);
	PyBuffer_Release(&values);
	return result;
}

/* The cascaded encodings. `base` is KNC_LANES int32, one per lane, and has
 * to be aligned like everything else: it is a kernel argument, and an
 * unaligned one faults on the first vector load rather than returning a
 * wrong answer. */
static PyObject *knc_cascade_py(PyObject *self, PyObject *args, int delta)
{
	Py_buffer out, packed, base;
	int bits;
	Py_ssize_t blocks, i;

	(void)self;
	if (!PyArg_ParseTuple(args, "w*y*iy*", &out, &packed, &bits, &base))
		return NULL;

	PyObject *result = NULL;
	if (check_width(bits) < 0 || check_aligned(&out, "out") < 0
	    || check_aligned(&packed, "packed") < 0 || check_aligned(&base, "base") < 0)
		goto done;
	if (base.len < (Py_ssize_t)(KNC_LANES * sizeof(int))) {
		PyErr_Format(PyExc_ValueError, "knc: base must hold %d int32, one per lane", KNC_LANES);
		goto done;
	}

	{
		Py_ssize_t per_block = (Py_ssize_t)bits * KNC_BLOCK / 8;
		Py_ssize_t a = out.len / ((Py_ssize_t)KNC_BLOCK * 4);
		Py_ssize_t b = packed.len / per_block;
		const knc_unpack_base_fn *table = delta ? knc_unpack_delta_table : knc_unpack_for_table;

		blocks = a < b ? a : b;
		Py_BEGIN_ALLOW_THREADS
		for (i = 0; i < blocks; i++)
			table[bits]((int *)out.buf + i * KNC_BLOCK,
				    (const unsigned *)((const char *)packed.buf + i * per_block),
				    (const int *)base.buf);
		Py_END_ALLOW_THREADS
		result = PyLong_FromSsize_t(blocks);
	}
done:
	PyBuffer_Release(&out);
	PyBuffer_Release(&packed);
	PyBuffer_Release(&base);
	return result;
}

PyDoc_STRVAR(unpack_for_doc,
"unpack_for(out, packed, bits, base) -> int\n\
\n\
Unpack, adding base[lane] to every value. `base` is BLOCK-independent: it\n\
is LANES int32, one per lane, 64-byte aligned. The stored residue must fit\n\
in `bits` bits unsigned.");

static PyObject *knc_unpack_for_py(PyObject *self, PyObject *args)
{
	return knc_cascade_py(self, args, 0);
}

PyDoc_STRVAR(unpack_delta_doc,
"unpack_delta(out, packed, bits, base) -> int\n\
\n\
Unpack as a running sum along positions within each lane, starting from\n\
base[lane]. Wants ascending data: each difference must fit in `bits` bits\n\
unsigned.");

static PyObject *knc_unpack_delta_py(PyObject *self, PyObject *args)
{
	return knc_cascade_py(self, args, 1);
}

/* The encode side, one pass over the values before packing. */
static PyObject *knc_encode_py(PyObject *self, PyObject *args, int delta)
{
	Py_buffer out, values, base;
	Py_ssize_t blocks, i;

	(void)self;
	if (!PyArg_ParseTuple(args, "w*y*y*", &out, &values, &base))
		return NULL;

	PyObject *result = NULL;
	if (check_aligned(&out, "out") < 0 || check_aligned(&values, "values") < 0
	    || check_aligned(&base, "base") < 0)
		goto done;
	if (base.len < (Py_ssize_t)(KNC_LANES * sizeof(int))) {
		PyErr_Format(PyExc_ValueError, "knc: base must hold %d int32, one per lane", KNC_LANES);
		goto done;
	}

	{
		Py_ssize_t a = out.len / ((Py_ssize_t)KNC_BLOCK * 4);
		Py_ssize_t b = values.len / ((Py_ssize_t)KNC_BLOCK * 4);

		blocks = a < b ? a : b;
		Py_BEGIN_ALLOW_THREADS
		for (i = 0; i < blocks; i++) {
			int *o = (int *)out.buf + i * KNC_BLOCK;
			const int *v = (const int *)values.buf + i * KNC_BLOCK;

			if (delta)
				knc_encode_delta(o, v, (const int *)base.buf);
			else
				knc_encode_for(o, v, (const int *)base.buf);
		}
		Py_END_ALLOW_THREADS
		result = PyLong_FromSsize_t(blocks);
	}
done:
	PyBuffer_Release(&out);
	PyBuffer_Release(&values);
	PyBuffer_Release(&base);
	return result;
}

PyDoc_STRVAR(encode_for_doc,
"encode_for(out, values, base) -> int\n\
\n\
out[i] = values[i] - base[i % LANES]. Feed the result to pack().");

static PyObject *knc_encode_for_py(PyObject *self, PyObject *args)
{
	return knc_encode_py(self, args, 0);
}

PyDoc_STRVAR(encode_delta_doc,
"encode_delta(out, values, base) -> int\n\
\n\
out[i] = values[i] minus the value before it in the same lane, with\n\
base[lane] standing in before position 0. Feed the result to pack().");

static PyObject *knc_encode_delta_py(PyObject *self, PyObject *args)
{
	return knc_encode_py(self, args, 1);
}

PyDoc_STRVAR(memcpy64_doc,
"memcpy64(dst, src) -> int\n\
\n\
Copy whole 64-byte blocks, as many as both buffers hold, and return how\n\
many bytes moved. Both must be 64-byte aligned.");

static PyObject *knc_memcpy64_py(PyObject *self, PyObject *args)
{
	Py_buffer dst, src;
	Py_ssize_t blocks;

	(void)self;
	if (!PyArg_ParseTuple(args, "w*y*", &dst, &src))
		return NULL;

	PyObject *result = NULL;
	if (check_aligned(&dst, "dst") < 0 || check_aligned(&src, "src") < 0)
		goto done;

	blocks = (dst.len < src.len ? dst.len : src.len) / 64;
	Py_BEGIN_ALLOW_THREADS
	knc_memcpy64(dst.buf, src.buf, (size_t)blocks);
	Py_END_ALLOW_THREADS
	result = PyLong_FromSsize_t(blocks * 64);
done:
	PyBuffer_Release(&dst);
	PyBuffer_Release(&src);
	return result;
}

PyDoc_STRVAR(packed_bytes_doc,
"packed_bytes(bits, blocks=1) -> int\n\
\n\
Bytes a packed buffer needs for that many blocks at that width.");

static PyObject *knc_packed_bytes_py(PyObject *self, PyObject *args)
{
	int bits;
	Py_ssize_t blocks = 1;

	(void)self;
	if (!PyArg_ParseTuple(args, "i|n", &bits, &blocks))
		return NULL;
	if (check_width(bits) < 0)
		return NULL;
	return PyLong_FromSsize_t((Py_ssize_t)bits * KNC_BLOCK / 8 * blocks);
}

static PyMethodDef knc_methods[] = {
	{ "unpack", knc_unpack_py, METH_VARARGS, unpack_doc },
	{ "pack", knc_pack_py, METH_VARARGS, pack_doc },
	{ "unpack_for", knc_unpack_for_py, METH_VARARGS, unpack_for_doc },
	{ "unpack_delta", knc_unpack_delta_py, METH_VARARGS, unpack_delta_doc },
	{ "encode_for", knc_encode_for_py, METH_VARARGS, encode_for_doc },
	{ "encode_delta", knc_encode_delta_py, METH_VARARGS, encode_delta_doc },
	{ "memcpy64", knc_memcpy64_py, METH_VARARGS, memcpy64_doc },
	{ "packed_bytes", knc_packed_bytes_py, METH_VARARGS, packed_bytes_doc },
	{ NULL, NULL, 0, NULL },
};

PyDoc_STRVAR(module_doc,
"The Knights Corner 512-bit vector unit.\n\
\n\
Bit packing at every width from 1 to 32 and a block copy, running on the\n\
card's vector unit through hand-encoded MVEX instructions that no compiler\n\
emits. Built into this interpreter because static musl has no dlopen.\n\
\n\
    import knc\n\
    values = knc.Buffer(knc.BLOCK * 4)\n\
    packed = knc.Buffer(knc.packed_bytes(11))\n\
    memoryview(values).cast('i')[:] = range(knc.BLOCK)\n\
    knc.pack(packed, values, 11)\n\
    knc.unpack(values, packed, 11)");

static struct PyModuleDef knc_module = {
	PyModuleDef_HEAD_INIT,
	.m_name = "knc",
	.m_doc = module_doc,
	.m_size = -1,
	.m_methods = knc_methods,
};

PyMODINIT_FUNC PyInit_knc(void)
{
	PyObject *m;

	if (PyType_Ready(&BufferType) < 0)
		return NULL;
	m = PyModule_Create(&knc_module);
	if (!m)
		return NULL;
	if (PyModule_AddIntConstant(m, "BLOCK", KNC_BLOCK) < 0
	    || PyModule_AddIntConstant(m, "ALIGN", 64) < 0
	    || PyModule_AddIntConstant(m, "LANES", KNC_LANES) < 0
	    || PyModule_AddObjectRef(m, "Buffer", (PyObject *)&BufferType) < 0) {
		Py_DECREF(m);
		return NULL;
	}
	return m;
}
