# sysfs.S: PCI addresses, sysfs lookups and the cards list
# (host/crates/phi-vfio/src/sysfs.rs and cards.rs). See sysfs.md.
	.include "defs.inc"

	.set NAME_MAX_LEN, 256

	.bss
	.p2align 3
	.globl bdf_buf
# The normalized address "dddd:bb:dd.f", NUL-terminated.
bdf_buf:	.skip 16
	.globl cards, ncards
# The cards list: 64-byte entries: 0 index (u64), 8 bdf (13 bytes + NUL),
# 24 present (u64), 32 disk (pointer or 0), 40 hostmem (pointer or 0).
ncards:	.skip 8
cards:	.skip MAX_CARDS * 64
# Enumerated addresses, 16 bytes each, sorted.
nenum:	.skip 8
enumd:	.skip MAX_CARDS * 16
cfg_text:	.skip 8192
cfg_len:	.skip 8
dirbuf:	.skip 32768
small:	.skip 256
	.globl sock_path
sock_path:	.skip PATHCAP

	.text

	.globl normalize_bdf
# normalize_bdf(rsi, rdx) -> rax = the canonical address in bdf_buf, or
# 0 when malformed: [dddd:]bb:dd.f, hex, device at most 31, function at
# most 7.
normalize_bdf:
	pushq %rbx
	pushq %r12
	pushq %r13
	pushq %r14
	subq $56, %rsp
	call trim
	movq %rsi, %r12
	movq %rdx, %r13
	# split on ':' into at most three fields
	xorl %ebx, %ebx			# fields found
	movq %r12, %rdi
1:	movq %rdi, (%rsp,%rbx,8)	# field start
	movq %rdi, %rcx
2:	leaq (%r12,%r13), %rax
	cmpq %rax, %rcx
	jae 3f
	cmpb $':', (%rcx)
	je 3f
	incq %rcx
	jmp 2b
3:	movq %rcx, %rax
	subq %rdi, %rax
	movq %rax, 24(%rsp,%rbx,8)	# field length (slots 3..5)
	incl %ebx
	cmpl $3, %ebx
	ja 90f
	leaq (%r12,%r13), %rax
	cmpq %rax, %rcx
	jae 4f
	leaq 1(%rcx), %rdi
	jmp 1b
4:	cmpl $2, %ebx
	jb 90f
	# domain: field 0 when three, else "0000"
	movl $0, %r14d			# domain value
	cmpl $3, %ebx
	jne 5f
	cmpq $4, 24(%rsp)
	jne 90f
	movq 0(%rsp), %rsi
	movl $4, %edx
	call parse_hex
	testl %edx, %edx
	jz 90f
	movl %eax, %r14d
	# shift the bus and dev.fn fields down
	movq 8(%rsp), %rax
	movq %rax, 0(%rsp)
	movq 32(%rsp), %rax
	movq %rax, 24(%rsp)
	movq 16(%rsp), %rax
	movq %rax, 8(%rsp)
	movq 40(%rsp), %rax
	movq %rax, 32(%rsp)
5:	# bus: 2 hex digits
	cmpq $2, 24(%rsp)
	jne 90f
	movq 0(%rsp), %rsi
	movl $2, %edx
	call parse_hex
	testl %edx, %edx
	jz 90f
	movl %eax, %ebx		# bus
	# dev.fn: "dd.f"
	cmpq $4, 32(%rsp)
	jne 90f
	movq 8(%rsp), %rsi
	cmpb $'.', 2(%rsi)
	jne 90f
	movl $2, %edx
	call parse_hex
	testl %edx, %edx
	jz 90f
	cmpl $31, %eax
	ja 90f
	movl %eax, %r13d	# dev
	movq 8(%rsp), %rsi
	addq $3, %rsi
	movl $1, %edx
	call parse_hex
	testl %edx, %edx
	jz 90f
	cmpl $7, %eax
	ja 90f
	movl %eax, %r12d	# fn
	# format
	movq wp(%rip), %rax
	pushq %rax
	leaq bdf_buf(%rip), %rax
	movq %rax, wp(%rip)
	movl %r14d, %edi
	call w_hex4
	movb $':', %dil
	call w_byte
	movl %ebx, %edi
	call w_hex2
	movb $':', %dil
	call w_byte
	movl %r13d, %edi
	call w_hex2
	movb $'.', %dil
	call w_byte
	leal '0'(%r12), %edi
	call w_byte
	xorl %edi, %edi
	call w_byte
	popq %rax
	movq %rax, wp(%rip)
	leaq bdf_buf(%rip), %rax
	jmp 99f
