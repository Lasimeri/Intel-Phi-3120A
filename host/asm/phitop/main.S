# main.S: phitop, a resource viewer for the cards in the spirit of
# glances, run on the host (host/crates/phitop's main.rs). Every interval
# it asks each card's daemon for a Stat sample (answered by the card's
# agent from /proc and sysfs) and for the daemon's PCIe byte counters
# (Traffic), then draws them. One card named (-c N, PHI_CARD, --socket
# or PHICTL_SOCKET) fills the screen; none named, every card in
# `phictl cards` gets its own block. See main.md.
	.include "defs.inc"

	.set STAT_TIMEOUT_MS, 5000
	.set TRAFFIC_TIMEOUT_MS, 2000

	.bss
	.p2align 3
	.globl argc, argv, envp
argc:	.skip 8
argv:	.skip 8
envp:	.skip 8
a_card:	.skip 8
a_has_card:	.skip 8
a_batch:	.skip 8
a_has_batch:	.skip 8
a_socket:	.skip 8
nslots:	.skip 8
focus:	.skip 8
have_focus:	.skip 8
req_fd:	.skip 8
req_dec:	.skip 64
next_ms:	.skip 8
	.p2align 6
req_buf:	.skip DECCAP
slots:	.skip MAX_SLOTS * SL_SIZE
sockaddr:	.skip 112
pfd1:	.skip 8

	.text
	.globl _start
_start:
	movq (%rsp), %rax
	movq %rax, argc(%rip)
	leaq 8(%rsp), %rax
	movq %rax, argv(%rip)
	movq argc(%rip), %rcx
	leaq 8(%rax,%rcx,8), %rax
	movq %rax, envp(%rip)
	leaq req_buf(%rip), %rax
	movq %rax, wp(%rip)
	movq $1000, opt_interval_ms(%rip)
	# options
	movl $1, %ebx
1:	cmpq argc(%rip), %rbx
	jae 20f
	movq argv(%rip), %rax
	movq (%rax,%rbx,8), %r12
	movq %r12, %rsi
	leaq s_o_c(%rip), %rdi
	call eq_cc
	testl %eax, %eax
	jnz 2f
	movq %r12, %rsi
	leaq s_o_card(%rip), %rdi
	call eq_cc
	testl %eax, %eax
	jz 3f
2:	incq %rbx
	call arg_at
	movq %rax, %rsi
	call cstr_len
	call parse_u64
	testl %edx, %edx
	jz usage
	cmpq $MAX_CARDS, %rax
	jae usage
	movq %rax, a_card(%rip)
	movq $1, a_has_card(%rip)
	jmp 19f
3:	movq %r12, %rsi
	leaq s_o_i(%rip), %rdi
	call eq_cc
	testl %eax, %eax
	jnz 4f
	movq %r12, %rsi
	leaq s_o_interval(%rip), %rdi
	call eq_cc
	testl %eax, %eax
	jz 5f
4:	incq %rbx
	call arg_at
	movq %rax, %rsi
	call cstr_len
	call parse_seconds_ms
	testl %edx, %edx
	jz usage
	movq %rax, opt_interval_ms(%rip)
	jmp 19f
5:	movq %r12, %rsi
	leaq s_o_b(%rip), %rdi
	call eq_cc
	testl %eax, %eax
	jnz 6f
	movq %r12, %rsi
	leaq s_o_batch(%rip), %rdi
	call eq_cc
	testl %eax, %eax
	jz 7f
6:	incq %rbx
	call arg_at
	movq %rax, %rsi
	call cstr_len
	call parse_u64
	testl %edx, %edx
	jz usage
	movq %rax, a_batch(%rip)
	movq $1, a_has_batch(%rip)
	jmp 19f
7:	movq %r12, %rsi
	leaq s_o_socket(%rip), %rdi
	call eq_cc
	testl %eax, %eax
	jz 8f
	incq %rbx
	call arg_at
	movq %rax, a_socket(%rip)
	jmp 19f
8:	movq %r12, %rsi
	leaq s_o_k(%rip), %rdi
	call eq_cc
	testl %eax, %eax
	jnz 9f
	movq %r12, %rsi
	leaq s_o_kthreads(%rip), %rdi
	call eq_cc
	testl %eax, %eax
	jz usage
