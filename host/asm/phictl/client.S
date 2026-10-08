# client.S: the client verbs (host/crates/phictl/src/client.rs): exec,
# put, get, status, sensors, traffic, each one connection to the
# control socket, one request, the replies relayed. See client.md.
	.include "defs.inc"

	.bss
	.p2align 3
cl_fd:	.skip 8
cl_dec:	.skip 64
cl_socket:	.skip 8
cl_cwd:	.skip 8
cl_mode:	.skip 8
cl_has_mode:	.skip 8
cl_argv0:	.skip 8
cl_file:	.skip 8
cl_total:	.skip 8
cl_stdin_open:	.skip 8
	.p2align 6
cl_buf:	.skip DECCAP
io_buf:	.skip READ_CHUNK
	.p2align 3
cl_pfd:	.skip 16

	.section .rodata
	.globl card_env
# The environment every command on the card gets.
e_path_k:	.asciz "PATH"
e_path_v:	.asciz "/opt/phi/bin:/bin:/sbin:/usr/bin:/usr/sbin"
e_home_k:	.asciz "HOME"
e_home_v:	.asciz "/root"
e_term_k:	.asciz "TERM"
e_term_v:	.asciz "dumb"
	.p2align 3
card_env:
	.quad e_path_k, e_path_v, e_home_k, e_home_v, e_term_k, e_term_v

	.text

# socket_option: --socket PATH from the arguments, else the default
# (PHICTL_SOCKET; the root path when root or when a root daemon
# answers there; else the user's).
socket_option:
	movq $0, cl_socket(%rip)
	movq $0, cl_cwd(%rip)
	movq $0, cl_has_mode(%rip)
	ret

	.globl cl_default_socket
cl_default_socket:
	cmpq $0, cl_socket(%rip)
	jne 9f
	leaq s_phictl_socket(%rip), %rdi
	call getenv
	testq %rax, %rax
	jz 1f
	movq %rax, cl_socket(%rip)
	ret
1:	SYS SYS_geteuid
	testq %rax, %rax
	jz 2f
	movq card_index(%rip), %rdi
	movl $1, %esi
	call socket_path_for
	movq %rax, %rdi
	call socket_answers
	testl %eax, %eax
	jz 3f
2:	movq card_index(%rip), %rdi
	movl $1, %esi
	call socket_path_for
	jmp 4f
3:	movq card_index(%rip), %rdi
	xorl %esi, %esi
	call socket_path_for
4:	movq %rax, %rsi
	leaq cl_sock_copy(%rip), %rdi
	call copy_cstr
	leaq cl_sock_copy(%rip), %rax
	movq %rax, cl_socket(%rip)
9:	ret
	.bss
cl_sock_copy:	.skip PATHCAP
	.text
STR s_phictl_socket, "PHICTL_SOCKET"

# connect_socket: the connection, or exit with the reason.
connect_socket:
	subq $112, %rsp
	call cl_default_socket
	movl $AF_UNIX, %edi
	movl $SOCK_STREAM | SOCK_CLOEXEC, %esi
	xorl %edx, %edx
	SYS SYS_socket
	testq %rax, %rax
	js 8f
	movq %rax, cl_fd(%rip)
	movw $AF_UNIX, (%rsp)
	leaq 2(%rsp), %rdi
	movq cl_socket(%rip), %rsi
	call copy_cstr
	movl cl_fd(%rip), %edi
	movq %rsp, %rsi
	movl $110, %edx
	SYS SYS_connect
	testq %rax, %rax
	js 9f
	leaq cl_dec(%rip), %rdi
	leaq cl_buf(%rip), %rsi
	movl $DECCAP, %edx
	call dec_init
	addq $112, %rsp
	ret
8:	negq %rax
	movq %rax, %rdi
	leaq s_e_socket(%rip), %rsi
	jmp fail_os