90:	xorl %eax, %eax
99:	addq $56, %rsp
	popq %r14
	popq %r13
	popq %r12
	popq %rbx
	ret

# w_hex4(edi): four lowercase hex digits.
w_hex4:
	pushq %rdi
	shrl $8, %edi
	call w_hex2
	popq %rdi
	jmp w_hex2

	.globl dev_path
# dev_path(rsi = bdf, rdi = "/attr") -> rax = "/sys/bus/pci/devices/<bdf>/attr".
dev_path:
	pushq %rdi
	pushq %rsi
	call p_begin
	leaq s_pci(%rip), %rsi
	call w_cstr
	popq %rsi
	call w_cstr
	popq %rsi
	call w_cstr
	jmp p_end
STR s_pci, "/sys/bus/pci/devices/"

	.globl device_exists
# device_exists(rsi = bdf) -> eax 1 when its sysfs directory exists.
device_exists:
	leaq s_empty(%rip), %rdi
	call dev_path
	movq %rax, %rdi
	xorl %esi, %esi
	SYS SYS_access
	xorl %ecx, %ecx
	testq %rax, %rax
	sete %cl
	movl %ecx, %eax
	ret
STR s_empty, ""

	.globl read_hex_attr
# read_hex_attr(rsi = bdf, rdi = "/attr") -> eax = the value, edx = 1, or
# edx = 0 ("0x8086\n").
read_hex_attr:
	call dev_path
	movq %rax, %rdi
	leaq small(%rip), %rsi
	movl $255, %edx
	call read_text
	leaq small(%rip), %rsi
	movq %rax, %rdx
	call trim
	leaq s_0x(%rip), %rdi
	call starts_cstr
	jmp parse_hex
STR s_0x, "0x"

	.globl ids_of
# ids_of(rsi = bdf) -> eax = vendor, ecx = device, edx = 1 or 0.
ids_of:
	pushq %rbx
	pushq %rsi
	leaq s_vendor(%rip), %rdi
	call read_hex_attr
	testl %edx, %edx
	jz 9f
	movl %eax, %ebx
	movq (%rsp), %rsi
	leaq s_device(%rip), %rdi
	call read_hex_attr
	testl %edx, %edx
	jz 9f
	movl %eax, %ecx
	movl %ebx, %eax
	movl $1, %edx
9:	popq %rsi
	popq %rbx
	ret
STR s_vendor, "/vendor"
STR s_device, "/device"

	.globl is_phi
# is_phi(rsi = bdf) -> eax 1 when 8086:225d.
is_phi:
	call ids_of
	testl %edx, %edx
	jz 1f
	cmpl $PCI_VENDOR_INTEL, %eax
	jne 1f
	cmpl $PCI_DEVICE_3120, %ecx
	jne 1f
	movl $1, %eax
	ret
1:	xorl %eax, %eax
	ret

	.globl iommu_group_of
# iommu_group_of(rsi = bdf) -> eax = group, edx = 1, or edx = 0.
iommu_group_of:
	leaq s_iommu(%rip), %rdi
	call dev_path
	movq %rax, %rdi
	leaq small(%rip), %rsi
	movl $255, %edx
	call read_link
	testq %rax, %rax
	js 8f
	leaq small(%rip), %rsi
	movq %rax, %rdx
	call base_name
	jmp parse_u32
8:	xorl %edx, %edx
	ret
STR s_iommu, "/iommu_group"

	.globl driver_of
