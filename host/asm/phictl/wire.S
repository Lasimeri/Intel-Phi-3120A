# wire.S: rpc frames on the host side (host/crates/phi-rpc/src/lib.rs):
# decoders over byte streams (one per client connection, one for the
# card), the same validation as phi-rpc's Decoder (a bad length loses
# the buffer, a bad tag or body loses the frame), and a frame builder
# through the write cursor. See wire.md.
	.include "defs.inc"

# A decoder is DEC_SIZE bytes (defs.inc): head, tail, buffer, capacity,
# then the frame it returned (tag, body, length).

	.bss
	.p2align 6
	.globl tx_buf
tx_buf:	.skip 4 + MAX_FRAME + 64

	.section .rodata
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
msg_names:
	.quad 0, n_exec, n_stdin, n_stdin_eof, n_stdout, n_stderr, n_exit
	.quad n_put_open, n_put_data, n_put_close, n_get, n_get_data, n_get_end
	.quad n_error, n_ping, n_pong, n_sensors, n_sensors_reply, n_stat
	.quad n_stat_reply, n_traffic, n_traffic_reply
v_table:
	.quad v_tag, v_exec, v_ok, v_ok, v_ok, v_ok, v_exit
	.quad v_put_open, v_ok, v_ok, v_str_only, v_ok, v_get_end
	.quad v_str_only, v_ok, v_str_only, v_ok, v_str_only, v_ok
	.quad v_stat_reply, v_ok, v_traffic_reply

	.text

	.globl dec_init
# dec_init(rdi = decoder, rsi = buffer, rdx = capacity).
dec_init:
	movq $0, DEC_HEAD(%rdi)
	movq $0, DEC_TAIL(%rdi)
	movq %rsi, DEC_BUF(%rdi)
	movq %rdx, DEC_CAP(%rdi)
	ret

	.globl dec_reset
dec_reset:
	movq $0, DEC_HEAD(%rdi)
	movq $0, DEC_TAIL(%rdi)
	ret

	.globl dec_space
# dec_space(rdi = decoder) -> rax = bytes a read may append after the
# buffered part is moved to the front (which dec_room does).
dec_space:
	movq DEC_CAP(%rdi), %rax
	subq DEC_TAIL(%rdi), %rax
	addq DEC_HEAD(%rdi), %rax
	ret

	.globl dec_room
# dec_room(rdi = decoder) -> rax = where a read may write, rdx = how
# much: the buffered bytes are moved to the front first when they do not
# start there (a decoder only ever holds part of one frame between
# reads, as phi-rpc's Decoder drains each frame it takes).
dec_room:
	pushq %rdi
	movq DEC_HEAD(%rdi), %rsi
	testq %rsi, %rsi
	jz 1f
	movq DEC_TAIL(%rdi), %rcx
	subq %rsi, %rcx
	movq %rcx, DEC_TAIL(%rdi)
	movq $0, DEC_HEAD(%rdi)
	movq DEC_BUF(%rdi), %rdi
	addq %rdi, %rsi
	rep movsb
1:	popq %rdi
	movq DEC_BUF(%rdi), %rax
	addq DEC_TAIL(%rdi), %rax
	movq DEC_CAP(%rdi), %rdx
	subq DEC_TAIL(%rdi), %rdx
	ret

	.globl dec_advance
# dec_advance(rdi = decoder, rsi = bytes appended by a read into the room).
dec_advance:
	addq %rsi, DEC_TAIL(%rdi)
	ret

	.globl dec_push
# dec_push(rdi = decoder, rsi = bytes, rdx = len): append (the caller
# checked dec_space).
dec_push:
	pushq %rdi
	pushq %rdx
	call dec_room
	popq %rdx
	movq %rax, %rdi
	movq %rdx, %rcx
	rep movsb
	popq %rdi
	addq %rdx, DEC_TAIL(%rdi)
	ret

	.globl dec_next