9:	movq $1, opt_show_kthreads(%rip)
19:	incq %rbx
	jmp 1b
20:	# the interval clamped to 0.2 .. 60 s
	cmpq $200, opt_interval_ms(%rip)
	jae 21f
	movq $200, opt_interval_ms(%rip)
21:	cmpq $60000, opt_interval_ms(%rip)
	jbe 22f
	movq $60000, opt_interval_ms(%rip)
22:	call make_slots
	# one card fills the screen; several start as blocks
	movq $0, have_focus(%rip)
	movq $0, focus(%rip)
	cmpq $1, nslots(%rip)
	jne 23f
	movq $1, have_focus(%rip)
23:	cmpq $0, a_has_batch(%rip)
	jne batch_mode
	jmp interactive
STR s_o_c, "-c"
STR s_o_card, "--card"
STR s_o_i, "-i"
STR s_o_interval, "--interval"
STR s_o_b, "-b"
STR s_o_batch, "--batch"
STR s_o_socket, "--socket"
STR s_o_k, "-k"
STR s_o_kthreads, "--kthreads"

arg_at:
	cmpq argc(%rip), %rbx
	jae usage
	movq argv(%rip), %rax
	movq (%rax,%rbx,8), %rax
	ret

usage:
	call l_begin
	leaq s_usage(%rip), %rsi
	call w_cstr
	call l_end_bare
	movl $2, %edi
	SYS SYS_exit_group
	.section .rodata
s_usage:	.ascii "usage: phitop [-c N] [-i SECONDS] [-b FRAMES] [--socket PATH] [-k]\n"
	.ascii "  -c, --card N        card index, 0 to 15 (default: $PHI_CARD; else every card in its own block)\n"
	.ascii "  -i, --interval S    seconds between samples (default 1)\n"
	.ascii "  -b, --batch N       print N frames as plain text and exit\n"
	.ascii "  --socket PATH       one daemon's control socket (default: PHICTL_SOCKET, else the card's)\n"
	.ascii "  -k, --kthreads      show the kernel threads the card reports\n"
	.byte 0
	.text

# parse_seconds_ms(rsi, rdx) -> rax = milliseconds, edx = 1: "N" or
# "N.F" (three decimals at most used).
parse_seconds_ms:
	pushq %rbx
	pushq %r12
	pushq %r13
	movq %rsi, %r12
	movq %rdx, %r13
	movb $'.', %cl
	call find_byte
	movq %rax, %rbx
	testq %rbx, %rbx
	js 3f
	movq %r12, %rsi
	movq %rbx, %rdx
	call parse_u64
	testl %edx, %edx
	jz 8f
	imulq $1000, %rax, %rax
	pushq %rax
	leaq 1(%r12,%rbx), %rsi
	movq %r13, %rdx
	subq %rbx, %rdx
	decq %rdx
	jz 2f
	cmpq $3, %rdx
	jbe 1f
	movl $3, %edx
1:	pushq %rdx
	call parse_u64
	popq %rcx
	testl %edx, %edx
	jz 7f
2:	# scale the fraction to milliseconds
	cmpq $3, %rcx
	jae 21f
	imulq $10, %rax, %rax
	incq %rcx
	jmp 2b
21:	popq %rcx
	addq %rcx, %rax
	movl $1, %edx
	jmp 9f
3:	movq %r12, %rsi
	movq %r13, %rdx
	call parse_u64
	testl %edx, %edx
	jz 8f
	imulq $1000, %rax, %rax
	movl $1, %edx
	jmp 9f
7:	popq %rcx
8:	xorl %eax, %eax
	xorl %edx, %edx
9:	popq %r13
	popq %r12
	popq %rbx
	ret

# ------------------------------------------------------------ slots

