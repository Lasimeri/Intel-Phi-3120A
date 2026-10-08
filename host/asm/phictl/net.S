# net.S: `phictl boot --net NAME`, the TAP bridge
# (host/crates/phictl/src/net.rs): a TAP device (root) bridged to the
# ring's network channel, frames as records of u16 length, payload,
# padding to 4. The device gets its address through iproute2, as the Rust
# bridge did. See net.md.
	.include "defs.inc"

	.set MAX_ETH_FRAME, 1518
	.set REC_HDR, 2
	.set TUNSETIFF, 0x400454ca
	.set IFF_TAP, 2
	.set IFF_NO_PI, 0x1000
	.set REPORT_MS, 30000

	.bss
	.p2align 3
tap_fd:	.skip 8
br_ready:	.skip 8
br_to_card:	.skip 8
br_from_card:	.skip 8
br_dropped:	.skip 8
br_bad:	.skip 8
br_reported:	.skip 32
br_next_report:	.skip 8
	.p2align 3
ifr:	.skip 40
tap_pfd:	.skip 8
	.p2align 6
tap_frame:	.skip MAX_ETH_FRAME + 64
tap_record:	.skip MAX_ETH_FRAME + 64
tap_inbound:	.skip 2 * READ_CHUNK
tap_inbound_len:	.skip 8
	.text

# br_open -> eax 1 once the card is up: the TAP created, configured,
# the network channel found.
br_open:
	pushq %rbx
	pushq %r12
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
	leaq s_tun(%rip), %rdi
	movl $O_RDWR | O_CLOEXEC, %esi
	xorl %edx, %edx
	SYS SYS_open
	testq %rax, %rax
	js 91f
	movq %rax, tap_fd(%rip)
	# ifreq: the name, flags IFF_TAP | IFF_NO_PI
	leaq ifr(%rip), %rdi
	xorl %eax, %eax
	movl $40, %ecx
	rep stosb
	movq o_net(%rip), %rsi
	call cstr_len
	cmpq $15, %rdx
	ja 92f
	leaq ifr(%rip), %rdi
	movq %rdx, %rcx
	rep movsb
	movw $IFF_TAP | IFF_NO_PI, ifr+16(%rip)
	movl tap_fd(%rip), %edi
	movl $TUNSETIFF, %esi
	leaq ifr(%rip), %rdx
	SYS SYS_ioctl
	testq %rax, %rax
	js 93f
	# ip addr replace ADDR dev NAME; ip link set NAME up
	leaq ip_args1(%rip), %rdi
	call run_ip
	leaq ip_args2(%rip), %rdi
	call run_ip
	call now_ms
	addq $REPORT_MS, %rax
	movq %rax, br_next_report(%rip)
	movq $1, br_ready(%rip)
	call l_begin
	call l_tag
	leaq s_br1(%rip), %rsi
	call w_cstr
	movq o_net(%rip), %rsi
	call w_cstr
	leaq s_br2(%rip), %rsi
	call w_cstr
	movq o_net_addr(%rip), %rsi
	call w_cstr
	leaq s_br3(%rip), %rsi
	call w_cstr
	call l_end
	movl $1, %eax
	popq %r12
	popq %rbx
	ret
90:	xorl %eax, %eax
	popq %r12
	popq %rbx
	ret
91:	negq %rax
	movq %rax, %rdi
	leaq s_tun(%rip), %rsi
	jmp fail_os
92:	leaq s_e_name(%rip), %rsi
	jmp fail
93:	negq %rax
	movq %rax, %rdi
	leaq s_e_tunsetiff(%rip), %rsi
	jmp fail_os
STR s_tun, "/dev/net/tun"
STR s_e_name, "--net: the TAP name is too long"
STR s_e_tunsetiff, "TUNSETIFF on /dev/net/tun (needs root)"
STR s_br1, "net: "
STR s_br2, " ("
STR s_br3, ") bridged to the ring network channel"

