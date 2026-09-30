# main.S: `phictl`, the host's control of the cards, in assembly (the
# subcommands of host/crates/phictl/src/main.rs). This file holds the
# entry, the argument walk, the card selection and the register
# subcommands (cards, info, postcode, spad, regs, reset, peek, poke,
# fill); boot.S, console.S, serve.S, client.S, disk.S, forward.S and
# net.S hold the rest. See main.md.
	.include "defs.inc"

	.bss
	.p2align 3
	.globl argc, argv, envp, card_index, bdf_ptr
argc:	.skip 8
argv:	.skip 8
envp:	.skip 8
# The card the command line names: its index (0 unless --card or
# PHI_CARD), and its address (0 until resolved).
card_index:	.skip 8
bdf_ptr:	.skip 8
bdf_given:	.skip 8
# The subcommand's arguments: argv indexes of what was not a global option.
	.globl args, nargs
nargs:	.skip 8
args:	.skip 64 * 8
	.globl opt_i
opt_i:	.skip 8
	.globl fill_data
fill_data:	.skip 8

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
	# SIGPIPE ignored, as Rust's runtime does: a client that goes away
	# is a failed write, not the daemon's end.
	subq $32, %rsp
	movq $SIG_IGN, (%rsp)
	movq $0, 8(%rsp)
	movq $0, 16(%rsp)
	movq $0, 24(%rsp)
	movl $SIGPIPE, %edi
	movq %rsp, %rsi
	xorl %edx, %edx
	movl $8, %r10d
	SYS SYS_rt_sigaction
	addq $32, %rsp
	leaq log_buf_start(%rip), %rax
	movq %rax, wp(%rip)
	# Global options anywhere: --bdf X, --card N. The rest are the
	# subcommand's, in order.
	movq $0, nargs(%rip)
	movq $-1, card_index(%rip)
	movl $1, %ebx
1:	cmpq argc(%rip), %rbx
	jae 5f
	movq argv(%rip), %rax
	movq (%rax,%rbx,8), %r12
	movq %r12, %rsi
	leaq s_opt_bdf(%rip), %rdi
	call eq_cc
	testl %eax, %eax
	jz 2f
	incq %rbx
	call arg_at
	movq %rax, bdf_given(%rip)
	jmp 4f
2:	movq %r12, %rsi
	leaq s_opt_card(%rip), %rdi
	call eq_cc
	testl %eax, %eax
	jz 3f
	incq %rbx
	call arg_at
	movq %rax, %rsi
	call cstr_len
	call parse_u64
	testl %edx, %edx
	jz 90f
	cmpq $MAX_CARDS, %rax
	jae 90f
	movq %rax, card_index(%rip)
	jmp 4f
3:	movq nargs(%rip), %rax
	cmpq $64, %rax
	jae 4f
	leaq args(%rip), %rcx
	movq %r12, (%rcx,%rax,8)
	incq nargs(%rip)
4:	incq %rbx
	jmp 1b
5:	cmpq $0, nargs(%rip)
	je usage
	# --bdf wins; --card resolves through the list; neither: PHI_CARD,
	# else card 0, resolved by the commands that open the card
	cmpq $0, bdf_given(%rip)
	je 6f
	cmpq $-1, card_index(%rip)
	jne 91f
	movq $0, card_index(%rip)
	movq bdf_given(%rip), %rsi
	call cstr_len
	call normalize_bdf
	testq %rax, %rax
	jz 92f
	movq %rax, bdf_ptr(%rip)
	# PHI_CARD still names the socket and files
	call index_from_env
	testq %rax, %rax
	js 8f
	movq %rax, card_index(%rip)
	jmp 8f
6:	cmpq $-1, card_index(%rip)
	jne 7f
	call index_from_env
	testq %rax, %rax
	js 61f
	movq %rax, card_index(%rip)
	jmp 8f
61:	movq $0, card_index(%rip)
	jmp 8f
7:	movq card_index(%rip), %rdi
	call resolve_card
	leaq 8(%rax), %rsi
	leaq bdf_static(%rip), %rdi
	call copy_cstr
	leaq bdf_static(%rip), %rax
	movq %rax, bdf_ptr(%rip)
8:	# dispatch on args[0]
	leaq args(%rip), %rax
	movq (%rax), %rsi
	leaq cmd_table(%rip), %rbx
9:	movq (%rbx), %rdi
	testq %rdi, %rdi
	jz usage
	pushq %rsi
	call eq_cc
	popq %rsi
	testl %eax, %eax
	jnz 10f
	addq $16, %rbx
	jmp 9b
10:	movq $1, opt_i(%rip)
	call *8(%rbx)
	call o_flush
	xorl %edi, %edi
	SYS SYS_exit_group
90:	leaq s_e_cardidx(%rip), %rsi
	jmp fail
91:	leaq s_e_both(%rip), %rsi
	jmp fail
92:	leaq s_e_badbdf(%rip), %rsi
	jmp fail

	.bss
bdf_static:	.skip 16
log_buf_start:	.skip 64
	.text
STR s_opt_bdf, "--bdf"
STR s_opt_card, "--card"
STR s_e_cardidx, "--card takes a card index, 0 to 15"
STR s_e_both, "--bdf and --card cannot be used together"
STR s_e_badbdf, "bad PCI address: expected [dddd:]bb:dd.f, hex digits, function 0..7"

	.section .rodata
	.p2align 3
