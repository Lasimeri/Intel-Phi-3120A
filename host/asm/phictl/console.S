# console.S: the daemon's loop and the console (the console_loop of
# phictl's main.rs). One thread serves everything: each pass gives every
# service a turn (the console, the control socket, the block services,
# the forwarder, the bridge); a pass that moved nothing sleeps briefly.
# The Rust daemon ran each service on its own thread; see console.md for
# what that changes.
	.include "defs.inc"

	.set SPIN_AFTER_ACTIVITY_US, 20000
	.set IDLE_SLEEP_US, 200
	.set RETRY_MS, 500

	.bss
	.p2align 3
	.globl init_reached, region_ok
# The card's kernel has reached init (POST K7 seen, or the region flag).
init_reached:	.skip 8
# The region header is valid; the console's rings are attached.
region_ok:	.skip 8
con_h2c:	.skip 8
con_h2c_size:	.skip 8
con_c2h:	.skip 8
con_c2h_size:	.skip 8
next_retry_ms:	.skip 8
last_post:	.skip 8
last_flags:	.skip 8
last_mark:	.skip 8
stdin_open:	.skip 8
last_active_us:	.skip 8
	.globl start_ms
start_ms:	.skip 8
con_buf:	.skip 4096
in_buf:	.skip 4096
in_len:	.skip 8
in_head:	.skip 8
	.p2align 3
pfd:	.skip 8

	.text

	.globl cmd_console
# `phictl console`: the daemon without a boot (the card must not be held
# by another process; opening it resets it).
cmd_console:
	movq $RING_REGION_BASE, o_ring_base(%rip)
	movq $RING_REGION_BYTES, o_ring_size(%rip)
1:	call opt_next
	testq %rax, %rax
	jz 9f
	leaq s_c_ring_base(%rip), %rdi
	call arg_is
	testl %eax, %eax
	jz 2f
	call opt_num
	movq %rax, o_ring_base(%rip)
	jmp 1b
2:	movq %r11, %rax
	leaq s_c_ring_size(%rip), %rdi
	call arg_is
	testl %eax, %eax
	jz 3f
	call opt_num
	movq %rax, o_ring_size(%rip)
	jmp 1b
3:	movq %r11, %rax
	leaq s_c_watch(%rip), %rdi
	call arg_is
	testl %eax, %eax
	jz 4f
	call opt_num
	movq %rax, o_watch(%rip)
	movq $1, o_has_watch(%rip)
	jmp 1b
4:	movq %r11, %rax
	leaq s_c_net(%rip), %rdi
	call arg_is
	testl %eax, %eax
	jz 5f
	call opt_value
	movq %rax, o_net(%rip)
	jmp 1b
5:	movq %r11, %rax
	leaq s_c_net_addr(%rip), %rdi
	call arg_is
	testl %eax, %eax
	jz 6f
	call opt_value
	movq %rax, o_net_addr(%rip)
	jmp 1b
6:	movq %r11, %rax
	leaq s_c_forward(%rip), %rdi
	call arg_is
	testl %eax, %eax
	jz usage
	call opt_value
	movq %rax, o_forward(%rip)
	jmp 1b
9:	call open_card
	jmp daemon
STR s_c_ring_base, "--ring-base"
STR s_c_ring_size, "--ring-size"
STR s_c_watch, "--watch"
STR s_c_net, "--net"
STR s_c_net_addr, "--net-addr"
STR s_c_forward, "--forward"

	.globl daemon
# daemon: the services until the process ends (never returns).
daemon:
	movq o_ring_base(%rip), %rax
	movq %rax, region_base(%rip)
	movq o_ring_size(%rip), %rcx
	movq %rcx, region_size(%rip)
	addq %rcx, %rax
	movq $GDDR_BYTES, %rcx
	cmpq %rcx, %rax
	ja 90f
	movq %rax, %rdx
	movq region_base(%rip), %rdi
	movq region_size(%rip), %rdx
	call aper_check
	call now_ms
	movq %rax, start_ms(%rip)
	movq %rax, %rcx
	addq $RETRY_MS, %rcx
	movq %rcx, next_retry_ms(%rip)
	movq $1, stdin_open(%rip)
	call postcode
	movq %rax, last_post(%rip)
	call console_attach
	testl %eax, %eax
	jnz 1f
	call l_begin
	call l_tag
	leaq s_noheader1(%rip), %rsi
	call w_cstr
	movq region_base(%rip), %rdi
	call w_hex
	leaq s_noheader2(%rip), %rsi
	call w_cstr
	call l_end
