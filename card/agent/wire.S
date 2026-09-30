# wire.S: the rpc channel's two ends inside the agent. The reader fills a
# buffer from the device and cuts it into frames, rejecting what phi-rpc's
# Decoder rejects (host/crates/phi-rpc/src/lib.rs, `next_frame` and
# `decode_body`); the writer builds one frame at a time in tx_buf and
# writes it whole. See wire.md.
	.include "defs.inc"

	.bss
	.p2align 3
	.globl dev_fd, dev_name
# The device descriptor and its name, for messages.
dev_fd:	.skip 8
dev_name:	.skip 8
	.globl fr_tag, fr_body, fr_len
# The frame next_frame returned: its tag, body and body length. The body
# lives in rx_buf until the next fill, so a caller consumes (or copies) it
# before reading again.
fr_tag:	.skip 8
fr_body:	.skip 8
fr_len:	.skip 8
# Bytes received and not yet cut into frames: rx_buf[rx_head, rx_tail).
rx_head:	.skip 8
rx_tail:	.skip 8
	.globl tx_buf
	.p2align 6
rx_buf:	.skip RXCAP
	.p2align 6
tx_buf:	.skip TXCAP

	.section .rodata
s_closed:	.asciz "device closed"
s_bad:	.asciz "bad frame from the host: "
s_badlen1:	.asciz "frame length "
s_badlen2:	.asciz " out of range"
s_badtag:	.asciz "unknown message tag "
s_malformed:	.asciz "malformed frame body"
s_colon:	.asciz ": "
n_exec:	.asciz "Exec"
n_stdin:	.asciz "Stdin"
n_stdin_eof:	.asciz "StdinEof"
n_stdout:	.asciz "Stdout"
n_stderr:	.asciz "Stderr"
n_exit:	.asciz "Exit"
n_put_open:	.asciz "PutOpen"
n_put_data:	.asciz "PutData"
n_put_close:	.asciz "PutClose"
n_get:	.asciz "Get"
n_get_data:	.asciz "GetData"
n_get_end:	.asciz "GetEnd"
n_error:	.asciz "Error"
n_ping:	.asciz "Ping"
n_pong:	.asciz "Pong"
n_sensors:	.asciz "Sensors"
n_sensors_reply:	.asciz "SensorsReply"
n_stat:	.asciz "Stat"
n_stat_reply:	.asciz "StatReply"
n_traffic:	.asciz "Traffic"
n_traffic_reply:	.asciz "TrafficReply"
	.p2align 3
	.globl msg_names
# Message names by tag, as phi-rpc's `Msg::name`.
msg_names:
	.quad 0, n_exec, n_stdin, n_stdin_eof, n_stdout, n_stderr, n_exit
	.quad n_put_open, n_put_data, n_put_close, n_get, n_get_data, n_get_end
	.quad n_error, n_ping, n_pong, n_sensors, n_sensors_reply, n_stat
	.quad n_stat_reply, n_traffic, n_traffic_reply
# Body checks by tag.
v_table:
	.quad v_tag, v_exec, v_ok, v_ok, v_ok, v_ok, v_exit
	.quad v_put_open, v_ok, v_ok, v_str_only, v_ok, v_get_end
	.quad v_str_only, v_ok, v_str_only, v_ok, v_str_only, v_ok
	.quad v_stat_reply, v_ok, v_traffic_reply

	.text

# ------------------------------------------------------------ writing

	.globl f_begin
# f_begin(dil = tag): start a frame in tx_buf; its body is written through
# the cursor.
f_begin:
	leaq tx_buf(%rip), %rax
	movb %dil, 4(%rax)
	addq $5, %rax
	movq %rax, wp(%rip)
	ret

	.globl f_send
# f_send: fill in the length and write the frame whole; a device that
# fails ends the agent (dev_fail), as in the Rust agent.
f_send:
	leaq tx_buf(%rip), %rsi
	movq wp(%rip), %rdx
	subq %rsi, %rdx
	leaq -4(%rdx), %rax
	movl %eax, (%rsi)
	movl dev_fd(%rip), %edi
	call write_all
	testq %rax, %rax
	jnz dev_fail
	ret

	.globl send_error
# send_error(rsi = ptr, rdx = len): an Error frame with this text.
send_error:
	pushq %rsi
	pushq %rdx
	movb $TAG_ERROR, %dil
	call f_begin
	popq %rdx
	popq %rsi
	call w_str
	jmp f_send

	.globl dev_fail
