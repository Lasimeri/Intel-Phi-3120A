# tcp.S: a small TCP/IP stack for the port forwarder (in place of the
# smoltcp the Rust forwarder used): Ethernet frames over the ring's
# network channel, ARP for the card's address, IPv4 without
# fragmentation, and TCP as an active-open client with in-order delivery,
# a fixed retransmission timer and a fixed send quota. The peer is the
# card's Linux, which tolerates a simple correspondent. See tcp.md.
	.include "defs.inc"

	.set MAX_ETH_FRAME, 1518
	.set REC_HDR, 2
	.set MSS, 1460
	.set NCONN, 32
	.set SNDCAP, 65536
	.set RCVCAP, 65536
	.set RTO_MS, 200
	.set RTO_MAX_MS, 3000
	.set MAX_RETRIES, 12
	.set SEND_QUOTA, 16
	.set ARP_RETRY_MS, 500
	.set ETH_ARP, 0x0806
	.set ETH_IP, 0x0800
	.set IPPROTO_TCP, 6
	.set TCP_FIN, 1
	.set TCP_SYN, 2
	.set TCP_RST, 4
	.set TCP_PSH, 8
	.set TCP_ACK, 16

	.set ST_FREE, 0
	.set ST_SYN_SENT, 1
	.set ST_ESTABLISHED, 2
	.set ST_FIN_WAIT_1, 3
	.set ST_FIN_WAIT_2, 4
	.set ST_CLOSE_WAIT, 5
	.set ST_LAST_ACK, 6
	.set ST_CLOSING, 7
	.set ST_CLOSED, 8

# A connection: 0 state, 8 host fd, 16 local port, 24 remote port, 32
# snd_una, 40 snd_nxt, 48 rcv_nxt, 56 snd_wnd, 64 send buffer head
# (index of snd_una's byte), 72 send buffer tail (next byte to add), 80
# rto deadline ms, 88 retries, 96 host_eof, 104 card_eof, 112 fin_seq
# (0 none), 120 receive buffer head, 128 receive buffer tail, 136 rto
# ms, 144 host_shut (SHUT_WR done); 160 bytes. Sequence numbers are kept
# as u32; the buffers are rings indexed modulo their capacity.
	.set C_STATE, 0
	.set C_FD, 8
	.set C_LPORT, 16
	.set C_RPORT, 24
	.set C_SND_UNA, 32
	.set C_SND_NXT, 40
	.set C_RCV_NXT, 48
	.set C_SND_WND, 56
	.set C_SB_HEAD, 64
	.set C_SB_TAIL, 72
	.set C_RTO_AT, 80
	.set C_RETRIES, 88
	.set C_HOST_EOF, 96
	.set C_CARD_EOF, 104
	.set C_FIN_SEQ, 112
	.set C_RB_HEAD, 120
	.set C_RB_TAIL, 128
	.set C_RTO, 136
	.set C_HOST_SHUT, 144
	.set C_ADV_WND, 152
	.set C_SIZE, 160

	.bss
	.p2align 3
	.globl net_h2c, net_h2c_size, net_c2h, net_c2h_size
net_h2c:	.skip 8
net_h2c_size:	.skip 8
net_c2h:	.skip 8
net_c2h_size:	.skip 8
	.globl host_ip, card_ip
host_ip:	.skip 4
card_ip:	.skip 4
card_mac:	.skip 8
have_card_mac:	.skip 8
arp_sent_ms:	.skip 8
ip_id:	.skip 8
next_port:	.skip 8
	.globl conns
conns:	.skip NCONN * C_SIZE
	.p2align 6
snd_bufs:	.skip NCONN * SNDCAP
rcv_bufs:	.skip NCONN * RCVCAP
inbound:	.skip 2 * READ_CHUNK
inbound_len:	.skip 8
	.globl frame
frame:	.skip 2048
outrec:	.skip 2048
seg:	.skip 2048
	.section .rodata
host_mac:	.byte 0x02, 0x50, 0x48, 0x49, 0x00, 0x01
	.text

	.globl tcp_init
tcp_init:
	movq $49152, next_port(%rip)
	movq $0, have_card_mac(%rip)
	movq $0, inbound_len(%rip)
	movq $0, arp_sent_ms(%rip)
	ret

# ------------------------------------------------------------ frames

# net_send(rsi = frame, rdx = len): a record (u16 length, the frame,
# padding to 4) into the host-to-card ring, whole or not at all.
net_send:
	cmpq $MAX_ETH_FRAME, %rdx
	ja 9f
	pushq %rsi
	pushq %rdx
	movl $'o', %edi
	call net_trace
	popq %rdx
	popq %rsi
	leaq REC_HDR(%rdx), %rcx
	addq $3, %rcx
	andq $-4, %rcx
	pushq %rsi
	pushq %rdx
	pushq %rcx
	movq net_h2c(%rip), %rdi
	movl net_h2c_size(%rip), %esi
	call ring_free
	popq %rcx
	popq %rdx
	popq %rsi
	cmpq %rcx, %rax
	jb 9f
	leaq outrec(%rip), %rdi
	movw %dx, (%rdi)
	addq $2, %rdi
	pushq %rcx
	movq %rdx, %rcx
	rep movsb
	movl $0, (%rdi)
	popq %rcx
	movq net_h2c(%rip), %rdi
	movl net_h2c_size(%rip), %esi
	leaq outrec(%rip), %rdx
	call ring_push
9:	ret

	.globl net_recv
# net_recv -> eax 1 with a frame in `frame`, rdx = its length; 0 when
# none. The card-to-host ring is drained into `inbound` and cut into
# records; a record that cannot be the card's drops the buffer.
net_recv:
	pushq %rbx