cmd_table:
	.quad c_cards, cmd_cards
	.quad c_info, cmd_info
	.quad c_postcode, cmd_postcode
	.quad c_spad, cmd_spad
	.quad c_regs, cmd_regs
	.quad c_reset, cmd_reset
	.quad c_peek, cmd_peek
	.quad c_poke, cmd_poke
	.quad c_fill, cmd_fill
	.quad c_boot, cmd_boot
	.quad c_console, cmd_console
	.quad c_exec, cmd_exec
	.quad c_put, cmd_put
	.quad c_get, cmd_get
	.quad c_status, cmd_status
	.quad c_sensors, cmd_sensors
	.quad c_traffic, cmd_traffic
	.quad 0, 0
c_cards:	.asciz "cards"
c_info:	.asciz "info"
c_postcode:	.asciz "postcode"
c_spad:	.asciz "spad"
c_regs:	.asciz "regs"
c_reset:	.asciz "reset"
c_peek:	.asciz "peek"
c_poke:	.asciz "poke"
c_fill:	.asciz "fill"
c_boot:	.asciz "boot"
c_console:	.asciz "console"
c_exec:	.asciz "exec"
c_put:	.asciz "put"
c_get:	.asciz "get"
c_status:	.asciz "status"
c_sensors:	.asciz "sensors"
c_traffic:	.asciz "traffic"
	.text

	.globl usage
usage:
	call l_begin
	leaq s_usage(%rip), %rsi
	call w_cstr
	call l_end_bare
	movl $2, %edi
	SYS SYS_exit_group
	.section .rodata
s_usage:	.ascii "usage: phictl [--card N | --bdf ADDR] COMMAND [options]\n"
	.ascii "  cards [--plain]                 the cards this host knows\n"
	.ascii "  info | postcode [--watch] [--timeout S] | spad [N] | regs | reset [--timeout S]\n"
	.ascii "  peek ADDR [--len N] | poke ADDR VALUE | fill ADDR LEN [--chunk N] [--no-readback]\n"
	.ascii "  boot --kernel PATH [--initrd PATH] [--cmdline S] [--ring-base A] [--ring-size N]\n"
	.ascii "       [--raw-cmdline] [--no-console] [--load-only] [--watch A] [--net NAME] [--net-addr A/24]\n"
	.ascii "       [--forward HOST:CARD] [--serve [PATH]] [--disk IMG] [--no-dma] [--host-mem SIZE] [--owner UID]\n"
	.ascii "  console [--ring-base A] [--ring-size N] [--watch A] [--net NAME] [--net-addr A/24] [--forward P]\n"
	.ascii "  exec [--socket P] [--cwd DIR] -- PROG ARGS | put [--socket P] SRC DST [--mode OCTAL]\n"
	.ascii "  get [--socket P] SRC DST | status | sensors | traffic   (each takes --socket P)\n"
	.byte 0
	.text

# arg_at(rbx = argv index) -> rax = that argument; usage when past the end.
arg_at:
	cmpq argc(%rip), %rbx
	jae usage
	movq argv(%rip), %rax
	movq (%rax,%rbx,8), %rax
	ret

	.globl opt_next
# opt_next() -> rax = the next subcommand argument (0 at the end).
opt_next:
	movq opt_i(%rip), %rax
	cmpq nargs(%rip), %rax
	jae 1f
	leaq args(%rip), %rcx
	movq (%rcx,%rax,8), %rax
	incq opt_i(%rip)
	ret
1:	xorl %eax, %eax
	ret

	.globl opt_peek
# opt_peek() -> rax = the next argument without taking it (0 at the end).
opt_peek:
	movq opt_i(%rip), %rax
	cmpq nargs(%rip), %rax
	jae 1f
	leaq args(%rip), %rcx
	movq (%rcx,%rax,8), %rax
	ret
1:	xorl %eax, %eax
	ret

	.globl opt_value
# opt_value() -> rax = the next argument, usage when there is none.
opt_value:
	call opt_next
	testq %rax, %rax
	jz usage
	ret

	.globl opt_num
# opt_num() -> rax: the next argument as a number (hex or decimal).
opt_num:
	call opt_value
	movq %rax, %rsi
	call cstr_len
	call parse_num
	testl %edx, %edx
	jz 1f
	ret
1:	leaq s_e_num(%rip), %rsi
	jmp fail
STR s_e_num, "a number (decimal or 0x hex) was expected"

	.globl arg_is
# arg_is(rax = argument, rdi = option name) -> eax 1 when equal; rax kept
# in r11.
arg_is:
	movq %rax, %r11
	movq %rax, %rsi
	call eq_cc
	movq %r11, %r11
	ret

	.globl open_card
# open_card: resolve the address if needed and open the card.
open_card:
	cmpq $0, bdf_ptr(%rip)
	jne 2f
	leaq s_phi_bdf(%rip), %rdi
	call getenv
	testq %rax, %rax
	jz 1f
	movq %rax, %rsi
	call cstr_len
	call trim
	testq %rdx, %rdx
	jz 1f
	call normalize_bdf
	testq %rax, %rax
	jz 3f
	movq %rax, %rsi
	leaq bdf_static(%rip), %rdi
	call copy_cstr
	leaq bdf_static(%rip), %rax
	movq %rax, bdf_ptr(%rip)
	jmp 2f
1:	movq card_index(%rip), %rdi
	call resolve_card
	leaq 8(%rax), %rsi
	leaq bdf_static(%rip), %rdi
	call copy_cstr
	leaq bdf_static(%rip), %rax
	movq %rax, bdf_ptr(%rip)
