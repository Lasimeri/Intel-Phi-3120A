# boot.S: `phictl boot`, the image loader (host/crates/phi-hw/src/boot.rs
# and the boot command of phictl's main.rs): the bootstrap waited for,
# the bzImage checked, the ring region formatted in host memory and
# copied in, the kernel with its header patched, the command line, the
# initramfs, SPAD5, the boot interrupt; then the services (console.S).
# See boot.md.
	.include "defs.inc"

	.set HOSTMEM_IOVA, 0x100000000

	.bss
	.p2align 3
# Options.
	.globl o_kernel, o_initrd, o_cmdline, o_ring_base, o_ring_size
	.globl o_raw, o_no_console, o_load_only, o_watch, o_has_watch
	.globl o_net, o_net_addr, o_forward, o_serve, o_disk, o_no_dma
	.globl o_host_mem, o_owner, o_has_owner
o_kernel:	.skip 8
o_initrd:	.skip 8
o_cmdline:	.skip 8
o_ring_base:	.skip 8
o_ring_size:	.skip 8
o_raw:	.skip 8
o_no_console:	.skip 8
o_load_only:	.skip 8
o_watch:	.skip 8
o_has_watch:	.skip 8
o_net:	.skip 8
o_net_addr:	.skip 8
o_forward:	.skip 8
o_serve:	.skip 8
o_disk:	.skip 8
o_no_dma:	.skip 8
o_host_mem:	.skip 8
o_owner:	.skip 8
o_has_owner:	.skip 8
# The images.
kernel_ptr:	.skip 8
kernel_len:	.skip 8
initrd_ptr:	.skip 8
initrd_len:	.skip 8
bootaddr:	.skip 8
cmdline_addr:	.skip 8
initrd_addr:	.skip 8
cmdline_max:	.skip 8
protocol:	.skip 8
	.globl hostmem_ptr, hostmem_len, hostmem_card
# The host memory window: its mapping, length and card address.
hostmem_ptr:	.skip 8
hostmem_len:	.skip 8
hostmem_card:	.skip 8
cmdline_buf:	.skip 4096
net_addr_buf:	.skip 32
	.globl boot_start_ms
boot_start_ms:	.skip 8

	.text
	.globl cmd_boot
cmd_boot:
	pushq %rbx
	pushq %r12
	leaq s_default_cmdline(%rip), %rax
	movq %rax, o_cmdline(%rip)
	movq $RING_REGION_BASE, o_ring_base(%rip)
	movq $RING_REGION_BYTES, o_ring_size(%rip)
1:	call opt_next
	testq %rax, %rax
	jz 20f
	movq %rax, %rbx
	leaq boot_opts(%rip), %r12
2:	movq (%r12), %rdi
	testq %rdi, %rdi
	jz usage
	movq %rbx, %rsi
	call eq_cc
	testl %eax, %eax
	jnz 3f
	addq $24, %r12
	jmp 2b
3:	movq 8(%r12), %rcx	# the variable
	movq 16(%r12), %rax	# 0 flag, 1 string, 2 number, 3 optional string, 4 size
	testq %rax, %rax
	jnz 4f
	movq $1, (%rcx)
	jmp 1b
4:	cmpq $1, %rax
	jne 5f
	pushq %rcx
	call opt_value
	popq %rcx
	movq %rax, (%rcx)
	jmp 1b
5:	cmpq $2, %rax
	jne 6f
	pushq %rcx
	call opt_num
	popq %rcx
	movq %rax, (%rcx)
	jmp 1b
6:	cmpq $3, %rax
	jne 7f
	# --serve [PATH]: a value unless the next argument is an option
	pushq %rcx
	call opt_peek
	popq %rcx
	testq %rax, %rax
	jz 61f
	cmpb $'-', (%rax)
	je 61f
	pushq %rcx
	call opt_next
	popq %rcx
	movq %rax, (%rcx)
	jmp 1b