1:	call services_open
	call l_begin
	call l_tag
	leaq s_tailing1(%rip), %rsi
	call w_cstr
	movq region_base(%rip), %rdi
	call w_hex
	leaq s_tailing2(%rip), %rsi
	call w_cstr
	call l_end
	call now_us
	movq %rax, last_active_us(%rip)
2:	xorl %ebx, %ebx		# work done this pass
	call console_step
	orl %eax, %ebx
	call services_step
	orl %eax, %ebx
	testl %ebx, %ebx
	jz 3f
	call now_us
	movq %rax, last_active_us(%rip)
	jmp 2b
3:	call o_flush
	call now_us
	subq last_active_us(%rip), %rax
	cmpq $SPIN_AFTER_ACTIVITY_US, %rax
	jb 2b
	movl $IDLE_SLEEP_US, %edi
	call sleep_us
	jmp 2b
90:	leaq s_e_beyond(%rip), %rsi
	jmp fail
STR s_e_beyond, "ring region beyond card memory"
STR s_noheader1, "ring region at "
STR s_noheader2, " has no valid header; tailing POST codes, retrying the ring every 500 ms"
STR s_tailing1, "console: tailing c2h ring at "
STR s_tailing2, "; Ctrl-C to stop"

# console_attach -> eax 1 when the region opened and the console channel
# was found; sets region_ok and the console's rings.
console_attach:
	call region_open
	testl %eax, %eax
	jz 9f
	movl $KIND_CONSOLE, %edi
	call endpoints_of
	testl %eax, %eax
	jz 9f
	movq %r8, con_h2c(%rip)
	movq %r9, con_h2c_size(%rip)
	movq %r10, con_c2h(%rip)
	movq %r11, con_c2h_size(%rip)
	movq $1, region_ok(%rip)
	movl $1, %eax
	ret
9:	xorl %eax, %eax
	ret

# console_step -> eax 1 when anything moved: the console ring to
# standard output, POST code changes, the watched word, the card's
# flags, standard input to the card.
console_step:
	pushq %rbx
	xorl %ebx, %ebx
	cmpq $0, region_ok(%rip)
	jne 2f
	call now_ms
	cmpq next_retry_ms(%rip), %rax
	jb 2f
	addq $RETRY_MS, %rax
	movq %rax, next_retry_ms(%rip)
	call console_attach
	testl %eax, %eax
	jz 2f
	call l_begin
	call l_tag
	call now_ms
	subq start_ms(%rip), %rax
	movq %rax, %rdi
	call l_elapsed
	leaq s_header_valid(%rip), %rsi
	call w_cstr
	call l_end
2:	cmpq $0, region_ok(%rip)
	je 4f
	movq con_c2h(%rip), %rdi
	movl con_c2h_size(%rip), %esi
	leaq con_buf(%rip), %rdx
	movl $4096, %ecx
	call ring_pop
	testq %rax, %rax
	jz 4f
	movl $1, %ebx
	movl $1, %edi
	leaq con_buf(%rip), %rsi
	movq %rax, %rdx
	call write_all
4:	call postcode
	cmpq last_post(%rip), %rax
	je 5f
	movq %rax, last_post(%rip)
	movl %eax, %edi
	call log_post_elapsed
5:	cmpq $0, o_has_watch(%rip)
	je 6f
	movq o_watch(%rip), %rdi
	movq %rdi, %rax
	addq $4, %rax
	cmpq aper_len(%rip), %rax
	ja 6f
	call aper_read32
	cmpq last_mark(%rip), %rax
	je 6f
	movq %rax, last_mark(%rip)
	movl %eax, %edi
	call log_mark
