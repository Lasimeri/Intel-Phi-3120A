# vfio.S: one PCI device through VFIO (host/crates/phi-vfio): the
# container, the group, the device fd, region maps, configuration space,
# DMA mapping. Structures are the UAPI's (include/uapi/linux/vfio.h),
# sizes pinned in defs.md. See vfio.md.
	.include "defs.inc"

	.bss
	.p2align 3
	.globl vfio_container, vfio_group, vfio_device, vfio_group_id
vfio_container:	.skip 8
vfio_group:	.skip 8
vfio_device:	.skip 8
vfio_group_id:	.skip 8
	.globl cfg_offset, cfg_size
cfg_offset:	.skip 8
cfg_size:	.skip 8
	.globl mmio_base, mmio_len, aper_base, aper_len
mmio_base:	.skip 8
mmio_len:	.skip 8
aper_base:	.skip 8
aper_len:	.skip 8
	.p2align 3
ioctl_buf:	.skip 64

	.text

	.globl vfio_open
# vfio_open(rsi = normalized bdf): open /dev/vfio/vfio, the device's
# group, attach, set type1v2, get the device fd, read the config region
# and map BARs 4 (registers) and 0 (aperture). Every failure exits with
# the call that failed.
vfio_open:
	pushq %rbx
	pushq %r12
	movq %rsi, %r12
	call iommu_group_of
	testl %edx, %edx
	jz 90f
	movq %rax, vfio_group_id(%rip)
	# the container
	leaq s_dev_vfio(%rip), %rdi
	movl $O_RDWR | O_CLOEXEC, %esi
	xorl %edx, %edx
	SYS SYS_open
	testq %rax, %rax
	js 91f
	movq %rax, vfio_container(%rip)
	movl %eax, %edi
	movl $VFIO_GET_API_VERSION, %esi
	xorl %edx, %edx
	SYS SYS_ioctl
	testq %rax, %rax
	jnz 92f
	movl vfio_container(%rip), %edi
	movl $VFIO_CHECK_EXTENSION, %esi
	movl $VFIO_TYPE1V2_IOMMU, %edx
	SYS SYS_ioctl
	cmpq $1, %rax
	jne 93f
	# the group
	call p_begin
	leaq s_dev_vfio_slash(%rip), %rsi
	call w_cstr
	movq vfio_group_id(%rip), %rdi
	call w_dec
	call p_end
	movq %rax, %rdi
	movl $O_RDWR | O_CLOEXEC, %esi
	xorl %edx, %edx
	SYS SYS_open
	testq %rax, %rax
	js 94f
	movq %rax, vfio_group(%rip)
	leaq ioctl_buf(%rip), %rdx
	movl $8, (%rdx)
	movl $0, 4(%rdx)
	movl %eax, %edi
	movl $VFIO_GROUP_GET_STATUS, %esi
	SYS SYS_ioctl
	testq %rax, %rax
	js 95f
	testl $VFIO_GROUP_FLAGS_VIABLE, ioctl_buf+4(%rip)
	jz 96f
	movl vfio_container(%rip), %eax
	movl %eax, ioctl_buf(%rip)
	movl vfio_group(%rip), %edi
	movl $VFIO_GROUP_SET_CONTAINER, %esi
	leaq ioctl_buf(%rip), %rdx
	SYS SYS_ioctl
	testq %rax, %rax
	js 97f
	movl vfio_container(%rip), %edi
	movl $VFIO_SET_IOMMU, %esi
	movl $VFIO_TYPE1V2_IOMMU, %edx
	SYS SYS_ioctl
	testq %rax, %rax
	js 98f
	movl vfio_group(%rip), %edi
	movl $VFIO_GROUP_GET_DEVICE_FD, %esi
	movq %r12, %rdx
	SYS SYS_ioctl
	testq %rax, %rax
	js 99f
	movq %rax, vfio_device(%rip)
	# device info (24 bytes), then the config region
	leaq ioctl_buf(%rip), %rdx
	movl $24, (%rdx)
	movl %eax, %edi
	movl $VFIO_DEVICE_GET_INFO, %esi
	SYS SYS_ioctl
	testq %rax, %rax
	js 100f
	movl $VFIO_PCI_CONFIG_REGION_INDEX, %edi
	call region_info
	movq ioctl_buf+24(%rip), %rax
	movq %rax, cfg_offset(%rip)
	movq ioctl_buf+16(%rip), %rax
	movq %rax, cfg_size(%rip)
	call enable_memory
	movl $MMIO_BAR, %edi
	call map_region
	movq %rax, mmio_base(%rip)
	movq %rdx, mmio_len(%rip)
	movl $APER_BAR, %edi
	call map_region
	movq %rax, aper_base(%rip)
	movq %rdx, aper_len(%rip)
	popq %r12
	popq %rbx
	ret