2:	movq bdf_ptr(%rip), %rsi
	jmp card_open
3:	leaq s_e_envbdf(%rip), %rsi
	jmp fail
STR s_phi_bdf, "PHI_BDF"
STR s_e_envbdf, "PHI_BDF is not a PCI address"

# ------------------------------------------------------------ cards

cmd_cards:
	pushq %rbx
	pushq %r12
	pushq %r13
	xorl %r13d, %r13d
	call opt_next
	testq %rax, %rax
	jz 1f
	leaq s_opt_plain(%rip), %rdi
	call arg_is
	testl %eax, %eax
	jz usage
	movl $1, %r13d
1:	call cards_list
	testl %r13d, %r13d
	jnz 20f
	call o_begin
	movq ncards(%rip), %rdi
	call w_dec
	leaq s_cards1(%rip), %rsi
	call w_cstr
	call config_path
	movq %rax, %rdi
	xorl %esi, %esi
	SYS SYS_access
	testq %rax, %rax
	jnz 2f
	leaq s_cards_from(%rip), %rsi
	call w_cstr
	call config_path
	movq %rax, %rsi
	call w_cstr
	jmp 3f
2:	leaq s_cards_bypci(%rip), %rsi
	call w_cstr
3:	call o_end
	call o_begin
	leaq s_cards_hdr(%rip), %rsi
	call w_cstr
	call o_end
	xorl %r12d, %r12d
4:	cmpq ncards(%rip), %r12
	jae 19f
	movq %r12, %rbx
	shlq $6, %rbx
	leaq cards(%rip), %rax
	addq %rax, %rbx
	call o_begin
	movq (%rbx), %rdi
	movl $6, %esi
	movb $' ', %dl
	call w_num_left
	leaq 8(%rbx), %rsi
	movl $14, %ecx
	call w_left_cstr
	cmpq $0, 24(%rbx)
	je 5f
	leaq s_yes(%rip), %rsi
	movl $8, %ecx
	call w_left_cstr
	# link: "5.0 x8"
	movq wp(%rip), %r13
	leaq 8(%rbx), %rsi
	leaq s_link_speed(%rip), %rdi
	call read_attr
	movq %rsi, %rdi
	movq %rdx, %r8
	pushq %rdi
	pushq %r8
	# the first word of "5.0 GT/s PCIe"
	leaq (%rsi,%rdx), %rdi
	call next_token
	testq %rax, %rax
	jnz 41f
	leaq s_qmark(%rip), %rax
	movl $1, %edx
41:	movq %rax, %rsi
	call w_bytes
	addq $16, %rsp
	leaq s_space_x(%rip), %rsi
	call w_cstr
	leaq 8(%rbx), %rsi
	leaq s_link_width(%rip), %rdi
	call read_attr
	call w_bytes
	movq wp(%rip), %rax
	subq %r13, %rax
	movl $13, %edi
	subl %eax, %edi
	jle 42f
	movb $' ', %sil
	call w_pad
42:	leaq 8(%rbx), %rsi
	call driver_of
	testq %rdx, %rdx
	jnz 43f
	leaq s_none(%rip), %rsi
	movl $4, %edx
43:	movl $10, %ecx
	call w_left
	leaq 8(%rbx), %rsi
	call iommu_group_of
	testl %edx, %edx
	jz 44f
	movq %rax, %rdi
	movl $7, %esi
	movb $' ', %dl
	call w_num_left
	jmp 45f
44:	leaq s_dash(%rip), %rsi
	movl $7, %ecx
	call w_left_cstr
45:	jmp 6f
5:	leaq s_no(%rip), %rsi
	movl $8, %ecx
	call w_left_cstr
	leaq s_dash(%rip), %rsi
	movl $13, %ecx
	call w_left_cstr
	leaq s_dash(%rip), %rsi
	movl $10, %ecx
	call w_left_cstr
	leaq s_dash(%rip), %rsi
	movl $7, %ecx
	call w_left_cstr
6:	# up: a daemon answers on the card's socket
	movq (%rbx), %rdi
	xorl %esi, %esi
	call socket_path_for
	movq %rax, %rdi
	call socket_answers
	testl %eax, %eax
	jz 7f
	leaq s_yes(%rip), %rsi
	jmp 8f
7:	leaq s_no(%rip), %rsi
8:	movl $6, %ecx
	call w_left_cstr
	movq 40(%rbx), %rsi
	testq %rsi, %rsi
	jnz 9f
	leaq s_dash(%rip), %rsi
9:	movl $9, %ecx
	call w_left_cstr
	movq 32(%rbx), %rsi
	testq %rsi, %rsi
	jnz 10f
	leaq s_dash(%rip), %rsi
10:	call w_cstr
	call o_end
	incq %r12
	jmp 4b
19:	popq %r13
	popq %r12
	popq %rbx
	ret
20:	# --plain: INDEX BDF yes|no DISK HOSTMEM
	xorl %r12d, %r12d
21:	cmpq ncards(%rip), %r12
	jae 19b
	movq %r12, %rbx
	shlq $6, %rbx
	leaq cards(%rip), %rax
	addq %rax, %rbx
	call o_begin
	movq (%rbx), %rdi
	call w_dec
	movb $' ', %dil
	call w_byte
	leaq 8(%rbx), %rsi
	call w_cstr
	movb $' ', %dil
	call w_byte
	leaq s_yes(%rip), %rsi
	cmpq $0, 24(%rbx)
	jne 22f
	leaq s_no(%rip), %rsi
