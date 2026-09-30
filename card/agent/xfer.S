# xfer.S: file transfers. `PutOpen` receives a file (`PutData` frames
# until `PutClose`, then `Exit(0)`); `Get` sends one back (`GetData`
# frames, then `GetEnd`). `Stat` is answered during both. Any failure is
# an `Error` "<path>: <why>" that ends the session. See xfer.md.
	.include "defs.inc"

	.bss
	.p2align 3
# The session's path, NUL-terminated, its length, and whether it holds a
# NUL byte (which no file name can).
xf_path:	.skip MAX_STRING + 8
xf_len:	.skip 8
xf_nul:	.skip 8
xf_fd:	.skip 8
xf_size:	.skip 8

	.section .rodata
s_colon:	.asciz ": "
s_nul_name:	.asciz "file name contained an unexpected NUL byte"
s_dropping:	.asciz "dropping "
s_put:	.asciz " during a put"
s_get:	.asciz " during a get"

	.text

# xf_take_path: copy the current frame's leading wire string (checked by
# validate) to xf_path. Returns rsi past it.
xf_take_path:
	movq fr_body(%rip), %rsi
	movzwl (%rsi), %ecx
	addq $2, %rsi
	movq %rcx, xf_len(%rip)
	leaq xf_path(%rip), %rdi
	xorl %edx, %edx
1:	testq %rcx, %rcx
	jz 2f
	movb (%rsi), %al
	testb %al, %al
	jnz 11f
	movl $1, %edx
11:	movb %al, (%rdi)
	incq %rsi
	incq %rdi
	decq %rcx
	jmp 1b
2:	movb $0, (%rdi)
	movq %rdx, xf_nul(%rip)
	ret

# xf_error_os(edi = errno): Error "<path>: <why> (os error N)".
xf_error_os:
	pushq %rdi
	call xf_error_begin
	popq %rdi
	call w_oserr
	jmp xf_error_end

# xf_error_io(rdi = a write_all result).
xf_error_io:
	pushq %rdi
	call xf_error_begin
	popq %rdi
	call w_ioerr
	jmp xf_error_end

xf_error_begin:
	movb $TAG_ERROR, %dil
	call f_begin
	call w_str_begin
	leaq xf_path(%rip), %rsi
	movq xf_len(%rip), %rdx
	call w_bytes
	leaq s_colon(%rip), %rsi
	jmp w_cstr

xf_error_end:
	call w_str_end
	jmp f_send

# xf_nul_error: the path holds a NUL byte, as Rust reports it.
xf_nul_error:
	call xf_error_begin
	leaq s_nul_name(%rip), %rsi
	call w_cstr
	jmp xf_error_end

# xf_drop(rdi = " during a put" or " during a get"): log a frame that has
# no business in this session and drop it.
xf_drop:
	pushq %rdi
	call l_begin
	leaq s_dropping(%rip), %rsi
	call w_cstr
	call frame_name
	call w_cstr
	popq %rsi
	call w_cstr
	jmp l_end

xf_close:
	movl xf_fd(%rip), %edi
	SYS SYS_close
	ret

	.globl run_put
# run_put: the current frame is a PutOpen. The file is created or
# truncated with exactly the mode asked for (open(2) applies the umask to
# a file it creates, so fchmod follows), filled from PutData, synced on
# PutClose; Exit(0) then, or Error.
run_put:
	pushq %rbx
	call xf_take_path
	movl (%rsi), %ebx		# mode
	cmpq $0, xf_nul(%rip)
	je 1f
	call xf_nul_error
	jmp 9f
1:	leaq xf_path(%rip), %rdi
	movl $O_WRONLY | O_CREAT | O_TRUNC | O_CLOEXEC, %esi
	movl %ebx, %edx
	SYS SYS_open
	testq %rax, %rax
	jns 2f
	movq %rax, %rdi
	negq %rdi
	call xf_error_os
	jmp 9f
2:	movq %rax, xf_fd(%rip)
	movl %eax, %edi
	movl %ebx, %esi
	SYS SYS_fchmod
	testq %rax, %rax
	jns 3f
	pushq %rax
	call xf_close
	popq %rdi
	negq %rdi
	call xf_error_os
	jmp 9f
3:	call next_blocking
	movq fr_tag(%rip), %rax
	cmpq $TAG_PUT_DATA, %rax
	jne 4f
	movl xf_fd(%rip), %edi
	movq fr_body(%rip), %rsi
	movq fr_len(%rip), %rdx
	call write_all
	testq %rax, %rax
	jz 3b
	pushq %rax
	call xf_close
	popq %rdi
	call xf_error_io
	jmp 9f
4:	cmpq $TAG_PUT_CLOSE, %rax
	jne 6f
	movl xf_fd(%rip), %edi
	SYS SYS_fsync
	pushq %rax
	call xf_close
	popq %rax
	testq %rax, %rax
	jns 5f
	movq %rax, %rdi
	negq %rdi
	call xf_error_os
	jmp 9f
5:	movb $TAG_EXIT, %dil
	call f_begin
	xorl %edi, %edi
	call w_u32
	call f_send
	jmp 9f
6:	cmpq $TAG_STAT, %rax
	jne 7f
	call stat_reply
	jmp 3b
7:	leaq s_put(%rip), %rdi
	call xf_drop
	jmp 3b
9:	popq %rbx
	ret

	.globl run_get
# run_get: the current frame is a Get. The file goes back in CHUNK-byte
# GetData frames, then GetEnd with the byte count; Stat requests that
# arrive meanwhile are answered between chunks.
run_get:
	call xf_take_path
	cmpq $0, xf_nul(%rip)
	je 1f
	jmp xf_nul_error
1:	leaq xf_path(%rip), %rdi
	movl $O_RDONLY | O_CLOEXEC, %esi
	xorl %edx, %edx
	SYS SYS_open
	testq %rax, %rax
	jns 2f
	movq %rax, %rdi
	negq %rdi
	jmp xf_error_os
2:	movq %rax, xf_fd(%rip)
	movq $0, xf_size(%rip)
3:	# frames already here or arriving now
	call next_frame
	testl %eax, %eax
	jnz 4f
	subq $8, %rsp
	movl dev_fd(%rip), %eax
	movl %eax, (%rsp)
	movl $POLLIN, 4(%rsp)
	movq %rsp, %rdi
	movl $1, %esi
	xorl %edx, %edx
	call poll_ms
	movzwl 6(%rsp), %ecx
	addq $8, %rsp
	testq %rax, %rax
	jz 6f
	testl $POLLIN, %ecx
	jz 6f
	call fill
	call next_frame
	testl %eax, %eax
	jz 6f
4:	cmpq $TAG_STAT, fr_tag(%rip)
	jne 5f
	call stat_reply
	jmp 3b
5:	leaq s_get(%rip), %rdi
	call xf_drop
	jmp 3b
6:	# the next chunk, read straight into the frame
	movl xf_fd(%rip), %edi
	leaq tx_buf+5(%rip), %rsi
	movl $CHUNK, %edx
	SYS SYS_read
	testq %rax, %rax
	jz 8f
	jns 7f
	cmpq $-EINTR, %rax
	je 3b
	pushq %rax
	call xf_close
	popq %rdi
	negq %rdi
	jmp xf_error_os
7:	addq %rax, xf_size(%rip)
	pushq %rax
	movb $TAG_GET_DATA, %dil
	call f_begin
	popq %rax
	addq %rax, wp(%rip)
	call f_send
	jmp 3b
8:	call xf_close
	movb $TAG_GET_END, %dil
	call f_begin
	movq xf_size(%rip), %rdi
	call w_u64
	jmp f_send
