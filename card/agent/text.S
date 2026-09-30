# text.S: the agent's byte-level helpers. A write cursor that frames, log
# lines and paths are built through; whole writes; the log; text scanning
# (tokens, lines, trimming) and number parsing with the rules of Rust's
# `str::parse`; UTF-8 validation; reading a whole file as Rust's
# `fs::read_to_string` does. Calling convention is the System V one
# (arguments in rdi, rsi, rdx, rcx, r8, r9; rbx, rbp, r12 to r15 kept).
# See text.md.
	.include "defs.inc"

	.bss
	.p2align 3
	.globl wp
# The write cursor: every w_* routine appends at wp and advances it.
wp:	.skip 8
# Where the string being written by w_str_begin starts (past its length).
str_at:	.skip 8
# wp while a log line or a path is being built elsewhere.
log_saved:	.skip 8
path_saved:	.skip 8
log_buf:	.skip LOGCAP
	.globl path_buf
# A path built with p_begin/p_end, NUL-terminated.
path_buf:	.skip PATHCAP
	.globl root_ptr, root_len
# Prefix of every /proc and /sys path the sampler reads (--root; empty on
# the card).
root_ptr:	.skip 8
root_len:	.skip 8
# The prefixed path read_text and open_dir open.
full_buf:	.skip 2 * PATHCAP

	.section .rodata
s_prefix:	.asciz "phi-agent: "
s_oserr:	.asciz " (os error "
s_zero:	.asciz "failed to write whole buffer"
hexdig:	.ascii "0123456789abcdef"

	.text

# ------------------------------------------------------------ the cursor

	.globl w_byte
# w_byte(dil): append one byte.
w_byte:
	movq wp(%rip), %rax
	movb %dil, (%rax)
	incq %rax
	movq %rax, wp(%rip)
	ret

	.globl w_u16
# w_u16(di), w_u32(edi), w_u64(rdi): little-endian, as the wire wants.
w_u16:
	movq wp(%rip), %rax
	movw %di, (%rax)
	addq $2, %rax
	movq %rax, wp(%rip)
	ret

	.globl w_u32
w_u32:
	movq wp(%rip), %rax
	movl %edi, (%rax)
	addq $4, %rax
	movq %rax, wp(%rip)
	ret

	.globl w_u64
w_u64:
	movq wp(%rip), %rax
	movq %rdi, (%rax)
	addq $8, %rax
	movq %rax, wp(%rip)
	ret

	.globl w_bytes
# w_bytes(rsi = ptr, rdx = len). Clobbers rdi, rsi, rcx.
w_bytes:
	movq wp(%rip), %rdi
	movq %rdx, %rcx
	rep movsb
	movq %rdi, wp(%rip)
	ret

	.globl w_cstr
# w_cstr(rsi): a NUL-terminated string, without its NUL.
w_cstr:
	movq wp(%rip), %rdi
1:	movb (%rsi), %al
	testb %al, %al
	jz 2f
	movb %al, (%rdi)
	incq %rsi
	incq %rdi
	jmp 1b
2:	movq %rdi, wp(%rip)
	ret

	.globl w_dec
# w_dec(rdi): unsigned decimal.
w_dec:
	subq $40, %rsp
	movq %rdi, %rax
	leaq 32(%rsp), %rcx
	movl $10, %r8d
1:	xorl %edx, %edx
	divq %r8
	addb $'0', %dl
	decq %rcx
	movb %dl, (%rcx)
	testq %rax, %rax
	jnz 1b
	movq wp(%rip), %rdi
	leaq 32(%rsp), %rdx
2:	movb (%rcx), %al
	movb %al, (%rdi)
	incq %rcx
	incq %rdi
	cmpq %rdx, %rcx
	jb 2b
	movq %rdi, wp(%rip)
	addq $40, %rsp
	ret

	.globl w_hex