22:	call w_cstr
	movb $' ', %dil
	call w_byte
	movq 32(%rbx), %rsi
	testq %rsi, %rsi
	jnz 23f
	leaq s_dash(%rip), %rsi
23:	call w_cstr
	movb $' ', %dil
	call w_byte
	movq 40(%rbx), %rsi
	testq %rsi, %rsi
	jnz 24f
	leaq s_dash(%rip), %rsi
24:	call w_cstr
	call o_end
	incq %r12
	jmp 21b
STR s_opt_plain, "--plain"
STR s_cards1, " card(s) known, "
STR s_cards_from, "order from "
STR s_cards_bypci, "order by PCI address (no ~/.config/phi/cards)"
STR s_cards_hdr, "card  bdf           present link         driver    group  up    hostmem  disk"
STR s_yes, "yes"
STR s_no, "no"
STR s_dash, "-"
STR s_none, "none"
STR s_qmark, "?"
STR s_space_x, " x"
STR s_link_speed, "/current_link_speed"
STR s_link_width, "/current_link_width"

# w_num_left(rdi, esi = width, dl = pad): a number left-aligned in width.
w_num_left:
	pushq %rsi
	movq wp(%rip), %rax
	pushq %rax
	call w_dec
	popq %rax
	popq %rsi
	movq wp(%rip), %rcx
	subq %rax, %rcx
	subl %ecx, %esi
	jle 1f
	movl %esi, %edi
	movb $' ', %sil
	jmp w_pad
1:	ret

	.globl socket_answers
# socket_answers(rdi = path) -> eax 1 when a connect succeeds.
socket_answers:
	pushq %rbx
	pushq %r12
	subq $112, %rsp
	movq %rdi, %r12
	movl $AF_UNIX, %edi
	movl $SOCK_STREAM | SOCK_CLOEXEC, %esi
	xorl %edx, %edx
	SYS SYS_socket
	testq %rax, %rax
	js 8f
	movl %eax, %ebx
	movw $AF_UNIX, (%rsp)
	leaq 2(%rsp), %rdi
	movq %r12, %rsi
	call copy_cstr
	movl %ebx, %edi
	movq %rsp, %rsi
	movl $110, %edx
	SYS SYS_connect
	movq %rax, %r12
	movl %ebx, %edi
	SYS SYS_close
	xorl %eax, %eax
	testq %r12, %r12
	setz %al
	jmp 9f
8:	xorl %eax, %eax
9:	addq $112, %rsp
	popq %r12
	popq %rbx
	ret

# ------------------------------------------------------------ info

cmd_info:
	pushq %rbx
	pushq %r12
	subq $8, %rsp
	call open_card
	call o_begin
	leaq s_i_device(%rip), %rsi
	call w_cstr
	movq bdf_ptr(%rip), %rsi
	call w_cstr
	call o_end
	call o_begin
	leaq s_i_ids(%rip), %rsi
	call w_cstr
	movl $PCI_VENDOR_ID, %edi
	call cfg_read16
	movl %eax, %edi
	call w_hex4x
	movb $':', %dil
	call w_byte
	movl $PCI_DEVICE_ID, %edi
	call cfg_read16
	movl %eax, %edi
	call w_hex4x
	leaq s_i_subsys(%rip), %rsi
	call w_cstr
	movl $PCI_SUBSYSTEM_VENDOR_ID, %edi
	call cfg_read16
	movl %eax, %edi
	call w_hex4x
	movb $':', %dil
	call w_byte
	movl $PCI_SUBSYSTEM_ID, %edi
	call cfg_read16
	movl %eax, %edi
	call w_hex4x
	call o_end
	call o_begin
	leaq s_i_cmd(%rip), %rsi
	call w_cstr
	movl $PCI_COMMAND, %edi
	call cfg_read16
	movl %eax, %ebx
	movl %eax, %edi
	movl $4, %esi
	call w_hexw
	leaq s_i_mem(%rip), %rsi
	call w_cstr
	movl %ebx, %edi
	andl $2, %edi
	call w_bool
	leaq s_i_bm(%rip), %rsi
	call w_cstr
	movl %ebx, %edi
	andl $4, %edi
	call w_bool
	movb $')', %dil
	call w_byte
	call o_end
	call o_begin
	leaq s_i_aper(%rip), %rsi
	call w_cstr
	movq aper_len(%rip), %rdi
	shrq $20, %rdi
	call w_dec
	leaq s_i_mib(%rip), %rsi
	call w_cstr
	call o_end
	call o_begin
	leaq s_i_post(%rip), %rsi
	call w_cstr
	call postcode
	movl %eax, %ebx
	movl %eax, %edi
	movl $8, %esi
	call w_hexw
	leaq s_i_arrow(%rip), %rsi
	call w_cstr
	movl %ebx, %edi
	call post_text
	call w_bytes
	leaq s_i_quote_sp(%rip), %rsi
	call w_cstr
	movl %ebx, %edi
	call post_describe
	cmpb $0, (%rsi)
	jne 1f
	leaq s_i_unknown(%rip), %rsi