# driver_of(rsi = bdf) -> rsi = the driver's name, rdx = length (0 when
# none).
driver_of:
	leaq s_driver(%rip), %rdi
	call dev_path
	movq %rax, %rdi
	leaq small(%rip), %rsi
	movl $255, %edx
	call read_link
	testq %rax, %rax
	js 8f
	leaq small(%rip), %rsi
	movq %rax, %rdx
	jmp base_name
8:	leaq small(%rip), %rsi
	xorl %edx, %edx
	ret
STR s_driver, "/driver"

	.globl read_attr
# read_attr(rsi = bdf, rdi = "/attr") -> rsi, rdx: the text, trimmed.
read_attr:
	call dev_path
	movq %rax, %rdi
	leaq small(%rip), %rsi
	movl $255, %edx
	call read_text
	leaq small(%rip), %rsi
	movq %rax, %rdx
	jmp trim

	.globl find_phis
# find_phis: every 8086:225d device, sorted, into enumd/nenum.
find_phis:
	pushq %rbx
	pushq %r12
	pushq %r13
	pushq %r14
	subq $8, %rsp
	movq $0, nenum(%rip)
	leaq s_pcidir(%rip), %rdi
	movl $O_RDONLY | O_DIRECTORY | O_CLOEXEC, %esi
	xorl %edx, %edx
	SYS SYS_open
	testq %rax, %rax
	js 8f
	movl %eax, %ebx
1:	movl %ebx, %edi
	leaq dirbuf(%rip), %rsi
	movl $32768, %edx
	SYS SYS_getdents64
	testq %rax, %rax
	jle 7f
	movq %rax, %r13
	xorl %r12d, %r12d
2:	cmpq %r13, %r12
	jae 1b
	leaq dirbuf(%rip), %r14
	addq %r12, %r14
	movzwl 16(%r14), %eax
	addq %rax, %r12
	addq $19, %r14
	movq %r14, %rsi
	call cstr_len
	cmpq $12, %rdx
	jne 2b
	movq %r14, %rsi
	call normalize_bdf
	testq %rax, %rax
	jz 2b
	movq %rax, %rsi
	call is_phi
	testl %eax, %eax
	jz 2b
	movq nenum(%rip), %rcx
	cmpq $MAX_CARDS, %rcx
	jae 2b
	shlq $4, %rcx
	leaq enumd(%rip), %rdi
	addq %rcx, %rdi
	leaq bdf_buf(%rip), %rsi
	movl $13, %ecx
	rep movsb
	incq nenum(%rip)
	jmp 2b
7:	movl %ebx, %edi
	SYS SYS_close
	# sort (insertion; at most 16 entries of 13 bytes)
	movq nenum(%rip), %r13
	movl $1, %r12d
3:	cmpq %r13, %r12
	jae 8f
	movq %r12, %rbx
4:	testq %rbx, %rbx
	jz 5f
	movq %rbx, %rax
	shlq $4, %rax
	leaq enumd(%rip), %rsi
	addq %rax, %rsi
	leaq -16(%rsi), %rdi
	movl $13, %ecx
	repe cmpsb
	jae 5f
	# swap entries rbx-1 and rbx
	movq %rbx, %rax
	shlq $4, %rax
	leaq enumd(%rip), %rsi
	addq %rax, %rsi
	movq (%rsi), %rax
	movq 8(%rsi), %rcx
	movq -16(%rsi), %rdx
	movq %rdx, (%rsi)
	movq -8(%rsi), %rdx
	movq %rdx, 8(%rsi)
	movq %rax, -16(%rsi)
	movq %rcx, -8(%rsi)
	decq %rbx
	jmp 4b
5:	incq %r12
	jmp 3b
8:	addq $8, %rsp
	popq %r14
	popq %r13
	popq %r12
	popq %rbx
	ret
STR s_pcidir, "/sys/bus/pci/devices"

	.globl config_path
# config_path() -> rax: $XDG_CONFIG_HOME/phi/cards, else ~/.config/phi/cards.
config_path:
	call p_begin
	leaq s_xdg_config(%rip), %rdi
	call getenv
	testq %rax, %rax
	jz 1f
	cmpb $0, (%rax)
	je 1f
	movq %rax, %rsi
	call w_cstr
	leaq s_phicards(%rip), %rsi
	call w_cstr
	jmp p_end