61:	leaq s_auto(%rip), %rax
	movq %rax, (%rcx)
	jmp 1b
7:	cmpq $4, %rax
	jne 8f
	pushq %rcx
	call opt_value
	movq %rax, %rsi
	call cstr_len
	call parse_size
	popq %rcx
	testl %edx, %edx
	jz 90f
	movq %rax, (%rcx)
	jmp 1b
8:	# 5: --watch (number + flag); 6: --owner (number + flag)
	pushq %rcx
	call opt_num
	popq %rcx
	movq %rax, (%rcx)
	movq $1, 8(%rcx)
	jmp 1b
20:	cmpq $0, o_kernel(%rip)
	je usage
	cmpq $0, o_net(%rip)
	je 21f
	cmpq $0, o_forward(%rip)
	jne 91f
21:	# --net-addr default 10.9.N.1/24
	cmpq $0, o_net_addr(%rip)
	jne 22f
	movq wp(%rip), %rax
	pushq %rax
	leaq net_addr_buf(%rip), %rax
	movq %rax, wp(%rip)
	leaq s_10_9(%rip), %rsi
	call w_cstr
	movq card_index(%rip), %rdi
	call w_dec
	leaq s_1_24(%rip), %rsi
	call w_cstr
	xorl %edi, %edi
	call w_byte
	popq %rax
	movq %rax, wp(%rip)
	leaq net_addr_buf(%rip), %rax
	movq %rax, o_net_addr(%rip)
22:	call open_card
	# the images
	movq o_kernel(%rip), %rdi
	call map_file_whole
	testq %rax, %rax
	js 92f
	movq %rax, kernel_ptr(%rip)
	movq %rdx, kernel_len(%rip)
	cmpq $0, o_initrd(%rip)
	je 23f
	movq o_initrd(%rip), %rdi
	call map_file_whole
	testq %rax, %rax
	js 93f
	movq %rax, initrd_ptr(%rip)
	movq %rdx, initrd_len(%rip)
23:	# host memory, pinned before the boot so its address can be announced
	cmpq $0, o_host_mem(%rip)
	je 24f
	call hostmem_pin
24:	call now_ms
	movq %rax, boot_start_ms(%rip)
	call do_boot
	# the card reaches host memory through the SMPT: its first pages
	# identity, so the window announced above is valid before the card
	# touches it
	movl $4, %edi
	call smpt_identity
	call report
	# services, unless nothing is to follow
	cmpq $0, o_load_only(%rip)
	jne 29f
	cmpq $0, o_no_console(%rip)
	jne 29f
	call daemon
29:	popq %r12
	popq %rbx
	ret
90:	leaq s_e_size(%rip), %rsi
	jmp fail
91:	leaq s_e_netfwd(%rip), %rsi
	jmp fail
92:	negq %rax
	movq %rax, %rdi
	movq o_kernel(%rip), %rsi
	jmp fail_os
93:	negq %rax
	movq %rax, %rdi
	movq o_initrd(%rip), %rsi
	jmp fail_os
STR s_default_cmdline, "earlyprintk=phiring,keep loglevel=8"
STR s_auto, "auto"
STR s_10_9, "10.9."
STR s_1_24, ".1/24"
STR s_e_size, "--host-mem takes a size (K, M, G) that is a positive multiple of 2 MiB"
STR s_e_netfwd, "--net and --forward cannot be used together"

	.section .rodata
	.p2align 3
# option name, its variable, its kind.
boot_opts:
	.quad b_kernel, o_kernel, 1
	.quad b_initrd, o_initrd, 1
	.quad b_cmdline, o_cmdline, 1
	.quad b_ring_base, o_ring_base, 2
	.quad b_ring_size, o_ring_size, 2
	.quad b_raw, o_raw, 0
	.quad b_no_console, o_no_console, 0
	.quad b_load_only, o_load_only, 0
	.quad b_watch, o_watch, 5
	.quad b_net, o_net, 1
	.quad b_net_addr, o_net_addr, 1
	.quad b_forward, o_forward, 1
	.quad b_serve, o_serve, 3
	.quad b_disk, o_disk, 1
	.quad b_no_dma, o_no_dma, 0
	.quad b_host_mem, o_host_mem, 4
	.quad b_owner, o_owner, 5
	.quad 0, 0, 0
