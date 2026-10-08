# serve.S: `phictl boot --serve`, the control socket
# (host/crates/phictl/src/serve.rs): a Unix socket in a root-owned 0711
# directory, the socket file 0600 and owned by one uid, SO_PEERCRED on
# every connection; rpc frames relayed between up to 16 local clients and
# the card's agent over the rpc channel, one session at a time, Stat
# interleaved and routed to whoever asked first, Traffic answered here and
# Sensors by sensors.S. See serve.md.
	.include "defs.inc"

	.set MAX_CLIENTS, 16
# A client: 0 id, 8 fd, 16 dead, 24 pending head, 32 pending tail, 40
# decoder (64 bytes); 128 bytes each.
	.set CL_ID, 0
	.set CL_FD, 8
	.set CL_DEAD, 16
	.set CL_PHEAD, 24
	.set CL_PTAIL, 32
	.set CL_DEC, 40
	.set CL_SIZE, 128
	.set PENDCAP, 2 * MAX_FRAME
	.set TOCARDCAP, 4 * MAX_FRAME
# Sessions.
	.set S_NONE, 0
	.set S_EXEC_OPEN, 1
	.set S_EXEC_CLOSED, 2
	.set S_PUT_OPEN, 3
	.set S_PUT_CLOSED, 4
	.set S_OTHER, 5

	.bss
	.p2align 3
listen_fd:	.skip 8
	.globl serve_owner
serve_owner:	.skip 8
nclients:	.skip 8
next_id:	.skip 8
holder:	.skip 8
session:	.skip 8
# Clients awaiting a StatReply, in order: a ring of ids.
stat_q:	.skip 8 * 64
stat_qh:	.skip 8
stat_qt:	.skip 8
to_card_head:	.skip 8
to_card_tail:	.skip 8
rpc_ready:	.skip 8
rpc_h2c:	.skip 8
rpc_h2c_size:	.skip 8
rpc_c2h:	.skip 8
rpc_c2h_size:	.skip 8
clients:	.skip MAX_CLIENTS * CL_SIZE
card_dec:	.skip 64
	.p2align 6
to_card:	.skip TOCARDCAP
pend_bufs:	.skip MAX_CLIENTS * PENDCAP
dec_bufs:	.skip MAX_CLIENTS * DECCAP
card_dec_buf:	.skip DECCAP
chunk:	.skip READ_CHUNK
sock_addr:	.skip 112
peer_cred:	.skip 16

	.text

	.globl default_owner
# default_owner() -> rax: SUDO_UID, else the effective uid.
default_owner:
	leaq s_sudo_uid(%rip), %rdi
	call getenv
	testq %rax, %rax
	jz 1f
	movq %rax, %rsi
	call cstr_len
	call parse_u64
	testl %edx, %edx
	jz 1f
	ret
1:	SYS SYS_geteuid
	ret
STR s_sudo_uid, "SUDO_UID"

	.globl serve_open
# serve_open: the listener at o_serve ("auto" = the default path for a
# daemon: root's when root, else the user's), directory 0711, stale
# socket removed, file 0600 owned by the owner, nonblocking.
serve_open:
	pushq %rbx
	pushq %r12
	# the owner
	cmpq $0, o_has_owner(%rip)
	je 1f
	movq o_owner(%rip), %rax
	jmp 2f
1:	call default_owner
2:	movq %rax, serve_owner(%rip)
	# the path
	movq o_serve(%rip), %rsi
	leaq s_auto(%rip), %rdi
	call eq_cc
	testl %eax, %eax
	jz 3f
	SYS SYS_geteuid
	xorl %esi, %esi
	testq %rax, %rax
	setz %sil
	movq card_index(%rip), %rdi
	call socket_path_for
	movq %rax, o_serve(%rip)
3:	movq o_serve(%rip), %r12
	# the directory: everything up to the last '/'
	movq %r12, %rsi
	call cstr_len
	movq %rdx, %rcx