1:	leaq s_home(%rip), %rdi
	call getenv
	testq %rax, %rax
	jnz 2f
	leaq s_root(%rip), %rax
2:	movq %rax, %rsi
	call w_cstr
	leaq s_config_cards(%rip), %rsi
	call w_cstr
	jmp p_end
STR s_xdg_config, "XDG_CONFIG_HOME"
STR s_phicards, "/phi/cards"
STR s_home, "HOME"
STR s_root, "/root"
STR s_config_cards, "/.config/phi/cards"

	.globl cards_list
# cards_list: fill cards/ncards from the configuration file when it
# exists, else from the enumerated cards; presence from sysfs. Exits with
# a message on a malformed file.
cards_list:
	pushq %rbx
	pushq %r12
	pushq %r13
	pushq %r14
	pushq %r15
	subq $8, %rsp
	call find_phis
	movq $0, ncards(%rip)
	call config_path
	movq %rax, %rdi
	leaq cfg_text(%rip), %rsi
	movl $8191, %edx
	call read_file
	testq %rax, %rax
	js 30f
	movq %rax, cfg_len(%rip)
	leaq cfg_text(%rip), %r12
	leaq (%r12,%rax), %r13
	xorl %r14d, %r14d		# line number
1:	movq %r12, %rsi
	movq %r13, %rdi
	call next_line
	movq %rsi, %r12
	testq %rax, %rax
	jz 40f
	incl %r14d
	# cut at '#'
	movq %rax, %rsi
	movb $'#', %cl
	pushq %rsi
	pushq %rdx
	call find_byte
	popq %rdx
	popq %rsi
	testq %rax, %rax
	js 2f
	movq %rax, %rdx
2:	leaq (%rsi,%rdx), %rdi
	movq %rdi, (%rsp)	# the cut line's end
	call next_token
	testq %rax, %rax
	jz 1b
	movq %rsi, %r15		# past the address
	movq %rax, %rsi
	call normalize_bdf
	testq %rax, %rax
	jz 91f
	# duplicate?
	movq ncards(%rip), %rcx
3:	testq %rcx, %rcx
	jz 4f
	decq %rcx
	movq %rcx, %rax
	shlq $6, %rax
	leaq cards+8(%rip), %rsi
	addq %rax, %rsi
	leaq bdf_buf(%rip), %rdi
	call eq_cc
	testl %eax, %eax
	jnz 92f
	jmp 3b
4:	cmpq $MAX_CARDS, ncards(%rip)
	jae 93f
	movq ncards(%rip), %rax
	movq %rax, %rcx
	shlq $6, %rcx
	leaq cards(%rip), %rdi
	addq %rcx, %rdi
	movq %rax, (%rdi)
	pushq %rdi
	addq $8, %rdi
	leaq bdf_buf(%rip), %rsi
	movl $13, %ecx
	rep movsb
	popq %rdi
	movq $0, 24(%rdi)
	movq $0, 32(%rdi)
	movq $0, 40(%rdi)
	# optional disk and hostmem fields, NUL-terminated in place (the
	# text buffer is this program's); the bound is the cut line's end
	movq %rdi, %rbx
	movq %r15, %rsi
	movq (%rsp), %rdi
	call next_token
	testq %rax, %rax
	jz 6f
	movq %rax, 32(%rbx)
	movq %rdx, 48(%rbx)
	call next_token
	testq %rax, %rax
	jz 5f
	movq %rax, 40(%rbx)
	movb $0, (%rax,%rdx)
5:	movq 32(%rbx), %rax
	movq 48(%rbx), %rdx
	movb $0, (%rax,%rdx)
6:	incq ncards(%rip)
	jmp 1b
30:	# no file: the enumerated cards, in address order
	movq nenum(%rip), %r13
	xorl %r12d, %r12d