9:	pushq %rax
	call l_begin
	leaq s_prefix(%rip), %rsi
	call w_cstr
	leaq s_e_conn1(%rip), %rsi
	call w_cstr
	movq cl_socket(%rip), %rsi
	call w_cstr
	leaq s_e_conn2(%rip), %rsi
	call w_cstr
	popq %rdi
	negq %rdi
	call w_oserr
	call l_end
	movl $1, %edi
	SYS SYS_exit_group
STR s_prefix, "phictl: "
STR s_e_socket, "socket"
STR s_e_conn1, "connecting to "
STR s_e_conn2, " (is `phictl boot --serve` running, and are you its --owner?): "

# send_tx: the frame in tx_buf to the daemon.
send_tx:
	call f_finish
	movl cl_fd(%rip), %edi
	call write_all
	testq %rax, %rax
	jnz 1f
	ret
1:	leaq s_e_write(%rip), %rsi
	jmp fail
STR s_e_write, "writing to the control socket"

# recv_frame: the next frame from the daemon (blocking); exit when the
# daemon closes or sends what phi-rpc rejects.
recv_frame:
1:	leaq cl_dec(%rip), %rdi
	call dec_next
	testq %rax, %rax
	jg 9f
	js 8f
	leaq cl_dec(%rip), %rdi
	call dec_room
	movq %rax, %rsi
	cmpq $READ_CHUNK, %rdx
	jbe 2f
	movl $READ_CHUNK, %edx
2:	movl cl_fd(%rip), %edi
	SYS SYS_read
	testq %rax, %rax
	jz 7f
	js 3f
	leaq cl_dec(%rip), %rdi
	movq %rax, %rsi
	call dec_advance
	jmp 1b
3:	cmpq $-EINTR, %rax
	je 2b
	negq %rax
	movq %rax, %rdi
	leaq s_e_read(%rip), %rsi
	jmp fail_os
7:	leaq s_e_closed(%rip), %rsi
	jmp fail
8:	leaq s_e_badframe(%rip), %rsi
	jmp fail
9:	ret
STR s_e_read, "reading the control socket"
STR s_e_closed, "the daemon closed the connection"
STR s_e_badframe, "the daemon sent a frame phi-rpc rejects"

# unexpected: "phictl: unexpected reply <name>", exit 1.
unexpected:
	call l_begin
	leaq s_prefix(%rip), %rsi
	call w_cstr
	leaq s_unexpected(%rip), %rsi
	call w_cstr
	movq cl_dec+DEC_TAG(%rip), %rax
	leaq msg_names(%rip), %rsi
	movq (%rsi,%rax,8), %rsi
	call w_cstr
	call l_end
	movl $1, %edi
	SYS SYS_exit_group
STR s_unexpected, "unexpected reply "

# error_reply(rsi = prefix): "phictl: <prefix><the Error's text>", exit 1.
error_reply:
	pushq %rsi
	call l_begin
	leaq s_prefix(%rip), %rsi
	call w_cstr
	popq %rsi
	call w_cstr
	movq cl_dec+DEC_BODY(%rip), %rsi
	movzwl (%rsi), %edx
	addq $2, %rsi
	call w_bytes
	call l_end
	movl $1, %edi
	SYS SYS_exit_group

# take_socket_opt(rax = argument) -> eax 1 when it was --socket (its
# value taken).
take_socket_opt:
	leaq s_opt_socket(%rip), %rdi
	call arg_is
	testl %eax, %eax
	jz 1f
	call opt_value
	movq %rax, cl_socket(%rip)
	movl $1, %eax
1:	ret
STR s_opt_socket, "--socket"

# ------------------------------------------------------------ exec

	.globl cmd_exec
cmd_exec:
	pushq %rbx
	call socket_option
1:	call opt_peek
	testq %rax, %rax
	jz usage
	cmpb $'-', (%rax)
	jne 4f
	call opt_next
	call take_socket_opt
	testl %eax, %eax
	jnz 1b
	movq %r11, %rax
	leaq s_opt_cwd(%rip), %rdi
	call arg_is
	testl %eax, %eax
	jz 3f
	call opt_value
	movq %rax, cl_cwd(%rip)
	jmp 1b
