# forward.S: `phictl boot --forward HOSTPORT:CARDPORT`, root-free TCP
# forwarding to the card (host/crates/phictl/src/forward.rs): phictl is
# the host end of the ring network with its own MAC and address,
# connections accepted on 127.0.0.1:HOSTPORT become connections of the
# stack in tcp.S to the card's CARDPORT, bytes are shuttled both ways.
# See forward.md.
	.include "defs.inc"

	.bss
	.p2align 3
fw_listen:	.skip 8
fw_ready:	.skip 8
fw_host_port:	.skip 8
fw_card_port:	.skip 8
	.p2align 3
fw_addr:	.skip 16

	.text

	.globl parse_ports
# parse_ports(rsi = "HOSTPORT:CARDPORT" or "HOSTPORT"): fw_host_port and
# fw_card_port (22 when absent); exits when malformed.
parse_ports:
	pushq %rbx
	movq %rsi, %rbx
	call cstr_len
	movq %rbx, %rsi
	movb $':', %cl
	call find_byte
	testq %rax, %rax
	js 1f
	movq %rax, %rdx
	movq %rbx, %rsi
	pushq %rax
	call parse_u16
	testl %edx, %edx
	jz 9f
	movq %rax, fw_host_port(%rip)
	popq %rax
	leaq 1(%rbx,%rax), %rsi
	call cstr_len
	call parse_u16
	testl %edx, %edx
	jz 9f
	movq %rax, fw_card_port(%rip)
	popq %rbx
	ret
1:	movq %rbx, %rsi
	call cstr_len
	call parse_u16
	testl %edx, %edx
	jz 9f
	movq %rax, fw_host_port(%rip)
	movq $22, fw_card_port(%rip)
	popq %rbx
	ret
9:	leaq s_e_ports(%rip), %rsi
	jmp fail
STR s_e_ports, "--forward takes HOSTPORT:CARDPORT (or HOSTPORT for port 22)"

	.globl parse_subnet
# parse_subnet(rsi = "A.B.C.D/24"): host_ip, and card_ip = .2 of it.
parse_subnet:
	pushq %rbx
	movq %rsi, %rbx
	call cstr_len
	movq %rbx, %rsi
	movb $'/', %cl
	call find_byte
	testq %rax, %rax
	jns 1f
	movq %rbx, %rsi
	call cstr_len
	movq %rdx, %rax
1:	movq %rbx, %rsi
	movq %rax, %rdx
	call parse_ipv4
	testl %edx, %edx
	jz 9f
	movl %eax, host_ip(%rip)
	andl $0x00ffffff, %eax
	orl $0x02000000, %eax
	movl %eax, card_ip(%rip)
	popq %rbx
	ret
9:	leaq s_e_subnet(%rip), %rsi
	jmp fail
STR s_e_subnet, "--net-addr: not an IPv4 address"

# fw_open -> eax 1 once the card is up and the network channel found:
# the listener on 127.0.0.1:HOSTPORT.
fw_open:
	pushq %rbx
	call wait_init_seen
	testl %eax, %eax
	jz 90f
	cmpq $0, region_ok(%rip)
	je 90f
	movl $KIND_NETWORK, %edi
	call endpoints_of
	testl %eax, %eax
	jz 90f
	movq %r8, net_h2c(%rip)
	movq %r9, net_h2c_size(%rip)
	movq %r10, net_c2h(%rip)
	movq %r11, net_c2h_size(%rip)
	movq o_forward(%rip), %rsi
	call parse_ports
	movq o_net_addr(%rip), %rsi
	call parse_subnet
	call tcp_init
	movl $AF_INET, %edi
	movl $SOCK_STREAM | SOCK_CLOEXEC | SOCK_NONBLOCK, %esi
	xorl %edx, %edx
	SYS SYS_socket
	testq %rax, %rax
	js 91f
	movq %rax, fw_listen(%rip)
	movl %eax, %edi
	movl $SOL_SOCKET, %esi
	movl $SO_REUSEADDR, %edx
	leaq one4(%rip), %r10
	movl $4, %r8d
	SYS SYS_setsockopt
	leaq fw_addr(%rip), %rsi
	movw $AF_INET, (%rsi)
	movq fw_host_port(%rip), %rax
	rolw $8, %ax
	movw %ax, 2(%rsi)
	movl $0x0100007f, 4(%rsi)	# 127.0.0.1
	movq $0, 8(%rsi)
	movl fw_listen(%rip), %edi
	movl $16, %edx
	SYS SYS_bind
	testq %rax, %rax
	js 92f
	movl fw_listen(%rip), %edi
	movl $16, %esi
	SYS SYS_listen
	testq %rax, %rax
	js 92f
	movq $1, fw_ready(%rip)
	call l_begin
	call l_tag
	leaq s_fw1(%rip), %rsi
	call w_cstr
	movq fw_host_port(%rip), %rdi
	call w_dec
	leaq s_fw2(%rip), %rsi
	call w_cstr
	movl card_ip(%rip), %edi
	call w_ip
	movb $':', %dil
	call w_byte
	movq fw_card_port(%rip), %rdi
	call w_dec
	leaq s_fw3(%rip), %rsi
	call w_cstr
	call l_end
	movl $1, %eax
	popq %rbx
	ret
