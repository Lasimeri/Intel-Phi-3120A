# term.S: the terminal (host/crates/phitop's term.rs): unbuffered keys,
# the screen size, the alternate screen, and restoring all of it on exit
# or on a signal. See term.md.
	.include "defs.inc"

	.bss
	.p2align 3
	.globl stop_flag
stop_flag:	.skip 8
orig_termios:	.skip 64
raw_termios:	.skip 64
winsize:	.skip 8
term_open_flag:	.skip 8
key_buf:	.skip 16
	.p2align 3
sa:	.skip 32
kpfd:	.skip 8

	.section .rodata
s_enter:	.ascii "\033[?1049h\033[?25l\033[H\033[2J"
	.set s_enter_len, . - s_enter
s_leave:	.ascii "\033[?25h\033[?1049l"
	.set s_leave_len, . - s_leave
	.text

# on_signal: SIGINT or SIGTERM sets the flag; the loop exits at its next
# check.
on_signal:
	movq $1, stop_flag(%rip)
	ret

# The restorer the kernel returns through after a handler (rt_sigreturn).
sig_restorer:
	movl $15, %eax
	syscall

	.globl term_open
# term_open -> eax 1 when standard input is a terminal: no echo, no line
# buffering, the alternate screen, the cursor hidden; 0 otherwise.
term_open:
	xorl %edi, %edi
	movl $TCGETS, %esi
	leaq orig_termios(%rip), %rdx
	SYS SYS_ioctl
	testq %rax, %rax
	js 9f
	leaq orig_termios(%rip), %rsi
	leaq raw_termios(%rip), %rdi
	movl $TERMIOS_SIZE, %ecx
	rep movsb
	andl $~(ICANON | ECHO), raw_termios+12(%rip)
	movb $0, raw_termios+17+VMIN(%rip)
	movb $0, raw_termios+17+VTIME(%rip)
	xorl %edi, %edi
	movl $TCSETS, %esi
	leaq raw_termios(%rip), %rdx
	SYS SYS_ioctl
	# the handlers, with a restorer as rt_sigaction on x86-64 requires
	leaq on_signal(%rip), %rax
	movq %rax, sa(%rip)
	movq $SA_RESTORER, sa+8(%rip)
	leaq sig_restorer(%rip), %rax
	movq %rax, sa+16(%rip)
	movq $0, sa+24(%rip)
	movl $SIGINT, %edi
	leaq sa(%rip), %rsi
	xorl %edx, %edx
	movl $8, %r10d
	SYS SYS_rt_sigaction
	movl $SIGTERM, %edi
	leaq sa(%rip), %rsi
	xorl %edx, %edx
	movl $8, %r10d
	SYS SYS_rt_sigaction
	movl $1, %edi
	leaq s_enter(%rip), %rsi
	movl $s_enter_len, %edx
	call write_all
	movq $1, term_open_flag(%rip)
	movl $1, %eax
	ret
9:	xorl %eax, %eax
	ret

	.globl term_close
# term_close: the cursor back, the main screen, the original attributes.
term_close:
	cmpq $0, term_open_flag(%rip)
	je 1f
	movl $1, %edi
	leaq s_leave(%rip), %rsi
	movl $s_leave_len, %edx
	call write_all
	xorl %edi, %edi
	movl $TCSETS, %esi
	leaq orig_termios(%rip), %rdx
	SYS SYS_ioctl
	movq $0, term_open_flag(%rip)
1:	ret

	.globl term_size
# term_size -> eax = columns, edx = rows; 80 by 24 when unknown.
term_size:
	movl $1, %edi
	movl $TIOCGWINSZ, %esi
	leaq winsize(%rip), %rdx
	SYS SYS_ioctl
	testq %rax, %rax
	js 1f
	movzwl winsize+2(%rip), %eax
	movzwl winsize(%rip), %edx
	testl %eax, %eax
	jz 1f
	testl %edx, %edx
	jz 1f
	ret
1:	movl $80, %eax
	movl $24, %edx
	ret

	.globl term_key
# term_key(edi = ms) -> eax = the key, 0 for an escape sequence, -1 for
# none within ms.
term_key:
	movl %edi, %edx
	movl $0, kpfd(%rip)
	movw $POLLIN, kpfd+4(%rip)
	leaq kpfd(%rip), %rdi
	movl $1, %esi
	call poll_ms
	testq %rax, %rax
	jz 9f
	xorl %edi, %edi
	leaq key_buf(%rip), %rsi
	movl $16, %edx
	SYS SYS_read
	testq %rax, %rax
	jle 9f
	movzbl key_buf(%rip), %eax
	cmpl $0x1b, %eax
	jne 1f
	xorl %eax, %eax
1:	ret
9:	movl $-1, %eax
	ret