3:	movq %r11, %rax
	leaq s_dashdash(%rip), %rdi
	call arg_is
	testl %eax, %eax
	jz usage
4:	# the rest is argv
	movq opt_i(%rip), %rax
	cmpq nargs(%rip), %rax
	jae usage
	call connect_socket
	movl $TAG_EXEC, %edi
	call f_begin
	movq nargs(%rip), %rdi
	subq opt_i(%rip), %rdi
	call w_u16
5:	call opt_next
	testq %rax, %rax
	jz 6f
	movq %rax, %rsi
	call w_str_cstr
	jmp 5b
6:	movl $3, %edi
	call w_u16
	xorl %ebx, %ebx
7:	cmpl $6, %ebx
	jae 8f
	leaq card_env(%rip), %rax
	movq (%rax,%rbx,8), %rsi
	call w_str_cstr
	incl %ebx
	jmp 7b
8:	movq cl_cwd(%rip), %rsi
	testq %rsi, %rsi
	jnz 9f
	leaq s_empty(%rip), %rsi
9:	call w_str_cstr
	call send_tx
	movq $1, cl_stdin_open(%rip)
	# standard input to the daemon and the replies from it, one poll loop
10:	movl cl_fd(%rip), %eax
	movl %eax, cl_pfd(%rip)
	movw $POLLIN, cl_pfd+4(%rip)
	movl $-1, %eax
	cmpq $0, cl_stdin_open(%rip)
	je 11f
	xorl %eax, %eax
11:	movl %eax, cl_pfd+8(%rip)
	movw $POLLIN, cl_pfd+12(%rip)
	leaq cl_pfd(%rip), %rdi
	movl $2, %esi
	movl $-1, %edx
	call poll_ms
	movzwl cl_pfd+14(%rip), %eax
	testl $POLLIN | POLLHUP | POLLERR, %eax
	jz 13f
	xorl %edi, %edi
	leaq io_buf(%rip), %rsi
	movl $16384, %edx
	SYS SYS_read
	testq %rax, %rax
	jg 12f
	cmpq $-EINTR, %rax
	je 13f
	movl $TAG_STDIN_EOF, %edi
	call f_begin
	call send_tx
	movq $0, cl_stdin_open(%rip)
	jmp 13f
12:	pushq %rax
	movl $TAG_STDIN, %edi
	call f_begin
	leaq io_buf(%rip), %rsi
	popq %rdx
	call w_bytes
	call send_tx
13:	movzwl cl_pfd+6(%rip), %eax
	testl $POLLIN | POLLHUP | POLLERR, %eax
	jz 10b
	# frames from the daemon
	leaq cl_dec(%rip), %rdi
	call dec_room
	movq %rax, %rsi
	cmpq $READ_CHUNK, %rdx
	jbe 14f
	movl $READ_CHUNK, %edx
14:	movl cl_fd(%rip), %edi
	SYS SYS_read
	testq %rax, %rax
	jz 20f
	js 15f
	leaq cl_dec(%rip), %rdi
	movq %rax, %rsi
	call dec_advance
16:	leaq cl_dec(%rip), %rdi
	call dec_next
	testq %rax, %rax
	jz 10b
	js 21f
	movq cl_dec+DEC_TAG(%rip), %rax
	cmpq $TAG_STDOUT, %rax
	jne 17f
	movl $1, %edi
	jmp 18f
17:	cmpq $TAG_STDERR, %rax
	jne 19f
	movl $2, %edi
18:	movq cl_dec+DEC_BODY(%rip), %rsi
	movq cl_dec+DEC_LEN(%rip), %rdx
	call write_all
	jmp 16b
19:	cmpq $TAG_EXIT, %rax
	jne 191f
	movq cl_dec+DEC_BODY(%rip), %rax
	movl (%rax), %edi
	SYS SYS_exit_group