1:	movq inbound_len(%rip), %rcx
	cmpq $REC_HDR, %rcx
	jb 3f
	leaq inbound(%rip), %rsi
	movzwl (%rsi), %eax
	testl %eax, %eax
	jz 2f
	cmpl $MAX_ETH_FRAME, %eax
	ja 2f
	leaq REC_HDR(%rax), %rdx
	addq $3, %rdx
	andq $-4, %rdx
	cmpq %rdx, %rcx
	jb 3f
	movq %rax, %rbx
	leaq frame(%rip), %rdi
	leaq REC_HDR(%rsi), %rsi
	movq %rbx, %rcx
	rep movsb
	leaq inbound(%rip), %rdi
	leaq (%rdi,%rdx), %rsi
	movq inbound_len(%rip), %rcx
	subq %rdx, %rcx
	movq %rcx, inbound_len(%rip)
	rep movsb
	movq %rbx, %rdx
	movl $1, %eax
	popq %rbx
	ret
2:	movq $0, inbound_len(%rip)
3:	movq net_c2h(%rip), %rdi
	movl net_c2h_size(%rip), %esi
	call ring_avail
	testl %eax, %eax
	jz 9f
	movq $2 * READ_CHUNK, %rcx
	subq inbound_len(%rip), %rcx
	jz 9f
	movq net_c2h(%rip), %rdi
	movl net_c2h_size(%rip), %esi
	leaq inbound(%rip), %rdx
	addq inbound_len(%rip), %rdx
	call ring_pop
	testq %rax, %rax
	jz 9f
	addq %rax, inbound_len(%rip)
	jmp 1b
9:	xorl %eax, %eax
	popq %rbx
	ret

# ------------------------------------------------------------ headers

# bswap16(eax) -> eax.
	.macro BSWAP16 r
	rolw $8, \r
	.endm

# eth_header(rdi = buffer, esi = ethertype): the card's MAC as the
# destination (broadcast before ARP answered), ours as the source.
eth_header:
	cmpq $0, have_card_mac(%rip)
	je 1f
	movq card_mac(%rip), %rax
	movl %eax, (%rdi)
	shrq $32, %rax
	movw %ax, 4(%rdi)
	jmp 2f
1:	movl $0xffffffff, (%rdi)
	movw $0xffff, 4(%rdi)
2:	leaq host_mac(%rip), %rax
	movl (%rax), %ecx
	movl %ecx, 6(%rdi)
	movw 4(%rax), %cx
	movw %cx, 10(%rdi)
	movw %si, %ax
	BSWAP16 %ax
	movw %ax, 12(%rdi)
	ret

# checksum(rsi = data, rdx = len, eax = initial sum) -> eax: the
# one's-complement sum folded, not yet inverted.
checksum:
	movq %rdx, %rcx
	shrq $1, %rcx
	andl $1, %edx
	movl %edx, %r8d
1:	testq %rcx, %rcx
	jz 2f
	movzwl (%rsi), %edx
	addl %edx, %eax
	addq $2, %rsi
	decq %rcx
	jmp 1b
2:	testl %r8d, %r8d
	jz 3f
	movzbl (%rsi), %edx
	addl %edx, %eax
3:	movl %eax, %edx
	shrl $16, %edx
	andl $0xffff, %eax
	addl %edx, %eax
	movl %eax, %edx
	shrl $16, %edx
	addl %edx, %eax
	andl $0xffff, %eax
	ret

# arp_request: who has the card's address, broadcast.
arp_request:
	leaq seg(%rip), %rdi
	movl $ETH_ARP, %esi
	call eth_header
	leaq seg+14(%rip), %rdi
	movw $0x0100, 0(%rdi)		# hardware type 1
	movw $0x0008, 2(%rdi)		# protocol 0x0800
	movb $6, 4(%rdi)
	movb $4, 5(%rdi)
	movw $0x0100, 6(%rdi)		# request
	leaq host_mac(%rip), %rax
	movl (%rax), %ecx
	movl %ecx, 8(%rdi)
	movw 4(%rax), %cx
	movw %cx, 12(%rdi)
	movl host_ip(%rip), %eax
	movl %eax, 14(%rdi)
	movl $0, 18(%rdi)
	movw $0, 22(%rdi)
	movl card_ip(%rip), %eax
	movl %eax, 24(%rdi)
	leaq seg(%rip), %rsi
	movl $42, %edx
	call net_send
	call now_ms
	movq %rax, arp_sent_ms(%rip)
	ret

# arp_in(rsi = frame, rdx = len): a reply teaches the card's MAC; a
# request for our address is answered.
arp_in:
	cmpq $42, %rdx
	jb 9f
	leaq 14(%rsi), %rdi
	movl 14(%rdi), %eax		# sender IP
	cmpl card_ip(%rip), %eax
	jne 1f
	movl 8(%rdi), %eax
	movzwl 12(%rdi), %ecx
	shlq $32, %rcx
	orq %rcx, %rax
	movq %rax, card_mac(%rip)
	movq $1, have_card_mac(%rip)
1:	cmpw $0x0100, 6(%rdi)
	jne 9f
	movl 24(%rdi), %eax		# target IP
	cmpl host_ip(%rip), %eax
	jne 9f
	# reply
	pushq %rdi
	leaq seg(%rip), %rdi
	movl $ETH_ARP, %esi
	call eth_header
	popq %rsi
	leaq seg(%rip), %rdi
	movl 8(%rsi), %eax
	movl %eax, (%rdi)		# destination: the requester
	movw 12(%rsi), %ax
	movw %ax, 4(%rdi)
	leaq 14(%rdi), %rdi
	movw $0x0100, 0(%rdi)
	movw $0x0008, 2(%rdi)
	movb $6, 4(%rdi)
	movb $4, 5(%rdi)
	movw $0x0200, 6(%rdi)		# reply
	leaq host_mac(%rip), %rax
	movl (%rax), %ecx
	movl %ecx, 8(%rdi)
	movw 4(%rax), %cx
	movw %cx, 12(%rdi)
	movl host_ip(%rip), %eax
	movl %eax, 14(%rdi)
	movl 8(%rsi), %eax
	movl %eax, 18(%rdi)
	movw 12(%rsi), %ax
	movw %ax, 22(%rdi)
	movl 14(%rsi), %eax
	movl %eax, 24(%rdi)
	leaq seg(%rip), %rsi
	movl $42, %edx
	call net_send