# slot_init(rdi = slot, rsi = index, rdx = bdf (NUL-terminated or 0),
# rcx = socket path): a slot with its name (the card's hostname) and
# its buffers.
slot_init:
	pushq %rbx
	pushq %r12
	movq %rdi, %rbx
	movq %rsi, SL_INDEX(%rbx)
	movq $0, SL_HAVE_PREV(%rbx)
	movq $0, SL_HAVE_DERIVED(%rbx)
	movq $0, SL_ERROR_LEN(%rbx)
	movb $0, SL_ERROR(%rbx)
	leaq SL_SNAP_A(%rbx), %rax
	movq %rax, SL_CUR(%rbx)
	leaq SL_SNAP_B(%rbx), %rax
	movq %rax, SL_PREV(%rbx)
	movb $0, SL_BDF(%rbx)
	testq %rdx, %rdx
	jz 1f
	pushq %rcx
	leaq SL_BDF(%rbx), %rdi
	movq %rdx, %rsi
	call copy_cstr
	popq %rcx
1:	leaq SL_SOCKET(%rbx), %rdi
	movq %rcx, %rsi
	call copy_cstr
	# the name: phi, then phi1, phi2, ...
	movq wp(%rip), %r12
	leaq SL_NAME(%rbx), %rax
	movq %rax, wp(%rip)
	leaq s_phi(%rip), %rsi
	call w_cstr
	movq SL_INDEX(%rbx), %rdi
	testq %rdi, %rdi
	jz 2f
	call w_dec
2:	xorl %edi, %edi
	call w_byte
	movq %r12, wp(%rip)
	popq %r12
	popq %rbx
	ret
STR s_phi, "phi"

# make_slots: which cards to watch. One when anything names one; else
# all of them.
make_slots:
	pushq %rbx
	pushq %r12
	pushq %r13
	call index_from_env
	movq %rax, %r12			# -1 or PHI_CARD
	cmpq $0, a_has_card(%rip)
	je 1f
	movq a_card(%rip), %r12
1:	movq %r12, %r13
	testq %r13, %r13
	jns 2f
	xorl %r13d, %r13d		# one, defaulting to 0 where a socket is given
2:	movq a_socket(%rip), %rcx
	testq %rcx, %rcx
	jnz 3f
	leaq s_phictl_socket(%rip), %rdi
	call getenv
	movq %rax, %rcx
	testq %rcx, %rcx
	jz 4f
3:	leaq slots(%rip), %rdi
	movq %r13, %rsi
	xorl %edx, %edx
	call slot_init
	movq $1, nslots(%rip)
	jmp 9f
4:	call cards_list
	testq %r12, %r12
	js 6f
	# the one card: its address from the list when listed
	movq %r12, %rdi
	call card_entry
	xorl %edx, %edx
	testq %rax, %rax
	jz 5f
	leaq 8(%rax), %rdx
5:	pushq %rdx
	movq %r12, %rdi
	call socket_of
	movq %rax, %rcx
	popq %rdx
	leaq slots(%rip), %rdi
	movq %r12, %rsi
	call slot_init
	movq $1, nslots(%rip)
	jmp 9f
6:	cmpq $0, ncards(%rip)
	je 90f
	xorl %r13d, %r13d
7:	cmpq ncards(%rip), %r13
	jae 8f
	movq %r13, %rax
	shlq $6, %rax
	leaq cards(%rip), %rbx
	addq %rax, %rbx
	movq (%rbx), %rdi
	call socket_of
	movq %rax, %rcx
	movq %r13, %rax
	imulq $SL_SIZE, %rax, %rax
	leaq slots(%rip), %rdi
	addq %rax, %rdi
	movq (%rbx), %rsi
	leaq 8(%rbx), %rdx
	call slot_init
	incq %r13
	jmp 7b
8:	movq %r13, nslots(%rip)
9:	popq %r13
	popq %r12
	popq %rbx
	ret
90:	leaq s_e_nocards(%rip), %rsi
	jmp fail_phitop
STR s_phictl_socket, "PHICTL_SOCKET"
STR s_e_nocards, "no Xeon Phi card is known to this host (phictl cards)"

# socket_of(rdi = index) -> rax: a root daemon's socket when root or when
# one answers there, else the card's socket in the user's runtime
# directory (a copy in path_buf, valid until the next path).
socket_of:
	pushq %rbx
	movq %rdi, %rbx
	SYS SYS_geteuid
	testq %rax, %rax
	jz 1f
	movq %rbx, %rdi
	movl $1, %esi
	call socket_path_for
	movq %rax, %rdi
	call socket_answers
	testl %eax, %eax
	jz 2f
