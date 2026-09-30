# exec.S: one command, `Exec` to `Exit`. The command gets pipes for its
# three standard streams; its input is relayed from `Stdin` frames, its
# output and errors go back as `Stdout` and `Stderr` frames, and `Stat` is
# answered meanwhile. The Rust agent did this with two pump threads and a
# lock; here one poll loop serves the device, the three pipes and the
# child, so frames never interleave and nothing is ever sent after `Exit`.
# See exec.md.
	.include "defs.inc"

	.bss
	.p2align 3
ex_pid:	.skip 8
ex_status:	.skip 8
# The pipes: stdin read/write, stdout read/write, stderr read/write, and
# the exec-error pipe read/write, as pipe2 fills them (ints).
ex_pipes:	.skip 32
# The parent's ends while the command runs; -1 once closed.
in_fd:	.skip 8
out_fd:	.skip 8
err_fd:	.skip 8
pid_fd:	.skip 8
# StdinEof has arrived: close the input once what is queued is written.
in_eof:	.skip 8
# Input the command has not taken yet: in_buf[in_head, in_tail).
in_head:	.skip 8
in_tail:	.skip 8
# The decoded command, in the arena.
ex_argc:	.skip 8
ex_argv:	.skip 8
ex_envc:	.skip 8
ex_envrec:	.skip 8
ex_idx_a:	.skip 8
ex_idx_b:	.skip 8
ex_envp:	.skip 8
ex_path:	.skip 8
ex_cwd:	.skip 8
ex_nul:	.skip 8
ex_prog_len:	.skip 8
# The arena's next free byte.
ab:	.skip 8
# Page-aligned, so its pages can be handed back after each command.
	.p2align 12
in_buf:	.skip INCAP
	.p2align 6
arena:	.skip ARENACAP

	.section .rodata
s_empty_argv:	.ascii "empty argv"
	.set s_empty_argv_len, . - s_empty_argv
s_nul:	.asciz "nul byte found in provided data"
s_colon:	.asciz ": "
s_stdin_pipe:	.asciz "stdin pipe: "
s_dropping:	.asciz "dropping "
s_during:	.asciz " during a command"
s_path:	.asciz "PATH"
# musl's execvp default when the environment has no PATH.
s_default_path:	.asciz "/usr/local/bin:/bin:/usr/bin"

	.text

	.globl run_exec
# run_exec: the current frame is an Exec. Returns once Exit (or an Error)
# has been sent.
run_exec:
	pushq %rbx
	pushq %r12
	pushq %r13
	pushq %r14
	pushq %r15
	call ex_decode
	cmpq $0, ex_argc(%rip)
	jne 1f
	leaq s_empty_argv(%rip), %rsi
	movl $s_empty_argv_len, %edx
	call send_error
	jmp ex_return
1:	call ex_env
	cmpq $0, ex_nul(%rip)
	je 2f
	call ex_error_begin
	leaq s_nul(%rip), %rsi
	call w_cstr
	call ex_error_end
	jmp ex_return
	# four pipes, all close-on-exec (the child's dup2 clears it on 0-2)
2:	xorl %ebx, %ebx
3:	leaq ex_pipes(%rip), %rdi
	leaq (%rdi,%rbx,8), %rdi
	movl $O_CLOEXEC, %esi
	SYS SYS_pipe2
	testq %rax, %rax
	js 4f
	incl %ebx
	cmpl $4, %ebx
	jb 3b
	jmp 5f
4:	# pipe2 failed after %ebx pipes: close those, report
	movq %rax, %r12
	leal (%rbx,%rbx), %edi
	call close_pipes
	movq %r12, %rdi
	negq %rdi
	call ex_error_os
	jmp ex_return
5:	SYS SYS_fork
	testq %rax, %rax
	jz ex_child
	jns 6f
	movq %rax, %r12
	movl $8, %edi
	call close_pipes
	movq %r12, %rdi
	negq %rdi
	call ex_error_os
	jmp ex_return
6:	movq %rax, ex_pid(%rip)
	# the child's ends
	leaq ex_pipes(%rip), %rbx
	movl 0(%rbx), %edi
	SYS SYS_close
	movl 12(%rbx), %edi
	SYS SYS_close
	movl 20(%rbx), %edi
	SYS SYS_close
	movl 28(%rbx), %edi
	SYS SYS_close
	# The exec-error pipe: EOF means the exec succeeded; four bytes are
	# the errno of whatever failed first in the child.
	subq $16, %rsp
	movl $0, (%rsp)
	xorl %r12d, %r12d
