/*
 * vpu-layout-check.c: verify that the C offload protocol matches the Rust
 * one. Run: tcc -run tools/vpu-layout-check.c   (or `make layout-check`)
 * Exit status 0 when every offset matches the constants pinned by the
 * unit tests in host/crates/phi-vpu/src/proto.rs; 1 otherwise.
 */
#include <stddef.h>
#include <stdio.h>

#include "../card/vpu/vpu_proto.h"
#include "../card/vpu/vpu_exec.h"

static int fails;

static void check(const char *name, size_t have, size_t want)
{
	if (have != want) {
		printf("MISMATCH %-24s C=%zu rust=%zu\n", name, have, want);
		fails = 1;
	} else {
		printf("ok       %-24s %zu\n", name, have);
	}
}

int main(void)
{
	/* Values on the right are what proto.rs asserts. */
	check("sizeof(vpu_request)", sizeof(struct vpu_request), 56);
	check("request.seq", offsetof(struct vpu_request, seq), 0);
	check("request.kernel", offsetof(struct vpu_request, kernel), 8);
	check("request.threads", offsetof(struct vpu_request, threads), 12);
	check("request.n", offsetof(struct vpu_request, n), 16);
	check("request.in_off", offsetof(struct vpu_request, in_off), 24);
	check("request.out_off", offsetof(struct vpu_request, out_off), 32);
	check("request.aux_off", offsetof(struct vpu_request, aux_off), 40);
	check("request.aux_len", offsetof(struct vpu_request, aux_len), 48);

	check("sizeof(vpu_reply)", sizeof(struct vpu_reply), 48);
	check("sizeof(vpu_regs)", sizeof(struct vpu_regs), 2256);
	check("regs.k", offsetof(struct vpu_regs, k), 2048);
	check("regs.gpr", offsetof(struct vpu_regs, gpr), 2112);
	check("regs.rflags", offsetof(struct vpu_regs, rflags), 2240);
	check("regs.rip", offsetof(struct vpu_regs, rip), 2248);
	check("sizeof(vpu_exec)", sizeof(struct vpu_exec), 2512);
	check("exec.exit_rip", offsetof(struct vpu_exec, exit_rip), 128);
	check("exec.exit_kind", offsetof(struct vpu_exec, exit_kind), 144);
	check("exec.regs", offsetof(struct vpu_exec, regs), 256);
	check("sizeof(vpu_mail)", sizeof(struct vpu_mail), 48);
	check("mail.ack", offsetof(struct vpu_mail, ack), 32);
	check("mail.status", offsetof(struct vpu_mail, status), 40);
	check("reply.seq", offsetof(struct vpu_reply, seq), 0);
	check("reply.compute_ns", offsetof(struct vpu_reply, compute_ns), 8);
	check("reply.total_ns", offsetof(struct vpu_reply, total_ns), 16);
	check("reply.pull_ns", offsetof(struct vpu_reply, pull_ns), 24);
	check("reply.push_ns", offsetof(struct vpu_reply, push_ns), 32);
	check("reply.status", offsetof(struct vpu_reply, status), 40);
	check("reply.threads", offsetof(struct vpu_reply, threads), 44);

	check("VPU_OFF_READY", VPU_OFF_READY, 0);
	check("VPU_OFF_REQ", VPU_OFF_REQ, 64);
	check("VPU_OFF_REPLY", VPU_OFF_REPLY, 256);
	check("VPU_OFF_DATA", VPU_OFF_DATA, 1u << 20);
	check("VPU_BLOCK", VPU_BLOCK, 4096);
	check("VPU_CHUNK", VPU_CHUNK, 128);

	if (fails) {
		printf("vpu-layout-check: MISMATCH\n");
		return 1;
	}
	printf("vpu-layout-check: ok\n");
	return 0;
}