9:	ret

# tcp_send_segment(rbx = connection, esi = flags, rdx = payload pointer
# (0 none), ecx = payload length, r8d = sequence number): one segment
# built in `seg` with Ethernet and IP headers and sent.
tcp_send_segment:
	pushq %r12
	pushq %r13
	pushq %r14
	pushq %r15
	movl %esi, %r12d
	movq %rdx, %r13
	movl %ecx, %r14d
	movl %r8d, %r15d
	cmpq $0, have_card_mac(%rip)
	je 9f
	leaq seg(%rip), %rdi
	movl $ETH_IP, %esi
	call eth_header
	# IP header at 14
	leaq seg+14(%rip), %rdi
	movb $0x45, 0(%rdi)
	movb $0, 1(%rdi)
	movw $0, 2(%rdi)		# the total length, once the TCP header is known
	movq ip_id(%rip), %rax
	incq ip_id(%rip)
	BSWAP16 %ax
	movw %ax, 4(%rdi)
	movw $0x0040, 6(%rdi)		# DF
	movb $64, 8(%rdi)
	movb $IPPROTO_TCP, 9(%rdi)
	movw $0, 10(%rdi)
	movl host_ip(%rip), %eax
	movl %eax, 12(%rdi)
	movl card_ip(%rip), %eax
	movl %eax, 16(%rdi)
	# TCP header at 34
	leaq seg+34(%rip), %rdi
	movq C_LPORT(%rbx), %rax
	BSWAP16 %ax
	movw %ax, 0(%rdi)
	movq C_RPORT(%rbx), %rax
	BSWAP16 %ax
	movw %ax, 2(%rdi)
	movl %r15d, %eax
	bswapl %eax
	movl %eax, 4(%rdi)
	movq C_RCV_NXT(%rbx), %rax
	bswapl %eax
	movl %eax, 8(%rdi)
	movb $0x50, 12(%rdi)
	movb %r12b, 13(%rdi)
	# window: free space in the receive buffer
	movq C_RB_TAIL(%rbx), %rax
	subq C_RB_HEAD(%rbx), %rax
	movl $RCVCAP, %ecx
	subq %rax, %rcx
	cmpq $65535, %rcx
	jbe 1f
	movl $65535, %ecx
1:	movq %rcx, C_ADV_WND(%rbx)
	movw %cx, %ax
	BSWAP16 %ax
	movw %ax, 14(%rdi)
	movw $0, 16(%rdi)
	movw $0, 18(%rdi)
	# a SYN carries the MSS option
	testl $TCP_SYN, %r12d
	jz 2f
	movb $0x60, 12(%rdi)
	movb $2, 20(%rdi)
	movb $4, 21(%rdi)
	movw $MSS, %ax
	BSWAP16 %ax
	movw %ax, 22(%rdi)
2:	# payload after the header
	movzbl 12(%rdi), %eax
	shrl $4, %eax
	shll $2, %eax
	movl %eax, %r12d		# header length
	testq %r13, %r13
	jz 3f
	leaq (%rdi,%r12), %rdi
	movq %r13, %rsi
	movl %r14d, %ecx
	rep movsb
3:	leal (%r12,%r14), %edx
	movl %edx, %r13d		# TCP length
	# the IP total length and checksum
	leal 20(%r13), %eax
	BSWAP16 %ax
	movw %ax, seg+14+2(%rip)
	leaq seg+14(%rip), %rsi
	movl $20, %edx
	xorl %eax, %eax
	call checksum
	notl %eax
	movw %ax, seg+14+10(%rip)
	# the TCP checksum over the pseudo header, the header and the payload
	movl host_ip(%rip), %eax
	movl %eax, %ecx
	shrl $16, %ecx
	andl $0xffff, %eax
	addl %ecx, %eax
	movl card_ip(%rip), %ecx
	movl %ecx, %esi
	shrl $16, %esi
	andl $0xffff, %ecx
	addl %ecx, %eax
	addl %esi, %eax
	addl $IPPROTO_TCP << 8, %eax
	movl %r13d, %ecx
	BSWAP16 %cx
	movzwl %cx, %ecx
	addl %ecx, %eax
	leaq seg+34(%rip), %rsi
	movl %r13d, %edx
	call checksum
	notl %eax
	movw %ax, seg+34+16(%rip)
	leaq seg(%rip), %rsi
	leal 34(%r13), %edx
	call net_send
9:	popq %r15
	popq %r14
	popq %r13
	popq %r12
	ret

# ------------------------------------------------------------ connections

	.globl conn_at
# conn_at(edi = index) -> rax.
conn_at:
	movl %edi, %eax
	imull $C_SIZE, %eax, %eax
	leaq conns(%rip), %rcx
	addq %rcx, %rax
	ret

# snd_buf(rbx) -> rax = the connection's send buffer; rcv_buf likewise.
snd_buf:
	movq %rbx, %rax
	leaq conns(%rip), %rcx
	subq %rcx, %rax
	xorl %edx, %edx
	movl $C_SIZE, %ecx
	divq %rcx
	shlq $16, %rax
	leaq snd_bufs(%rip), %rcx
	addq %rcx, %rax
	ret
