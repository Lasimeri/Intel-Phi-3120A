# agent.S: phi-agent, the card end of `phictl exec`, `put`, `get`, `status`
# and of `phitop`'s samples. Reads rpc frames (phi-rpc) from /dev/phirpc
# (kernel patch 0022) and answers them one session at a time: a command
# with its standard streams relayed (exec.S), a file written or read back
# (xfer.S); `Stat` is answered in every state (stat.S). Started once by
# /init, without respawn, so nothing the host sends may end it: only the
# device failing does. See agent.md.
	.include "defs.inc"

	.bss
	.p2align 3
	.globl page_kb
# Kilobytes per page, from the auxiliary vector (4 without one).
page_kb:	.skip 8

	.section .rodata
	.globl agent_version
agent_version:	.asciz "0.2.0"
s_device:	.asciz "/dev/phirpc"
s_devnull:	.asciz "/dev/null"
s_listen1:	.asciz "phi-agent "
s_listen2:	.asciz " listening on "
s_opt_device:	.asciz "--device"
s_opt_root:	.asciz "--root"
s_dash:	.asciz "-"
s_usage:	.asciz "usage: phi-agent [--device PATH|-] [--root DIR]"
s_colon:	.asciz ": "
s_unexpected1:	.asciz "unexpected "
s_unexpected2:	.asciz " outside a session"

	.text
	.globl _start
_start:
	movq (%rsp), %r12		# argc
	leaq 8(%rsp), %r13		# argv
	# the auxiliary vector follows the environment's NULL
	leaq 8(%r13,%r12,8), %rdx
1:	cmpq $0, (%rdx)
	leaq 8(%rdx), %rdx
	jne 1b
	movq $4, page_kb(%rip)
2:	movq (%rdx), %rax
	testq %rax, %rax
	jz 4f
	cmpq $AT_PAGESZ, %rax
	jne 3f
	movq 8(%rdx), %rax
	shrq $10, %rax
	jz 3f
	movq %rax, page_kb(%rip)
3:	addq $16, %rdx
	jmp 2b
4:
	# Standard descriptors that are closed get /dev/null, as Rust's
	# runtime does before main, so a pipe never lands on 0, 1 or 2.
	xorl %ebx, %ebx
5:	movl %ebx, %edi
	movl $F_GETFD, %esi
	SYS SYS_fcntl
	cmpq $-EBADF, %rax
	jne 6f
	leaq s_devnull(%rip), %rdi
	movl $O_RDWR, %esi
	xorl %edx, %edx
	SYS SYS_open
6:	incl %ebx
	cmpl $3, %ebx
	jb 5b
	# SIGPIPE ignored, as Rust's runtime does: a command that closes its
	# input shows as EPIPE on the write, not as the agent's death.
	subq $32, %rsp
	movq $SIG_IGN, (%rsp)
	movq $0, 8(%rsp)
	movq $0, 16(%rsp)
	movq $0, 24(%rsp)
	movl $SIGPIPE, %edi
	movq %rsp, %rsi
	xorl %edx, %edx
	movl $8, %r10d
	SYS SYS_rt_sigaction
	addq $32, %rsp

	# Arguments: none on the card. --device - reads frames from standard
	# input (the host tests pass a socket there); --root prefixes every
	# /proc and /sys path (the tests' fixtures).
	leaq s_device(%rip), %rax
	movq %rax, dev_name(%rip)
	movl $1, %r14d
7:	cmpq %r12, %r14
	jae 10f
	movq (%r13,%r14,8), %rbx
	leaq 1(%r14), %r15
	cmpq %r12, %r15
	jae usage
	movq %rbx, %rsi
	call cstr_len
	movq %rbx, %rsi
	leaq s_opt_device(%rip), %rdi
	call eq_cstr
	testl %eax, %eax
	jz 8f
	movq (%r13,%r15,8), %rax
	movq %rax, dev_name(%rip)
	jmp 9f
8:	movq %rbx, %rsi
	call cstr_len
	movq %rbx, %rsi
	leaq s_opt_root(%rip), %rdi
	call eq_cstr
	testl %eax, %eax
	jz usage
	movq (%r13,%r15,8), %rsi
	movq %rsi, root_ptr(%rip)
	call cstr_len
	movq %rdx, root_len(%rip)
9:	addq $2, %r14
	jmp 7b
10:
	movq dev_name(%rip), %rsi
	call cstr_len
	movq dev_name(%rip), %rsi
	leaq s_dash(%rip), %rdi
	call eq_cstr
	testl %eax, %eax
	jz 11f
	movq $0, dev_fd(%rip)
	jmp 12f
11:	movq dev_name(%rip), %rdi
	movl $O_RDWR | O_CLOEXEC, %esi
	xorl %edx, %edx
	SYS SYS_open
	testq %rax, %rax
	js open_failed
	movq %rax, dev_fd(%rip)
12:	call l_begin_bare
	leaq s_listen1(%rip), %rsi
	call w_cstr
	leaq agent_version(%rip), %rsi
	call w_cstr
	leaq s_listen2(%rip), %rsi
	call w_cstr
	movq dev_name(%rip), %rsi
	call w_cstr
	call l_end

# One frame at a time, outside any session.
main_loop:
	call next_blocking
	movq fr_tag(%rip), %rax
	cmpq $TAG_EXEC, %rax
	je 20f
	cmpq $TAG_PUT_OPEN, %rax
	je 21f
	cmpq $TAG_GET, %rax
	je 22f
	cmpq $TAG_PING, %rax
	je 23f
	cmpq $TAG_STAT, %rax
	je 24f
	# Stream frames that outlived their session (the daemon closes a
	# departed client's input after the fact, and a command may exit
	# before its input has all arrived): nothing to answer.
	cmpq $TAG_STDIN, %rax
	je main_loop
	cmpq $TAG_STDIN_EOF, %rax
	je main_loop
	cmpq $TAG_PUT_DATA, %rax
	je main_loop
	cmpq $TAG_PUT_CLOSE, %rax
	je main_loop
	# Card-to-host and host-only messages have no business here; the
	# daemon reads the Error as the end of whatever it forwarded.
	movb $TAG_ERROR, %dil
	call f_begin
	call w_str_begin
	leaq s_unexpected1(%rip), %rsi
	call w_cstr
	call frame_name
	call w_cstr
	leaq s_unexpected2(%rip), %rsi
	call w_cstr
	call w_str_end
	call f_send
	jmp main_loop
20:	call run_exec
	jmp main_loop
21:	call run_put
	jmp main_loop
22:	call run_get
	jmp main_loop
23:	movb $TAG_PONG, %dil
	call f_begin
	leaq agent_version(%rip), %rsi
	call cstr_len
	call w_str
	call f_send
	jmp main_loop
24:	call stat_reply
	jmp main_loop

open_failed:
	pushq %rax
	call l_begin
	movq dev_name(%rip), %rsi
	call w_cstr
	leaq s_colon(%rip), %rsi
	call w_cstr
	popq %rdi
	negq %rdi
	call w_oserr
	call l_end
	movl $1, %edi
	SYS SYS_exit_group

usage:
	call l_begin_bare
	leaq s_usage(%rip), %rsi
	call w_cstr
	call l_end
	movl $2, %edi
	SYS SYS_exit_group

	.globl cstr_len
# cstr_len(rsi) -> rdx = length of a NUL-terminated string; rsi kept.
cstr_len:
	xorl %edx, %edx
1:	cmpb $0, (%rsi,%rdx)
	je 2f
	incq %rdx
	jmp 1b
2:	ret