4:	testq %rcx, %rcx
	jz 6f
	cmpb $'/', -1(%r12,%rcx)
	je 5f
	decq %rcx
	jmp 4b
5:	decq %rcx
	jz 6f
	leaq path_buf(%rip), %rdi
	movq %r12, %rsi
	rep movsb
	movb $0, (%rdi)
	leaq path_buf(%rip), %rdi
	call mkdir_p
	leaq path_buf(%rip), %rdi
	movl $0711, %esi
	SYS SYS_chmod
6:	movq %r12, %rdi
	SYS SYS_unlink
	movl $AF_UNIX, %edi
	movl $SOCK_STREAM | SOCK_CLOEXEC | SOCK_NONBLOCK, %esi
	xorl %edx, %edx
	SYS SYS_socket
	testq %rax, %rax
	js 90f
	movq %rax, listen_fd(%rip)
	leaq sock_addr(%rip), %rdi
	movw $AF_UNIX, (%rdi)
	addq $2, %rdi
	movq %r12, %rsi
	call copy_cstr
	movl listen_fd(%rip), %edi
	leaq sock_addr(%rip), %rsi
	movl $110, %edx
	SYS SYS_bind
	testq %rax, %rax
	js 91f
	movq %r12, %rdi
	movl $0600, %esi
	SYS SYS_chmod
	movq %r12, %rdi
	movq serve_owner(%rip), %rsi
	movq $-1, %rdx
	SYS SYS_chown
	testq %rax, %rax
	js 92f
	movl listen_fd(%rip), %edi
	movl $16, %esi
	SYS SYS_listen
	testq %rax, %rax
	js 93f
	# decoders
	xorl %ebx, %ebx
7:	cmpl $MAX_CLIENTS, %ebx
	jae 8f
	movl %ebx, %edi
	call client_at
	leaq CL_DEC(%rax), %rdi
	movl %ebx, %eax
	imull $DECCAP, %eax, %eax
	leaq dec_bufs(%rip), %rsi
	addq %rax, %rsi
	movl $DECCAP, %edx
	call dec_init
	incl %ebx
	jmp 7b
8:	leaq card_dec(%rip), %rdi
	leaq card_dec_buf(%rip), %rsi
	movl $DECCAP, %edx
	call dec_init
	popq %r12
	popq %rbx
	ret
90:	leaq s_e_socket(%rip), %rsi
	jmp fail_neg
91:	leaq s_e_bind(%rip), %rsi
	jmp fail_neg
92:	leaq s_e_chown(%rip), %rsi
	jmp fail_neg
93:	leaq s_e_listen(%rip), %rsi
	jmp fail_neg
STR s_auto, "auto"
STR s_e_socket, "socket"
STR s_e_bind, "binding the control socket"
STR s_e_chown, "chown of the control socket"
STR s_e_listen, "listen"

	.globl mkdir_p
# mkdir_p(rdi = NUL-terminated path): every component, existing ones
# left alone.
mkdir_p:
	pushq %rbx
	movq %rdi, %rbx
	leaq 1(%rbx), %rcx
1:	movb (%rcx), %al
	testb %al, %al
	jz 3f
	cmpb $'/', %al
	jne 2f
	movb $0, (%rcx)
	pushq %rcx
	movq %rbx, %rdi
	movl $0755, %esi
	SYS SYS_mkdir
	popq %rcx
	movb $'/', (%rcx)
2:	incq %rcx
	jmp 1b
3:	movq %rbx, %rdi
	movl $0755, %esi
	SYS SYS_mkdir
	popq %rbx
	ret

# ------------------------------------------------------------ the client table

# client_at(edi = index) -> rax = the client's slot (CL_SIZE bytes each,
# in clients); rdi clobbered, nothing else.
client_at:
	shll $7, %edi			# CL_SIZE
	leaq clients(%rip), %rax
	addq %rdi, %rax
	ret