1:	movq %rbx, %rdi
	movl $1, %esi
	call socket_path_for
	popq %rbx
	ret
2:	movq %rbx, %rdi
	xorl %esi, %esi
	call socket_path_for
	popq %rbx
	ret

# fail_phitop(rsi): "phitop: <message>", exit 1 (after the terminal is
# restored).
fail_phitop:
	pushq %rsi
	call term_close
	call l_begin
	leaq s_phitop_pre(%rip), %rsi
	call w_cstr
	popq %rsi
	call w_cstr
	call l_end
	movl $1, %edi
	SYS SYS_exit_group
STR s_phitop_pre, "phitop: "

# ------------------------------------------------------------ sampling

# request(rdi = slot, esi = tag, edx = timeout ms) -> eax 1 with the
# reply in req_dec (DEC_TAG, DEC_BODY, DEC_LEN); 0 with the slot's error
# set. One request per fresh connection, so the daemon can interleave
# it with whatever session another client holds.
request:
	pushq %rbx
	pushq %r12
	pushq %r13
	pushq %r14
	movq %rdi, %rbx
	movl %esi, %r12d
	movl %edx, %r13d
	movl $AF_UNIX, %edi
	movl $SOCK_STREAM | SOCK_CLOEXEC, %esi
	xorl %edx, %edx
	SYS SYS_socket
	testq %rax, %rax
	js 90f
	movq %rax, req_fd(%rip)
	leaq sockaddr(%rip), %rdi
	movw $AF_UNIX, (%rdi)
	addq $2, %rdi
	leaq SL_SOCKET(%rbx), %rsi
	call copy_cstr
	movl req_fd(%rip), %edi
	leaq sockaddr(%rip), %rsi
	movl $110, %edx
	SYS SYS_connect
	testq %rax, %rax
	js 91f
	movl %r12d, %edi
	call f_begin
	call f_finish
	movl req_fd(%rip), %edi
	call write_all
	testq %rax, %rax
	jnz 92f
	leaq req_dec(%rip), %rdi
	leaq req_buf(%rip), %rsi
	movl $DECCAP, %edx
	call dec_init
	call now_ms
	movq %rax, %r14
	addq %r13, %r14			# deadline
1:	leaq req_dec(%rip), %rdi
	call dec_next
	testq %rax, %rax
	jg 8f
	js 94f
	call now_ms
	cmpq %r14, %rax
	jae 93f
	movq %r14, %rdx
	subq %rax, %rdx
	movl req_fd(%rip), %eax
	movl %eax, pfd1(%rip)
	movw $POLLIN, pfd1+4(%rip)
	leaq pfd1(%rip), %rdi
	movl $1, %esi
	call poll_ms
	testq %rax, %rax
	jz 93f
	leaq req_dec(%rip), %rdi
	call dec_room
	movq %rax, %rsi
	cmpq $READ_CHUNK, %rdx
	jbe 2f
	movl $READ_CHUNK, %edx
2:	movl req_fd(%rip), %edi
	SYS SYS_read
	testq %rax, %rax
	jz 95f
	js 96f
	leaq req_dec(%rip), %rdi
	movq %rax, %rsi
	call dec_advance
	jmp 1b
8:	movl req_fd(%rip), %edi
	SYS SYS_close
	movl $1, %eax
	jmp 99f
90:	negq %rax
	movq %rax, %rdi
	leaq s_e_socket(%rip), %rsi
	call slot_error_os
	jmp 98f
91:	pushq %rax
	movq %rbx, %rdi
	call slot_error_begin
	leaq s_e_conn1(%rip), %rsi
	call w_cstr
	leaq SL_SOCKET(%rbx), %rsi
	call w_cstr
	leaq s_e_conn2(%rip), %rsi
	call w_cstr
	popq %rdi
	negq %rdi
	call w_oserr
	call slot_error_end
	jmp 97f
92:	leaq s_e_write(%rip), %rsi
	call slot_error_text
	jmp 97f