90:	leaq s_e_group(%rip), %rsi
	jmp fail
91:	negq %rax
	movq %rax, %rdi
	leaq s_dev_vfio(%rip), %rsi
	jmp fail_os_open
92:	leaq s_e_api(%rip), %rsi
	jmp fail
93:	leaq s_e_type1(%rip), %rsi
	jmp fail
94:	negq %rax
	movq %rax, %rdi
	leaq s_e_open_group(%rip), %rsi
	jmp fail_os
95:	negq %rax
	movq %rax, %rdi
	leaq s_ioc_status(%rip), %rsi
	jmp fail_os
96:	leaq s_e_viable(%rip), %rsi
	jmp fail
97:	negq %rax
	movq %rax, %rdi
	leaq s_ioc_setc(%rip), %rsi
	jmp fail_os
98:	negq %rax
	movq %rax, %rdi
	leaq s_ioc_setiommu(%rip), %rsi
	jmp fail_os
99:	negq %rax
	movq %rax, %rdi
	leaq s_ioc_getdev(%rip), %rsi
	jmp fail_os
100:	negq %rax
	movq %rax, %rdi
	leaq s_ioc_devinfo(%rip), %rsi
	jmp fail_os
fail_os_open:
	pushq %rdi
	pushq %rsi
	call l_begin
	leaq s_prefix(%rip), %rsi
	call w_cstr
	leaq s_open(%rip), %rsi
	call w_cstr
	popq %rsi
	call w_cstr
	leaq s_colon(%rip), %rsi
	call w_cstr
	popq %rdi
	call w_oserr
	call l_end
	movl $1, %edi
	SYS SYS_exit_group
STR s_prefix, "phictl: "
STR s_open, "open "
STR s_colon, ": "
STR s_dev_vfio, "/dev/vfio/vfio"
STR s_dev_vfio_slash, "/dev/vfio/"
STR s_e_group, "no iommu_group for the device (is the IOMMU enabled?)"
STR s_e_api, "unexpected VFIO API version"
STR s_e_type1, "VFIO type1v2 IOMMU extension not supported by this kernel"
STR s_e_open_group, "open /dev/vfio/<group>"
STR s_e_viable, "IOMMU group is not viable: every device in it must be bound to vfio-pci"
STR s_ioc_status, "VFIO_GROUP_GET_STATUS"
STR s_ioc_setc, "VFIO_GROUP_SET_CONTAINER"
STR s_ioc_setiommu, "VFIO_SET_IOMMU"
STR s_ioc_getdev, "VFIO_GROUP_GET_DEVICE_FD"
STR s_ioc_devinfo, "VFIO_DEVICE_GET_INFO"
STR s_ioc_region, "VFIO_DEVICE_GET_REGION_INFO"
STR s_e_mmap, "mmap region"
STR s_e_nommap, "region is not mmappable"

	.globl region_info