7:	movl 24(%rbx), %edi
	leaq (%rsp,%r12), %rsi
	movl $4, %edx
	subq %r12, %rdx
	jz 8f
	SYS SYS_read
	testq %rax, %rax
	jz 8f
	jns 71f
	cmpq $-EINTR, %rax
	je 7b
	jmp 8f
71:	addq %rax, %r12
	jmp 7b
8:	movl (%rsp), %r13d
	addq $16, %rsp
	movl 24(%rbx), %edi
	SYS SYS_close
	testq %r12, %r12
	jz 10f
	# the exec failed: reap the child, close our ends, report
9:	movq ex_pid(%rip), %rdi
	leaq ex_status(%rip), %rsi
	xorl %edx, %edx
	xorl %r10d, %r10d
	SYS SYS_wait4
	cmpq $-EINTR, %rax
	je 9b
	movl 4(%rbx), %edi
	SYS SYS_close
	movl 8(%rbx), %edi
	SYS SYS_close
	movl 16(%rbx), %edi
	SYS SYS_close
	movl %r13d, %edi
	call ex_error_os
	jmp ex_return
10:	movslq 4(%rbx), %rax
	movq %rax, in_fd(%rip)
	movslq 8(%rbx), %rax
	movq %rax, out_fd(%rip)
	movslq 16(%rbx), %rax
	movq %rax, err_fd(%rip)
	movq $0, in_head(%rip)
	movq $0, in_tail(%rip)
	movq $0, in_eof(%rip)
	# The input pipe is non-blocking, so a command slow to read (or never
	# reading) cannot stall the loop and with it the Stat replies.
	movl 4(%rbx), %edi
	movl $F_GETFL, %esi
	SYS SYS_fcntl
	testq %rax, %rax
	js 11f
	movl 4(%rbx), %edi
	movl $F_SETFL, %esi
	movq %rax, %rdx
	orq $O_NONBLOCK, %rdx
	SYS SYS_fcntl
	testq %rax, %rax
	jns 12f
11:	# writes then block until the command reads; only responsiveness is lost
	pushq %rax
	call l_begin
	leaq s_stdin_pipe(%rip), %rsi
	call w_cstr
	popq %rdi
	negq %rdi
	call w_oserr
	call l_end
12:	# A pidfd turns the command's exit into a poll event; without one
	# (-1) the loop still notices within CHILD_POLL_MS.
	movq ex_pid(%rip), %rdi
	xorl %esi, %esi
	SYS SYS_pidfd_open
	testq %rax, %rax
	jns 13f
	movq $-1, %rax
13:	movq %rax, pid_fd(%rip)
	jmp ex_loop

ex_return:
	popq %r15
	popq %r14
	popq %r13
	popq %r12
	popq %rbx
	ret

# The command loop: relay input until the command ends, answer Stat
# meanwhile, pass its output on as it comes.
ex_loop:
	movq ex_pid(%rip), %rdi
	leaq ex_status(%rip), %rsi
	movl $WNOHANG, %edx
	xorl %r10d, %r10d
	SYS SYS_wait4
	testq %rax, %rax
	jg ex_done
	jz 1f
	movq %rax, %rdi
	jmp dev_fail
1:	call in_flush
	call next_frame
	testl %eax, %eax
	jz 2f
	# a frame was already buffered: act on it, then look at the pipes
	call ex_frame
	xorl %edi, %edi
	call pump_poll
	jmp ex_loop
2:	subq $40, %rsp
	# fds[0]: the device, unless the input backlog is full
	movl dev_fd(%rip), %eax
	movq in_tail(%rip), %rcx
	subq in_head(%rip), %rcx
	cmpq $INPUT_BACKLOG, %rcx
	jb 3f
	movl $-1, %eax
3:	movl %eax, 0(%rsp)
	movl $POLLIN, 4(%rsp)
	# fds[1]: the input pipe, while it has something to take
	movl $-1, %eax
	testq %rcx, %rcx
	jz 4f
	movl in_fd(%rip), %eax