# client_wants_read(rdi = client) -> eax 1 when the next pass reads its
# socket: a live client that holds the card or has nothing pending, with
# room for a read chunk in its decoder and for what it may bring in
# to_card (within two frames of full the client waits: the
# backpressure). clients_to_card reads by this and serve_fds waits by
# it, so the two cannot drift apart.
client_wants_read:
	cmpq $0, CL_FD(%rdi)
	je 8f
	cmpq $0, CL_DEAD(%rdi)
	jne 8f
	movq CL_PTAIL(%rdi), %rax
	cmpq CL_PHEAD(%rdi), %rax
	je 1f
	movq CL_ID(%rdi), %rax
	cmpq holder(%rip), %rax
	jne 8f
1:	addq $CL_DEC, %rdi
	call dec_space
	cmpq $READ_CHUNK, %rax
	jb 8f
	movq to_card_tail(%rip), %rax
	subq to_card_head(%rip), %rax
	cmpq $TOCARDCAP - 2 * MAX_FRAME, %rax
	jae 8f
	movl $1, %eax
	ret
8:	xorl %eax, %eax
	ret

# ------------------------------------------------------------ the pass

	.globl serve_step
# serve_step -> eax 1 when anything moved. Before the card has reached
# init nothing is relayed (connections are accepted and wait).
serve_step:
	pushq %r15
	xorl %r15d, %r15d	# moved
	call accept_clients
	orl %eax, %r15d
	cmpq $0, rpc_ready(%rip)
	jne 1f
	call rpc_attach
	testl %eax, %eax
	jz 9f
1:	call clients_to_card
	orl %eax, %r15d
	call reap_clients
	call pick_holder
	orl %eax, %r15d
	call push_to_card
	orl %eax, %r15d
	call card_to_clients
	orl %eax, %r15d
9:	movl %r15d, %eax
	popq %r15
	ret

# rpc_attach -> eax 1 once the card has reached init and the rpc
# channel's rings are attached.
rpc_attach:
	call wait_init_seen
	testl %eax, %eax
	jz 9f
	cmpq $0, region_ok(%rip)
	je 9f
	movl $KIND_RPC, %edi
	call endpoints_of
	testl %eax, %eax
	jz 9f
	movq %r8, rpc_h2c(%rip)
	movq %r9, rpc_h2c_size(%rip)
	movq %r10, rpc_c2h(%rip)
	movq %r11, rpc_c2h_size(%rip)
	movq $1, rpc_ready(%rip)
	call l_begin
	call l_tag
	leaq s_ready1(%rip), %rsi
	call w_cstr
	movq serve_owner(%rip), %rdi
	call w_dec
	leaq s_ready2(%rip), %rsi
	call w_cstr
	call l_end
	movl $1, %eax
	ret
9:	xorl %eax, %eax
	ret
STR s_ready1, "serve: control socket ready for uid "
STR s_ready2, " (and root)"

# accept_clients -> eax 1 when one was accepted: every pending
# connection, checked against the owner (or root), refused past
# MAX_CLIENTS.
accept_clients:
	pushq %rbx
	pushq %r12
	xorl %r12d, %r12d
1:	movl listen_fd(%rip), %edi
	xorl %esi, %esi
	xorl %edx, %edx
	movl $SOCK_CLOEXEC | SOCK_NONBLOCK, %r10d
	SYS SYS_accept4
	testq %rax, %rax
	js 9f
	movl %eax, %ebx
	# SO_PEERCRED: pid, uid, gid
	movl $16, peer_cred+12(%rip)
	movl %ebx, %edi
	movl $SOL_SOCKET, %esi
	movl $SO_PEERCRED, %edx
	leaq peer_cred(%rip), %r10
	leaq peer_cred+12(%rip), %r8
	SYS SYS_getsockopt
	testq %rax, %rax
	js 7f
	movl peer_cred+4(%rip), %eax
	testl %eax, %eax
	jz 2f
	cmpq serve_owner(%rip), %rax
	jne 6f