# run_ip(rdi = argument table: pointers, 0-terminated; entries that are
# 1 stand for the address, 2 for the device name): fork, execve
# /usr/bin/ip, wait; a failure exits.
run_ip:
	pushq %rbx
	pushq %r12
	subq $96, %rsp
	movq %rdi, %rbx
	# the argv on the stack, with the placeholders filled
	xorl %ecx, %ecx
1:	movq (%rbx,%rcx,8), %rax
	cmpq $1, %rax
	jne 2f
	movq o_net_addr(%rip), %rax
	jmp 3f
2:	cmpq $2, %rax
	jne 3f
	movq o_net(%rip), %rax
3:	movq %rax, (%rsp,%rcx,8)
	testq %rax, %rax
	jz 4f
	incl %ecx
	cmpl $10, %ecx
	jb 1b
4:	SYS 57				# fork
	testq %rax, %rax
	js 91f
	jnz 5f
	leaq s_ip(%rip), %rdi
	movq %rsp, %rsi
	movq envp(%rip), %rdx
	SYS 59				# execve
	movl $127, %edi
	SYS SYS_exit
5:	movq %rax, %rdi
	leaq 88(%rsp), %rsi
	xorl %edx, %edx
	xorl %r10d, %r10d
	SYS 61				# wait4
	movl 88(%rsp), %eax
	andl $0xff7f, %eax
	jnz 92f
	addq $96, %rsp
	popq %r12
	popq %rbx
	ret
91:	negq %rax
	movq %rax, %rdi
	leaq s_e_fork(%rip), %rsi
	jmp fail_os
92:	leaq s_e_ip(%rip), %rsi
	jmp fail
	.section .rodata
	.p2align 3
ip_args1:	.quad s_ip, s_addr, s_replace, 1, s_dev, 2, 0
ip_args2:	.quad s_ip, s_link, s_set, 2, s_up, 0
s_ip:	.asciz "/usr/bin/ip"
s_addr:	.asciz "addr"
s_replace:	.asciz "replace"
s_dev:	.asciz "dev"
s_link:	.asciz "link"
s_set:	.asciz "set"
s_up:	.asciz "up"
	.text
STR s_e_fork, "net: fork"
STR s_e_ip, "net: ip failed (address or link)"

	.globl net_fds
# net_fds(rdi = pollfd cursor) -> rax: the TAP device for POLLIN once
# the bridge is up (net_step reads one frame from it per pass).
net_fds:
	movq %rdi, %rax
	cmpq $0, br_ready(%rip)
	je 9f
	movl tap_fd(%rip), %ecx
	PFD %rax, %ecx, $POLLIN
9:	ret

	.globl net_step
# net_step -> eax 1 when a frame moved: one frame from the TAP into the
# ring per pass (whole or dropped, like a NIC queue), everything the
# card published cut into records and written to the TAP.
net_step:
	pushq %rbx
	pushq %r12
	xorl %r12d, %r12d
	cmpq $0, br_ready(%rip)
	jne 1f
	call br_open
	testl %eax, %eax
	jz 9f
1:	movl tap_fd(%rip), %eax
	movl %eax, tap_pfd(%rip)
	movw $POLLIN, tap_pfd+4(%rip)
	leaq tap_pfd(%rip), %rdi
	movl $1, %esi
	xorl %edx, %edx
	call poll_ms
	testq %rax, %rax
	jz 3f
	movl tap_fd(%rip), %edi
	leaq tap_frame(%rip), %rsi
	movl $MAX_ETH_FRAME + 64, %edx
	SYS SYS_read
	testq %rax, %rax
	jle 3f
	cmpq $MAX_ETH_FRAME, %rax
	ja 3f
	movl $1, %r12d
	movq %rax, %rbx
	leaq REC_HDR(%rbx), %rcx
	addq $3, %rcx
	andq $-4, %rcx
	pushq %rcx
	movq net_h2c(%rip), %rdi
	movl net_h2c_size(%rip), %esi
	call ring_free
	popq %rcx
	cmpq %rcx, %rax
	jb 2f
	leaq tap_record(%rip), %rdi
	movw %bx, (%rdi)
	addq $2, %rdi
	leaq tap_frame(%rip), %rsi
	pushq %rcx
	movq %rbx, %rcx
	rep movsb
	movl $0, (%rdi)
	popq %rcx
	movq net_h2c(%rip), %rdi
	movl net_h2c_size(%rip), %esi
	leaq tap_record(%rip), %rdx
	call ring_push
	incq br_to_card(%rip)
	jmp 3f