rcv_buf:
	movq %rbx, %rax
	leaq conns(%rip), %rcx
	subq %rcx, %rax
	xorl %edx, %edx
	movl $C_SIZE, %ecx
	divq %rcx
	shlq $16, %rax
	leaq rcv_bufs(%rip), %rcx
	addq %rcx, %rax
	ret

	.globl tcp_connect
# tcp_connect(edi = host socket fd, esi = card port) -> rax = the
# connection, or 0 when the table is full: a SYN goes out (after ARP,
# when the card's MAC is not yet known).
tcp_connect:
	pushq %rbx
	pushq %r12
	pushq %r13
	movl %edi, %r12d
	movl %esi, %r13d
	xorl %ecx, %ecx
1:	cmpl $NCONN, %ecx
	jae 8f
	movl %ecx, %edi
	pushq %rcx
	call conn_at
	popq %rcx
	cmpq $ST_FREE, C_STATE(%rax)
	je 2f
	incl %ecx
	jmp 1b
2:	movq %rax, %rbx
	movq %r12, C_FD(%rbx)
	movq %r13, C_RPORT(%rbx)
	movq next_port(%rip), %rax
	movq %rax, C_LPORT(%rbx)
	incq %rax
	cmpq $65535, %rax
	jbe 3f
	movl $49152, %eax
3:	movq %rax, next_port(%rip)
	# the initial sequence number from the clock
	call now_us
	movl %eax, %ecx
	shll $7, %ecx
	xorl $0x5a5a1234, %ecx
	movq %rcx, C_SND_UNA(%rbx)
	leaq 1(%rcx), %rax
	movl %eax, %eax
	movq %rax, C_SND_NXT(%rbx)
	movq $0, C_RCV_NXT(%rbx)
	movq $MSS, C_SND_WND(%rbx)
	movq $0, C_SB_HEAD(%rbx)
	movq $0, C_SB_TAIL(%rbx)
	movq $0, C_RB_HEAD(%rbx)
	movq $0, C_RB_TAIL(%rbx)
	movq $0, C_RETRIES(%rbx)
	movq $0, C_HOST_EOF(%rbx)
	movq $0, C_CARD_EOF(%rbx)
	movq $0, C_FIN_SEQ(%rbx)
	movq $0, C_HOST_SHUT(%rbx)
	movq $RTO_MS, C_RTO(%rbx)
	movq $ST_SYN_SENT, C_STATE(%rbx)
	call send_syn
	movq %rbx, %rax
	jmp 9f
8:	xorl %eax, %eax
9:	popq %r13
	popq %r12
	popq %rbx
	ret

# send_syn(rbx): the SYN with the retransmission timer armed.
send_syn:
	movl $TCP_SYN, %esi
	xorl %edx, %edx
	xorl %ecx, %ecx
	movq C_SND_UNA(%rbx), %r8
	call tcp_send_segment
	jmp arm_rto

# arm_rto(rbx): the deadline C_RTO from now.
arm_rto:
	call now_ms
	addq C_RTO(%rbx), %rax
	movq %rax, C_RTO_AT(%rbx)
	ret

# conn_close(rbx): the host socket closed, the slot freed.
conn_close:
	movq C_FD(%rbx), %rdi
	testq %rdi, %rdi
	js 1f
	SYS SYS_close
	movq $-1, C_FD(%rbx)
1:	movq $ST_FREE, C_STATE(%rbx)
	ret

	.globl tcp_step
# tcp_step -> eax 1 when anything moved: frames from the card handled,
# ARP kept alive, each connection's host socket read and written, data
# sent, retransmissions, closes.
tcp_step:
	pushq %rbx
	pushq %r12
	pushq %r13
	xorl %r13d, %r13d
1:	call net_recv
	testl %eax, %eax
	jz 3f
	movl $1, %r13d
	leaq frame(%rip), %rsi
	pushq %rdx
	movl $'i', %edi
	call net_trace
	leaq frame(%rip), %rsi
	popq %rdx
	cmpq $14, %rdx
	jb 1b
	movzwl 12(%rsi), %eax
	BSWAP16 %ax
	cmpw $ETH_ARP, %ax
	jne 2f
	call arp_in
	jmp 1b
2:	cmpw $ETH_IP, %ax
	jne 1b
	call ip_in
	jmp 1b
3:	# ARP: ask while unknown and any connection waits
	cmpq $0, have_card_mac(%rip)
	jne 4f
	call any_conn
	testl %eax, %eax
	jz 4f
	call now_ms
	subq arp_sent_ms(%rip), %rax
	cmpq $ARP_RETRY_MS, %rax
	jb 4f
	call arp_request
4:	xorl %r12d, %r12d
5:	cmpl $NCONN, %r12d
	jae 9f
	movl %r12d, %edi
	call conn_at
	movq %rax, %rbx
	cmpq $ST_FREE, C_STATE(%rbx)
	je 6f
	call conn_step
	orl %eax, %r13d
6:	incl %r12d
	jmp 5b
9:	movl %r13d, %eax
	popq %r13
	popq %r12
	popq %rbx
	ret

# any_conn -> eax 1 when a connection exists.
any_conn:
	xorl %ecx, %ecx
1:	cmpl $NCONN, %ecx
	jae 8f
	movl %ecx, %eax
	imull $C_SIZE, %eax, %eax
	leaq conns(%rip), %rdx
	cmpq $ST_FREE, C_STATE(%rdx,%rax)
	jne 9f
	incl %ecx
	jmp 1b
8:	xorl %eax, %eax
	ret
9:	movl $1, %eax
	ret