1:	call w_cstr
	call o_end
	# spad2
	call download_info
	movl %eax, %ebx
	call o_begin
	leaq s_i_spad2(%rip), %rsi
	call w_cstr
	movl %ebx, %edi
	movl $8, %esi
	call w_hexw
	leaq s_i_ready(%rip), %rsi
	call w_cstr
	movl %ebx, %edi
	andl $1, %edi
	call w_bool
	leaq s_i_apic(%rip), %rsi
	call w_cstr
	movl %ebx, %edi
	shrl $1, %edi
	andl $0x1ff, %edi
	call w_dec
	leaq s_i_dl(%rip), %rsi
	call w_cstr
	movl %ebx, %edi
	andl $0xfffff000, %edi
	call w_hex
	call o_end
	# spad4
	movl $SPAD_PLATFORM_INFO, %edi
	call spad
	movl %eax, %ebx
	call o_begin
	leaq s_i_spad4(%rip), %rsi
	call w_cstr
	movl %ebx, %edi
	movl $8, %esi
	call w_hexw
	leaq s_i_threads(%rip), %rsi
	call w_cstr
	movl %ebx, %edi
	andl $0xf, %edi
	call w_hex
	leaq s_i_l2(%rip), %rsi
	call w_cstr
	movl %ebx, %edi
	shrl $4, %edi
	andl $3, %edi
	movl $512, %eax
	cmpl $3, %edi
	jne 2f
	movl $256, %eax
2:	movl %eax, %edi
	call w_dec
	leaq s_i_channels(%rip), %rsi
	call w_cstr
	movl %ebx, %edi
	shrl $6, %edi
	andl $0xf, %edi
	incl %edi
	call w_dec
	leaq s_i_icc(%rip), %rsi
	call w_cstr
	movl %ebx, %edi
	shrl $25, %edi
	andl $0x1f, %edi
	call w_dec
	leaq s_i_ref(%rip), %rsi
	call w_cstr
	movl %ebx, %edi
	call reference_mhz
	movl %eax, %edi
	call w_dec
	leaq s_i_soft(%rip), %rsi
	call w_cstr
	movl %ebx, %edi
	shrl $30, %edi
	andl $1, %edi
	call w_bool
	call o_end
	# clock ratio
	movl $CURRENT_CLK_RATIO, %edi
	call sbox_read
	movl %eax, %r12d
	call o_begin
	leaq s_i_clk(%rip), %rsi
	call w_cstr
	movl %r12d, %edi
	movl $8, %esi
	call w_hexw
	leaq s_i_fb(%rip), %rsi
	call w_cstr
	movl %r12d, %edi
	shrl $1, %edi
	andl $0xff, %edi
	call w_dec
	leaq s_i_ff(%rip), %rsi
	call w_cstr
	movl %r12d, %edi
	call feedforward_div
	movl %eax, %edi
	call w_dec
	leaq s_i_core(%rip), %rsi
	call w_cstr
	movl %r12d, %edi
	movl %ebx, %esi
	call core_mhz
	movl %eax, %edi
	call w_dec
	leaq s_i_mhz(%rip), %rsi
	call w_cstr
	call o_end
	xorl %r12d, %r12d
3:	cmpl $16, %r12d
	jae 4f
	call o_begin
	leaq s_i_spad(%rip), %rsi
	call w_cstr
	movl %r12d, %edi
	movl $2, %esi
	movb $' ', %dl
	call w_num_left
	leaq s_i_spadsp(%rip), %rsi
	call w_cstr
	movl %r12d, %edi
	call spad
	movl %eax, %edi
	movl $8, %esi
	call w_hexw
	call o_end
	incl %r12d
	jmp 3b
4:	addq $8, %rsp
	popq %r12
	popq %rbx
	ret
STR s_i_device, "device        "
STR s_i_ids, "ids           "
STR s_i_subsys, " subsystem "
STR s_i_cmd, "pci command   "
STR s_i_mem, " (memory "
STR s_i_bm, " bus master "
STR s_i_aper, "aperture      "
STR s_i_mib, " MiB mapped"
STR s_i_post, "postcode      "
STR s_i_arrow, " -> \""
STR s_i_quote_sp, "\" "
STR s_i_unknown, "(unknown code)"
STR s_i_spad2, "spad2         "
STR s_i_ready, ": ready="
STR s_i_apic, " apic_id="
STR s_i_dl, " download_addr="
STR s_i_spad4, "spad4         "
STR s_i_threads, ": threads/core "
STR s_i_l2, " l2 "
STR s_i_channels, " KiB channels "
STR s_i_icc, " icc_div "
STR s_i_ref, " ref "
STR s_i_soft, " MHz soft_reset="
STR s_i_clk, "clk ratio     "
STR s_i_fb, ": fb "
STR s_i_ff, " ff "
STR s_i_core, " -> core "
STR s_i_mhz, " MHz"
STR s_i_spad, "spad"
STR s_i_spadsp, "        "
STR s_true, "true"
STR s_false, "false"

# w_bool(edi): "true" when non-zero.
w_bool:
	leaq s_true(%rip), %rsi
	testl %edi, %edi
	jnz w_cstr
	leaq s_false(%rip), %rsi
	jmp w_cstr

# w_hex4x(edi): four lowercase hex digits ({:04x}).
w_hex4x:
	pushq %rdi
	shrl $8, %edi
	call w_hex2
	popq %rdi
	jmp w_hex2

	.globl reference_mhz
# reference_mhz(edi = spad4) -> eax: 4000 / the ICC divider (20 when 0).
reference_mhz:
	shrl $25, %edi
	andl $0x1f, %edi
	jnz 1f
	movl $ICC_NOMINAL_DIVIDER, %edi