93:	leaq s_e_timeout(%rip), %rsi
	call slot_error_text
	jmp 97f
94:	leaq s_e_badframe(%rip), %rsi
	call slot_error_text
	jmp 97f
95:	leaq s_e_closed(%rip), %rsi
	call slot_error_text
	jmp 97f
96:	negq %rax
	movq %rax, %rdi
	leaq s_e_read(%rip), %rsi
	call slot_error_os
97:	movl req_fd(%rip), %edi
	SYS SYS_close
98:	xorl %eax, %eax
99:	popq %r14
	popq %r13
	popq %r12
	popq %rbx
	ret
STR s_e_socket, "socket"
STR s_e_conn1, "connecting to "
STR s_e_conn2, " (is phictl boot --serve running?): "
STR s_e_write, "writing the request"
STR s_e_timeout, "waiting for the daemon's reply: timed out"
STR s_e_badframe, "the daemon sent a frame phi-rpc rejects"
STR s_e_closed, "the daemon closed the connection"
STR s_e_read, "waiting for the daemon's reply"

# slot_error_begin(rdi = slot) / slot_error_end: the slot's error text
# built through the cursor (at most 400 bytes).
slot_error_begin:
	movq wp(%rip), %rax
	movq %rax, tmp_wp(%rip)
	leaq SL_ERROR(%rdi), %rax
	movq %rax, wp(%rip)
	movq %rdi, err_slot(%rip)
	ret
slot_error_end:
	movq err_slot(%rip), %rdi
	movq wp(%rip), %rax
	leaq SL_ERROR(%rdi), %rcx
	subq %rcx, %rax
	cmpq $400, %rax
	jbe 1f
	movl $400, %eax
1:	movq %rax, SL_ERROR_LEN(%rdi)
	movb $0, (%rcx,%rax)
	movq tmp_wp(%rip), %rax
	movq %rax, wp(%rip)
	ret
	.bss
tmp_wp:	.skip 8
err_slot:	.skip 8
	.text
# slot_error_text(rsi; rbx = slot).
slot_error_text:
	pushq %rsi
	movq %rbx, %rdi
	call slot_error_begin
	popq %rsi
	call w_cstr
	jmp slot_error_end
# slot_error_os(rsi = what, rdi = errno; rbx = slot).
slot_error_os:
	pushq %rdi
	pushq %rsi
	movq %rbx, %rdi
	call slot_error_begin
	popq %rsi
	call w_cstr
	leaq s_colon(%rip), %rsi
	call w_cstr
	popq %rdi
	call w_oserr
	jmp slot_error_end
STR s_colon, ": "

# slot_poll(rdi = slot): a Stat and a Traffic into the slot's incoming
# snapshot, then the model updated; on failure the error is kept and
# the model untouched.
slot_poll:
	pushq %rbx
	movq %rdi, %rbx
	movl $TAG_STAT, %esi
	movl $STAT_TIMEOUT_MS, %edx
	call request
	testl %eax, %eax
	jz 9f
	movq req_dec+DEC_TAG(%rip), %rax
	cmpq $TAG_STAT_REPLY, %rax
	jne 5f
	movq SL_CUR(%rbx), %rdi
	movq req_dec+DEC_BODY(%rip), %rsi
	movq req_dec+DEC_LEN(%rip), %rdx
	call snapshot_decode
	call now_us
	movq SL_CUR(%rbx), %rdi
	movq %rax, SN_AT_US(%rdi)
	movq %rbx, %rdi
	movl $TAG_TRAFFIC, %esi
	movl $TRAFFIC_TIMEOUT_MS, %edx
	call request
	testl %eax, %eax
	jz 9f
	cmpq $TAG_TRAFFIC_REPLY, req_dec+DEC_TAG(%rip)
	jne 6f
	movq SL_CUR(%rbx), %rdi
	movq req_dec+DEC_BODY(%rip), %rsi
	call traffic_decode
	movq $0, SL_ERROR_LEN(%rbx)
	movb $0, SL_ERROR(%rbx)
	movq %rbx, %rdi
	call model_update
	jmp 9f