# ip_in(rsi = frame, rdx = len): an IPv4 packet for us carrying TCP.
ip_in:
	cmpq $54, %rdx
	jb 9f
	leaq 14(%rsi), %rdi
	movzbl (%rdi), %eax
	cmpb $0x45, %al
	jne 9f
	cmpb $IPPROTO_TCP, 9(%rdi)
	jne 9f
	movl 16(%rdi), %eax
	cmpl host_ip(%rip), %eax
	jne 9f
	movl 12(%rdi), %eax
	cmpl card_ip(%rip), %eax
	jne 9f
	movzwl 2(%rdi), %eax
	BSWAP16 %ax
	movzwl %ax, %eax		# total length
	cmpq $40, %rax
	jb 9f
	leaq -14(%rdx), %rcx
	cmpq %rcx, %rax
	ja 9f
	leaq 20(%rdi), %rsi		# the TCP header
	leaq -20(%rax), %rdx		# TCP length
	jmp tcp_in
9:	ret

# tcp_in(rsi = header, rdx = TCP length): the connection found by ports,
# the segment applied.
tcp_in:
	pushq %rbx
	pushq %r12
	pushq %r13
	pushq %r14
	pushq %r15
	movq %rsi, %r12
	movq %rdx, %r13
	movzwl 2(%r12), %eax
	BSWAP16 %ax
	movzwl %ax, %r14d		# destination port (ours)
	movzwl 0(%r12), %eax
	BSWAP16 %ax
	movzwl %ax, %r15d		# source port (theirs)
	xorl %ecx, %ecx
1:	cmpl $NCONN, %ecx
	jae 90f
	movl %ecx, %eax
	imull $C_SIZE, %eax, %eax
	leaq conns(%rip), %rbx
	addq %rax, %rbx
	cmpq $ST_FREE, C_STATE(%rbx)
	je 2f
	cmpq %r14, C_LPORT(%rbx)
	jne 2f
	cmpq %r15, C_RPORT(%rbx)
	je 3f
2:	incl %ecx
	jmp 1b
3:	movzbl 12(%r12), %eax
	shrl $4, %eax
	shll $2, %eax
	movq %rax, %r14			# header length
	cmpq %r13, %r14
	ja 90f
	movl 4(%r12), %eax
	bswapl %eax
	movq %rax, %r15			# seg.seq
	movzbl 13(%r12), %eax
	movl %eax, %ecx			# flags
	testl $TCP_RST, %ecx
	jnz 80f
	cmpq $ST_SYN_SENT, C_STATE(%rbx)
	jne 10f
	# SYN-ACK expected
	testl $TCP_SYN, %ecx
	jz 90f
	testl $TCP_ACK, %ecx
	jz 90f
	movl 8(%r12), %eax
	bswapl %eax
	cmpq C_SND_NXT(%rbx), %rax
	jne 90f
	leaq 1(%r15), %rax
	movl %eax, %eax
	movq %rax, C_RCV_NXT(%rbx)
	movq C_SND_NXT(%rbx), %rax
	movq %rax, C_SND_UNA(%rbx)
	movzwl 14(%r12), %eax
	BSWAP16 %ax
	movzwl %ax, %eax
	movq %rax, C_SND_WND(%rbx)
	movq $ST_ESTABLISHED, C_STATE(%rbx)
	movq $0, C_RETRIES(%rbx)
	movq $RTO_MS, C_RTO(%rbx)
	movl $TCP_ACK, %esi
	xorl %edx, %edx
	xorl %ecx, %ecx
	movq C_SND_NXT(%rbx), %r8
	call tcp_send_segment
	jmp 99f
10:	# established or closing: the ACK first
	testl $TCP_ACK, %ecx
	jz 20f
	movl 8(%r12), %eax
	bswapl %eax
	movq %rax, %rdx			# seg.ack
	movzwl 14(%r12), %eax
	BSWAP16 %ax
	movzwl %ax, %eax
	movq %rax, C_SND_WND(%rbx)
	# acked = seg.ack - snd_una, valid when <= snd_nxt - snd_una
	movl %edx, %eax
	subl C_SND_UNA(%rbx), %eax
	movl C_SND_NXT(%rbx), %esi
	subl C_SND_UNA(%rbx), %esi
	cmpl %esi, %eax
	ja 20f
	testl %eax, %eax
	jz 20f
	# new data acknowledged: the send buffer's head advances (a FIN
	# counts one)
	movq %rdx, C_SND_UNA(%rbx)
	movq C_FIN_SEQ(%rbx), %rsi
	testq %rsi, %rsi
	jz 11f
	cmpl %edx, %esi
	jae 11f			# the FIN itself not yet acked
	# fin acked when seg.ack == fin_seq + 1
	decl %eax
11:	addq %rax, C_SB_HEAD(%rbx)
	movq $0, C_RETRIES(%rbx)
	movq $RTO_MS, C_RTO(%rbx)
	call arm_rto
	# state changes on the FIN's acknowledgement
	movq C_FIN_SEQ(%rbx), %rsi
	testq %rsi, %rsi
	jz 20f
	incl %esi
	cmpl %esi, %edx
	jne 20f
	cmpq $ST_FIN_WAIT_1, C_STATE(%rbx)
	jne 12f
	movq $ST_FIN_WAIT_2, C_STATE(%rbx)
	jmp 20f
12:	cmpq $ST_CLOSING, C_STATE(%rbx)
	jne 13f
	movq $ST_CLOSED, C_STATE(%rbx)
	jmp 20f
13:	cmpq $ST_LAST_ACK, C_STATE(%rbx)
	jne 20f
	movq $ST_CLOSED, C_STATE(%rbx)