b_kernel:	.asciz "--kernel"
b_initrd:	.asciz "--initrd"
b_cmdline:	.asciz "--cmdline"
b_ring_base:	.asciz "--ring-base"
b_ring_size:	.asciz "--ring-size"
b_raw:	.asciz "--raw-cmdline"
b_no_console:	.asciz "--no-console"
b_load_only:	.asciz "--load-only"
b_watch:	.asciz "--watch"
b_net:	.asciz "--net"
b_net_addr:	.asciz "--net-addr"
b_forward:	.asciz "--forward"
b_serve:	.asciz "--serve"
b_disk:	.asciz "--disk"
b_no_dma:	.asciz "--no-dma"
b_host_mem:	.asciz "--host-mem"
b_owner:	.asciz "--owner"
	.text

# hostmem_pin: the shared file /dev/shm/phi-hostmem[-N] sized to
# --host-mem, mapped populated, pinned for the card at HOSTMEM_IOVA.
hostmem_pin:
	pushq %rbx
	movq card_index(%rip), %rdi
	call hostmem_path
	movq %rax, %rdi
	movl $O_RDWR | O_CREAT | O_CLOEXEC, %esi
	movl $0600, %edx
	SYS SYS_open
	testq %rax, %rax
	js 8f
	movl %eax, %ebx
	movl %ebx, %edi
	movq o_host_mem(%rip), %rsi
	SYS SYS_ftruncate
	testq %rax, %rax
	js 8f
	xorl %edi, %edi
	movq o_host_mem(%rip), %rsi
	movl $PROT_READ | PROT_WRITE, %edx
	movl $MAP_SHARED | MAP_POPULATE, %r10d
	movl %ebx, %r8d
	xorl %r9d, %r9d
	SYS SYS_mmap
	cmpq $-4096, %rax
	ja 8f
	movq %rax, hostmem_ptr(%rip)
	movq o_host_mem(%rip), %rax
	movq %rax, hostmem_len(%rip)
	movl %ebx, %edi
	SYS SYS_close
	movq hostmem_ptr(%rip), %rdi
	movq $HOSTMEM_IOVA, %rsi
	movq hostmem_len(%rip), %rdx
	call vfio_map_dma
	testq %rax, %rax
	js 9f
	movq $HOSTMEM_IOVA, %rax
	movabsq $SMPT_BASE_HI << 32, %rcx
	addq %rcx, %rax
	movq %rax, hostmem_card(%rip)
	popq %rbx
	ret
8:	negq %rax
	movq %rax, %rdi
	leaq s_e_hostmem(%rip), %rsi
	jmp fail_os
9:	negq %rax
	movq %rax, %rdi
	leaq s_e_hostmem_pin(%rip), %rsi
	jmp fail_os
STR s_e_hostmem, "the host memory file"
STR s_e_hostmem_pin, "pinning the host memory for the card (VFIO_IOMMU_MAP_DMA)"

	.globl smpt_identity
# smpt_identity(edi = pages): SMPT entries 0..pages map card address
# SMPT_BASE + n * 16 GiB onto IOMMU address n * 16 GiB, snooped.
smpt_identity:
	pushq %rbx
	pushq %r12
	movl %edi, %r12d
	cmpl $SMPT_COUNT, %r12d
	jbe 1f
	movl $SMPT_COUNT, %r12d
1:	xorl %ebx, %ebx
2:	cmpl %r12d, %ebx
	jae 3f
	leal SMPT00(,%rbx,4), %edi
	movl %ebx, %esi
	shll $2, %esi		# (n << 34) >> 34 << 2
	call sbox_write
	incl %ebx
	jmp 2b