# w_hex(rdi): "0x" and lowercase hex without leading zeros, as Rust's {:#x}.
w_hex:
	subq $40, %rsp
	movq %rdi, %rax
	leaq 32(%rsp), %rcx
	leaq hexdig(%rip), %r8
1:	movq %rax, %rdx
	andl $15, %edx
	movb (%r8,%rdx), %dl
	decq %rcx
	movb %dl, (%rcx)
	shrq $4, %rax
	jnz 1b
	movq wp(%rip), %rdi
	movb $'0', (%rdi)
	movb $'x', 1(%rdi)
	addq $2, %rdi
	leaq 32(%rsp), %rdx
2:	movb (%rcx), %al
	movb %al, (%rdi)
	incq %rcx
	incq %rdi
	cmpq %rdx, %rcx
	jb 2b
	movq %rdi, wp(%rip)
	addq $40, %rsp
	ret

	.globl w_oserr
# w_oserr(edi = errno): "<musl's message> (os error N)", which is how Rust
# displays an io::Error made from an errno.
w_oserr:
	pushq %rbx
	movl %edi, %ebx
	movl %edi, %eax
	cmpq errno_count(%rip), %rax
	jb 1f
	xorl %eax, %eax
1:	leaq errno_index(%rip), %rcx
	movzwl (%rcx,%rax,2), %eax
	leaq errno_text(%rip), %rsi
	addq %rax, %rsi
	call w_cstr
	leaq s_oserr(%rip), %rsi
	call w_cstr
	movl %ebx, %edi
	call w_dec
	movb $')', %dil
	call w_byte
	popq %rbx
	ret

	.globl w_ioerr
# w_ioerr(rdi = a write_all result): 1 is Rust's WriteZero, otherwise
# -errno.
w_ioerr:
	cmpq $1, %rdi
	jne 1f
	leaq s_zero(%rip), %rsi
	jmp w_cstr
1:	negq %rdi
	jmp w_oserr

	.globl w_str_begin
# w_str_begin / w_str_end: a wire string (u16 length, bytes) written in
# pieces between the two. The end clips it to MAX_STRING bytes at a
# character boundary, as phi-rpc's `clip` does.
w_str_begin:
	movq wp(%rip), %rax
	addq $2, %rax
	movq %rax, str_at(%rip)
	movq %rax, wp(%rip)
	ret

	.globl w_str_end
w_str_end:
	movq str_at(%rip), %rcx
	movq wp(%rip), %rax
	subq %rcx, %rax
	cmpq $MAX_STRING, %rax
	jbe 2f
	movl $MAX_STRING, %eax
1:	movb (%rcx,%rax), %dl
	andb $0xc0, %dl
	cmpb $0x80, %dl
	jne 3f
	decq %rax
	jmp 1b
3:	leaq (%rcx,%rax), %rdx
	movq %rdx, wp(%rip)
2:	movw %ax, -2(%rcx)
	ret

	.globl w_str
# w_str(rsi = ptr, rdx = len): a whole wire string.
w_str:
	pushq %rsi
	pushq %rdx
	call w_str_begin
	popq %rdx
	popq %rsi
	call w_bytes
	jmp w_str_end

# ------------------------------------------------------------ writes, log

	.globl write_all
# write_all(edi = fd, rsi = buf, rdx = len) -> rax: 0, -errno, or 1 when
# write(2) returned 0 (Rust's WriteZero). EINTR is retried.
write_all:
	pushq %rbx
	pushq %r12
	pushq %r13
	movl %edi, %ebx
	movq %rsi, %r12
	movq %rdx, %r13
1:	testq %r13, %r13
	jz 3f
	movl %ebx, %edi
	movq %r12, %rsi
	movq %r13, %rdx
	SYS SYS_write
	testq %rax, %rax
	js 2f
	jz 4f
	addq %rax, %r12
	subq %rax, %r13
	jmp 1b