191:	cmpq $TAG_ERROR, %rax
	jne unexpected
	leaq s_exec_err(%rip), %rsi
	call error_reply_255
15:	cmpq $-EINTR, %rax
	je 10b
	negq %rax
	movq %rax, %rdi
	leaq s_e_read(%rip), %rsi
	jmp fail_os
20:	leaq s_e_closed(%rip), %rsi
	jmp fail
21:	leaq s_e_badframe(%rip), %rsi
	jmp fail
# error_reply_255: "phictl exec: <text>", exit 255.
error_reply_255:
	call l_begin
	leaq s_exec_err(%rip), %rsi
	call w_cstr
	movq cl_dec+DEC_BODY(%rip), %rsi
	movzwl (%rsi), %edx
	addq $2, %rsi
	call w_bytes
	call l_end
	movl $255, %edi
	SYS SYS_exit_group
STR s_opt_cwd, "--cwd"
STR s_dashdash, "--"
STR s_empty, ""
STR s_exec_err, "phictl exec: "

# ------------------------------------------------------------ put

	.globl cmd_put
cmd_put:
	pushq %rbx
	pushq %r12
	subq $152, %rsp
	call socket_option
	xorl %ebx, %ebx		# positional count: src, dst
1:	call opt_next
	testq %rax, %rax
	jz 4f
	cmpb $'-', (%rax)
	jne 3f
	call take_socket_opt
	testl %eax, %eax
	jnz 1b
	movq %r11, %rax
	leaq s_opt_mode(%rip), %rdi
	call arg_is
	testl %eax, %eax
	jz usage
	call opt_value
	movq %rax, %rsi
	call cstr_len
	call parse_octal
	testl %edx, %edx
	jz 90f
	movq %rax, cl_mode(%rip)
	movq $1, cl_has_mode(%rip)
	jmp 1b
3:	cmpl $2, %ebx
	jae usage
	leaq cl_argv0(%rip), %rcx
	movq %rax, (%rcx,%rbx,8)	# cl_argv0 = src, cl_file = dst
	incl %ebx
	jmp 1b
4:	cmpl $2, %ebx
	jne usage
	movq cl_argv0(%rip), %rdi
	movl $O_RDONLY | O_CLOEXEC, %esi
	xorl %edx, %edx
	SYS SYS_open
	testq %rax, %rax
	js 91f
	movq %rax, %r12
	cmpq $0, cl_has_mode(%rip)
	jne 5f
	movl %r12d, %edi
	movq %rsp, %rsi
	SYS SYS_fstat
	movl 24(%rsp), %eax		# st_mode
	andl $07777, %eax
	movq %rax, cl_mode(%rip)
5:	call connect_socket
	movl $TAG_PUT_OPEN, %edi
	call f_begin
	movq cl_file(%rip), %rsi
	call w_str_cstr
	movq cl_mode(%rip), %rdi
	call w_u32
	call send_tx
	movq $0, cl_total(%rip)
6:	movl %r12d, %edi
	leaq io_buf(%rip), %rsi
	movl $READ_CHUNK, %edx
	SYS SYS_read
	testq %rax, %rax
	jz 8f
	js 7f
	addq %rax, cl_total(%rip)
	pushq %rax
	movl $TAG_PUT_DATA, %edi
	call f_begin
	leaq io_buf(%rip), %rsi
	popq %rdx
	call w_bytes
	call send_tx
	jmp 6b
7:	cmpq $-EINTR, %rax
	je 6b
	negq %rax
	movq %rax, %rdi
	movq cl_argv0(%rip), %rsi
	jmp fail_os
8:	movl $TAG_PUT_CLOSE, %edi
	call f_begin
	call send_tx
	call recv_frame
	movq cl_dec+DEC_TAG(%rip), %rax
	cmpq $TAG_EXIT, %rax
	jne 10f
	movq cl_dec+DEC_BODY(%rip), %rax
	movl (%rax), %edi
	testl %edi, %edi
	jnz 11f
	call l_begin
	leaq s_put_done(%rip), %rsi
	call w_cstr
	movq cl_total(%rip), %rdi
	call w_dec
	leaq s_bytes(%rip), %rsi
	call w_cstr
	call l_end
	addq $152, %rsp
	popq %r12
	popq %rbx
	ret