4:	movl %eax, 8(%rsp)
	movl $POLLOUT, 12(%rsp)
	movl out_fd(%rip), %eax
	movl %eax, 16(%rsp)
	movl $POLLIN, 20(%rsp)
	movl err_fd(%rip), %eax
	movl %eax, 24(%rsp)
	movl $POLLIN, 28(%rsp)
	movl pid_fd(%rip), %eax
	movl %eax, 32(%rsp)
	movl $POLLIN, 36(%rsp)
	movq %rsp, %rdi
	movl $5, %esi
	movl $CHILD_POLL_MS, %edx
	call poll_ms
	testq %rax, %rax
	jz 6f
	movzwl 22(%rsp), %eax
	testl $POLLIN | POLLHUP | POLLERR, %eax
	jz 41f
	leaq out_fd(%rip), %rdi
	movl $TAG_STDOUT, %esi
	call pump_read
41:	movzwl 30(%rsp), %eax
	testl $POLLIN | POLLHUP | POLLERR, %eax
	jz 42f
	leaq err_fd(%rip), %rdi
	movl $TAG_STDERR, %esi
	call pump_read
42:	movzwl 6(%rsp), %eax
	testl $POLLIN, %eax
	jz 5f
	call fill
5:	call next_frame
	testl %eax, %eax
	jz 6f
	call ex_frame
6:	addq $40, %rsp
	jmp ex_loop

# The command has exited (ex_status). Its input goes, its output gets
# OUTPUT_GRACE_MS to drain: a background process it started may hold the
# pipes open, and must not hold the session open too. Then Exit.
ex_done:
	movq in_fd(%rip), %rdi
	testq %rdi, %rdi
	js 1f
	SYS SYS_close
	movq $-1, in_fd(%rip)
1:	call now_ms
	leaq OUTPUT_GRACE_MS(%rax), %rbx
2:	movq out_fd(%rip), %rax
	andq err_fd(%rip), %rax
	js 3f
	call now_ms
	movq %rbx, %rdi
	subq %rax, %rdi
	jle 3f
	call pump_poll
	jmp 2b
3:	# exit status, or 128 + the signal (Rust's ExitStatus: code or signal)
	movl ex_status(%rip), %eax
	movl %eax, %ecx
	andl $0x7f, %ecx
	jnz 4f
	shrl $8, %eax
	andl $0xff, %eax
	jmp 5f
4:	leal 128(%rcx), %eax
5:	movl %eax, %r12d
	movb $TAG_EXIT, %dil
	call f_begin
	movl %r12d, %edi
	call w_u32
	call f_send
	# what the pipes carry from now on belongs to no session
	leaq out_fd(%rip), %rdi
	call close_var
	leaq err_fd(%rip), %rdi
	call close_var
	leaq pid_fd(%rip), %rdi
	call close_var
	# the input queue's pages go back: a 1 MiB backlog need not stay resident
	leaq in_buf(%rip), %rdi
	movl $INCAP, %esi
	movl $MADV_DONTNEED, %edx
	SYS SYS_madvise
	jmp ex_return

# close_var(rdi = address of a descriptor variable): close it unless it
# is -1, then set it to -1.
close_var:
	movq (%rdi), %rax
	movq $-1, (%rdi)
	testq %rax, %rax
	js 1f
	movl %eax, %edi
	SYS SYS_close
1:	ret

# close_pipes(edi = n): close the first n descriptors of ex_pipes.
close_pipes:
	pushq %rbx
	pushq %r12
	subq $8, %rsp
	movl %edi, %r12d
	xorl %ebx, %ebx
1:	cmpl %r12d, %ebx
	jae 2f
	leaq ex_pipes(%rip), %rax
	movl (%rax,%rbx,4), %edi
	SYS SYS_close
	incl %ebx
	jmp 1b
2:	addq $8, %rsp
	popq %r12
	popq %rbx
	ret

# pump_poll(edi = ms): wait up to ms for the command's output, pass on what
# came. With both pipes closed it returns at once.
pump_poll:
	movq out_fd(%rip), %rax
	andq err_fd(%rip), %rax
	js 9f
	movl %edi, %edx
	subq $24, %rsp
	movl out_fd(%rip), %eax
	movl %eax, 0(%rsp)
	movl $POLLIN, 4(%rsp)
	movl err_fd(%rip), %eax
	movl %eax, 8(%rsp)
	movl $POLLIN, 12(%rsp)
	movq %rsp, %rdi
	movl $2, %esi
	call poll_ms
	testq %rax, %rax
	jz 8f
	movzwl 6(%rsp), %eax
	testl $POLLIN | POLLHUP | POLLERR, %eax
	jz 1f
	leaq out_fd(%rip), %rdi
	movl $TAG_STDOUT, %esi
	call pump_read