2:	cmpq $-EINTR, %rax
	je 1b
	jmp 5f
3:	xorl %eax, %eax
	jmp 5f
4:	movl $1, %eax
5:	popq %r13
	popq %r12
	popq %rbx
	ret

	.globl l_begin
# l_begin: start a log line "phi-agent: ..." (l_begin_bare: no prefix);
# l_end writes it to standard error with its newline. The cursor is
# saved and restored, so a line may be logged in the middle of a frame.
l_begin:
	call l_begin_bare
	leaq s_prefix(%rip), %rsi
	jmp w_cstr

	.globl l_begin_bare
l_begin_bare:
	movq wp(%rip), %rax
	movq %rax, log_saved(%rip)
	leaq log_buf(%rip), %rax
	movq %rax, wp(%rip)
	ret

	.globl l_end
l_end:
	movb $'\n', %dil
	call w_byte
	leaq log_buf(%rip), %rsi
	movq wp(%rip), %rdx
	subq %rsi, %rdx
	movl $2, %edi
	call write_all
	movq log_saved(%rip), %rax
	movq %rax, wp(%rip)
	ret

	.globl p_begin
# p_begin / p_end: build a NUL-terminated path in path_buf through the
# cursor; p_end returns it in rax.
p_begin:
	movq wp(%rip), %rax
	movq %rax, path_saved(%rip)
	leaq path_buf(%rip), %rax
	movq %rax, wp(%rip)
	ret

	.globl p_end
p_end:
	xorl %edi, %edi
	call w_byte
	movq path_saved(%rip), %rax
	movq %rax, wp(%rip)
	leaq path_buf(%rip), %rax
	ret

# ------------------------------------------------------------ scanning

	.globl next_token
# next_token(rsi = cursor, rdi = end) -> rax = start, rdx = length (0 and
# rax 0 when no token is left), rsi = past the token. Tokens are split on
# ASCII whitespace (space, \t \n \v \f \r), which is all /proc and sysfs
# use.
next_token:
1:	cmpq %rdi, %rsi
	jae 9f
	movzbl (%rsi), %ecx
	cmpb $' ', %cl
	je 2f
	subl $9, %ecx
	cmpl $4, %ecx
	ja 3f
2:	incq %rsi
	jmp 1b
3:	movq %rsi, %rax
4:	incq %rsi
	cmpq %rdi, %rsi
	jae 5f
	movzbl (%rsi), %ecx
	cmpb $' ', %cl
	je 5f
	subl $9, %ecx
	cmpl $4, %ecx
	ja 4b
5:	movq %rsi, %rdx
	subq %rax, %rdx
	ret
9:	xorl %eax, %eax
	xorl %edx, %edx
	ret

	.globl next_line
# next_line(rsi = cursor, rdi = end) -> rax = start (0 when no line is
# left), rdx = length, rsi = past its newline. As Rust's `lines`: the last
# line needs no newline, and "\r\n" ends a line like "\n".
next_line:
	cmpq %rdi, %rsi
	jae 9f
	movq %rsi, %rax
1:	cmpq %rdi, %rsi
	jae 3f
	cmpb $'\n', (%rsi)
	je 2f
	incq %rsi
	jmp 1b
2:	movq %rsi, %rdx
	subq %rax, %rdx
	incq %rsi
	testq %rdx, %rdx
	jz 4f
	cmpb $'\r', -2(%rsi)
	jne 4f
	decq %rdx
4:	ret
3:	movq %rsi, %rdx
	subq %rax, %rdx
	ret
9:	xorl %eax, %eax
	xorl %edx, %edx
	ret

	.globl trim
# trim(rsi = ptr, rdx = len) -> rsi, rdx without leading and trailing
# ASCII whitespace.
trim:
1:	testq %rdx, %rdx
	jz 5f
	movzbl (%rsi), %ecx
	cmpb $' ', %cl
	je 2f
	subl $9, %ecx
	cmpl $4, %ecx
	ja 3f