1:	movl $CORE_VCO_MHZ, %eax
	xorl %edx, %edx
	divl %edi
	ret

	.globl feedforward_div
# feedforward_div(edi = ratio) -> eax: code 3 is 1, 2 is 2, else 4.
feedforward_div:
	shrl $9, %edi
	andl $3, %edi
	movl $4, %eax
	cmpl $3, %edi
	jne 1f
	movl $1, %eax
	ret
1:	cmpl $2, %edi
	jne 2f
	movl $2, %eax
2:	ret

	.globl core_mhz
# core_mhz(edi = ratio, esi = spad4) -> eax: reference * feedback / ff.
core_mhz:
	pushq %rbx
	pushq %r12
	movl %edi, %ebx
	movl %esi, %edi
	call reference_mhz
	movl %ebx, %edi
	shrl $1, %edi
	andl $0xff, %edi
	imull %edi, %eax
	movl %eax, %r12d
	movl %ebx, %edi
	call feedforward_div
	movl %eax, %ecx
	movl %r12d, %eax
	xorl %edx, %edx
	divl %ecx
	popq %r12
	popq %rbx
	ret

# ------------------------------------------------------------ postcode

cmd_postcode:
	pushq %rbx
	pushq %r12
	pushq %r13
	xorl %r12d, %r12d		# watch
	movq $-1, %r13			# timeout ms
1:	call opt_next
	testq %rax, %rax
	jz 3f
	leaq s_opt_watch(%rip), %rdi
	call arg_is
	testl %eax, %eax
	jz 2f
	movl $1, %r12d
	jmp 1b
2:	movq %r11, %rax
	leaq s_opt_timeout(%rip), %rdi
	call arg_is
	testl %eax, %eax
	jz usage
	call opt_num
	imulq $1000, %rax, %r13
	jmp 1b
3:	call open_card
	call postcode
	movl %eax, %ebx
	call o_begin
	movb $'"', %dil
	call w_byte
	movl %ebx, %edi
	call post_text
	call w_bytes
	leaq s_i_quote_sp(%rip), %rsi
	call w_cstr
	movl %ebx, %edi
	call post_describe
	call w_cstr
	call o_end
	testl %r12d, %r12d
	jz 9f
	call o_flush
	call now_ms
	movq %rax, %r12			# start
	testq %r13, %r13
	js 4f
	addq %rax, %r13
4:	call postcode
	cmpl %ebx, %eax
	je 5f
	movl %eax, %ebx
	call o_begin
	call now_ms
	subq %r12, %rax
	movq %rax, %rdi
	call l_elapsed
	movb $'"', %dil
	call w_byte
	movl %ebx, %edi
	call post_text
	call w_bytes
	leaq s_i_quote_sp(%rip), %rsi
	call w_cstr
	movl %ebx, %edi
	call post_describe
	call w_cstr
	call o_end
	call o_flush
5:	testq %r13, %r13
	js 6f
	call now_ms
	cmpq %r13, %rax
	jae 9f
6:	movl $20, %edi
	call sleep_ms
	jmp 4b
9:	popq %r13
	popq %r12
	popq %rbx
	ret
STR s_opt_watch, "--watch"
STR s_opt_timeout, "--timeout"

# ------------------------------------------------------------ spad

cmd_spad:
	pushq %rbx
	call opt_next
	testq %rax, %rax
	jz 2f
	movq %rax, %rsi
	call cstr_len
	call parse_u64
	testl %edx, %edx
	jz usage
	cmpq $16, %rax
	jae 8f
	movq %rax, %rbx
	call open_card
	call o_begin
	leaq s_i_spad(%rip), %rsi
	call w_cstr
	movq %rbx, %rdi
	call w_dec
	movb $' ', %dil
	call w_byte
	movl %ebx, %edi
	call spad
	movl %eax, %edi
	movl $8, %esi
	call w_hexw
	call o_end
	popq %rbx
	ret
2:	call open_card
	xorl %ebx, %ebx
3:	call o_begin
	leaq s_i_spad(%rip), %rsi
	call w_cstr
	movl %ebx, %edi
	movl $2, %esi
	movb $' ', %dl
	call w_num_left
	movb $' ', %dil
	call w_byte
	movl %ebx, %edi
	call spad
	movl %eax, %edi
	movl $8, %esi
	call w_hexw
	call o_end
	incl %ebx
	cmpl $16, %ebx
	jb 3b
	popq %rbx
	ret
8:	leaq s_e_spad(%rip), %rsi
	jmp fail
STR s_e_spad, "scratchpad index out of range 0..16"

# ------------------------------------------------------------ regs

cmd_regs:
	pushq %rbx
	pushq %r12
	call open_card
	call o_begin
	leaq s_r_hdr(%rip), %rsi
	call w_cstr
	call o_end
	leaq s_r_postcode(%rip), %rsi
	movl $POSTCODE, %edi
	call postcode
	movl %eax, %edx
	call reg_line
	leaq reg_table(%rip), %rbx
1:	movq (%rbx), %rsi
	testq %rsi, %rsi
	jz 2f
	movl 8(%rbx), %edi
	call sbox_read
	movl %eax, %edx
	movq (%rbx), %rsi
	movl 8(%rbx), %edi
	call reg_line
	addq $16, %rbx
	jmp 1b