# region_info(edi = index): VFIO_DEVICE_GET_REGION_INFO into ioctl_buf
# (argsz 0, flags 4, index 8, cap_offset 12, size 16, offset 24).
region_info:
	leaq ioctl_buf(%rip), %rdx
	movl $32, (%rdx)
	movl $0, 4(%rdx)
	movl %edi, 8(%rdx)
	movl $0, 12(%rdx)
	movq $0, 16(%rdx)
	movq $0, 24(%rdx)
	movl vfio_device(%rip), %edi
	movl $VFIO_DEVICE_GET_REGION_INFO, %esi
	SYS SYS_ioctl
	testq %rax, %rax
	js 1f
	ret
1:	negq %rax
	movq %rax, %rdi
	leaq s_ioc_region(%rip), %rsi
	jmp fail_os

# map_region(edi = index) -> rax = base, rdx = length: the whole region
# mmapped read/write shared. Exits when it has no MMAP flag or is empty.
map_region:
	call region_info
	testl $VFIO_REGION_INFO_FLAG_MMAP, ioctl_buf+4(%rip)
	jz 8f
	movq ioctl_buf+16(%rip), %rsi
	testq %rsi, %rsi
	jz 8f
	xorl %edi, %edi
	movl $PROT_READ | PROT_WRITE, %edx
	movl $MAP_SHARED, %r10d
	movl vfio_device(%rip), %r8d
	movq ioctl_buf+24(%rip), %r9
	SYS SYS_mmap
	cmpq $-4096, %rax
	ja 9f
	movq ioctl_buf+16(%rip), %rdx
	ret
8:	leaq s_e_nommap(%rip), %rsi
	jmp fail
9:	negq %rax
	movq %rax, %rdi
	leaq s_e_mmap(%rip), %rsi
	jmp fail_os

	.globl cfg_read16
# cfg_read16(edi = offset) -> eax: a 16-bit configuration register.
cfg_read16:
	movl %edi, %r10d
	addq cfg_offset(%rip), %r10
	movl vfio_device(%rip), %edi
	leaq ioctl_buf+48(%rip), %rsi
	movl $2, %edx
	SYS SYS_pread64
	cmpq $2, %rax
	jne 1f
	movzwl ioctl_buf+48(%rip), %eax
	ret
1:	leaq s_e_cfgread(%rip), %rsi
	jmp fail
STR s_e_cfgread, "pread config space: short read"

	.globl cfg_write16
# cfg_write16(edi = offset, esi = value).
cfg_write16:
	movw %si, ioctl_buf+48(%rip)
	movl %edi, %r10d
	addq cfg_offset(%rip), %r10
	movl vfio_device(%rip), %edi
	leaq ioctl_buf+48(%rip), %rsi
	movl $2, %edx
	SYS SYS_pwrite64
	cmpq $2, %rax
	jne 1f
	ret
1:	leaq s_e_cfgwrite(%rip), %rsi
	jmp fail
STR s_e_cfgwrite, "pwrite config space: short write"

# enable_memory: Memory Space Enable and Bus Master Enable in COMMAND
# (vfio-pci enables the device on open, so normally a no-op).
enable_memory:
	movl $PCI_COMMAND, %edi
	call cfg_read16
	movl %eax, %esi
	orl $PCI_COMMAND_MEMORY | PCI_COMMAND_BUS_MASTER, %esi
	cmpl %eax, %esi
	je 1f
	movl $PCI_COMMAND, %edi
	call cfg_write16
1:	ret

	.globl vfio_map_dma
# vfio_map_dma(rdi = vaddr, rsi = iova, rdx = len) -> rax = 0 or -errno:
# pin and map for the device, readable and writable.
vfio_map_dma:
	leaq ioctl_buf(%rip), %rcx
	movl $32, (%rcx)
	movl $VFIO_DMA_MAP_FLAG_READ | VFIO_DMA_MAP_FLAG_WRITE, 4(%rcx)
	movq %rdi, 8(%rcx)
	movq %rsi, 16(%rcx)
	movq %rdx, 24(%rcx)
	movl vfio_container(%rip), %edi
	movl $VFIO_IOMMU_MAP_DMA, %esi
	movq %rcx, %rdx
	SYS SYS_ioctl
	ret