2:	incq %rsi
	decq %rdx
	jmp 1b
3:	testq %rdx, %rdx
	jz 5f
	movzbl -1(%rsi,%rdx), %ecx
	cmpb $' ', %cl
	je 4f
	subl $9, %ecx
	cmpl $4, %ecx
	ja 5f
4:	decq %rdx
	jmp 3b
5:	ret

	.globl eq_cstr
# eq_cstr(rsi = ptr, rdx = len, rdi = NUL-terminated) -> eax 1 when the
# bytes equal the string.
eq_cstr:
1:	movb (%rdi), %al
	testq %rdx, %rdx
	jz 2f
	testb %al, %al
	jz 3f
	cmpb (%rsi), %al
	jne 3f
	incq %rsi
	incq %rdi
	decq %rdx
	jmp 1b
2:	testb %al, %al
	jnz 3f
	movl $1, %eax
	ret
3:	xorl %eax, %eax
	ret

	.globl starts_cstr
# starts_cstr(rsi = ptr, rdx = len, rdi = NUL-terminated) -> eax 1 when the
# bytes start with the string.
starts_cstr:
1:	movb (%rdi), %al
	testb %al, %al
	jz 2f
	testq %rdx, %rdx
	jz 3f
	cmpb (%rsi), %al
	jne 3f
	incq %rsi
	incq %rdi
	decq %rdx
	jmp 1b
2:	movl $1, %eax
	ret
3:	xorl %eax, %eax
	ret

# ------------------------------------------------------------ numbers

# digits(rsi = ptr, r9 = len > 0) -> rax = value, edx = 1, or edx = 0 on a
# non-digit or on overflow of 64 bits. Clobbers rcx, r8, r9, rsi.
digits:
	xorl %eax, %eax
1:	movzbl (%rsi), %ecx
	subl $'0', %ecx
	cmpl $9, %ecx
	ja 8f
	movl $10, %r8d
	mulq %r8
	jc 8f
	addq %rcx, %rax
	jc 8f
	incq %rsi
	decq %r9
	jnz 1b
	movl $1, %edx
	ret
8:	xorl %eax, %eax
	xorl %edx, %edx
	ret

	.globl parse_u64
# parse_u64(rsi = ptr, rdx = len) -> rax = value, edx = 1 (ok) or 0. As
# Rust's u64::from_str: an optional '+', then at least one digit, no
# overflow. Clobbers rcx, r8, r9, rsi.
parse_u64:
	movq %rdx, %r9
	testq %r9, %r9
	jz 8f
	cmpb $'+', (%rsi)
	jne digits
	incq %rsi
	decq %r9
	jnz digits
8:	xorl %eax, %eax
	xorl %edx, %edx
	ret

	.globl parse_u32
# parse_u32 / parse_u16: parse_u64 with the type's range.
parse_u32:
	call parse_u64
	movl $0xffffffff, %ecx
	cmpq %rcx, %rax
	ja 8f
	ret
8:	xorl %eax, %eax
	xorl %edx, %edx
	ret

	.globl parse_u16
parse_u16:
	call parse_u64
	cmpq $0xffff, %rax
	ja 8f
	ret
8:	xorl %eax, %eax
	xorl %edx, %edx
	ret

	.globl parse_i64
# parse_i64(rsi, rdx) -> rax, edx: an optional sign, then digits, within
# the i64 range, as Rust's i64::from_str.
parse_i64:
	movq %rdx, %r9
	xorl %r10d, %r10d
	testq %r9, %r9
	jz 8f
	movb (%rsi), %al
	cmpb $'-', %al
	jne 1f
	movl $1, %r10d
	jmp 2f
1:	cmpb $'+', %al
	jne 3f
2:	incq %rsi
	decq %r9
	jz 8f