2:	cmpq $MAX_CLIENTS, nclients(%rip)
	jae 5f
	# the first free slot (fd 0)
	xorl %ecx, %ecx
3:	movl %ecx, %edi
	call client_at
	movq %rax, %rdi
	cmpq $0, CL_FD(%rdi)
	je 4f
	incl %ecx
	cmpl $MAX_CLIENTS, %ecx
	jb 3b
	jmp 5f
4:	incq next_id(%rip)
	movq next_id(%rip), %rax
	movq %rax, CL_ID(%rdi)
	movq %rbx, CL_FD(%rdi)
	movq $0, CL_DEAD(%rdi)
	movq $0, CL_PHEAD(%rdi)
	movq $0, CL_PTAIL(%rdi)
	pushq %rdi
	addq $CL_DEC, %rdi
	call dec_reset
	popq %rdi
	incq nclients(%rip)
	movl $1, %r12d
	jmp 1b
5:	call l_begin
	call l_tag
	leaq s_refused_many(%rip), %rsi
	call w_cstr
	call l_end
	jmp 8f
6:	pushq %rax
	call l_begin
	call l_tag
	leaq s_refused_uid(%rip), %rsi
	call w_cstr
	popq %rdi
	call w_dec
	call l_end
	jmp 8f
7:	call l_begin
	call l_tag
	leaq s_peercred(%rip), %rsi
	call w_cstr
	call l_end
8:	movl %ebx, %edi
	SYS SYS_close
	jmp 1b
9:	movl %r12d, %eax
	popq %r12
	popq %rbx
	ret
STR s_refused_many, "serve: refused a connection: 16 clients already"
STR s_refused_uid, "serve: refused a connection from uid "
STR s_peercred, "serve: SO_PEERCRED failed on a connection"

# clients_to_card -> eax 1 when bytes were read: each live client's
# socket read and cut into frames; a client waiting for the card keeps
# its later frames in its socket until its turn.
clients_to_card:
	pushq %rbx
	pushq %r12
	pushq %r13
	xorl %r13d, %r13d
	xorl %r12d, %r12d
1:	cmpl $MAX_CLIENTS, %r12d
	jae 9f
	movl %r12d, %edi
	call client_at
	movq %rax, %rbx
	movq %rax, %rdi
	call client_wants_read
	testl %eax, %eax
	jz 8f
	leaq CL_DEC(%rbx), %rdi
	call dec_room
	movq %rax, %rsi
	movq %rdx, %rcx
	cmpq $READ_CHUNK, %rcx
	jbe 3f
	movl $READ_CHUNK, %ecx
3:	movl CL_FD(%rbx), %edi
	movq %rcx, %rdx
	SYS SYS_read
	testq %rax, %rax
	jz 6f
	js 5f
	movl $1, %r13d
	leaq CL_DEC(%rbx), %rdi
	movq %rax, %rsi
	call dec_advance
4:	leaq CL_DEC(%rbx), %rdi
	call dec_next
	testq %rax, %rax
	jz 8f
	js 7f
	movq %rbx, %rdi
	call handle_frame
	jmp 4b
5:	cmpq $-EAGAIN, %rax
	je 8f
	cmpq $-EINTR, %rax
	je 8f
6:	movq $1, CL_DEAD(%rbx)
	jmp 8f
7:	leaq CL_DEC(%rbx), %rdi
	leaq s_a_client(%rip), %rsi
	call log_bad_frame
	movq $1, CL_DEAD(%rbx)
8:	incl %r12d
	jmp 1b
9:	movl %r13d, %eax
	popq %r13
	popq %r12
	popq %rbx
	ret
STR s_a_client, "a client"