10:	cmpq $TAG_ERROR, %rax
	jne unexpected
	leaq s_empty(%rip), %rsi
	jmp error_reply
11:	pushq %rdi
	call l_begin
	leaq s_prefix(%rip), %rsi
	call w_cstr
	leaq s_status(%rip), %rsi
	call w_cstr
	popq %rdi
	call w_dec
	call l_end
	movl $1, %edi
	SYS SYS_exit_group
90:	leaq s_e_mode(%rip), %rsi
	jmp fail
91:	negq %rax
	movq %rax, %rdi
	movq cl_argv0(%rip), %rsi
	jmp fail_os
STR s_opt_mode, "--mode"
STR s_e_mode, "--mode takes an octal mode"
STR s_put_done, "phictl put: "
STR s_bytes, " bytes"
STR s_status, "card reported status "

# ------------------------------------------------------------ get

	.globl cmd_get
cmd_get:
	pushq %rbx
	pushq %r12
	call socket_option
	xorl %ebx, %ebx
1:	call opt_next
	testq %rax, %rax
	jz 4f
	cmpb $'-', (%rax)
	jne 3f
	call take_socket_opt
	testl %eax, %eax
	jnz 1b
	jmp usage
3:	cmpl $2, %ebx
	jae usage
	leaq cl_argv0(%rip), %rcx
	movq %rax, (%rcx,%rbx,8)	# src on the card, dst here
	incl %ebx
	jmp 1b
4:	cmpl $2, %ebx
	jne usage
	call connect_socket
	movl $TAG_GET, %edi
	call f_begin
	movq cl_argv0(%rip), %rsi
	call w_str_cstr
	call send_tx
	movq $-1, %r12			# the local file, once opened
5:	call recv_frame
	movq cl_dec+DEC_TAG(%rip), %rax
	cmpq $TAG_GET_DATA, %rax
	jne 7f
	testq %r12, %r12
	jns 6f
	call open_dst
	movq %rax, %r12
6:	movl %r12d, %edi
	movq cl_dec+DEC_BODY(%rip), %rsi
	movq cl_dec+DEC_LEN(%rip), %rdx
	call write_all
	testq %rax, %rax
	jnz 92f
	jmp 5b
7:	cmpq $TAG_GET_END, %rax
	jne 8f
	testq %r12, %r12
	jns 71f
	call open_dst
	movq %rax, %r12
71:	call l_begin
	leaq s_get_done(%rip), %rsi
	call w_cstr
	movq cl_dec+DEC_BODY(%rip), %rax
	movq (%rax), %rdi
	call w_dec
	leaq s_bytes(%rip), %rsi
	call w_cstr
	call l_end
	popq %r12
	popq %rbx
	ret
8:	cmpq $TAG_ERROR, %rax
	jne unexpected
	leaq s_empty(%rip), %rsi
	jmp error_reply
92:	leaq s_e_write_dst(%rip), %rsi
	jmp fail
open_dst:
	movq cl_file(%rip), %rdi
	movl $O_WRONLY | O_CREAT | O_TRUNC | O_CLOEXEC, %esi
	movl $0644, %edx
	SYS SYS_open
	testq %rax, %rax
	js 1f
	ret
1:	negq %rax
	movq %rax, %rdi
	movq cl_file(%rip), %rsi
	jmp fail_os
STR s_get_done, "phictl get: "
STR s_e_write_dst, "writing the local file"

# ------------------------------------------------------------ status, sensors, traffic

# simple_request(edi = tag): --socket taken, one frame sent, the reply
# received (an Error exits).
simple_request:
	pushq %rbx
	movl %edi, %ebx
	call socket_option