3:	popq %r12
	popq %rbx
	ret

# do_boot: the loading sequence.
do_boot:
	pushq %rbx
	pushq %r12
	pushq %r13
	# 1. the bootstrap at "12"
	movl $20000, %edi
	call wait_ready
	call download_info
	andl $0xfffff000, %eax
	movq %rax, bootaddr(%rip)
	testq %rax, %rax
	jz 90f
	movl $0x80000000, %ecx
	cmpq %rcx, %rax
	ja 90f
	# 2. the image
	call parse_bzimage
	# 3. the command line
	movq wp(%rip), %rax
	pushq %rax
	leaq cmdline_buf(%rip), %rax
	movq %rax, wp(%rip)
	movq o_cmdline(%rip), %rsi
	call cstr_len
	call trim
	movq %rdx, %rbx
	call w_bytes
	cmpq $0, o_raw(%rip)
	jne 2f
	testq %rbx, %rbx
	jz 1f
	movb $' ', %dil
	call w_byte
1:	call ring_params
2:	xorl %edi, %edi
	call w_byte
	movq wp(%rip), %rax
	leaq cmdline_buf(%rip), %rcx
	subq %rcx, %rax
	decq %rax
	movq %rax, %rbx		# cmdline length without the NUL
	popq %rax
	movq %rax, wp(%rip)
	cmpq cmdline_max(%rip), %rbx
	ja 91f
	movq bootaddr(%rip), %rax
	addq kernel_len(%rip), %rax
	movq %rax, cmdline_addr(%rip)
	# 4. the initramfs at twice the download address
	movq bootaddr(%rip), %rax
	addq %rax, %rax
	movq %rax, initrd_addr(%rip)
	cmpq $0, initrd_ptr(%rip)
	je 3f
	movq cmdline_addr(%rip), %rcx
	addq %rbx, %rcx
	incq %rcx
	cmpq %rcx, %rax
	jb 92f
	addq initrd_len(%rip), %rax
	movq $GDDR_BYTES, %rcx
	cmpq %rcx, %rax
	ja 93f
3:	# the ring region against everything
	movq o_ring_base(%rip), %rax
	movq %rax, %rcx
	addq o_ring_size(%rip), %rcx
	movq bootaddr(%rip), %rdx
	cmpq %rdx, %rax
	jae 4f
	cmpq %rdx, %rcx
	ja 94f
	jmp 5f
4:	movq initrd_addr(%rip), %rdx
	addq initrd_len(%rip), %rdx
	cmpq %rdx, %rax
	jb 95f
5:	# 5. the region, formatted in host memory and copied in one go
	xorl %edi, %edi
	movq o_ring_size(%rip), %rsi
	movl $PROT_READ | PROT_WRITE, %edx
	movl $MAP_PRIVATE | MAP_ANONYMOUS, %r10d
	movq $-1, %r8
	xorl %r9d, %r9d
	SYS SYS_mmap
	cmpq $-4096, %rax
	ja 96f
	movq %rax, %r12
	call now_ns_realtime
	movq %r12, %rdi
	movq o_ring_size(%rip), %rsi
	movq %rax, %rdx
	movq hostmem_card(%rip), %rcx
	movq hostmem_len(%rip), %r8
	call region_format
	movq o_ring_base(%rip), %rdi
	movq %r12, %rsi
	movq o_ring_size(%rip), %rdx
	call write_card_memory
	movq %r12, %rdi
	movq o_ring_size(%rip), %rsi
	SYS SYS_munmap
	# 6. the kernel, its header patched in the private copy
	movq kernel_ptr(%rip), %r12
	cmpq $0, initrd_ptr(%rip)
	je 6f
	movq initrd_addr(%rip), %rax
	movl %eax, OFF_RAMDISK_IMAGE(%r12)
	movq initrd_len(%rip), %rax
	movl %eax, OFF_RAMDISK_SIZE(%r12)