20:	# payload in order: appended to the receive buffer while it fits
	movq %r13, %rcx
	subq %r14, %rcx			# payload length
	movl %r15d, %eax
	cmpl C_RCV_NXT(%rbx), %eax
	jne 30f
	testq %rcx, %rcx
	jz 25f
	movq C_RB_TAIL(%rbx), %rax
	subq C_RB_HEAD(%rbx), %rax
	movl $RCVCAP, %edx
	subq %rax, %rdx			# free
	cmpq %rdx, %rcx
	jbe 21f
	movq %rdx, %rcx
21:	testq %rcx, %rcx
	jz 25f
	pushq %rcx
	call rcv_buf
	popq %rcx
	# copy in two pieces around the end
	movq C_RB_TAIL(%rbx), %rdx
	andq $RCVCAP - 1, %rdx
	leaq (%rax,%rdx), %rdi
	movl $RCVCAP, %r8d
	subq %rdx, %r8
	cmpq %rcx, %r8
	jbe 22f
	movq %rcx, %r8
22:	leaq (%r12,%r14), %rsi
	pushq %rcx
	pushq %r8
	movq %r8, %rcx
	rep movsb
	popq %r8
	popq %rcx
	subq %r8, %rcx
	jz 23f
	movq %rax, %rdi
	movq %rcx, %rdx
	rep movsb
	addq %rdx, %r8
23:	addq %r8, C_RB_TAIL(%rbx)
	addq %r8, C_RCV_NXT(%rbx)
	movl C_RCV_NXT(%rbx), %eax
	movq %rax, C_RCV_NXT(%rbx)
	movl $1, %ecx			# something to ACK
	jmp 26f
25:	xorl %ecx, %ecx
26:	# a FIN in order (after every byte it carried)
	movzbl 13(%r12), %eax
	testl $TCP_FIN, %eax
	jz 27f
	movq %r13, %rdx
	subq %r14, %rdx
	movl %r15d, %eax
	addl %edx, %eax
	cmpl C_RCV_NXT(%rbx), %eax
	jne 27f
	incq C_RCV_NXT(%rbx)
	movl C_RCV_NXT(%rbx), %eax
	movq %rax, C_RCV_NXT(%rbx)
	movq $1, C_CARD_EOF(%rbx)
	movl $1, %ecx
	cmpq $ST_ESTABLISHED, C_STATE(%rbx)
	jne 261f
	movq $ST_CLOSE_WAIT, C_STATE(%rbx)
	jmp 27f
261:	cmpq $ST_FIN_WAIT_1, C_STATE(%rbx)
	jne 262f
	movq $ST_CLOSING, C_STATE(%rbx)
	jmp 27f
262:	cmpq $ST_FIN_WAIT_2, C_STATE(%rbx)
	jne 27f
	movq $ST_CLOSED, C_STATE(%rbx)
27:	testl %ecx, %ecx
	jz 99f
	movl $TCP_ACK, %esi
	xorl %edx, %edx
	xorl %ecx, %ecx
	movq C_SND_NXT(%rbx), %r8
	call tcp_send_segment
	jmp 99f
30:	# out of order: an ACK restates what is expected
	movl $TCP_ACK, %esi
	xorl %edx, %edx
	xorl %ecx, %ecx
	movq C_SND_NXT(%rbx), %r8
	call tcp_send_segment
	jmp 99f
80:	# reset from the card
	movq $ST_CLOSED, C_STATE(%rbx)
	movq $1, C_CARD_EOF(%rbx)
	jmp 99f
90:	# no connection, or a segment that makes no sense here: dropped
99:	popq %r15
	popq %r14
	popq %r13
	popq %r12
	popq %rbx
	ret

# conn_step(rbx) -> eax 1 when it moved bytes: the receive buffer to the
# host socket, the host socket to the send buffer, segments out, the
# retransmission timer, the closes.
conn_step:
	pushq %r12
	pushq %r13
	pushq %r14
	xorl %r14d, %r14d
	cmpq $ST_CLOSED, C_STATE(%rbx)
	je 50f
	# 1. received bytes to the host
	movq C_RB_TAIL(%rbx), %r9
	subq C_RB_HEAD(%rbx), %r9	# available (rcv_buf clobbers rcx and rdx)
	jz 5f
	call rcv_buf
	movq C_RB_HEAD(%rbx), %rdx
	andq $RCVCAP - 1, %rdx
	leaq (%rax,%rdx), %rsi
	movl $RCVCAP, %r8d
	subq %rdx, %r8
	cmpq %r9, %r8
	jbe 1f
	movq %r9, %r8
1:	movl C_FD(%rbx), %edi
	movq %r8, %rdx
	SYS SYS_write
	testq %rax, %rax
	js 2f
	addq %rax, C_RB_HEAD(%rbx)
	movl $1, %r14d
	jmp 4f
2:	cmpq $-EAGAIN, %rax
	je 5f
	cmpq $-EINTR, %rax
	je 5f
	# the host side is gone: abort
	movl $TCP_RST, %esi
	xorl %edx, %edx
	xorl %ecx, %ecx
	movq C_SND_NXT(%rbx), %r8
	call tcp_send_segment
	movq $ST_CLOSED, C_STATE(%rbx)
	jmp 50f
4:	movq C_RB_TAIL(%rbx), %rax
	cmpq C_RB_HEAD(%rbx), %rax
	jne 41f
	movq $0, C_RB_HEAD(%rbx)
	movq $0, C_RB_TAIL(%rbx)
41:	# a window update when the peer was throttled and half the buffer
	# is free again (there is no persist timer on this side; Linux
	# probes a zero window only every RTO, doubling)
	cmpq $RCVCAP / 2, C_ADV_WND(%rbx)
	jae 5f
	movq C_RB_TAIL(%rbx), %rax
	subq C_RB_HEAD(%rbx), %rax
	cmpq $RCVCAP / 2, %rax
	ja 5f
	cmpq $ST_ESTABLISHED, C_STATE(%rbx)
	je 42f
	cmpq $ST_FIN_WAIT_1, C_STATE(%rbx)
	je 42f
	cmpq $ST_FIN_WAIT_2, C_STATE(%rbx)
	jne 5f
