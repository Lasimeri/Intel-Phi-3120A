# postcode.S: POST codes as the bootstrap and the card kernel write them
# (host/crates/phi-regs/src/postcode.rs): the register's low two bytes are
# two ASCII characters, low byte first. See postcode.md.
	.include "defs.inc"

	.bss
post_txt:	.skip 16

	.text

	.globl post_text
# post_text(edi = raw) -> rsi = text, rdx = length: the two characters
# when both are printable ASCII, else "0x%08x".
post_text:
	pushq %rdi
	movl %edi, %eax
	movl %edi, %ecx
	shrl $8, %ecx
	andl $0xff, %eax
	andl $0xff, %ecx
	cmpb $0x21, %al
	jb 1f
	cmpb $0x7e, %al
	ja 1f
	cmpb $0x21, %cl
	jb 1f
	cmpb $0x7e, %cl
	ja 1f
	leaq post_txt(%rip), %rsi
	movb %al, (%rsi)
	movb %cl, 1(%rsi)
	movl $2, %edx
	popq %rdi
	ret
1:	movq wp(%rip), %rax
	pushq %rax
	leaq post_txt(%rip), %rax
	movq %rax, wp(%rip)
	movq 8(%rsp), %rdi
	movl $8, %esi
	call w_hexw
	popq %rax
	movq %rax, wp(%rip)
	popq %rdi
	leaq post_txt(%rip), %rsi
	movl $10, %edx
	ret

	.globl post_is_ready
# post_is_ready(edi = raw) -> eax 1 for "12".
post_is_ready:
	andl $0xffff, %edi
	xorl %eax, %eax
	cmpl $0x3231, %edi
	sete %al
	ret

	.globl post_describe
# post_describe(edi = raw) -> rsi = description (NUL-terminated, "" when
# unknown), matched case-insensitively.
post_describe:
	movl %edi, %eax
	andl $0xffff, %eax
	orl $0x2020, %eax
	leaq post_table(%rip), %rcx
1:	movzwl (%rcx), %edx
	testl %edx, %edx
	jz 2f
	cmpl %eax, %edx
	je 3f
	addq $16, %rcx
	jmp 1b
2:	leaq d_none(%rip), %rsi
	ret
3:	movq 8(%rcx), %rsi
	ret

# One entry per code: the two lowercased bytes as a little-endian word,
# then the text. Intel's table (MPSS 2.1 readme) and this kernel's marks.
	.macro POST code, sym
	.short (\code & 0xff) | ((\code >> 8) << 8)
	.short 0, 0, 0
	.quad \sym
	.endm
	.macro C a, b
	.short (\a | 0x20) | ((\b | 0x20) << 8), 0, 0, 0
	.endm

	.section .rodata
	.p2align 3