2:	xorl %r12d, %r12d
3:	leal SMPT00(,%r12,4), %edi
	call sbox_read
	testl %eax, %eax
	jz 4f
	movl %eax, %r11d
	call o_begin
	leaq s_r_smpt(%rip), %rsi
	call w_cstr
	movl %r12d, %edi
	movl $2, %esi
	movb $'0', %dl
	pushq %r11
	call w_decw
	movl $15 - 6 + 2, %edi
	movb $' ', %sil
	call w_pad
	leal SMPT00(,%r12,4), %edi
	movl $4, %esi
	call w_hexw
	leaq s_two_sp(%rip), %rsi
	call w_cstr
	popq %rdi
	movl $8, %esi
	call w_hexw
	call o_end
4:	incl %r12d
	cmpl $SMPT_COUNT, %r12d
	jb 3b
	xorl %r12d, %r12d
5:	leal RDMASR0(,%r12,4), %edi
	call sbox_read
	movl %eax, %r11d
	call o_begin
	leaq s_r_rdmasr(%rip), %rsi
	call w_cstr
	movl %r12d, %edi
	pushq %r11
	call w_dec
	movl $15 - 7 + 2, %edi
	movb $' ', %sil
	call w_pad
	leal RDMASR0(,%r12,4), %edi
	movl $4, %esi
	call w_hexw
	leaq s_two_sp(%rip), %rsi
	call w_cstr
	popq %rdi
	movl $8, %esi
	call w_hexw
	call o_end
	incl %r12d
	cmpl $8, %r12d
	jb 5b
	popq %r12
	popq %rbx
	ret
# reg_line(rsi = name, edi = offset, edx = value): "name  0xoffs  0xvalue".
reg_line:
	pushq %rdi
	pushq %rdx
	call o_begin
	movq 8(%rsp), %rdi
	movq (%rsp), %rdx
	# name padded to 14, a space, the offset right-aligned in 8 ({:>#8x})
	movl $15, %ecx
	pushq %rdi
	pushq %rdx
	call w_left_cstr
	popq %rdx
	popq %rdi
	pushq %rdx
	movl $4, %esi
	cmpl $0xfff, %edi
	ja 1f
	movl $3, %esi
1:	pushq %rdi
	pushq %rsi
	movl $8 - 2, %edi
	subl %esi, %edi
	movb $' ', %sil
	call w_pad
	popq %rsi
	popq %rdi
	movl $0, %esi
	call w_hex
	leaq s_two_sp(%rip), %rsi
	call w_cstr
	popq %rdi
	movl $8, %esi
	call w_hexw
	call o_end
	popq %rdx
	popq %rdi
	ret
STR s_r_hdr, "register         offset  value"
STR s_r_postcode, "POSTCODE"
STR s_r_smpt, "SMPT"
STR s_r_rdmasr, "RDMASR"
STR s_two_sp, "  "
	.section .rodata
	.p2align 3
reg_table:
	.quad r_rgcr; .long RGCR, 0
	.quad r_sicr0; .long SICR0, 0
	.quad r_sice0; .long SICE0, 0
	.quad r_sicc0; .long SICC0, 0
	.quad r_siac0; .long SIAC0, 0
	.quad r_mxar0; .long MXAR0, 0
	.quad r_msixpbacr; .long MSIXPBACR, 0
	.quad r_icr7lo; .long APICICR0 + 56, 0
	.quad r_icr7hi; .long APICICR0 + 60, 0
	.quad r_sdbic0; .long SDBIC0, 0
	.quad 0; .long 0, 0
r_rgcr:	.asciz "RGCR"
r_sicr0:	.asciz "SICR0"
r_sice0:	.asciz "SICE0"
r_sicc0:	.asciz "SICC0"
r_siac0:	.asciz "SIAC0"
r_mxar0:	.asciz "MXAR0"
r_msixpbacr:	.asciz "MSIXPBACR"
r_icr7lo:	.asciz "APICICR7.lo"
r_icr7hi:	.asciz "APICICR7.hi"
r_sdbic0:	.asciz "SDBIC0"
	.text

# ------------------------------------------------------------ reset

cmd_reset:
	pushq %rbx
	pushq %r12
	movl $30000, %r12d
1:	call opt_next
	testq %rax, %rax
	jz 2f
	leaq s_opt_timeout(%rip), %rdi
	call arg_is
	testl %eax, %eax
	jz usage
	call opt_num
	imulq $1000, %rax, %r12
	jmp 1b
2:	call open_card
	call download_info
	movl %eax, %ebx
	movq %r12, %rdi
	call card_reset
	movq %rax, %r12
	call o_begin
	leaq s_rs1(%rip), %rsi
	call w_cstr
	movl %ebx, %edi
	movl $8, %esi
	call w_hexw
	leaq s_rs2(%rip), %rsi
	call w_cstr
	call download_info
	movl %eax, %ebx
	movl %eax, %edi
	movl $8, %esi
	call w_hexw
	leaq s_rs3(%rip), %rsi
	call w_cstr
	movq %r12, %rdi
	movl $1000, %esi
	movl $2, %edx
	call w_fixed
	movb $'s', %dil
	call w_byte
	call o_end
	call o_begin
	leaq s_rs4(%rip), %rsi
	call w_cstr
	movl %ebx, %edi
	andl $1, %edi
	call w_bool
	leaq s_i_apic(%rip), %rsi
	call w_cstr
	movl %ebx, %edi
	shrl $1, %edi
	andl $0x1ff, %edi
	call w_dec
	leaq s_i_dl(%rip), %rsi
	call w_cstr
	movl %ebx, %edi
	andl $0xfffff000, %edi
	call w_hex
	call o_end
	popq %r12
	popq %rbx
	ret