31:	cmpq %r13, %r12
	jae 40f
	movq %r12, %rax
	shlq $6, %rax
	leaq cards(%rip), %rdi
	addq %rax, %rdi
	movq %r12, (%rdi)
	movq $0, 24(%rdi)
	movq $0, 32(%rdi)
	movq $0, 40(%rdi)
	addq $8, %rdi
	movq %r12, %rax
	shlq $4, %rax
	leaq enumd(%rip), %rsi
	addq %rax, %rsi
	movl $13, %ecx
	rep movsb
	incq %r12
	movq %r12, ncards(%rip)
	jmp 31b
40:	# presence
	movq ncards(%rip), %r13
	xorl %r12d, %r12d
41:	cmpq %r13, %r12
	jae 50f
	movq %r12, %rax
	shlq $6, %rax
	leaq cards(%rip), %rbx
	addq %rax, %rbx
	movq nenum(%rip), %r15
42:	testq %r15, %r15
	jz 44f
	decq %r15
	movq %r15, %rax
	shlq $4, %rax
	leaq enumd(%rip), %rsi
	addq %rax, %rsi
	leaq 8(%rbx), %rdi
	call eq_cc
	testl %eax, %eax
	jz 42b
	movq $1, 24(%rbx)
44:	incq %r12
	jmp 41b
50:	addq $8, %rsp
	popq %r15
	popq %r14
	popq %r13
	popq %r12
	popq %rbx
	ret
91:	leaq s_cfg_bad(%rip), %rsi
	jmp cfg_fail
92:	leaq s_cfg_dup(%rip), %rsi
	jmp cfg_fail
93:	leaq s_cfg_many(%rip), %rsi
cfg_fail:
	pushq %rsi
	call l_begin
	leaq s_prefix2(%rip), %rsi
	call w_cstr
	movl %r14d, %edi
	call w_dec
	leaq s_colon2(%rip), %rsi
	call w_cstr
	popq %rsi
	call w_cstr
	call l_end
	movl $1, %edi
	SYS SYS_exit_group
STR s_prefix2, "phictl: cards file line "
STR s_colon2, ": "
STR s_cfg_bad, "not a PCI address"
STR s_cfg_dup, "listed twice"
STR s_cfg_many, "more than 16 cards"

	.globl card_entry
# card_entry(rdi = index) -> rax = its entry in cards, or 0.
card_entry:
	movq ncards(%rip), %rcx
1:	testq %rcx, %rcx
	jz 9f
	decq %rcx
	movq %rcx, %rax
	shlq $6, %rax
	leaq cards(%rip), %rdx
	addq %rdx, %rax
	cmpq %rdi, (%rax)
	jne 1b
	ret
9:	xorl %eax, %eax
	ret

	.globl resolve_card
# resolve_card(rdi = index) -> rax = the entry; exits when the card is
# unknown or not on the bus.
resolve_card:
	pushq %rdi
	call cards_list
	movq (%rsp), %rdi
	call card_entry
	testq %rax, %rax
	jz 1f
	cmpq $0, 24(%rax)
	je 2f
	popq %rdi
	ret
1:	call l_begin
	leaq s_prefix(%rip), %rsi
	call w_cstr
	leaq s_nocard1(%rip), %rsi
	call w_cstr
	movq (%rsp), %rdi
	call w_dec
	leaq s_nocard2(%rip), %rsi
	call w_cstr
	movq ncards(%rip), %rdi
	call w_dec
	leaq s_nocard3(%rip), %rsi
	call w_cstr
	call l_end
	movl $1, %edi
	SYS SYS_exit_group
2:	pushq %rax
	call l_begin
	leaq s_prefix(%rip), %rsi
	call w_cstr
	leaq s_card(%rip), %rsi
	call w_cstr
	movq 8(%rsp), %rdi
	call w_dec
	leaq s_paren(%rip), %rsi
	call w_cstr
	movq (%rsp), %rsi
	addq $8, %rsi
	call w_cstr
	leaq s_notbus(%rip), %rsi
	call w_cstr
	call l_end
	movl $1, %edi
	SYS SYS_exit_group