1:	movzwl 14(%rsp), %eax
	testl $POLLIN | POLLHUP | POLLERR, %eax
	jz 8f
	leaq err_fd(%rip), %rdi
	movl $TAG_STDERR, %esi
	call pump_read
8:	addq $24, %rsp
9:	ret

# pump_read(rdi = &out_fd or &err_fd, esi = TAG_STDOUT or TAG_STDERR): one
# read of at most CHUNK bytes, sent as one frame; end of file or a failed
# read closes that pipe for good.
pump_read:
	pushq %rbx
	pushq %r12
	subq $8, %rsp
	movq %rdi, %rbx
	movl %esi, %r12d
	movl (%rbx), %edi
	leaq tx_buf+5(%rip), %rsi
	movl $CHUNK, %edx
	SYS SYS_read
	testq %rax, %rax
	jg 2f
	cmpq $-EINTR, %rax
	je 9f
	movq %rbx, %rdi
	call close_var
	jmp 9f
2:	pushq %rax
	movl %r12d, %edi
	call f_begin
	popq %rax
	addq %rax, wp(%rip)
	call f_send
9:	addq $8, %rsp
	popq %r12
	popq %rbx
	ret

# ex_frame: a frame arrived while the command runs. Stdin is queued,
# StdinEof closes the input once drained, Stat is answered; anything else
# is a client's mistake and is dropped (an Error reply would tell the
# daemon the session is over while the command runs on).
ex_frame:
	movq fr_tag(%rip), %rax
	cmpq $TAG_STDIN, %rax
	je in_push
	cmpq $TAG_STDIN_EOF, %rax
	jne 1f
	movq $1, in_eof(%rip)
	ret
1:	cmpq $TAG_STAT, %rax
	je stat_reply
	call l_begin
	leaq s_dropping(%rip), %rsi
	call w_cstr
	call frame_name
	call w_cstr
	leaq s_during(%rip), %rsi
	call w_cstr
	jmp l_end

# in_push: queue the current Stdin frame's bytes, unless the input is
# closed or StdinEof has come.
in_push:
	cmpq $0, in_eof(%rip)
	jne 9f
	cmpq $0, in_fd(%rip)
	jl 9f
	movq fr_len(%rip), %rdx
	movq in_tail(%rip), %rax
	addq %rdx, %rax
	cmpq $INCAP, %rax
	jbe 1f
	# move what is queued to the front
	leaq in_buf(%rip), %rdi
	movq in_head(%rip), %rsi
	movq in_tail(%rip), %rcx
	subq %rsi, %rcx
	movq %rcx, in_tail(%rip)
	movq $0, in_head(%rip)
	addq %rdi, %rsi
	rep movsb
	movq fr_len(%rip), %rdx
	movq in_tail(%rip), %rax
	addq %rdx, %rax
	cmpq $INCAP, %rax
	ja 9f
1:	leaq in_buf(%rip), %rdi
	addq in_tail(%rip), %rdi
	movq fr_body(%rip), %rsi
	movq %rdx, %rcx
	rep movsb
	addq %rdx, in_tail(%rip)
9:	ret

# in_flush: write what the input pipe takes now. It is closed once all is
# written after StdinEof, or as soon as it breaks (the command closed its
# end or exited: the rest of the input is moot).
in_flush:
	pushq %rbx
1:	movq in_fd(%rip), %rbx
	testq %rbx, %rbx
	js 9f
	movq in_head(%rip), %rsi
	movq in_tail(%rip), %rdx
	subq %rsi, %rdx
	jz 5f
	leaq in_buf(%rip), %rax
	addq %rax, %rsi
	movl %ebx, %edi
	SYS SYS_write
	testq %rax, %rax
	jz 9f
	js 2f
	addq %rax, in_head(%rip)
	jmp 1b