# dec_next(rdi = decoder) -> eax 1 with DEC_TAG/BODY/LEN set, 0 when no
# whole frame is buffered, -1 when a frame was rejected (rdx = 1 bad
# length, 2 bad tag, 3 malformed; DEC_TAG holds the tag or length).
dec_next:
	pushq %rbx
	pushq %r12
	pushq %r13
	movq %rdi, %rbx
	movq DEC_HEAD(%rbx), %r12
	movq DEC_TAIL(%rbx), %rcx
	subq %r12, %rcx
	cmpq $4, %rcx
	jb 8f
	addq DEC_BUF(%rbx), %r12
	movl (%r12), %r13d
	testl %r13d, %r13d
	jz 5f
	cmpl $MAX_FRAME, %r13d
	ja 5f
	leaq 4(%r13), %rax
	cmpq %rax, %rcx
	jb 8f
	addq %rax, DEC_HEAD(%rbx)
	movzbl 4(%r12), %edi
	leaq 5(%r12), %rsi
	leaq -1(%r13), %rdx
	movq %rdi, DEC_TAG(%rbx)
	movq %rsi, DEC_BODY(%rbx)
	movq %rdx, DEC_LEN(%rbx)
	call validate
	testl %eax, %eax
	jz 9f
	leal 1(%rax), %edx
	movq $-1, %rax
	jmp 10f
5:	movq $0, DEC_HEAD(%rbx)
	movq $0, DEC_TAIL(%rbx)
	movq %r13, DEC_TAG(%rbx)
	movl $1, %edx
	movq $-1, %rax
	jmp 10f
8:	xorl %eax, %eax
	jmp 10f
9:	movl $1, %eax
10:	popq %r13
	popq %r12
	popq %rbx
	ret

	.globl log_bad_frame
# log_bad_frame(rdi = decoder, edx = the code dec_next gave, rsi = who):
# "[phictl] serve: bad frame from <who>: <why>".
log_bad_frame:
	pushq %rdi
	pushq %rdx
	pushq %rsi
	call l_begin
	call l_tag
	leaq s_bad1(%rip), %rsi
	call w_cstr
	popq %rsi
	call w_cstr
	leaq s_colon(%rip), %rsi
	call w_cstr
	popq %rdx
	popq %rdi
	cmpl $1, %edx
	jne 1f
	leaq s_badlen1(%rip), %rsi
	pushq %rdi
	call w_cstr
	popq %rdi
	movq DEC_TAG(%rdi), %rdi
	call w_dec
	leaq s_badlen2(%rip), %rsi
	call w_cstr
	jmp l_end
1:	cmpl $2, %edx
	jne 2f
	leaq s_badtag(%rip), %rsi
	pushq %rdi
	call w_cstr
	popq %rdi
	movq DEC_TAG(%rdi), %rdi
	call w_hex
	jmp l_end
2:	leaq s_malformed(%rip), %rsi
	call w_cstr
	jmp l_end
STR s_bad1, "serve: bad frame from "
STR s_colon, ": "
STR s_badlen1, "frame length "
STR s_badlen2, " out of range"
STR s_badtag, "unknown message tag "
STR s_malformed, "malformed frame body"

# ------------------------------------------------------------ building

	.globl f_begin
# f_begin(dil = tag): a frame in tx_buf, its body through the cursor.
f_begin:
	leaq tx_buf(%rip), %rax
	movb %dil, 4(%rax)
	addq $5, %rax
	movq %rax, wp(%rip)
	ret

	.globl f_finish
# f_finish -> rsi = the frame, rdx = its length (the length field set).
f_finish:
	leaq tx_buf(%rip), %rsi
	movq wp(%rip), %rdx
	subq %rsi, %rdx
	leaq -4(%rdx), %rax
	movl %eax, (%rsi)
	ret

# ------------------------------------------------------------ checking

# validate(edi = tag, rsi = body, rdx = length) -> eax 0 (decodes), 1
# (unknown tag), 2 (malformed).
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
v_u16:
	movl $2, %ecx
	call v_need
	jc 1f
	movzwl -2(%rbx), %eax
	clc
1:	ret
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

	.globl utf8_ok
# utf8_ok(rsi, rdx) -> eax 1 when valid UTF-8 (no overlong forms, no
# surrogates, nothing past U+10FFFF).
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