1:	call opt_next
	testq %rax, %rax
	jz 2f
	call take_socket_opt
	testl %eax, %eax
	jnz 1b
	jmp usage
2:	call connect_socket
	movl %ebx, %edi
	call f_begin
	call send_tx
	call recv_frame
	cmpq $TAG_ERROR, cl_dec+DEC_TAG(%rip)
	jne 3f
	leaq s_empty(%rip), %rsi
	jmp error_reply
3:	popq %rbx
	ret

	.globl cmd_status
cmd_status:
	movl $TAG_PING, %edi
	call simple_request
	cmpq $TAG_PONG, cl_dec+DEC_TAG(%rip)
	jne unexpected
	call o_begin
	leaq s_st1(%rip), %rsi
	call w_cstr
	movq cl_dec+DEC_BODY(%rip), %rsi
	movzwl (%rsi), %edx
	addq $2, %rsi
	call w_bytes
	leaq s_st2(%rip), %rsi
	call w_cstr
	movq cl_socket(%rip), %rsi
	call w_cstr
	jmp o_end
STR s_st1, "card agent "
STR s_st2, " reachable through "

	.globl cmd_sensors
cmd_sensors:
	movl $TAG_SENSORS, %edi
	call simple_request
	cmpq $TAG_SENSORS_REPLY, cl_dec+DEC_TAG(%rip)
	jne unexpected
	call o_begin
	movq cl_dec+DEC_BODY(%rip), %rsi
	movzwl (%rsi), %edx
	addq $2, %rsi
	call w_bytes
	jmp o_end_bare

	.globl cmd_traffic
cmd_traffic:
	pushq %rbx
	movl $TAG_TRAFFIC, %edi
	call simple_request
	cmpq $TAG_TRAFFIC_REPLY, cl_dec+DEC_TAG(%rip)
	jne unexpected
	movq cl_dec+DEC_BODY(%rip), %rbx
	call o_begin
	leaq s_tr1(%rip), %rsi
	call w_cstr
	movq 0(%rbx), %rdi
	call w_dec
	leaq s_tr2(%rip), %rsi
	call w_cstr
	movq 32(%rbx), %rdi
	call w_dec
	leaq s_tr3(%rip), %rsi
	call w_cstr
	movq 0(%rbx), %rdi
	movq 32(%rbx), %rsi
	call w_mean
	leaq s_tr4(%rip), %rsi
	call w_cstr
	movq 8(%rbx), %rdi
	call w_dec
	leaq s_tr5(%rip), %rsi
	call w_cstr
	movq 40(%rbx), %rdi
	call w_dec
	leaq s_tr3(%rip), %rsi
	call w_cstr
	movq 8(%rbx), %rdi
	movq 40(%rbx), %rsi
	call w_mean
	leaq s_tr6(%rip), %rsi
	call w_cstr
	call o_end
	call o_begin
	leaq s_tr7(%rip), %rsi
	call w_cstr
	movq 16(%rbx), %rdi
	call w_dec
	leaq s_tr8(%rip), %rsi
	call w_cstr
	movq 24(%rbx), %rdi
	call w_dec
	leaq s_tr9(%rip), %rsi
	call w_cstr
	call o_end
	popq %rbx
	ret
# w_mean(rdi = bytes, rsi = copies): bytes / copies, 0 for none.
w_mean:
	testq %rsi, %rsi
	jz 1f
	movq %rdi, %rax
	xorl %edx, %edx
	divq %rsi
	movq %rax, %rdi
	jmp w_dec
1:	xorl %edi, %edi
	jmp w_dec
STR s_tr1, "dma: "
STR s_tr2, " bytes to the card in "
STR s_tr3, " copies (mean "
STR s_tr4, " bytes), "
STR s_tr5, " bytes from it in "
STR s_tr6, " bytes)"
STR s_tr7, "aperture: "
STR s_tr8, " bytes to the card, "
STR s_tr9, " bytes from it"