6:	cmpq $0, region_ok(%rip)
	je 7f
	call card_boot_flags
	cmpq last_flags(%rip), %rax
	je 7f
	movq %rax, last_flags(%rip)
	pushq %rax
	call l_begin
	call l_tag
	call now_ms
	subq start_ms(%rip), %rax
	movq %rax, %rdi
	call l_elapsed
	leaq s_card_flags(%rip), %rsi
	call w_cstr
	popq %rdi
	call w_hex
	call l_end
7:	# standard input to the card, whatever the console ring takes
	cmpq $0, stdin_open(%rip)
	je 9f
	movq in_len(%rip), %rax
	cmpq in_head(%rip), %rax
	jne 8f
	movq $0, in_len(%rip)
	movq $0, in_head(%rip)
	movl $0, pfd(%rip)
	movw $POLLIN, pfd+4(%rip)
	leaq pfd(%rip), %rdi
	movl $1, %esi
	xorl %edx, %edx
	call poll_ms
	testq %rax, %rax
	jz 9f
	xorl %edi, %edi
	leaq in_buf(%rip), %rsi
	movl $4096, %edx
	SYS SYS_read
	testq %rax, %rax
	jle 71f
	movq %rax, in_len(%rip)
	jmp 8f
71:	movq $0, stdin_open(%rip)
	jmp 9f
8:	cmpq $0, region_ok(%rip)
	je 9f
	movq con_h2c(%rip), %rdi
	movl con_h2c_size(%rip), %esi
	leaq in_buf(%rip), %rdx
	addq in_head(%rip), %rdx
	movq in_len(%rip), %rcx
	subq in_head(%rip), %rcx
	call ring_push
	addq %rax, in_head(%rip)
	testq %rax, %rax
	jz 9f
	movl $1, %ebx
9:	movl %ebx, %eax
	popq %rbx
	ret
STR s_header_valid, "ring region header valid"
STR s_card_flags, "card flags "

# log_post_elapsed(edi = raw): "[phictl] +   1.234s postcode "xx" ...".
log_post_elapsed:
	pushq %rdi
	call l_begin
	call l_tag
	call now_ms
	subq start_ms(%rip), %rax
	movq %rax, %rdi
	call l_elapsed
	leaq s_postcode_q(%rip), %rsi
	call w_cstr
	movq (%rsp), %rdi
	call post_text
	call w_bytes
	leaq s_quote_sp(%rip), %rsi
	call w_cstr
	popq %rdi
	call post_describe
	call w_cstr
	jmp l_end
STR s_postcode_q, "postcode \""
STR s_quote_sp, "\" "

# log_mark(edi = raw): the watched word as a POST-style code.
log_mark:
	pushq %rdi
	call l_begin
	call l_tag
	call now_ms
	subq start_ms(%rip), %rax
	movq %rax, %rdi
	call l_elapsed
	leaq s_mark_q(%rip), %rsi
	call w_cstr
	movq (%rsp), %rdi
	call post_text
	call w_bytes
	leaq s_quote_sp(%rip), %rsi
	call w_cstr
	popq %rdi
	call post_describe
	call w_cstr
	jmp l_end
STR s_mark_q, "mark \""

	.globl wait_init_seen
# wait_init_seen -> eax 1 once the card has reached init: POST K7, or the
# region's flag. KH and KP (halted, panic) count as never.
wait_init_seen:
	cmpq $0, init_reached(%rip)
	jne 1f
	call postcode
	andl $0xffff, %eax
	cmpl $0x374b, %eax		# "K7"
	je 2f
	cmpq $0, region_ok(%rip)
	je 3f
	call card_boot_flags
	testq $CARD_FLAG_INIT_REACHED, %rax
	jz 3f
2:	movq $1, init_reached(%rip)
1:	movl $1, %eax
	ret
3:	xorl %eax, %eax
	ret