2:	cmpq $-EAGAIN, %rax
	je 9f
	cmpq $-EINTR, %rax
	je 1b
	# broken: drop the pipe and everything queued for it
	movl %ebx, %edi
	SYS SYS_close
	movq $-1, in_fd(%rip)
	movq $0, in_head(%rip)
	movq $0, in_tail(%rip)
	jmp 9f
5:	movq $0, in_head(%rip)
	movq $0, in_tail(%rip)
	cmpq $0, in_eof(%rip)
	je 9f
	movl %ebx, %edi
	SYS SYS_close
	movq $-1, in_fd(%rip)
9:	popq %rbx
	ret

# ex_error_begin / ex_error_end: an Error frame "<program>: ..." around
# what is written between them.
ex_error_begin:
	movb $TAG_ERROR, %dil
	call f_begin
	call w_str_begin
	movq ex_argv(%rip), %rax
	movq (%rax), %rsi
	movq ex_prog_len(%rip), %rdx
	call w_bytes
	leaq s_colon(%rip), %rsi
	jmp w_cstr

ex_error_end:
	call w_str_end
	jmp f_send

# ex_error_os(edi = errno): "<program>: <why> (os error N)".
ex_error_os:
	pushq %rdi
	call ex_error_begin
	popq %rdi
	call w_oserr
	jmp ex_error_end

# ------------------------------------------------------------ decoding

# ex_copy(rsi = cursor at a wire string) -> rax = its NUL-terminated copy
# in the arena, rdx = its length, rsi = past it; ecx = 1 when it holds a
# NUL byte.
ex_copy:
	movzwl (%rsi), %edx
	addq $2, %rsi
	movq ab(%rip), %rdi
	movq %rdi, %rax
	xorl %r8d, %r8d
	movq %rdx, %rcx
1:	testq %rcx, %rcx
	jz 2f
	movb (%rsi), %r9b
	testb %r9b, %r9b
	jnz 11f
	movl $1, %r8d
11:	movb %r9b, (%rdi)
	incq %rsi
	incq %rdi
	decq %rcx
	jmp 1b
2:	movb $0, (%rdi)
	incq %rdi
	movq %rdi, ab(%rip)
	movl %r8d, %ecx
	ret

# ab_align: round the arena's next free byte up to 8.
ab_align:
	movq ab(%rip), %rax
	addq $7, %rax
	andq $-8, %rax
	movq %rax, ab(%rip)
	ret

# ex_decode: copy the Exec frame (already checked by validate) into the
# arena: argv as a NULL-terminated pointer array, one 32-byte record per
# environment entry (key, value, key length, value length, NUL flag), cwd
# (NULL when empty, as phi-rpc decodes it).
ex_decode:
	pushq %rbx
	pushq %r12
	pushq %r13
	leaq arena(%rip), %rax
	movq %rax, ab(%rip)
	movq $0, ex_nul(%rip)
	movq $0, ex_path(%rip)
	movq fr_body(%rip), %rbx
	movzwl (%rbx), %r12d
	addq $2, %rbx
	movq %r12, ex_argc(%rip)
	movq ab(%rip), %rax
	movq %rax, ex_argv(%rip)
	leaq 8(%rax,%r12,8), %rax
	movq %rax, ab(%rip)
	xorl %r13d, %r13d
1:	cmpq %r12, %r13
	jae 2f
	movq %rbx, %rsi
	call ex_copy
	movq %rsi, %rbx
	orq %rcx, ex_nul(%rip)
	movq ex_argv(%rip), %rdi
	movq %rax, (%rdi,%r13,8)
	testq %r13, %r13
	jnz 11f
	movq %rdx, ex_prog_len(%rip)
11:	incq %r13
	jmp 1b
2:	movq ex_argv(%rip), %rdi
	movq $0, (%rdi,%r12,8)
	# environment records
	movzwl (%rbx), %r12d
	addq $2, %rbx
	movq %r12, ex_envc(%rip)
	call ab_align
	movq ab(%rip), %rax
	movq %rax, ex_envrec(%rip)
	movq %r12, %rcx
	shlq $5, %rcx
	addq %rcx, %rax
	movq %rax, ex_idx_a(%rip)
	leaq (%rax,%r12,4), %rax
	movq %rax, ex_idx_b(%rip)
	leaq (%rax,%r12,4), %rax
	movq %rax, ab(%rip)
	call ab_align
	movq ab(%rip), %rax
	movq %rax, ex_envp(%rip)
	leaq 8(%rax,%r12,8), %rax
	movq %rax, ab(%rip)
	xorl %r13d, %r13d