3:	pushq %r10
	call digits
	popq %r10
	testl %edx, %edx
	jz 8f
	movq $0x8000000000000000, %rcx
	testl %r10d, %r10d
	jz 4f
	cmpq %rcx, %rax
	ja 8f
	negq %rax
	ret
4:	cmpq %rcx, %rax
	jae 8f
	ret
8:	xorl %eax, %eax
	xorl %edx, %edx
	ret

	.globl hundredths
# hundredths(rsi = ptr, rdx = len) -> rax: "1.33" as 133, "139.78" as
# 13978, "7" as 700; extra decimals dropped, anything unparsable 0. The
# rules of the Rust agent's `hundredths`, which parsed the whole part and
# at most two decimals separately.
hundredths:
	pushq %rbx
	pushq %r12
	pushq %r13
	call trim
	movq %rsi, %r12
	movq %rdx, %r13
	# the whole part: up to the first '.'
	xorl %ecx, %ecx
1:	cmpq %r13, %rcx
	jae 2f
	cmpb $'.', (%r12,%rcx)
	je 2f
	incq %rcx
	jmp 1b
2:	movq %rcx, %rbx
	movq %r12, %rsi
	movq %rcx, %rdx
	call parse_u64
	testl %edx, %edx
	jnz 3f
	xorl %eax, %eax
3:	imulq $100, %rax, %rax
	pushq %rax
	# the decimals: what follows the '.', if there is one
	leaq 1(%rbx), %rcx
	cmpq %r13, %rcx
	ja 7f
	leaq (%r12,%rcx), %rsi
	movq %r13, %rdx
	subq %rcx, %rdx
	jz 7f
	cmpq $1, %rdx
	jne 4f
	call parse_u64
	testl %edx, %edx
	jz 7f
	imulq $10, %rax, %rax
	jmp 8f
4:	# two or more: the first two bytes, if they end on a character boundary
	cmpq $2, %rdx
	je 5f
	movb 2(%rsi), %cl
	andb $0xc0, %cl
	cmpb $0x80, %cl
	je 7f
5:	movl $2, %edx
	call parse_u64
	testl %edx, %edx
	jnz 8f
7:	xorl %eax, %eax
8:	popq %rcx
	addq %rcx, %rax
	popq %r13
	popq %r12
	popq %rbx
	ret

# ------------------------------------------------------------ UTF-8

	.globl utf8_ok
# utf8_ok(rsi = ptr, rdx = len) -> eax 1 when the bytes are valid UTF-8 (no
# overlong forms, no surrogates, nothing past U+10FFFF), as Rust's
# `str::from_utf8` requires.
utf8_ok:
	addq %rsi, %rdx
1:	cmpq %rdx, %rsi
	jae 7f
	movzbl (%rsi), %eax
	cmpb $0x80, %al
	jae 2f
	incq %rsi
	jmp 1b
2:	movb $0x80, %r8b
	movb $0xbf, %r9b
	cmpb $0xc2, %al
	jb 8f
	cmpb $0xdf, %al
	ja 3f
	movl $1, %ecx
	jmp 5f
3:	cmpb $0xef, %al
	ja 4f
	movl $2, %ecx
	cmpb $0xe0, %al
	jne 31f
	movb $0xa0, %r8b
31:	cmpb $0xed, %al
	jne 5f
	movb $0x9f, %r9b
	jmp 5f
4:	cmpb $0xf4, %al
	ja 8f
	movl $3, %ecx
	cmpb $0xf0, %al
	jne 41f
	movb $0x90, %r8b
41:	cmpb $0xf4, %al
	jne 5f
	movb $0x8f, %r9b
5:	leaq 1(%rsi,%rcx), %r10
	cmpq %rdx, %r10
	ja 8f
	movb 1(%rsi), %al
	cmpb %r8b, %al
	jb 8f
	cmpb %r9b, %al
	ja 8f
	leaq 2(%rsi), %r11
	decq %rcx
