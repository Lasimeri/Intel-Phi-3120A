/*
 * hello.c: phase P2 exit test, C half. Exercises the knc64-x87 ABI:
 * float and double arguments (stack), returns (x87 ST0), varargs of
 * doubles (overflow area), long double, 64-bit selects (no CMOV), and
 * the libc paths a real program hits (printf with %f, malloc, pthread).
 * Compiled with knc-cc, audited, and run on the host (same ISA subset).
 */
#include <math.h>
#include <pthread.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

double sum_va(int n, ...)
{
	va_list ap;
	double s = 0;
	va_start(ap, n);
	for (int i = 0; i < n; i++)
		s += va_arg(ap, double);
	va_end(ap);
	return s;
}

float halve(float x) { return x / 2.0f; }
double scale(double x, double y) { return x * y + 1.5; }
long double bigsum(long double a, long double b) { return a + b; }

/* A 64-bit select: without CMOV this must become a branch (patch 0005). */
long pick(long a, long b, int c) { return c ? a : b; }

static void *worker(void *arg)
{
	long *v = arg;
	*v = *v * 2 + 1;
	return NULL;
}

/* Provided by hello_rs.rs when the Rust half is linked in; weak so the
 * C-only build still links. */
__attribute__((weak)) double rust_hypot(double a, double b) { return -1.0; }

int main(void)
{
	double d = scale(3.0, 4.0);
	float f = halve(5.0f);
	long double l = bigsum(1.0L, 2.0L);
	double v = sum_va(3, 1.5, 2.5, 3.0);
	long p = pick(10, 20, d > 13.0);
	char *buf = malloc(64);
	pthread_t t;
	long tv = 20;

	strcpy(buf, "malloc ok");
	pthread_create(&t, NULL, worker, &tv);
	pthread_join(t, NULL);

	printf("scale=%g halve=%g bigsum=%Lg va=%g pick=%ld sqrt=%g %s thread=%ld\n",
	       d, (double)f, l, v, p, sqrt(2.0), buf, tv);
	printf("rust_hypot=%g\n", rust_hypot(3.0, 4.0));
	free(buf);

	/* Expected values, checked so the exit status is the verdict. */
	if (d != 13.5 || f != 2.5f || l != 3.0L || v != 7.0 || p != 10 || tv != 41)
		return 1;
	if (fabs(sqrt(2.0) - 1.4142135623730951) > 1e-15)
		return 2;
	return 0;
}