# handle_frame(rdi = client; its decoder holds the frame): answered
# here (Sensors, Traffic), queued for the card (Stat, with the requester
# remembered), forwarded (the holder's), or held until the card is free.
handle_frame:
	pushq %rbx
	movq %rdi, %rbx
	movq CL_DEC+DEC_TAG(%rbx), %rax
	cmpq $TAG_SENSORS, %rax
	jne 1f
	call sensors_frame
	movq %rbx, %rdi
	call client_send_tx
	jmp 9f
1:	cmpq $TAG_TRAFFIC, %rax
	jne 2f
	call traffic_frame
	movq %rbx, %rdi
	call client_send_tx
	jmp 9f
2:	cmpq $TAG_STAT, %rax
	jne 3f
	movq %rbx, %rdi
	call frame_to_card
	movq stat_qt(%rip), %rax
	movq CL_ID(%rbx), %rcx
	andq $63, %rax
	leaq stat_q(%rip), %rdx
	movq %rcx, (%rdx,%rax,8)
	incq stat_qt(%rip)
	jmp 9f
3:	movq CL_ID(%rbx), %rax
	cmpq holder(%rip), %rax
	jne 4f
	movq %rbx, %rdi
	call forward_frame
	jmp 9f
4:	# pending: the frame's bytes appended to the client's pending buffer
	movq CL_DEC+DEC_LEN(%rbx), %rdx
	addq $5, %rdx
	movq CL_PTAIL(%rbx), %rax
	addq %rdx, %rax
	cmpq $PENDCAP, %rax
	ja 5f
	movq %rbx, %rdi
	call pending_buf
	addq CL_PTAIL(%rbx), %rax
	movq %rax, %rdi
	movq CL_DEC+DEC_BODY(%rbx), %rsi
	subq $5, %rsi
	movq %rdx, %rcx
	rep movsb
	addq %rdx, CL_PTAIL(%rbx)
	jmp 9f
5:	# more than two frames held while waiting: the client is dropped
	movq $1, CL_DEAD(%rbx)
9:	popq %rbx
	ret

# pending_buf(rdi = client) -> rax = its pending buffer.
pending_buf:
	movq %rdi, %rax
	leaq clients(%rip), %rcx
	subq %rcx, %rax
	shrq $7, %rax
	imulq $PENDCAP, %rax, %rax
	leaq pend_bufs(%rip), %rcx
	addq %rcx, %rax
	ret

# frame_to_card(rdi = client): the client's current frame appended to
# to_card (room was checked before the read).
frame_to_card:
	movq CL_DEC+DEC_BODY(%rdi), %rsi
	subq $5, %rsi
	movq CL_DEC+DEC_LEN(%rdi), %rdx
	addq $5, %rdx
	.globl to_card_append
# to_card_append(rsi = frame, rdx = length): compacts first when the
# frame would not fit at the tail.
to_card_append:
	movq to_card_tail(%rip), %rax
	addq %rdx, %rax
	cmpq $TOCARDCAP, %rax
	jbe 1f
	pushq %rsi
	pushq %rdx
	leaq to_card(%rip), %rdi
	movq to_card_head(%rip), %rsi
	movq to_card_tail(%rip), %rcx
	subq %rsi, %rcx
	movq %rcx, to_card_tail(%rip)
	movq $0, to_card_head(%rip)
	addq %rdi, %rsi
	rep movsb
	popq %rdx
	popq %rsi
1:	leaq to_card(%rip), %rdi
	addq to_card_tail(%rip), %rdi
	movq %rdx, %rcx
	rep movsb
	addq %rdx, to_card_tail(%rip)
	ret

# forward_frame(rdi = client): a session frame to the card, the session
# state following it.
forward_frame:
	movq CL_DEC+DEC_TAG(%rdi), %rax
	cmpq $TAG_EXEC, %rax
	jne 1f
	movq $S_EXEC_OPEN, session(%rip)
	jmp 6f