90:	xorl %eax, %eax
	popq %rbx
	ret
91:	negq %rax
	movq %rax, %rdi
	leaq s_e_fwsock(%rip), %rsi
	jmp fail_os
92:	negq %rax
	movq %rax, %rdi
	leaq s_e_fwbind(%rip), %rsi
	jmp fail_os
	.section .rodata
	.p2align 2
one4:	.long 1
	.text
STR s_fw1, "forward: 127.0.0.1:"
STR s_fw2, " -> "
STR s_fw3, " through the ring (no root)"
STR s_e_fwsock, "forward: socket"
STR s_e_fwbind, "forward: listening on 127.0.0.1:HOSTPORT"

	.globl w_ip
# w_ip(edi = address, memory order): "A.B.C.D".
w_ip:
	pushq %rbx
	movl %edi, %ebx
	movzbl %bl, %edi
	call w_dec
	movb $'.', %dil
	call w_byte
	movzbl %bh, %edi
	call w_dec
	movb $'.', %dil
	call w_byte
	movl %ebx, %edi
	shrl $16, %edi
	andl $0xff, %edi
	call w_dec
	movb $'.', %dil
	call w_byte
	movl %ebx, %edi
	shrl $24, %edi
	call w_dec
	popq %rbx
	ret

	.globl forward_fds
# forward_fds(rdi = pollfd cursor) -> rax: the listener for POLLIN once
# it exists, then the connections' host sockets (tcp_fds).
forward_fds:
	cmpq $0, fw_ready(%rip)
	jne 1f
	movq %rdi, %rax
	ret
1:	movl fw_listen(%rip), %eax
	PFD %rdi, %eax, $POLLIN
	jmp tcp_fds

	.globl forward_step
# forward_step -> eax 1 when anything moved: new connections accepted
# and opened toward the card, then the stack's step.
forward_step:
	pushq %rbx
	xorl %ebx, %ebx
	cmpq $0, fw_ready(%rip)
	jne 1f
	call fw_open
	testl %eax, %eax
	jz 9f
1:	movl fw_listen(%rip), %edi
	xorl %esi, %esi
	xorl %edx, %edx
	movl $SOCK_CLOEXEC | SOCK_NONBLOCK, %r10d
	SYS SYS_accept4
	testq %rax, %rax
	js 2f
	movl $1, %ebx
	movl %eax, %edi
	movq fw_card_port(%rip), %rsi
	pushq %rdi
	call tcp_connect
	popq %rdi
	testq %rax, %rax
	jnz 1b
	# the table is full: the connection is refused
	SYS SYS_close
	call l_begin
	call l_tag
	leaq s_fw_full(%rip), %rsi
	call w_cstr
	call l_end
	jmp 1b
2:	call tcp_step
	orl %eax, %ebx
9:	movl %ebx, %eax
	popq %rbx
	ret
STR s_fw_full, "forward: connect to the card failed: no free connection slot"