post_table:
	C 'k','0'; .quad d_k0
	C 'k','1'; .quad d_k1
	C 'k','2'; .quad d_k2
	C 'k','3'; .quad d_k3
	C 'k','4'; .quad d_k4
	C 'k','5'; .quad d_k5
	C 'k','6'; .quad d_k6
	C 'k','7'; .quad d_k7
	C 'k','8'; .quad d_k8
	C 'k','f'; .quad d_kf
	C 'k','g'; .quad d_kg
	C 'k','9'; .quad d_k9
	C 'k','j'; .quad d_kj
	C 'k','l'; .quad d_kl
	C 'k','m'; .quad d_km
	C 'k','n'; .quad d_kn
	C 'k','o'; .quad d_ko
	C 'k','q'; .quad d_kq
	C 'k','r'; .quad d_kr
	C 'k','-'; .quad d_kdash
	C 'k','a'; .quad d_ka
	C 'k','b'; .quad d_kb
	C 'k','c'; .quad d_kc
	C 'k','d'; .quad d_kd
	C 'k','h'; .quad d_kh
	C 'k','p'; .quad d_kp
	C 's','0'; .quad d_s0
	C 's','1'; .quad d_s1
	C 's','2'; .quad d_s2
	C 's','3'; .quad d_s3
	C 's','4'; .quad d_s4
	C 's','5'; .quad d_s5
	C 's','6'; .quad d_s6
	C 'a','1'; .quad d_a1
	C 'a','2'; .quad d_a2
	C 'a','3'; .quad d_a3
	C 'a','4'; .quad d_a4
	C 'a','5'; .quad d_a5
	C 'a','6'; .quad d_a6
	C 'a','7'; .quad d_a7
	C 'a','8'; .quad d_a8
	C 'k','e'; .quad d_ke
	C '0','1'; .quad d_01
	C '0','2'; .quad d_02
	C '0','3'; .quad d_03
	C '0','4'; .quad d_04
	C '0','5'; .quad d_05
	C '0','6'; .quad d_06
	C '0','9'; .quad d_09
	C '0','b'; .quad d_0b
	C '0','c'; .quad d_0c
	C '0','d'; .quad d_0d
	C '0','e'; .quad d_0e
	C '0','f'; .quad d_0f
	C '1','0'; .quad d_10
	C '1','1'; .quad d_11
	C '1','2'; .quad d_12
	C '1','3'; .quad d_13
	C '1','5'; .quad d_15
	C '1','6'; .quad d_16
	C '1','7'; .quad d_17
	C '3','0'; .quad d_30
	C '3','1'; .quad d_31
	C '3','2'; .quad d_32
	C '3','3'; .quad d_33
	C '3','4'; .quad d_34
	C '3','5'; .quad d_35
	C '3','6'; .quad d_36
	C '3','7'; .quad d_37
	C '3','8'; .quad d_38
	C '3','9'; .quad d_39
	C '3','a'; .quad d_3a
	C '3','b'; .quad d_3b
	C '3','c'; .quad d_3c
	C '3','d'; .quad d_3d
	C '3','e'; .quad d_3e
	C '3','f'; .quad d_3f
	C '4','0'; .quad d_40
	C '5','0'; .quad d_5x
	C '5','1'; .quad d_5x
	C '5','2'; .quad d_5x
	C '5','3'; .quad d_5x
	C '5','4'; .quad d_5x
	C '5','5'; .quad d_5x
	C '5','6'; .quad d_5x
	C '5','7'; .quad d_5x
	C '5','8'; .quad d_5x
	C '5','9'; .quad d_5x
	C '5','a'; .quad d_5x
	C '5','b'; .quad d_5x
	C '5','c'; .quad d_5x
	C '5','d'; .quad d_5x
	C '5','e'; .quad d_5x
	C '5','f'; .quad d_5x
	C '7','5'; .quad d_75
	C '8','7'; .quad d_87
	C 'a','c'; .quad d_ac
	C 'c','c'; .quad d_cc
	C 'd','b'; .quad d_db
	C 'd','e'; .quad d_de
	C 'd','f'; .quad d_df
	C 'e','e'; .quad d_ee
	C 'f','0'; .quad d_f0
	C 'f','1'; .quad d_f1
	C 'f','2'; .quad d_f2
	C 'f','3'; .quad d_f3
	C 'f','4'; .quad d_f4
	C 'f','5'; .quad d_f5
	C 'f','f'; .quad d_ff
	.short 0, 0, 0, 0
	.quad 0