6:	testq %rcx, %rcx
	jz 61f
	movb (%r11), %al
	andb $0xc0, %al
	cmpb $0x80, %al
	jne 8f
	incq %r11
	decq %rcx
	jmp 6b
61:	movq %r10, %rsi
	jmp 1b
7:	movl $1, %eax
	ret
8:	xorl %eax, %eax
	ret

# ------------------------------------------------------------ files

	.globl full_path
# full_path(rdi = NUL-terminated absolute path) -> rax = the same path
# under root_ptr, in full_buf.
full_path:
	leaq full_buf(%rip), %rax
	movq %rax, %r8
	movq root_ptr(%rip), %rsi
	movq root_len(%rip), %rcx
	testq %rcx, %rcx
	jz 2f
1:	movb (%rsi), %dl
	movb %dl, (%r8)
	incq %rsi
	incq %r8
	decq %rcx
	jnz 1b
2:	movb (%rdi), %dl
	movb %dl, (%r8)
	incq %rdi
	incq %r8
	testb %dl, %dl
	jnz 2b
	ret

	.globl read_text
# read_text(rdi = path, rsi = buf, rdx = cap) -> rax = length. The file
# under root, read to its end (or to cap); 0 when it cannot be opened or
# read or is not UTF-8. That is Rust's `fs::read_to_string(path)
# .unwrap_or_default()`, the Rust agent's `read`.
read_text:
	pushq %rbx
	pushq %r12
	pushq %r13
	pushq %r14
	subq $8, %rsp
	movq %rsi, %r12
	movq %rdx, %r13
	call full_path
	movq %rax, %rdi
	movl $O_RDONLY | O_CLOEXEC, %esi
	xorl %edx, %edx
	SYS SYS_open
	testq %rax, %rax
	js 8f
	movl %eax, %ebx
	xorl %r14d, %r14d
1:	movq %r13, %rdx
	subq %r14, %rdx
	jz 3f
	movl %ebx, %edi
	leaq (%r12,%r14), %rsi
	SYS SYS_read
	testq %rax, %rax
	jz 3f
	js 2f
	addq %rax, %r14
	jmp 1b
2:	cmpq $-EINTR, %rax
	je 1b
	movl %ebx, %edi
	SYS SYS_close
	jmp 8f
3:	movl %ebx, %edi
	SYS SYS_close
	movq %r12, %rsi
	movq %r14, %rdx
	call utf8_ok
	testl %eax, %eax
	jz 8f
	movq %r14, %rax
	jmp 9f
8:	xorl %eax, %eax
9:	addq $8, %rsp
	popq %r14
	popq %r13
	popq %r12
	popq %rbx
	ret

	.globl open_dir
# open_dir(rdi = path) -> rax = a directory descriptor under root, or
# -errno.
open_dir:
	call full_path
	movq %rax, %rdi
	movl $O_RDONLY | O_DIRECTORY | O_CLOEXEC, %esi
	xorl %edx, %edx
	SYS SYS_open
	ret

# ------------------------------------------------------------ time, poll

	.globl now_ms
# now_ms() -> rax: CLOCK_MONOTONIC in milliseconds.
now_ms:
	subq $24, %rsp
	movl $CLOCK_MONOTONIC, %edi
	movq %rsp, %rsi
	SYS SYS_clock_gettime
	movq 8(%rsp), %rax
	xorl %edx, %edx
	movl $1000000, %ecx
	divq %rcx
	imulq $1000, (%rsp), %rcx
	addq %rcx, %rax
	addq $24, %rsp
	ret

	.globl poll_ms
# poll_ms(rdi = pollfds, esi = count, edx = ms) -> rax = ready count;
# EINTR and failures count as none, as the Rust agent's `poll`.
poll_ms:
	SYS SYS_poll
	testq %rax, %rax
	jns 1f
	xorl %eax, %eax
1:	ret