1:	cmpq $TAG_STDIN_EOF, %rax
	jne 2f
	cmpq $S_EXEC_OPEN, session(%rip)
	jne 6f
	movq $S_EXEC_CLOSED, session(%rip)
	jmp 6f
2:	cmpq $TAG_PUT_OPEN, %rax
	jne 3f
	movq $S_PUT_OPEN, session(%rip)
	jmp 6f
3:	cmpq $TAG_PUT_CLOSE, %rax
	jne 4f
	cmpq $S_PUT_OPEN, session(%rip)
	jne 6f
	movq $S_PUT_CLOSED, session(%rip)
	jmp 6f
4:	cmpq $TAG_GET, %rax
	je 5f
	cmpq $TAG_PING, %rax
	jne 6f
5:	movq $S_OTHER, session(%rip)
6:	jmp frame_to_card

# reap_clients: a dead holder abandons its session (what it left open is
# closed so the card's session ends); dead clients are closed and their
# slots freed.
reap_clients:
	pushq %rbx
	pushq %r12
	xorl %r12d, %r12d
1:	cmpl $MAX_CLIENTS, %r12d
	jae 9f
	movl %r12d, %edi
	call client_at
	movq %rax, %rbx
	cmpq $0, CL_FD(%rbx)
	je 8f
	cmpq $0, CL_DEAD(%rbx)
	je 8f
	movq CL_ID(%rbx), %rax
	cmpq holder(%rip), %rax
	jne 3f
	movq $0, holder(%rip)
	cmpq $S_EXEC_OPEN, session(%rip)
	jne 2f
	movl $TAG_STDIN_EOF, %edi
	call f_begin
	call f_finish
	call to_card_append
	movq $S_EXEC_CLOSED, session(%rip)
	jmp 3f
2:	cmpq $S_PUT_OPEN, session(%rip)
	jne 3f
	movl $TAG_PUT_CLOSE, %edi
	call f_begin
	call f_finish
	call to_card_append
	movq $S_PUT_CLOSED, session(%rip)
3:	movl CL_FD(%rbx), %edi
	SYS SYS_close
	movq $0, CL_FD(%rbx)
	movq $0, CL_DEAD(%rbx)
	decq nclients(%rip)
8:	incl %r12d
	jmp 1b
9:	popq %r12
	popq %rbx
	ret

# pick_holder -> eax 1 when a waiting client got the card: with no
# session, the first client with pending frames becomes the holder and
# its frames go to the card.
pick_holder:
	pushq %rbx
	pushq %r12
	xorl %eax, %eax
	cmpq $0, session(%rip)
	jne 9f
	cmpq $0, holder(%rip)
	jne 9f
	xorl %r12d, %r12d
1:	cmpl $MAX_CLIENTS, %r12d
	jae 8f
	movl %r12d, %edi
	call client_at
	movq %rax, %rbx
	cmpq $0, CL_FD(%rbx)
	je 2f
	movq CL_PTAIL(%rbx), %rax
	cmpq CL_PHEAD(%rbx), %rax
	jne 3f
2:	incl %r12d
	jmp 1b
8:	# nobody waiting: rax is the last slot here, not 0 (until 2026-10-08
	# it was the slot's offset, 15 << 7, reported as movement every pass)
	xorl %eax, %eax
	jmp 9f
3:	movq CL_ID(%rbx), %rax
	movq %rax, holder(%rip)
	# each pending frame through forward_frame: the decoder is pointed at
	# the pending buffer for the walk
	movq %rbx, %rdi
	call pending_buf
	movq %rax, %r12
4:	movq CL_PHEAD(%rbx), %rcx
	cmpq CL_PTAIL(%rbx), %rcx
	jae 5f
	leaq (%r12,%rcx), %rax
	movl (%rax), %edx
	movzbl 4(%rax), %ecx
	movq %rcx, CL_DEC+DEC_TAG(%rbx)
	leaq 5(%rax), %rcx
	movq %rcx, CL_DEC+DEC_BODY(%rbx)
	leaq -1(%rdx), %rcx
	movq %rcx, CL_DEC+DEC_LEN(%rbx)
	pushq %rdx
	movq %rbx, %rdi
	call forward_frame
	popq %rdx
	leaq 4(%rdx), %rax
	addq %rax, CL_PHEAD(%rbx)
	jmp 4b