# dev_fail(rdi = -errno, or 1 for a write of zero): "phi-agent: DEVICE: why"
# and exit status 1.
dev_fail:
	pushq %rdi
	call l_begin
	movq dev_name(%rip), %rsi
	call w_cstr
	leaq s_colon(%rip), %rsi
	call w_cstr
	popq %rdi
	call w_ioerr
	call l_end
	movl $1, %edi
	SYS SYS_exit_group

	.globl dev_closed
# dev_closed: the device read returned 0.
dev_closed:
	call l_begin
	movq dev_name(%rip), %rsi
	call w_cstr
	leaq s_colon(%rip), %rsi
	call w_cstr
	leaq s_closed(%rip), %rsi
	call w_cstr
	call l_end
	movl $1, %edi
	SYS SYS_exit_group

# ------------------------------------------------------------ reading

	.globl fill
# fill: one read from the device into rx_buf, after moving what is
# buffered (part of one frame) to the front, as phi-rpc's Decoder drains
# each frame it takes: the buffer then only ever holds one frame and one
# read, so its pages stay few. Only called with no whole frame buffered,
# so what is buffered is under 4 + MAX_FRAME bytes. A closed device or a
# failed read ends the agent.
fill:
	movq rx_head(%rip), %rsi
	movq rx_tail(%rip), %rcx
	subq %rsi, %rcx
	jnz 1f
	movq $0, rx_head(%rip)
	movq $0, rx_tail(%rip)
	jmp 2f
1:	testq %rsi, %rsi
	jz 2f
	leaq rx_buf(%rip), %rdi
	addq %rdi, %rsi
	movq %rcx, rx_tail(%rip)
	movq $0, rx_head(%rip)
	rep movsb
2:	movl dev_fd(%rip), %edi
	leaq rx_buf(%rip), %rsi
	addq rx_tail(%rip), %rsi
	movl $READ_CHUNK, %edx
	SYS SYS_read
	testq %rax, %rax
	jz dev_closed
	jns 3f
	cmpq $-EINTR, %rax
	je 2b
	movq %rax, %rdi
	jmp dev_fail
3:	addq %rax, rx_tail(%rip)
	ret

	.globl next_frame
# next_frame -> eax = 1 with fr_tag, fr_body, fr_len set, or 0 when no
# whole frame is buffered. A frame that does not decode is logged and
# dropped, as the Rust agent's `Reader::buffered` did: a bad length loses
# everything buffered, a bad tag or body loses that frame only.
next_frame:
	pushq %rbx
	pushq %r12
	pushq %r13
0:	movq rx_head(%rip), %rbx
	movq rx_tail(%rip), %rcx
	subq %rbx, %rcx
	cmpq $4, %rcx
	jb 8f
	leaq rx_buf(%rip), %r12
	addq %rbx, %r12
	movl (%r12), %r13d
	testl %r13d, %r13d
	jz 5f
	cmpl $MAX_FRAME, %r13d
	ja 5f
	leaq 4(%r13), %rax
	cmpq %rax, %rcx
	jb 8f
	addq %rax, rx_head(%rip)
	movzbl 4(%r12), %edi
	leaq 5(%r12), %rsi
	leaq -1(%r13), %rdx
	movq %rdi, fr_tag(%rip)
	movq %rsi, fr_body(%rip)
	movq %rdx, fr_len(%rip)
	call validate
	testl %eax, %eax
	jz 9f
	# a bad tag or body: log it, drop the frame, look at the next one
	pushq %rax
	call l_begin
	leaq s_bad(%rip), %rsi
	call w_cstr
	popq %rax
	cmpl $1, %eax
	jne 4f
	leaq s_badtag(%rip), %rsi
	call w_cstr
	movq fr_tag(%rip), %rdi
	call w_hex
	jmp 6f
4:	leaq s_malformed(%rip), %rsi
	call w_cstr
	jmp 6f
5:	# a bad length: everything buffered goes
	movq $0, rx_head(%rip)
	movq $0, rx_tail(%rip)
	call l_begin
	leaq s_bad(%rip), %rsi
	call w_cstr
	leaq s_badlen1(%rip), %rsi
	call w_cstr
	movl %r13d, %edi
	call w_dec
	leaq s_badlen2(%rip), %rsi
	call w_cstr
6:	call l_end
	jmp 0b
8:	xorl %eax, %eax
	jmp 10f
9:	movl $1, %eax
10:	popq %r13
	popq %r12
	popq %rbx
	ret

	.globl next_blocking
# next_blocking: the next frame, reading the device (blocking) until one
# is whole. A closed device ends the agent, as in the Rust agent.
next_blocking:
1:	call next_frame
	testl %eax, %eax
	jnz 2f
	call fill
	jmp 1b