6:	movq cmdline_addr(%rip), %rax
	movl %eax, OFF_CMD_LINE_PTR(%r12)
	movb $0xff, OFF_TYPE_OF_LOADER(%r12)
	movq bootaddr(%rip), %rdi
	movq %r12, %rsi
	movq kernel_len(%rip), %rdx
	call write_card_memory
	movl $SPAD_FW_SIZE, %edi
	movq kernel_len(%rip), %rsi
	call set_spad
	# 7. the command line, NUL included
	movq cmdline_addr(%rip), %rdi
	leaq cmdline_buf(%rip), %rsi
	leaq 1(%rbx), %rdx
	call write_card_memory
	# 8. the initramfs
	cmpq $0, initrd_ptr(%rip)
	je 7f
	movq initrd_addr(%rip), %rdi
	movq initrd_ptr(%rip), %rsi
	movq initrd_len(%rip), %rdx
	call write_card_memory
7:	# 9. go: the boot interrupt is a register write the card orders after
	# the posted aperture writes; no read-back here
	cmpq $0, o_load_only(%rip)
	jne 8f
	call send_boot_interrupt
8:	popq %r13
	popq %r12
	popq %rbx
	ret
90:	leaq s_e_bootaddr(%rip), %rsi
	jmp fail
91:	leaq s_e_cmdline(%rip), %rsi
	jmp fail
92:	leaq s_e_initrd_overlap(%rip), %rsi
	jmp fail
93:	leaq s_e_initrd_gddr(%rip), %rsi
	jmp fail
94:	leaq s_e_ring_dl(%rip), %rsi
	jmp fail
95:	leaq s_e_ring_kernel(%rip), %rsi
	jmp fail
96:	leaq s_e_mmap(%rip), %rsi
	jmp fail
STR s_e_bootaddr, "implausible download address from SPAD2"
STR s_e_cmdline, "the command line is longer than the kernel allows (cmdline_size)"
STR s_e_initrd_overlap, "the initramfs at twice the download address would overlap the kernel or the command line"
STR s_e_initrd_gddr, "the initramfs exceeds card memory"
STR s_e_ring_dl, "the ring region overlaps the download address"
STR s_e_ring_kernel, "the ring region overlaps the kernel or the initramfs"
STR s_e_mmap, "mmap for the ring image failed"

# ring_params: "memmap=<size>K$<base> phi.ring=<base>,<size> mem=6144M"
# at the cursor.
ring_params:
	leaq s_memmap(%rip), %rsi
	call w_cstr
	movq o_ring_size(%rip), %rdi
	shrq $10, %rdi
	call w_dec
	leaq s_kdollar(%rip), %rsi
	call w_cstr
	movq o_ring_base(%rip), %rdi
	call w_hex
	leaq s_phiring(%rip), %rsi
	call w_cstr
	movq o_ring_base(%rip), %rdi
	call w_hex
	movb $',', %dil
	call w_byte
	movq o_ring_size(%rip), %rdi
	call w_hex
	leaq s_mem(%rip), %rsi
	call w_cstr
	movq $GDDR_BYTES >> 20, %rdi
	call w_dec
	movb $'M', %dil
	jmp w_byte
STR s_memmap, "memmap="
STR s_kdollar, "K$"
STR s_phiring, " phi.ring="
STR s_mem, " mem="

# parse_bzimage: validate the header, set cmdline_max and protocol.
parse_bzimage:
	movq kernel_ptr(%rip), %rax
	cmpq $MIN_IMAGE_LEN, kernel_len(%rip)
	jb 90f
	cmpw $0xaa55, OFF_BOOT_FLAG(%rax)
	jne 91f
	cmpl $0x53726448, OFF_HEADER(%rax)	# "HdrS"
	jne 92f
	movzwl OFF_VERSION(%rax), %ecx
	movq %rcx, protocol(%rip)
	cmpl $0x0203, %ecx
	jb 93f
	movl $255, %edx
	cmpl $0x0206, %ecx
	jb 1f
	movl OFF_CMDLINE_SIZE(%rax), %edx
