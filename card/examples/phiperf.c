/*
 * phiperf.c: a small perf stat for the card. Counts the six generic
 * hardware events the kernel's Knights Corner PMU driver maps (cycles,
 * instructions, cache references and misses, branches and mispredictions;
 * arch/x86/events/intel/knc.c) plus optional raw events, around a command,
 * in every thread of it and its children. The core has two counters, so
 * the kernel multiplexes and the tool scales by time running. Compiled on
 * the card:
 *   cc -O2 -o phiperf phiperf.c
 *   ./phiperf [-n] [-r EVENT[,EVENT...]] COMMAND [ARGS...]
 *     -r 0x10cb,0x10cc     raw KNC event codes (L2_READ_MISS, L2_WRITE_HIT)
 *     -n                   only the raw events (two counters: two events are exact,
 *                          more are multiplexed estimates)
 * See phiperf.md.
 */
#include <errno.h>
#include <linux/perf_event.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/syscall.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

struct counter {
	const char *name;
	uint32_t type;
	uint64_t config;
	int fd;
	uint64_t value, enabled, running;
};

static int open_counter(struct counter *c, pid_t pid)
{
	struct perf_event_attr a;

	memset(&a, 0, sizeof a);
	a.size = sizeof a;
	a.type = c->type;
	a.config = c->config;
	a.disabled = 1;
	a.inherit = 1;
	a.exclude_kernel = 0;
	a.exclude_hv = 1;
	a.read_format = PERF_FORMAT_TOTAL_TIME_ENABLED | PERF_FORMAT_TOTAL_TIME_RUNNING;
	c->fd = syscall(SYS_perf_event_open, &a, pid, -1, -1, 0);
	return c->fd;
}

static double now(void)
{
	struct timespec t;
	clock_gettime(CLOCK_MONOTONIC, &t);
	return t.tv_sec + t.tv_nsec * 1e-9;
}

int main(int argc, char **argv)
{
	struct counter cs[16] = {
		{ "cycles", PERF_TYPE_HARDWARE, PERF_COUNT_HW_CPU_CYCLES },
		{ "instructions", PERF_TYPE_HARDWARE, PERF_COUNT_HW_INSTRUCTIONS },
		{ "cache-references", PERF_TYPE_HARDWARE, PERF_COUNT_HW_CACHE_REFERENCES },
		{ "cache-misses", PERF_TYPE_HARDWARE, PERF_COUNT_HW_CACHE_MISSES },
		{ "branches", PERF_TYPE_HARDWARE, PERF_COUNT_HW_BRANCH_INSTRUCTIONS },
		{ "branch-misses", PERF_TYPE_HARDWARE, PERF_COUNT_HW_BRANCH_MISSES },
	};
	int n = 6, generic = 1;
	static char names[10][24];
	int arg = 1;
	if (argc > 1 && strcmp(argv[1], "-n") == 0) {
		generic = 0;
		n = 0;
		arg = 2;
	}
	if (argc > arg + 1 && strcmp(argv[arg], "-r") == 0) {
		char *list = argv[arg + 1], *tok;
		int k = 0;
		for (tok = strtok(list, ","); tok && n < 16 && k < 10; tok = strtok(NULL, ","), k++) {
			snprintf(names[k], sizeof names[k], "raw %s", tok);
			cs[n].name = names[k];
			cs[n].type = PERF_TYPE_RAW;
			cs[n].config = strtoull(tok, NULL, 0);
			n++;
		}
		arg += 2;
	}
	if (arg >= argc) {
		fprintf(stderr, "usage: %s [-r EVENT,...] COMMAND [ARGS...]\n", argv[0]);
		return 2;
	}
	pid_t pid = fork();
	if (pid < 0) {
		perror("fork");
		return 1;
	}
	if (pid == 0) {
		/* Wait for the counters, then exec. */
		char c;
		if (read(0, &c, 1) <= 0) {
			/* stdin closed or not a pipe: just go */
		}
		execvp(argv[arg], argv + arg);
		perror(argv[arg]);
		_exit(127);
	}
	for (int i = 0; i < n; i++) {
		if (open_counter(&cs[i], pid) < 0) {
			fprintf(stderr, "phiperf: %s: perf_event_open: %s\n", cs[i].name, strerror(errno));
			cs[i].fd = -1;
		}
	}
	double t0 = now();
	for (int i = 0; i < n; i++)
		if (cs[i].fd >= 0)
			ioctl(cs[i].fd, PERF_EVENT_IOC_ENABLE, 0);
	/* The child blocks on stdin; if stdin is a terminal it ran already. */
	int status;
	waitpid(pid, &status, 0);
	double dt = now() - t0;
	for (int i = 0; i < n; i++)
		if (cs[i].fd >= 0)
			ioctl(cs[i].fd, PERF_EVENT_IOC_DISABLE, 0);
	printf("\n%-20s %16s %10s\n", "event", "count", "scaled");
	uint64_t cycles = 0, instr = 0;
	for (int i = 0; i < n; i++) {
		uint64_t v[3] = { 0, 0, 0 };
		if (cs[i].fd < 0)
			continue;
		if (read(cs[i].fd, v, sizeof v) != sizeof v)
			continue;
		double scale = v[2] ? (double)v[1] / v[2] : 1.0;
		uint64_t est = (uint64_t)(v[0] * scale);
		printf("%-20s %16llu %9.0f%%\n", cs[i].name, (unsigned long long)est, 100.0 * (v[2] ? (double)v[2] / v[1] : 1.0));
		if (generic && i == 0) cycles = est;
		if (generic && i == 1) instr = est;
		close(cs[i].fd);
	}
	if (cycles && instr)
		printf("%-20s %16.3f\n", "instructions/cycle", (double)instr / cycles);
	printf("%-20s %16.3f s%s\n", "wall", dt, WIFEXITED(status) && WEXITSTATUS(status) ? " (command failed)" : "");
	return WIFEXITED(status) ? WEXITSTATUS(status) : 1;
}