STR s_rs1, "spad2 before "
STR s_rs2, ", after "
STR s_rs3, "; ready after "
STR s_rs4, "now: ready="

# ------------------------------------------------------------ peek, poke, fill

	.bss
peek_buf:	.skip 256
	.text
cmd_peek:
	pushq %rbx
	pushq %r12
	call opt_num
	movq %rax, %rbx
	movl $4, %r12d
1:	call opt_next
	testq %rax, %rax
	jz 2f
	leaq s_opt_len(%rip), %rdi
	call arg_is
	testl %eax, %eax
	jz usage
	call opt_num
	cmpq $256, %rax
	jbe 11f
	movl $256, %eax
11:	movq %rax, %r12
	jmp 1b
2:	call open_card
	movl $20000, %edi
	call wait_ready
	movq %rbx, %rdi
	leaq peek_buf(%rip), %rsi
	movq %r12, %rdx
	call read_card_memory
	call o_begin
	movq %rbx, %rdi
	call w_hex
	leaq s_colon2(%rip), %rsi
	call w_cstr
	xorl %ebx, %ebx
3:	cmpq %r12, %rbx
	jae 4f
	testq %rbx, %rbx
	jz 31f
	movb $' ', %dil
	call w_byte
31:	leaq peek_buf(%rip), %rax
	movzbl (%rax,%rbx), %edi
	call w_hex2
	incq %rbx
	jmp 3b
4:	call o_end
	popq %r12
	popq %rbx
	ret
STR s_opt_len, "--len"
STR s_colon2, ": "

cmd_poke:
	pushq %rbx
	pushq %r12
	call opt_num
	movq %rax, %rbx
	call opt_num
	movq %rax, %r12
	call open_card
	movl $20000, %edi
	call wait_ready
	movq %r12, peek_buf(%rip)
	movq %rbx, %rdi
	leaq peek_buf(%rip), %rsi
	movl $8, %edx
	call write_card_memory
	call o_begin
	movq %rbx, %rdi
	call w_hex
	leaq s_arrow(%rip), %rsi
	call w_cstr
	movq %r12, %rdi
	call w_hex
	call o_end
	popq %r12
	popq %rbx
	ret
STR s_arrow, " <- "

cmd_fill:
	pushq %rbx
	pushq %r12
	pushq %r13
	pushq %r14
	pushq %r15
	call opt_num
	movq %rax, %rbx			# addr
	call opt_num
	movq %rax, %r12			# len
	movl $4096, %r13d		# chunk
	movl $1, %r14d			# readback
1:	call opt_next
	testq %rax, %rax
	jz 3f
	leaq s_opt_chunk(%rip), %rdi
	call arg_is
	testl %eax, %eax
	jz 2f
	call opt_num
	movq %rax, %r13
	jmp 1b
2:	movq %r11, %rax
	leaq s_opt_noreadback(%rip), %rdi
	call arg_is
	testl %eax, %eax
	jz usage
	xorl %r14d, %r14d
	jmp 1b
3:	call open_card
	movl $20000, %edi
	call wait_ready
	# an address-derived pattern: byte (addr + i) ^ 0x5a
	xorl %edi, %edi
	movq %r12, %rsi
	movl $PROT_READ | PROT_WRITE, %edx
	movl $MAP_PRIVATE | MAP_ANONYMOUS, %r10d
	movq $-1, %r8
	xorl %r9d, %r9d
	SYS SYS_mmap
	cmpq $-4096, %rax
	ja 8f
	movq %rax, %r15
	xorl %ecx, %ecx
4:	cmpq %r12, %rcx
	jae 5f
	leaq (%rbx,%rcx), %rax
	xorb $0x5a, %al
	movb %al, (%r15,%rcx)
	incq %rcx
	jmp 4b
5:	call now_ms
	pushq %rax
	movq %rbx, %rdi
	movq %r15, %rsi
	movq %r12, %rdx
	movq %r13, %rcx
	movl %r14d, %r8d
	call write_paced
	call now_ms
	popq %rcx
	subq %rcx, %rax
	pushq %rax
	call o_begin
	movq %r12, %rdi
	call w_hex
	leaq s_f_bytes(%rip), %rsi
	call w_cstr
	movq %rbx, %rdi
	call w_hex
	leaq s_f_in(%rip), %rsi
	call w_cstr
	popq %rdi
	movl $1000, %esi
	movl $3, %edx
	call w_fixed
	leaq s_f_chunk(%rip), %rsi
	call w_cstr
	movq %r13, %rdi
	call w_hex
	leaq s_f_readback(%rip), %rsi
	call w_cstr
	movl %r14d, %edi
	call w_bool
	movb $')', %dil
	call w_byte
	call o_end
	popq %r15
	popq %r14
	popq %r13
	popq %r12
	popq %rbx
	ret
8:	leaq s_e_mmap(%rip), %rsi
	jmp fail
STR s_opt_chunk, "--chunk"
STR s_opt_noreadback, "--no-readback"
STR s_f_bytes, " bytes at "
STR s_f_in, " in "
STR s_f_chunk, "s (chunk "
STR s_f_readback, ", readback "
STR s_e_mmap, "mmap of the fill pattern failed"