d_none:	.asciz ""
d_k0:	.asciz "kernel: decompressor entered"
d_k1:	.asciz "kernel: decompressed, jumping to the kernel"
d_k2:	.asciz "kernel: KNC platform layer running"
d_k3:	.asciz "kernel: SFI tables parsed"
d_k4:	.asciz "kernel: timer init"
d_k5:	.asciz "kernel: CPUs and I/O APIC registered"
d_k6:	.asciz "kernel: ring console attached"
d_k7:	.asciz "kernel: late initcalls done, starting init"
d_k8:	.asciz "kernel: startup_64 entered"
d_kf:	.asciz "kernel: startup_64, verify_cpu passed"
d_kg:	.asciz "kernel: startup_64, page tables built, switching"
d_k9:	.asciz "kernel: first C code, early exceptions set up"
d_kj:	.asciz "kernel: startup_64, new CR3 loaded"
d_kl:	.asciz "kernel: startup_64, running at the virtual address"
d_km:	.asciz "kernel: startup_64, stack and GS base set"
d_kn:	.asciz "kernel: startup_64, bringup IDT loaded"
d_ko:	.asciz "kernel: first C code entered"
d_kq:	.asciz "kernel: bss cleared, early page tables reset"
d_kr:	.asciz "kernel: early exception handlers installed"
d_kdash:	.asciz "kernel: boot mark untouched"
d_ka:	.asciz "kernel: setup_arch entered"
d_kb:	.asciz "kernel: early CPU init done"
d_kc:	.asciz "kernel: memory map set up"
d_kd:	.asciz "kernel: early parameters parsed"
d_kh:	.asciz "kernel: halted (restart or power off requested)"
d_kp:	.asciz "kernel: panic"
d_s0:	.asciz "kernel: smp: kicking an AP"
d_s1:	.asciz "kernel: smp: INIT asserted"
d_s2:	.asciz "kernel: smp: INIT assert delivered"
d_s3:	.asciz "kernel: smp: INIT deasserted"
d_s4:	.asciz "kernel: smp: SIPI 1 sent"
d_s5:	.asciz "kernel: smp: SIPI 2 sent"
d_s6:	.asciz "kernel: smp: wake-up sequence done"
d_a1:	.asciz "kernel: AP: trampoline, 32-bit protected mode"
d_a2:	.asciz "kernel: AP: trampoline, long mode"
d_a3:	.asciz "kernel: AP: start_secondary entered"
d_a4:	.asciz "kernel: AP: alive, released by the BSP"
d_a5:	.asciz "kernel: AP: cpu_init and FPU done"
d_a6:	.asciz "kernel: AP: local APIC set up"
d_a7:	.asciz "kernel: AP: TSC sync check passed"
d_a8:	.asciz "kernel: AP: entering idle (online)"
d_ke:	.asciz "kernel: fatal error before console (no SFI tables)"
d_01:	.asciz "LIDT"
d_02:	.asciz "SBOX initialization"
d_03:	.asciz "Set GDDR top"
d_04:	.asciz "Begin memory test"
d_05:	.asciz "Program E820 table"
d_06:	.asciz "Initialize DBOX"
d_09:	.asciz "Enable caching"
d_0b:	.asciz "Pass initialization parameters to APs"
d_0c:	.asciz "Cache C code"
d_0d:	.asciz "Program MP table"
d_0e:	.asciz "Copy AP boot code to GDDR"
d_0f:	.asciz "Wake up APs"
d_10:	.asciz "Wait for APs to boot"
d_11:	.asciz "Signal host to download coprocessor OS"
d_12:	.asciz "Wait for coprocessor OS download (ready)"
d_13:	.asciz "Signal received from host to boot coprocessor OS"
d_15:	.asciz "Report platform information"
d_16:	.asciz "(undocumented; observed between GDDR finalize and enable caching, 2026-09-13)"
d_17:	.asciz "Page table setup"
d_30:	.asciz "Begin memory training"
d_31:	.asciz "Begin GDDR training to query memory modules"
d_32:	.asciz "Find GDDR training parameters in flash"
d_33:	.asciz "Begin GDDR MMIO training"
d_34:	.asciz "Begin GDDR RCOMP training"
d_35:	.asciz "Begin GDDR DCC disable training"
d_36:	.asciz "Begin GDDR HCK training"
d_37:	.asciz "Begin GDDR ucode training"
d_38:	.asciz "Begin GDDR vendor specific training"
d_39:	.asciz "Begin GDDR address training"
d_3a:	.asciz "Begin GDDR memory module identification"
d_3b:	.asciz "Begin GDDR WCK training"
d_3c:	.asciz "Begin GDDR read training with CDR enabled"
d_3d:	.asciz "Begin GDDR read training with CDR disabled"
d_3e:	.asciz "Begin GDDR write training"
d_3f:	.asciz "Finalize GDDR training"
d_40:	.asciz "Begin coprocessor OS authentication"
d_5x:	.asciz "Coprocessor OS loading and setup"
d_75:	.asciz "int 10 Invalid TSS"
d_87:	.asciz "int 16 x87 FPU floating point error"
d_ac:	.asciz "int 17 Alignment check"
d_cc:	.asciz "int 18 Machine check"
d_db:	.asciz "int 1 Debug"
d_de:	.asciz "int 0 Divide error"
d_df:	.asciz "int 8 Double fault"
d_ee:	.asciz "Memory test failed"
d_f0:	.asciz "GDDR parameters not found in flash"
d_f1:	.asciz "GBOX PLL lock failure"
d_f2:	.asciz "GDDR failed memory training"
d_f3:	.asciz "GDDR memory module query failed"
d_f4:	.asciz "Memory preservation failure"
d_f5:	.asciz "int 12 Stack fault"
d_ff:	.asciz "Bootstrap finished execution"
	.text