1:	movq %rdx, cmdline_max(%rip)
	ret
90:	leaq s_e_short(%rip), %rsi
	jmp fail
91:	leaq s_e_bootflag(%rip), %rsi
	jmp fail
92:	leaq s_e_magic(%rip), %rsi
	jmp fail
93:	leaq s_e_protocol(%rip), %rsi
	jmp fail
STR s_e_short, "kernel image: image is shorter than the setup header"
STR s_e_bootflag, "kernel image: boot flag 0xAA55 not found at 0x1FE"
STR s_e_magic, "kernel image: 'HdrS' magic not found at 0x202"
STR s_e_protocol, "kernel image: boot protocol is older than 2.03"

# report: what went where, as the Rust command printed it.
report:
	call o_begin
	cmpq $0, o_load_only(%rip)
	je 1f
	leaq s_rp_loaded(%rip), %rsi
	call w_cstr
	call elapsed_since_boot
	leaq s_rp_notsent(%rip), %rsi
	call w_cstr
	jmp 2f
1:	leaq s_rp_sent(%rip), %rsi
	call w_cstr
	call download_info
	movl %eax, %edi
	shrl $1, %edi
	andl $0x1ff, %edi
	call w_dec
	leaq s_rp_after(%rip), %rsi
	call w_cstr
	call elapsed_since_boot
	movb $'s', %dil
	call w_byte
2:	call o_end
	call o_begin
	leaq s_rp_image(%rip), %rsi
	call w_cstr
	movq protocol(%rip), %rdi
	movl $4, %esi
	call w_hexw
	leaq s_rp_bytes_at(%rip), %rsi
	call w_cstr
	movq kernel_len(%rip), %rdi
	call w_dec
	leaq s_rp_at(%rip), %rsi
	call w_cstr
	movq bootaddr(%rip), %rdi
	call w_hex
	call o_end
	call o_begin
	leaq s_rp_cmdline(%rip), %rsi
	call w_cstr
	movq cmdline_addr(%rip), %rdi
	call w_hex
	leaq s_colon2(%rip), %rsi
	call w_cstr
	leaq cmdline_buf(%rip), %rsi
	call w_cstr
	call o_end
	call o_begin
	leaq s_rp_initramfs(%rip), %rsi
	call w_cstr
	cmpq $0, initrd_ptr(%rip)
	jne 3f
	leaq s_rp_none(%rip), %rsi
	call w_cstr
	jmp 4f
3:	movq initrd_len(%rip), %rdi
	call w_dec
	leaq s_rp_bytes_at2(%rip), %rsi
	call w_cstr
	movq initrd_addr(%rip), %rdi
	call w_hex
4:	call o_end
	call o_begin
	leaq s_rp_ring(%rip), %rsi
	call w_cstr
	movq o_ring_size(%rip), %rdi
	call w_hex
	leaq s_rp_bytes_at2(%rip), %rsi
	call w_cstr
	movq o_ring_base(%rip), %rdi
	call w_hex
	call o_end
	jmp o_flush
elapsed_since_boot:
	call now_ms
	subq boot_start_ms(%rip), %rax
	movq %rax, %rdi
	movl $1000, %esi
	movl $2, %edx
	jmp w_fixed
STR s_rp_loaded, "loaded in "
STR s_rp_notsent, "s; boot interrupt NOT sent (--load-only)"
STR s_rp_sent, "boot interrupt sent to APIC "
STR s_rp_after, " after "
STR s_rp_image, "  image     protocol "
STR s_rp_bytes_at, ", "
STR s_rp_at, " bytes at "
STR s_rp_cmdline, "  cmdline   at "
STR s_colon2, ": "
STR s_rp_initramfs, "  initramfs "
STR s_rp_none, "none"
STR s_rp_bytes_at2, " bytes at "
STR s_rp_ring, "  ring      "