5:	cmpq $TAG_ERROR, %rax
	jne 6f
	movq %rbx, %rdi
	call slot_error_begin
	leaq s_card_pre(%rip), %rsi
	call w_cstr
	movq req_dec+DEC_BODY(%rip), %rsi
	movzwl (%rsi), %edx
	addq $2, %rsi
	call w_bytes
	call slot_error_end
	jmp 9f
6:	movq %rbx, %rdi
	call slot_error_begin
	leaq s_unexpected(%rip), %rsi
	call w_cstr
	movq req_dec+DEC_TAG(%rip), %rax
	leaq msg_names(%rip), %rsi
	movq (%rsi,%rax,8), %rsi
	call w_cstr
	call slot_error_end
9:	popq %rbx
	ret
STR s_card_pre, "card: "
STR s_unexpected, "unexpected reply "

# poll_all: the focused slot, or every slot.
poll_all:
	cmpq $0, have_focus(%rip)
	je 1f
	movq focus(%rip), %rax
	imulq $SL_SIZE, %rax, %rax
	leaq slots(%rip), %rdi
	addq %rax, %rdi
	jmp slot_poll
1:	pushq %rbx
	xorl %ebx, %ebx
2:	cmpq nslots(%rip), %rbx
	jae 3f
	movq %rbx, %rax
	imulq $SL_SIZE, %rax, %rax
	leaq slots(%rip), %rdi
	addq %rax, %rdi
	call slot_poll
	incq %rbx
	jmp 2b
3:	popq %rbx
	ret

# ------------------------------------------------------------ frames

	.globl frame_cr
# frame_cr(r8d = cols, r9d = rows): the frame for the current focus.
frame_cr:
	cmpq $0, have_focus(%rip)
	je 2f
	movq focus(%rip), %rcx
	imulq $SL_SIZE, %rcx, %rcx
	leaq slots(%rip), %rdi
	addq %rcx, %rdi
	movq $0, opt_error(%rip)
	cmpq $0, SL_ERROR_LEN(%rdi)
	je 1f
	leaq SL_ERROR(%rdi), %rcx
	movq %rcx, opt_error(%rip)
1:	movl %r8d, %esi
	movl %r9d, %edx
	cmpq $0, SL_HAVE_DERIVED(%rdi)
	je render_waiting
	jmp render
2:	movq $0, opt_error(%rip)
	leaq slots(%rip), %rdi
	movq nslots(%rip), %rsi
	movl %r8d, %edx
	movl %r9d, %ecx
	jmp render_multi

# draw: the terminal's size, the frame, written out.
draw:
	call term_size
	movl %eax, %r8d
	movl %edx, %r9d
	call frame_cr
	movl $1, %edi
	leaq frame_buf(%rip), %rsi
	movq frame_len(%rip), %rdx
	jmp write_all

# ------------------------------------------------------------ modes

batch_mode:
	xorl %ebx, %ebx			# printed
1:	cmpq a_batch(%rip), %rbx
	jae 9f
	call poll_all
	call any_ready
	testl %eax, %eax
	jz 2f
	movl $132, %r8d
	movl $32, %r9d
	call frame_cr
	movl $1, %edi
	leaq frame_buf(%rip), %rsi
	movq frame_len(%rip), %rdx
	call write_all
	movl $1, %edi
	leaq s_nl(%rip), %rsi
	movl $1, %edx
	call write_all
	incq %rbx
	jmp 3f
2:	call first_error
	testq %rax, %rax
	jz 3f
	pushq %rax
	call l_begin
	leaq s_phitop_pre(%rip), %rsi
	call w_cstr
	popq %rsi
	call w_cstr
	call l_end
3:	movq opt_interval_ms(%rip), %rdi
	call sleep_ms
	jmp 1b
9:	xorl %edi, %edi
	SYS SYS_exit_group
STR s_nl, "\n"

# any_ready -> eax 1 when the focused slot (or any slot) has a derivation.
any_ready:
	cmpq $0, have_focus(%rip)
	je 1f
	movq focus(%rip), %rax
	imulq $SL_SIZE, %rax, %rax
	leaq slots(%rip), %rcx
	movq SL_HAVE_DERIVED(%rcx,%rax), %rax
	ret
