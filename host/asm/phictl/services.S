# services.S: the services a booted card is given, each as an open and a
# step the daemon loop calls every pass (console.S). A service that was
# not asked for costs one compare per pass. See services.md.
	.include "defs.inc"

	.text

	.globl services_open
# services_open: create what can be created before the card is up (the
# control socket's listener).
services_open:
	cmpq $0, o_serve(%rip)
	je 1f
	call serve_open
1:	ret

	.globl services_step
# services_step -> eax 1 when any service moved bytes.
services_step:
	pushq %rbx
	xorl %ebx, %ebx
	cmpq $0, o_serve(%rip)
	je 1f
	call serve_step
	orl %eax, %ebx
1:	cmpq $0, o_disk(%rip)
	je 2f
	call disk_step
	orl %eax, %ebx
2:	cmpq $0, o_host_mem(%rip)
	je 3f
	call hostmem_step
	orl %eax, %ebx
3:	cmpq $0, o_forward(%rip)
	je 4f
	call forward_step
	orl %eax, %ebx
4:	cmpq $0, o_net(%rip)
	je 5f
	call net_step
	orl %eax, %ebx
5:	movl %ebx, %eax
	popq %rbx
	ret

	.globl services_fds
# services_fds(rdi = pollfd cursor) -> rax = the cursor past what was
# added: the descriptors whose readiness the services' next turn would
# act on (the block services have none: their events are the card's).
services_fds:
	cmpq $0, o_serve(%rip)
	je 1f
	call serve_fds
	movq %rax, %rdi
1:	cmpq $0, o_forward(%rip)
	je 2f
	call forward_fds
	movq %rax, %rdi
2:	cmpq $0, o_net(%rip)
	je 3f
	call net_fds
	movq %rax, %rdi
3:	movq %rdi, %rax
	ret