2:	incq br_dropped(%rip)
3:	# card to host
	movq net_c2h(%rip), %rdi
	movl net_c2h_size(%rip), %esi
	call ring_avail
	testl %eax, %eax
	jz 6f
	movq $2 * READ_CHUNK, %rcx
	subq tap_inbound_len(%rip), %rcx
	jz 6f
	movq net_c2h(%rip), %rdi
	movl net_c2h_size(%rip), %esi
	leaq tap_inbound(%rip), %rdx
	addq tap_inbound_len(%rip), %rdx
	call ring_pop
	testq %rax, %rax
	jz 6f
	movl $1, %r12d
	addq %rax, tap_inbound_len(%rip)
4:	movq tap_inbound_len(%rip), %rcx
	cmpq $REC_HDR, %rcx
	jb 6f
	leaq tap_inbound(%rip), %rsi
	movzwl (%rsi), %eax
	testl %eax, %eax
	jz 5f
	cmpl $MAX_ETH_FRAME, %eax
	ja 5f
	leaq REC_HDR(%rax), %rdx
	addq $3, %rdx
	andq $-4, %rdx
	cmpq %rdx, %rcx
	jb 6f
	pushq %rdx
	movl tap_fd(%rip), %edi
	leaq REC_HDR(%rsi), %rsi
	movq %rax, %rdx
	call write_all
	testq %rax, %rax
	jnz 41f
	incq br_from_card(%rip)
41:	popq %rdx
	leaq tap_inbound(%rip), %rdi
	leaq (%rdi,%rdx), %rsi
	movq tap_inbound_len(%rip), %rcx
	subq %rdx, %rcx
	movq %rcx, tap_inbound_len(%rip)
	rep movsb
	jmp 4b
5:	incq br_bad(%rip)
	movq $0, tap_inbound_len(%rip)
6:	# the report every 30 s when the counters moved
	call now_ms
	cmpq br_next_report(%rip), %rax
	jb 9f
	addq $REPORT_MS, %rax
	movq %rax, br_next_report(%rip)
	movq br_to_card(%rip), %rax
	cmpq br_reported(%rip), %rax
	jne 7f
	movq br_from_card(%rip), %rax
	cmpq br_reported+8(%rip), %rax
	jne 7f
	movq br_dropped(%rip), %rax
	cmpq br_reported+16(%rip), %rax
	jne 7f
	movq br_bad(%rip), %rax
	cmpq br_reported+24(%rip), %rax
	je 9f
7:	movq br_to_card(%rip), %rax
	movq %rax, br_reported(%rip)
	movq br_from_card(%rip), %rax
	movq %rax, br_reported+8(%rip)
	movq br_dropped(%rip), %rax
	movq %rax, br_reported+16(%rip)
	movq br_bad(%rip), %rax
	movq %rax, br_reported+24(%rip)
	call l_begin
	call l_tag
	leaq s_rep1(%rip), %rsi
	call w_cstr
	movq br_to_card(%rip), %rdi
	call w_dec
	leaq s_rep2(%rip), %rsi
	call w_cstr
	movq br_from_card(%rip), %rdi
	call w_dec
	leaq s_rep3(%rip), %rsi
	call w_cstr
	movq br_dropped(%rip), %rdi
	call w_dec
	leaq s_rep4(%rip), %rsi
	call w_cstr
	movq br_bad(%rip), %rdi
	call w_dec
	leaq s_rep5(%rip), %rsi
	call w_cstr
	call l_end
9:	movl %r12d, %eax
	popq %r12
	popq %rbx
	ret
STR s_rep1, "net: "
STR s_rep2, " frames to the card, "
STR s_rep3, " from it, "
STR s_rep4, " dropped (ring full), "
STR s_rep5, " bad records"