1:	xorl %ecx, %ecx
2:	cmpq nslots(%rip), %rcx
	jae 3f
	movq %rcx, %rax
	imulq $SL_SIZE, %rax, %rax
	leaq slots(%rip), %rdx
	cmpq $0, SL_HAVE_DERIVED(%rdx,%rax)
	jne 4f
	incq %rcx
	jmp 2b
3:	xorl %eax, %eax
	ret
4:	movl $1, %eax
	ret

# first_error -> rax = the first slot's error text, or 0.
first_error:
	xorl %ecx, %ecx
1:	cmpq nslots(%rip), %rcx
	jae 3f
	movq %rcx, %rax
	imulq $SL_SIZE, %rax, %rax
	leaq slots(%rip), %rdx
	addq %rax, %rdx
	cmpq $0, SL_ERROR_LEN(%rdx)
	jne 2f
	incq %rcx
	jmp 1b
2:	leaq SL_ERROR(%rdx), %rax
	ret
3:	xorl %eax, %eax
	ret

interactive:
	call term_open
	testl %eax, %eax
	jz 90f
	movq $1, opt_color(%rip)
	call now_ms
	movq %rax, next_ms(%rip)
1:	call poll_all
	call draw
	movq opt_interval_ms(%rip), %rax
	addq %rax, next_ms(%rip)
	call now_ms
	cmpq next_ms(%rip), %rax
	jbe 2f
	movq %rax, next_ms(%rip)
2:	cmpq $0, stop_flag(%rip)
	jne 80f
	call now_ms
	cmpq next_ms(%rip), %rax
	jae 1b
	movq next_ms(%rip), %rdi
	subq %rax, %rdi
	cmpq $500, %rdi
	jbe 3f
	movl $500, %edi
3:	call term_key
	testl %eax, %eax
	js 2b
	cmpl $'q', %eax
	je 80f
	cmpl $3, %eax
	je 80f
	cmpl $'c', %eax
	jne 4f
	movq $0, opt_sort_mem(%rip)
	jmp 20f
4:	cmpl $'m', %eax
	jne 5f
	movq $1, opt_sort_mem(%rip)
	jmp 20f
5:	cmpl $'k', %eax
	jne 6f
	xorq $1, opt_show_kthreads(%rip)
	jmp 20f
6:	cmpl $'+', %eax
	je 7f
	cmpl $'=', %eax
	jne 8f
7:	addq $500, opt_interval_ms(%rip)
	cmpq $60000, opt_interval_ms(%rip)
	jbe 20f
	movq $60000, opt_interval_ms(%rip)
	jmp 20f
8:	cmpl $'-', %eax
	jne 9f
	subq $500, opt_interval_ms(%rip)
	cmpq $250, opt_interval_ms(%rip)
	jae 20f
	movq $250, opt_interval_ms(%rip)
	jmp 20f
9:	cmpq $1, nslots(%rip)
	jbe 2b
	cmpl $'n', %eax
	jne 10f
	cmpq $0, have_focus(%rip)
	jne 91f
	movq $0, focus(%rip)
	movq $1, have_focus(%rip)
	jmp 20f
91:	movq focus(%rip), %rax
	incq %rax
	cmpq nslots(%rip), %rax
	jb 92f
	xorl %eax, %eax
92:	movq %rax, focus(%rip)
	jmp 20f
10:	cmpl $'p', %eax
	jne 11f
	cmpq $0, have_focus(%rip)
	jne 101f
	movq nslots(%rip), %rax
	decq %rax
	movq %rax, focus(%rip)
	movq $1, have_focus(%rip)
	jmp 20f
101:	movq focus(%rip), %rax
	testq %rax, %rax
	jnz 102f
	movq nslots(%rip), %rax
102:	decq %rax
	movq %rax, focus(%rip)
	jmp 20f
11:	cmpl $'a', %eax
	jne 2b
	movq $0, have_focus(%rip)
20:	call draw
	jmp 2b
80:	call term_close
	xorl %edi, %edi
	SYS SYS_exit_group
90:	leaq s_e_notty(%rip), %rsi
	jmp fail_phitop
STR s_e_notty, "standard input is not a terminal (use --batch N for plain text)"