STR s_prefix, "phictl: "
STR s_nocard1, "no card "
STR s_nocard2, ": "
STR s_nocard3, " known (phictl cards)"
STR s_card, "card "
STR s_paren, " ("
STR s_notbus, ") is not on the PCI bus"

	.globl index_from_env
# index_from_env() -> rax = PHI_CARD, or -1 when unset; exits when
# malformed or out of range.
index_from_env:
	leaq s_phi_card(%rip), %rdi
	call getenv
	testq %rax, %rax
	jz 8f
	movq %rax, %rsi
	call cstr_len
	call trim
	testq %rdx, %rdx
	jz 8f
	call parse_u64
	testl %edx, %edx
	jz 9f
	cmpq $MAX_CARDS, %rax
	jae 9f
	ret
8:	movq $-1, %rax
	ret
9:	leaq s_badcard(%rip), %rsi
	jmp fail
STR s_phi_card, "PHI_CARD"
STR s_badcard, "PHI_CARD is not a card index (0 to 15)"

	.globl socket_path_for
# socket_path_for(rdi = index, esi = 1 for the root path) -> rax = the
# control socket path in sock_path: PHICTL_SOCKET when set, else
# /run/phictl[/N]/control.sock, else $XDG_RUNTIME_DIR/phictl[/N]/control.sock
# (or /tmp/phictl-UID).
socket_path_for:
	pushq %rbx
	pushq %r12
	movq %rdi, %rbx
	movl %esi, %r12d
	leaq s_phictl_socket(%rip), %rdi
	call getenv
	testq %rax, %rax
	jz 1f
	leaq sock_path(%rip), %rdi
	movq %rax, %rsi
	call copy_cstr
	jmp 9f
1:	movq wp(%rip), %rax
	pushq %rax
	leaq sock_path(%rip), %rax
	movq %rax, wp(%rip)
	testl %r12d, %r12d
	jz 2f
	leaq s_run_phictl(%rip), %rsi
	call w_cstr
	jmp 4f
2:	leaq s_xdg_runtime(%rip), %rdi
	call getenv
	testq %rax, %rax
	jz 3f
	movq %rax, %rsi
	call w_cstr
	leaq s_slash_phictl(%rip), %rsi
	call w_cstr
	jmp 4f
3:	leaq s_tmp_phictl(%rip), %rsi
	call w_cstr
	SYS SYS_getuid
	movq %rax, %rdi
	call w_dec
	leaq s_slash_phictl(%rip), %rsi
	call w_cstr
4:	testq %rbx, %rbx
	jz 5f
	movb $'/', %dil
	call w_byte
	movq %rbx, %rdi
	call w_dec
5:	leaq s_control(%rip), %rsi
	call w_cstr
	xorl %edi, %edi
	call w_byte
	popq %rax
	movq %rax, wp(%rip)
9:	leaq sock_path(%rip), %rax
	popq %r12
	popq %rbx
	ret
STR s_phictl_socket, "PHICTL_SOCKET"
STR s_run_phictl, "/run/phictl"
STR s_xdg_runtime, "XDG_RUNTIME_DIR"
STR s_slash_phictl, "/phictl"
STR s_tmp_phictl, "/tmp/phictl-"
STR s_control, "/control.sock"

	.globl copy_cstr
# copy_cstr(rdi = dst, rsi = src): with the NUL.
copy_cstr:
1:	movb (%rsi), %al
	movb %al, (%rdi)
	incq %rsi
	incq %rdi
	testb %al, %al
	jnz 1b
	ret

	.globl hostmem_path
# hostmem_path(rdi = index) -> rax: /dev/shm/phi-hostmem[-N].
hostmem_path:
	pushq %rdi
	call p_begin
	leaq s_hostmem(%rip), %rsi
	call w_cstr
	popq %rdi
	testq %rdi, %rdi
	jz 1f
	pushq %rdi
	movb $'-', %dil
	call w_byte
	popq %rdi
	call w_dec
1:	jmp p_end
STR s_hostmem, "/dev/shm/phi-hostmem"