3:	cmpq %r12, %r13
	jae 4f
	movq %r13, %r8
	shlq $5, %r8
	addq ex_envrec(%rip), %r8
	pushq %r8
	movq %rbx, %rsi
	call ex_copy
	movq %rsi, %rbx
	movq (%rsp), %r8
	movq %rax, 0(%r8)
	movl %edx, 16(%r8)
	movl %ecx, 24(%r8)
	movq %rbx, %rsi
	call ex_copy
	movq %rsi, %rbx
	popq %r8
	movq %rax, 8(%r8)
	movl %edx, 20(%r8)
	orl %ecx, 24(%r8)
	incq %r13
	jmp 3b
4:	# cwd
	movq %rbx, %rsi
	call ex_copy
	orq %rcx, ex_nul(%rip)
	testq %rdx, %rdx
	jnz 5f
	xorl %eax, %eax
5:	movq %rax, ex_cwd(%rip)
	popq %r13
	popq %r12
	popq %rbx
	ret

# ------------------------------------------------------------ environment

# kcmp(rdi = record a, rsi = record b) -> eax < 0, 0 or > 0 as a's key
# sorts before, with, or after b's: bytewise, then shorter first, which is
# the order of the OsString keys in Rust's BTreeMap.
kcmp:
	movq 0(%rdi), %r8
	movq 0(%rsi), %r9
	movl 16(%rdi), %r10d
	movl 16(%rsi), %r11d
	movl %r10d, %ecx
	cmpl %r11d, %ecx
	jbe 1f
	movl %r11d, %ecx
1:	testl %ecx, %ecx
	jz 3f
	movzbl (%r8), %eax
	movzbl (%r9), %edx
	subl %edx, %eax
	jnz 4f
	incq %r8
	incq %r9
	decl %ecx
	jmp 1b
3:	movl %r10d, %eax
	subl %r11d, %eax
4:	ret

# rec(eax = index) -> rdi = its record.
.macro REC idx, reg
	movl \idx, %eax
	shlq $5, %rax
	movq ex_envrec(%rip), \reg
	addq %rax, \reg
.endm

# ex_env: the child's environment as Rust's Command builds it after
# env_clear and envs: one entry per key, the last value given winning,
# sorted by key; "k=v" strings in the arena; ex_path at PATH's value;
# ex_nul set when a kept entry holds a NUL byte. A stable bottom-up merge
# sort of indices, so equal keys stay in arrival order and the last of a
# run is the one kept.
ex_env:
	pushq %rbx
	pushq %rbp
	pushq %r12
	pushq %r13
	pushq %r14
	pushq %r15
	subq $40, %rsp
	movq ex_envc(%rip), %r13
	movq ex_idx_a(%rip), %rbx	# src
	movq ex_idx_b(%rip), %r12	# dst
	xorl %ecx, %ecx
1:	cmpq %r13, %rcx
	jae 2f
	movl %ecx, (%rbx,%rcx,4)
	incq %rcx
	jmp 1b
2:	movl $1, %r14d			# width
3:	cmpq %r13, %r14
	jae 20f
	xorl %r15d, %r15d		# lo
4:	cmpq %r13, %r15
	jae 19f
	# mid = min(lo + width, n), hi = min(lo + 2 width, n)
	leaq (%r15,%r14), %rax
	cmpq %r13, %rax
	jbe 5f
	movq %r13, %rax
5:	movq %rax, 0(%rsp)		# mid
	leaq (%r15,%r14,2), %rax
	cmpq %r13, %rax
	jbe 6f
	movq %r13, %rax
6:	movq %rax, 8(%rsp)		# hi
	movq %r15, 16(%rsp)		# i
	movq 0(%rsp), %rax
	movq %rax, 24(%rsp)		# j
	movq %r15, %rbp			# k
7:	movq 16(%rsp), %rax
	cmpq 0(%rsp), %rax
	jae 10f
	movq 24(%rsp), %rax
	cmpq 8(%rsp), %rax
	jae 10f
	# take j only when its key sorts strictly before i's (stability)
	movq 24(%rsp), %rax
	movl (%rbx,%rax,4), %eax
	REC %eax, %rdi
	movq 16(%rsp), %rax
	movl (%rbx,%rax,4), %eax
	REC %eax, %rsi
	call kcmp
	testl %eax, %eax
	js 8f
	movq 16(%rsp), %rax
	movl (%rbx,%rax,4), %eax
	movl %eax, (%r12,%rbp,4)
	incq 16(%rsp)
	incq %rbp
	jmp 7b