5:	movq $0, CL_PHEAD(%rbx)
	movq $0, CL_PTAIL(%rbx)
	movl $1, %eax
9:	popq %r12
	popq %rbx
	ret

	.globl serve_fds
# serve_fds(rdi = pollfd cursor) -> rax = the cursor past what was added:
# the listener, and every client the next pass would read (the
# conditions of clients_to_card, once the rpc channel is attached), each
# for POLLIN. The daemon's idle wait sleeps on them (console.md).
serve_fds:
	pushq %rbx
	pushq %r12
	pushq %r13
	movq %rdi, %r13
	movl listen_fd(%rip), %eax
	PFD %r13, %eax, $POLLIN
	cmpq $0, rpc_ready(%rip)
	je 9f
	xorl %r12d, %r12d
1:	cmpl $MAX_CLIENTS, %r12d
	jae 9f
	movl %r12d, %edi
	call client_at
	movq %rax, %rbx
	movq %rax, %rdi
	call client_wants_read
	testl %eax, %eax
	jz 8f
	movl CL_FD(%rbx), %eax
	PFD %r13, %eax, $POLLIN
8:	incl %r12d
	jmp 1b
9:	movq %r13, %rax
	popq %r13
	popq %r12
	popq %rbx
	ret

# push_to_card -> eax 1 when bytes went into the ring.
push_to_card:
	movq to_card_tail(%rip), %rcx
	subq to_card_head(%rip), %rcx
	jz 9f
	movq rpc_h2c(%rip), %rdi
	movl rpc_h2c_size(%rip), %esi
	leaq to_card(%rip), %rdx
	addq to_card_head(%rip), %rdx
	call ring_push
	addq %rax, to_card_head(%rip)
	movq to_card_head(%rip), %rcx
	cmpq to_card_tail(%rip), %rcx
	jne 1f
	movq $0, to_card_head(%rip)
	movq $0, to_card_tail(%rip)
1:	testq %rax, %rax
	setnz %al
	movzbl %al, %eax
	ret
9:	xorl %eax, %eax
	ret

# card_to_clients -> eax 1 when bytes came from the card: the ring
# drained into the card decoder, each frame delivered.
card_to_clients:
	pushq %rbx
	xorl %ebx, %ebx
	movq rpc_c2h(%rip), %rdi
	movl rpc_c2h_size(%rip), %esi
	call ring_avail
	testl %eax, %eax
	jz 9f
	leaq card_dec(%rip), %rdi
	call dec_room
	movq %rax, %rcx
	movq %rdx, %r8
	cmpq $READ_CHUNK, %r8
	jbe 1f
	movl $READ_CHUNK, %r8d
1:	movq rpc_c2h(%rip), %rdi
	movl rpc_c2h_size(%rip), %esi
	movq %rcx, %rdx
	movq %r8, %rcx
	call ring_pop
	testq %rax, %rax
	jz 9f
	movl $1, %ebx
	leaq card_dec(%rip), %rdi
	movq %rax, %rsi
	call dec_advance
2:	leaq card_dec(%rip), %rdi
	call dec_next
	testq %rax, %rax
	jz 9f
	js 3f
	call deliver
	jmp 2b
3:	leaq card_dec(%rip), %rdi
	leaq s_the_card(%rip), %rsi
	call log_bad_frame
	leaq card_dec(%rip), %rdi
	call dec_reset
9:	movl %ebx, %eax
	popq %rbx
	ret
STR s_the_card, "the card; discarding pending bytes"