42:	movl $TCP_ACK, %esi
	xorl %edx, %edx
	xorl %ecx, %ecx
	movq C_SND_NXT(%rbx), %r8
	call tcp_send_segment
5:	# the card ended and everything it sent is delivered: our end of
	# the host socket's output
	cmpq $0, C_CARD_EOF(%rbx)
	je 6f
	cmpq $0, C_HOST_SHUT(%rbx)
	jne 6f
	movq C_RB_TAIL(%rbx), %rax
	cmpq C_RB_HEAD(%rbx), %rax
	jne 6f
	movl C_FD(%rbx), %edi
	movl $SHUT_WR, %esi
	SYS SYS_shutdown
	movq $1, C_HOST_SHUT(%rbx)
6:	# 2. the host's bytes into the send buffer (established only)
	cmpq $0, C_HOST_EOF(%rbx)
	jne 10f
	cmpq $ST_ESTABLISHED, C_STATE(%rbx)
	je 7f
	cmpq $ST_CLOSE_WAIT, C_STATE(%rbx)
	jne 10f
7:	movq C_SB_TAIL(%rbx), %rcx
	subq C_SB_HEAD(%rbx), %rcx
	movl $SNDCAP, %r9d
	subq %rcx, %r9			# free
	jz 10f
	call snd_buf
	movq C_SB_TAIL(%rbx), %rcx
	andq $SNDCAP - 1, %rcx
	leaq (%rax,%rcx), %rsi
	movl $SNDCAP, %r8d
	subq %rcx, %r8
	cmpq %r9, %r8
	jbe 8f
	movq %r9, %r8
8:	movl C_FD(%rbx), %edi
	movq %r8, %rdx
	SYS SYS_read
	testq %rax, %rax
	jz 9f
	js 81f
	addq %rax, C_SB_TAIL(%rbx)
	movl $1, %r14d
	jmp 10f
81:	cmpq $-EAGAIN, %rax
	je 10f
	cmpq $-EINTR, %rax
	je 10f
9:	movq $1, C_HOST_EOF(%rbx)
10:	# 3. send: unsent bytes within the window and the quota
	cmpq $ST_ESTABLISHED, C_STATE(%rbx)
	je 11f
	cmpq $ST_CLOSE_WAIT, C_STATE(%rbx)
	jne 20f
11:	movl $SEND_QUOTA, %r13d
12:	movq C_SB_TAIL(%rbx), %rcx
	movq C_SB_HEAD(%rbx), %rax
	movl C_SND_NXT(%rbx), %edx
	subl C_SND_UNA(%rbx), %edx	# in flight
	addq %rdx, %rax			# index of the next unsent byte
	subq %rax, %rcx			# unsent
	jz 20f
	movq C_SND_WND(%rbx), %r8
	subq %rdx, %r8
	jle 20f
	cmpq %r8, %rcx
	jbe 13f
	movq %r8, %rcx
13:	cmpq $MSS, %rcx
	jbe 14f
	movl $MSS, %ecx
14:	# the piece must be contiguous in the ring
	pushq %rax
	pushq %rcx
	call snd_buf
	popq %rcx
	popq %rdx
	andq $SNDCAP - 1, %rdx
	leaq (%rax,%rdx), %r12
	movl $SNDCAP, %r8d
	subq %rdx, %r8
	cmpq %rcx, %r8
	jae 15f
	movq %r8, %rcx
15:	movl $TCP_ACK | TCP_PSH, %esi
	movq %r12, %rdx
	movq C_SND_NXT(%rbx), %r8
	pushq %rcx
	call tcp_send_segment
	popq %rcx
	addq %rcx, C_SND_NXT(%rbx)
	movl C_SND_NXT(%rbx), %eax
	movq %rax, C_SND_NXT(%rbx)
	movl $1, %r14d
	movq C_SND_NXT(%rbx), %rax
	subq %rcx, %rax
	cmpl C_SND_UNA(%rbx), %eax
	jne 16f
	call arm_rto		# the first byte in flight starts the timer
16:	decl %r13d
	jnz 12b
20:	# 4. our FIN once the host ended and everything is sent
	cmpq $0, C_HOST_EOF(%rbx)
	je 30f
	cmpq $0, C_FIN_SEQ(%rbx)
	jne 30f
	movq C_SB_TAIL(%rbx), %rcx
	movq C_SB_HEAD(%rbx), %rax
	movl C_SND_NXT(%rbx), %edx
	subl C_SND_UNA(%rbx), %edx
	addq %rdx, %rax
	cmpq %rax, %rcx
	jne 30f
	cmpq $ST_ESTABLISHED, C_STATE(%rbx)
	je 21f
	cmpq $ST_CLOSE_WAIT, C_STATE(%rbx)
	jne 30f
	movq $ST_LAST_ACK, C_STATE(%rbx)
	jmp 22f
21:	movq $ST_FIN_WAIT_1, C_STATE(%rbx)
22:	movq C_SND_NXT(%rbx), %rax
	movq %rax, C_FIN_SEQ(%rbx)
	movl $TCP_FIN | TCP_ACK, %esi
	xorl %edx, %edx
	xorl %ecx, %ecx
	movq %rax, %r8
	call tcp_send_segment
	incq C_SND_NXT(%rbx)
	movl C_SND_NXT(%rbx), %eax
	movq %rax, C_SND_NXT(%rbx)
	call arm_rto