8:	movq 24(%rsp), %rax
	movl (%rbx,%rax,4), %eax
	movl %eax, (%r12,%rbp,4)
	incq 24(%rsp)
	incq %rbp
	jmp 7b
10:	# the rest of either run
	movq 16(%rsp), %rax
	cmpq 0(%rsp), %rax
	jae 11f
	movl (%rbx,%rax,4), %eax
	movl %eax, (%r12,%rbp,4)
	incq 16(%rsp)
	incq %rbp
	jmp 10b
11:	movq 24(%rsp), %rax
	cmpq 8(%rsp), %rax
	jae 12f
	movl (%rbx,%rax,4), %eax
	movl %eax, (%r12,%rbp,4)
	incq 24(%rsp)
	incq %rbp
	jmp 11b
12:	movq 8(%rsp), %r15
	jmp 4b
19:	xchgq %rbx, %r12
	addq %r14, %r14
	jmp 3b
20:	# rbx holds the sorted indices: keep the last of each run of equal keys
	xorl %r15d, %r15d		# t
	xorl %ebp, %ebp			# entries kept
21:	cmpq %r13, %r15
	jae 29f
	leaq 1(%r15), %rax
	cmpq %r13, %rax
	jae 22f
	movl (%rbx,%rax,4), %eax
	REC %eax, %rsi
	movl (%rbx,%r15,4), %eax
	REC %eax, %rdi
	call kcmp
	testl %eax, %eax
	jz 28f
22:	movl (%rbx,%r15,4), %eax
	REC %eax, %r14
	movl 24(%r14), %eax
	orq %rax, ex_nul(%rip)
	# "k=v" at ab
	movq ab(%rip), %rdi
	movq ex_envp(%rip), %rax
	movq %rdi, (%rax,%rbp,8)
	incq %rbp
	movq 0(%r14), %rsi
	movl 16(%r14), %ecx
	rep movsb
	movb $'=', (%rdi)
	incq %rdi
	movq %rdi, 32(%rsp)		# the value's copy
	movq 8(%r14), %rsi
	movl 20(%r14), %ecx
	rep movsb
	movb $0, (%rdi)
	incq %rdi
	movq %rdi, ab(%rip)
	movq 0(%r14), %rsi
	movl 16(%r14), %edx
	leaq s_path(%rip), %rdi
	call eq_cstr
	testl %eax, %eax
	jz 28f
	movq 32(%rsp), %rax
	movq %rax, ex_path(%rip)
28:	incq %r15
	jmp 21b
29:	movq ex_envp(%rip), %rax
	movq $0, (%rax,%rbp,8)
	addq $40, %rsp
	popq %r15
	popq %r14
	popq %r13
	popq %r12
	popq %rbp
	popq %rbx
	ret

# ------------------------------------------------------------ the child

# The child side of fork: the pipes onto 0, 1 and 2, the working
# directory, a clean signal state (an empty mask, SIGPIPE back to its
# default), then exec. Any failure writes its errno to the exec-error
# pipe and exits 1, as Rust's Command does; nothing here returns.
ex_child:
	leaq ex_pipes(%rip), %rbx
	movl 0(%rbx), %edi
	xorl %esi, %esi
	SYS SYS_dup2
	testq %rax, %rax
	js 9f
	movl 12(%rbx), %edi
	movl $1, %esi
	SYS SYS_dup2
	testq %rax, %rax
	js 9f
	movl 20(%rbx), %edi
	movl $2, %esi
	SYS SYS_dup2
	testq %rax, %rax
	js 9f
	movq ex_cwd(%rip), %rdi
	testq %rdi, %rdi
	jz 1f
	SYS SYS_chdir
	testq %rax, %rax
	js 9f