# deliver: the card decoder's frame to a client: a StatReply to the
# oldest waiting requester, anything else to the session holder; a frame
# that ends the session (Exit, Error, GetEnd, Pong) frees the card.
deliver:
	pushq %rbx
	movq card_dec+DEC_TAG(%rip), %rax
	cmpq $TAG_STAT_REPLY, %rax
	jne 2f
	movq stat_qh(%rip), %rcx
	cmpq stat_qt(%rip), %rcx
	jae 9f
	andq $63, %rcx
	leaq stat_q(%rip), %rdx
	movq (%rdx,%rcx,8), %rdi
	incq stat_qh(%rip)
	call client_by_id
	testq %rax, %rax
	jz 9f
	movq %rax, %rdi
	call client_send_card_frame
	jmp 9f
2:	movq %rax, %rbx
	movq holder(%rip), %rdi
	testq %rdi, %rdi
	jz 3f
	call client_by_id
	testq %rax, %rax
	jz 3f
	movq %rax, %rdi
	call client_send_card_frame
3:	cmpq $TAG_EXIT, %rbx
	je 4f
	cmpq $TAG_ERROR, %rbx
	je 4f
	cmpq $TAG_GET_END, %rbx
	je 4f
	cmpq $TAG_PONG, %rbx
	jne 9f
4:	movq $0, session(%rip)
	movq $0, holder(%rip)
9:	popq %rbx
	ret

# client_by_id(rdi = id) -> rax = the client, or 0.
client_by_id:
	movq %rdi, %rsi
	xorl %ecx, %ecx
1:	cmpl $MAX_CLIENTS, %ecx
	jae 9f
	movl %ecx, %edi
	call client_at
	cmpq $0, CL_FD(%rax)
	je 2f
	cmpq %rsi, CL_ID(%rax)
	je 3f
2:	incl %ecx
	jmp 1b
9:	xorl %eax, %eax
3:	ret

# client_send_card_frame(rdi = client): the card decoder's frame.
client_send_card_frame:
	movq card_dec+DEC_BODY(%rip), %rsi
	subq $5, %rsi
	movq card_dec+DEC_LEN(%rip), %rdx
	addq $5, %rdx
	jmp client_send

# client_send_tx(rdi = client): the frame in tx_buf.
client_send_tx:
	pushq %rdi
	call f_finish
	popq %rdi
# client_send(rdi = client, rsi = frame, rdx = length): whole, the
# client marked dead when the write fails. A client that does not read
# its socket stalls the daemon's pass, as it stalled the Rust relay's
# thread (write_all on a blocking-free socket loop).
client_send:
	pushq %rdi
	movl CL_FD(%rdi), %edi
	call write_all_nb
	popq %rdi
	testq %rax, %rax
	jz 1f
	movq $1, CL_DEAD(%rdi)
1:	ret

	.globl write_all_nb
# write_all_nb(edi = fd, rsi, rdx) -> rax = 0 or -errno: a write that
# retries EAGAIN on a nonblocking descriptor by waiting for it.
write_all_nb:
	pushq %rbx
	pushq %r12
	pushq %r13
	subq $8, %rsp
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
	addq %rax, %r12
	subq %rax, %r13
	jmp 1b
2:	cmpq $-EINTR, %rax
	je 1b
	cmpq $-EAGAIN, %rax
	jne 5f
	movl %ebx, (%rsp)
	movw $POLLOUT, 4(%rsp)
	movq %rsp, %rdi
	movl $1, %esi
	movl $1000, %edx
	call poll_ms
	jmp 1b
3:	xorl %eax, %eax
5:	addq $8, %rsp
	popq %r13
	popq %r12
	popq %rbx
	ret

# ------------------------------------------------------------ traffic

# traffic_frame: TrafficReply in tx_buf from the daemon's counters.
traffic_frame:
	movl $TAG_TRAFFIC_REPLY, %edi
	call f_begin
	leaq traffic(%rip), %rsi
	movl $48, %edx
	jmp w_bytes