30:	# 5. the retransmission timer
	movl C_SND_NXT(%rbx), %eax
	cmpl C_SND_UNA(%rbx), %eax
	je 50f
	call now_ms
	cmpq C_RTO_AT(%rbx), %rax
	jb 50f
	incq C_RETRIES(%rbx)
	cmpq $MAX_RETRIES, C_RETRIES(%rbx)
	ja 40f
	movq C_RTO(%rbx), %rax
	addq %rax, %rax
	cmpq $RTO_MAX_MS, %rax
	jbe 31f
	movl $RTO_MAX_MS, %eax
31:	movq %rax, C_RTO(%rbx)
	cmpq $ST_SYN_SENT, C_STATE(%rbx)
	jne 32f
	call send_syn
	jmp 50f
32:	# resend from snd_una: one segment of data, or the FIN
	movq C_SB_TAIL(%rbx), %r9
	subq C_SB_HEAD(%rbx), %r9
	jz 33f
	cmpq $MSS, %r9
	jbe 321f
	movl $MSS, %r9d
321:	call snd_buf
	movq %r9, %rcx
	movq C_SB_HEAD(%rbx), %rdx
	andq $SNDCAP - 1, %rdx
	leaq (%rax,%rdx), %r12
	movl $SNDCAP, %r8d
	subq %rdx, %r8
	cmpq %rcx, %r8
	jae 322f
	movq %r8, %rcx
322:	movl $TCP_ACK | TCP_PSH, %esi
	movq %r12, %rdx
	movq C_SND_UNA(%rbx), %r8
	call tcp_send_segment
	call arm_rto
	jmp 50f
33:	cmpq $0, C_FIN_SEQ(%rbx)
	je 50f
	movl $TCP_FIN | TCP_ACK, %esi
	xorl %edx, %edx
	xorl %ecx, %ecx
	movq C_FIN_SEQ(%rbx), %r8
	call tcp_send_segment
	call arm_rto
	jmp 50f
40:	# given up
	call l_begin
	call l_tag
	leaq s_gave_up(%rip), %rsi
	call w_cstr
	call l_end
	movq $ST_CLOSED, C_STATE(%rbx)
50:	# 6. a closed connection ends the host socket
	cmpq $ST_CLOSED, C_STATE(%rbx)
	jne 59f
	cmpq $0, C_CARD_EOF(%rbx)
	jne 51f
	cmpq $0, C_HOST_EOF(%rbx)
	jne 51f
	call l_begin
	call l_tag
	leaq s_ended(%rip), %rsi
	call w_cstr
	call l_end
51:	movl C_FD(%rbx), %edi
	movl $SHUT_RDWR, %esi
	SYS SYS_shutdown
	call conn_close
	movl $1, %r14d
59:	movl %r14d, %eax
	popq %r14
	popq %r13
	popq %r12
	ret
STR s_gave_up, "forward: connection to the card gave up after 12 retransmissions"
STR s_ended, "forward: connection to the card ended without a close"

	.globl net_trace
# net_trace(rsi = frame, rdx = len, edi = 'i' or 'o'): with
# PHICTL_NET_TRACE set, one line per frame: direction, length, the
# ethertype, and for IP the protocol, ports and flags.
	.bss
trace_on:	.skip 8
trace_count:	.skip 8
	.text
net_trace:
	cmpq $0, trace_on(%rip)
	je 1f
	cmpq $1, trace_on(%rip)
	je 2f
	ret
1:	pushq %rdi
	pushq %rsi
	pushq %rdx
	leaq s_env_trace(%rip), %rdi
	call getenv
	movq $2, trace_on(%rip)
	testq %rax, %rax
	jz 3f
	movq $1, trace_on(%rip)
3:	popq %rdx
	popq %rsi
	popq %rdi
	cmpq $1, trace_on(%rip)
	jne 9f
2:	cmpq $400, trace_count(%rip)
	jae 9f
	incq trace_count(%rip)
	pushq %rbx
	pushq %r12
	pushq %r13
	movq %rsi, %rbx
	movq %rdx, %r12
	movl %edi, %r13d
	call l_begin
	call l_tag
	leaq s_net(%rip), %rsi
	call w_cstr
	movl %r13d, %edi
	call w_byte
	movb $' ', %dil
	call w_byte
	movq %r12, %rdi
	call w_dec
	leaq s_type(%rip), %rsi
	call w_cstr
	movzwl 12(%rbx), %edi
	rolw $8, %di
	movzwl %di, %edi
	call w_hex
	cmpq $54, %r12
	jb 8f
	cmpw $0x0008, 12(%rbx)
	jne 8f
	leaq s_proto(%rip), %rsi
	call w_cstr
	movzbl 23(%rbx), %edi
	call w_dec
	leaq s_ports(%rip), %rsi
	call w_cstr
	movzwl 34(%rbx), %edi
	rolw $8, %di
	movzwl %di, %edi
	call w_dec
	movb $'>', %dil
	call w_byte
	movzwl 36(%rbx), %edi
	rolw $8, %di
	movzwl %di, %edi
	call w_dec
	leaq s_flags(%rip), %rsi
	call w_cstr
	movzbl 47(%rbx), %edi
	call w_hex
	leaq s_seq(%rip), %rsi
	call w_cstr
	movl 38(%rbx), %edi
	bswapl %edi
	call w_hex
	leaq s_ack(%rip), %rsi
	call w_cstr
	movl 42(%rbx), %edi
	bswapl %edi
	call w_hex
8:	call l_end
	popq %r13
	popq %r12
	popq %rbx
9:	ret
STR s_env_trace, "PHICTL_NET_TRACE"
STR s_net, "net "
STR s_type, " type "
STR s_proto, " proto "
STR s_ports, " "
STR s_flags, " flags "
STR s_seq, " seq "
STR s_ack, " ack "