1:	subq $32, %rsp
	movq $0, (%rsp)
	movl $SIG_SETMASK, %edi
	movq %rsp, %rsi
	xorl %edx, %edx
	movl $8, %r10d
	SYS SYS_rt_sigprocmask
	testq %rax, %rax
	js 9f
	movq $SIG_DFL, 0(%rsp)
	movq $0, 8(%rsp)
	movq $0, 16(%rsp)
	movq $0, 24(%rsp)
	movl $SIGPIPE, %edi
	movq %rsp, %rsi
	xorl %edx, %edx
	movl $8, %r10d
	SYS SYS_rt_sigaction
	testq %rax, %rax
	js 9f
	call ex_execvp
	negq %rax
	movq %rax, %r12
	jmp 10f
9:	movq %rax, %r12
10:	negq %r12
	subq $16, %rsp
	movl %r12d, (%rsp)
	movl 28(%rbx), %edi
	movq %rsp, %rsi
	movl $4, %edx
	SYS SYS_write
	movl $1, %edi
	SYS SYS_exit

# ex_execvp -> eax = the errno to report (it returns only on failure):
# musl's execvp (src/process/execvp.c, __execvpe) over the child's own
# environment, which is what Rust's Command runs after installing it.
ex_execvp:
	pushq %rbx
	pushq %rbp
	pushq %r12
	pushq %r13
	pushq %r14
	pushq %r15
	subq $4408, %rsp
	movq ex_argv(%rip), %rax
	movq (%rax), %r12		# file
	movl $ENOENT, %ebx		# errno so far
	cmpb $0, (%r12)
	je 90f
	# a name with a slash is run as it is
	movq %r12, %rsi
1:	movb (%rsi), %al
	testb %al, %al
	jz 3f
	cmpb $'/', %al
	je 2f
	incq %rsi
	jmp 1b
2:	movq %r12, %rdi
	movq ex_argv(%rip), %rsi
	movq ex_envp(%rip), %rdx
	SYS SYS_execve
	negq %rax
	movl %eax, %ebx
	jmp 90f
3:	# k = strnlen(file, NAME_MAX + 1)
	movq %rsi, %r13
	subq %r12, %r13
	cmpq $255, %r13
	jbe 4f
	movl $ENAMETOOLONG, %ebx
	jmp 90f
4:	movq ex_path(%rip), %r14
	testq %r14, %r14
	jnz 5f
	leaq s_default_path(%rip), %r14
5:	# l = strnlen(path, PATH_MAX - 1) + 1
	xorl %ecx, %ecx
6:	cmpq $4095, %rcx
	jae 7f
	cmpb $0, (%r14,%rcx)
	je 7f
	incq %rcx
	jmp 6b
7:	leaq 1(%rcx), %r15
	xorl %ebp, %ebp			# seen EACCES
8:	# z = strchrnul(p, ':')
	movq %r14, %rsi
81:	movb (%rsi), %al
	testb %al, %al
	jz 82f
	cmpb $':', %al
	je 82f
	incq %rsi
	jmp 81b
82:	movq %rsi, %rdx
	subq %r14, %rdx			# z - p
	cmpq %r15, %rdx
	jb 83f
	cmpb $0, (%rsi)
	je 89f
	leaq 1(%rsi), %r14
	jmp 8b
83:	# b = p[0, z - p] "/" file, or just file for an empty entry
	pushq %rsi
	leaq 8(%rsp), %rdi
	movq %r14, %rsi
	movq %rdx, %rcx
	rep movsb
	testq %rdx, %rdx
	jz 84f
	movb $'/', (%rdi)
	incq %rdi
84:	movq %r12, %rsi
	leaq 1(%r13), %rcx
	rep movsb
	leaq 8(%rsp), %rdi
	movq ex_argv(%rip), %rsi
	movq ex_envp(%rip), %rdx
	SYS SYS_execve
	popq %rsi
	negq %rax
	movl %eax, %ebx
	cmpl $EACCES, %eax
	jne 85f
	movl $1, %ebp
	jmp 86f
85:	cmpl $ENOENT, %eax
	je 86f
	cmpl $ENOTDIR, %eax
	jne 90f
86:	cmpb $0, (%rsi)
	je 89f
	leaq 1(%rsi), %r14
	jmp 8b
89:	testl %ebp, %ebp
	jz 90f
	movl $EACCES, %ebx
90:	movl %ebx, %eax
	addq $4408, %rsp
	popq %r15
	popq %r14
	popq %r13
	popq %r12
	popq %rbp
	popq %rbx
	ret