2:	ret

	.globl frame_name
# frame_name -> rsi = the current frame's name.
frame_name:
	movq fr_tag(%rip), %rax
	leaq msg_names(%rip), %rsi
	movq (%rsi,%rax,8), %rsi
	ret

# ------------------------------------------------------------ checking

# validate(edi = tag, rsi = body, rdx = length) -> eax 0 (the body decodes),
# 1 (unknown tag) or 2 (malformed: short, or a string not UTF-8). Bytes
# after the last field are allowed, as phi-rpc allows them. Inside,
# rbx is the cursor and r12 the end; the v_* helpers share them.
validate:
	pushq %rbx
	pushq %r12
	pushq %r13
	pushq %r14
	subq $8, %rsp
	movq %rsi, %rbx
	leaq (%rsi,%rdx), %r12
	cmpl $TAG_LAST, %edi
	ja v_tag
	movl %edi, %eax
	leaq v_table(%rip), %rcx
	jmp *(%rcx,%rax,8)
v_ok:	xorl %eax, %eax
	jmp v_out
v_tag:	movl $1, %eax
	jmp v_out
v_bad:	movl $2, %eax
v_out:	addq $8, %rsp
	popq %r14
	popq %r13
	popq %r12
	popq %rbx
	ret

v_exit:	movl $4, %ecx
	call v_need
	jc v_bad
	jmp v_ok
v_get_end:
	movl $8, %ecx
	call v_need
	jc v_bad
	jmp v_ok
v_traffic_reply:
	movl $48, %ecx
	call v_need
	jc v_bad
	jmp v_ok
v_str_only:
	call v_str
	jc v_bad
	jmp v_ok
v_put_open:
	call v_str
	jc v_bad
	movl $4, %ecx
	call v_need
	jc v_bad
	jmp v_ok
v_exec:
	call v_u16
	jc v_bad
	movq %rax, %r13
1:	testq %r13, %r13
	jz 2f
	call v_str
	jc v_bad
	decq %r13
	jmp 1b
2:	call v_u16
	jc v_bad
	movq %rax, %r13
3:	testq %r13, %r13
	jz 4f
	call v_str
	jc v_bad
	call v_str
	jc v_bad
	decq %r13
	jmp 3b
4:	call v_str
	jc v_bad
	jmp v_ok
v_stat_reply:
	movl $8, %ecx
	call v_need
	jc v_bad
	call v_u16
	jc v_bad
	imulq $10, %rax, %rcx
	call v_need
	jc v_bad
	movl $48 + 6 + 8, %ecx
	call v_need
	jc v_bad
	call v_u16
	jc v_bad
	leaq (%rax,%rax), %rcx
	call v_need
	jc v_bad
	movl $2 + 4 + 4, %ecx
	call v_need
	jc v_bad
	call v_devs
	jc v_bad
	call v_devs
	jc v_bad
	call v_u16
	jc v_bad
	movq %rax, %r13
5:	testq %r13, %r13
	jz 6f
	movl $26, %ecx
	call v_need
	jc v_bad
	call v_str
	jc v_bad
	decq %r13
	jmp 5b
6:	movl $8, %ecx
	call v_need
	jc v_bad
	jmp v_ok

# v_need(rcx = n): CF set when fewer than n bytes are left; otherwise the
# cursor moves past them.
v_need:
	movq %r12, %rax
	subq %rbx, %rax
	cmpq %rcx, %rax
	jb 1f
	addq %rcx, %rbx
	clc
	ret
1:	stc
	ret

# v_u16 -> rax, CF on a short body.
v_u16:
	movl $2, %ecx
	call v_need
	jc 1f
	movzwl -2(%rbx), %eax
	clc
1:	ret

# v_str: a u16 length and that many bytes of UTF-8; CF when malformed.
v_str:
	call v_u16
	jc 1f
	movq %rax, %rcx
	movq %rbx, %r14
	call v_need
	jc 1f
	movq %r14, %rsi
	movq %rcx, %rdx
	call utf8_ok
	testl %eax, %eax
	jz 2f
	clc
	ret
2:	stc
1:	ret

# v_devs: a device list (u16 count; name, read, written).
v_devs:
	pushq %r13
	call v_u16
	jc 9f
	movq %rax, %r13
1:	testq %r13, %r13
	jz 2f
	call v_str
	jc 9f
	movl $16, %ecx
	call v_need
	jc 9f
	decq %r13
	jmp 1b
2:	clc
9:	popq %r13
	ret
