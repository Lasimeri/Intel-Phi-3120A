# defs.inc: constants and macros every source of phictl includes. Each
# number names its source in defs.md.

	.section .note.GNU-stack,"",@progbits

# System calls, x86-64 (arch/x86/entry/syscalls/syscall_64.tbl).
	.set SYS_read, 0
	.set SYS_write, 1
	.set SYS_open, 2
	.set SYS_close, 3
	.set SYS_stat, 4
	.set SYS_fstat, 5
	.set SYS_lseek, 8
	.set SYS_mmap, 9
	.set SYS_munmap, 11
	.set SYS_rt_sigaction, 13
	.set SYS_ioctl, 16
	.set SYS_pread64, 17
	.set SYS_pwrite64, 18
	.set SYS_access, 21
	.set SYS_madvise, 28
	.set SYS_nanosleep, 35
	.set SYS_socket, 41
	.set SYS_connect, 42
	.set SYS_accept, 43
	.set SYS_shutdown, 48
	.set SYS_bind, 49
	.set SYS_listen, 50
	.set SYS_setsockopt, 54
	.set SYS_getsockopt, 55
	.set SYS_exit, 60
	.set SYS_fcntl, 72
	.set SYS_fsync, 74
	.set SYS_fdatasync, 75
	.set SYS_ftruncate, 77
	.set SYS_getdents64, 217
	.set SYS_mkdir, 83
	.set SYS_readlink, 89
	.set SYS_chmod, 90
	.set SYS_chown, 92
	.set SYS_unlink, 87
	.set SYS_getuid, 102
	.set SYS_geteuid, 107
	.set SYS_clock_gettime, 228
	.set SYS_exit_group, 231
	.set SYS_accept4, 288
	.set SYS_prctl, 157
	.set SYS_ppoll, 271

# open(2), fcntl(2), mmap(2), madvise(2) (uapi asm-generic/fcntl.h,
# asm-generic/mman-common.h, asm/mman.h).
	.set O_RDONLY, 0
	.set O_WRONLY, 1
	.set O_RDWR, 2
	.set O_CREAT, 0x40
	.set O_TRUNC, 0x200
	.set O_NONBLOCK, 0x800
	.set O_DIRECTORY, 0x10000
	.set O_CLOEXEC, 0x80000
	.set F_GETFL, 3
	.set F_SETFL, 4
	.set PROT_READ, 1
	.set PROT_WRITE, 2
	.set MAP_SHARED, 1
	.set MAP_PRIVATE, 2
	.set MAP_ANONYMOUS, 0x20
	.set MAP_POPULATE, 0x8000
	.set MADV_DONTNEED, 4

# poll(2), sockets (uapi asm-generic/poll.h, linux/socket.h, asm/socket.h).
	.set POLLIN, 1
	.set POLLOUT, 4
	.set POLLERR, 8
	.set POLLHUP, 0x10
	.set AF_UNIX, 1
	.set AF_INET, 2
	.set SOCK_STREAM, 1
	.set SOCK_NONBLOCK, 0x800
	.set SOCK_CLOEXEC, 0x80000
	.set SOL_SOCKET, 1
	.set SO_REUSEADDR, 2
	.set SO_PEERCRED, 17
	.set SHUT_RD, 0
	.set SHUT_WR, 1
	.set SHUT_RDWR, 2

# prctl(2) (uapi linux/prctl.h).
	.set PR_SET_TIMERSLACK, 29

# Signals, clocks, errors (uapi asm-generic/signal.h, linux/time.h,
# asm-generic/errno-base.h, errno.h).
	.set SIGPIPE, 13
	.set SIG_IGN, 1
	.set CLOCK_REALTIME, 0
	.set CLOCK_MONOTONIC, 1
	.set ENOENT, 2
	.set EINTR, 4
	.set EIO, 5
	.set EAGAIN, 11
	.set EEXIST, 17
	.set EINVAL, 22
	.set EOPNOTSUPP, 95
	.set ECONNREFUSED, 111
	.set S_IFMT, 0xf000
	.set S_IFDIR, 0x4000

# VFIO (include/uapi/linux/vfio.h): _IO(';', 100 + n) = 0x3b00 | (100 + n).
	.set VFIO_GET_API_VERSION, 0x3b64
	.set VFIO_CHECK_EXTENSION, 0x3b65
	.set VFIO_SET_IOMMU, 0x3b66
	.set VFIO_GROUP_GET_STATUS, 0x3b67
	.set VFIO_GROUP_SET_CONTAINER, 0x3b68
	.set VFIO_GROUP_GET_DEVICE_FD, 0x3b6a
	.set VFIO_DEVICE_GET_INFO, 0x3b6b
	.set VFIO_DEVICE_GET_REGION_INFO, 0x3b6c
	.set VFIO_IOMMU_GET_INFO, 0x3b70
	.set VFIO_IOMMU_MAP_DMA, 0x3b71
	.set VFIO_IOMMU_UNMAP_DMA, 0x3b72
	.set VFIO_TYPE1V2_IOMMU, 3
	.set VFIO_GROUP_FLAGS_VIABLE, 1
	.set VFIO_REGION_INFO_FLAG_MMAP, 4
	.set VFIO_DMA_MAP_FLAG_READ, 1
	.set VFIO_DMA_MAP_FLAG_WRITE, 2
	.set VFIO_PCI_CONFIG_REGION_INDEX, 7
# PCI configuration space (PCI Local Bus 3.0, 6.1).
	.set PCI_VENDOR_ID, 0x00
	.set PCI_DEVICE_ID, 0x02
	.set PCI_COMMAND, 0x04
	.set PCI_COMMAND_MEMORY, 2
	.set PCI_COMMAND_BUS_MASTER, 4
	.set PCI_SUBSYSTEM_VENDOR_ID, 0x2c
	.set PCI_SUBSYSTEM_ID, 0x2e
	.set PCI_VENDOR_INTEL, 0x8086
	.set PCI_DEVICE_3120, 0x225d

# The card (host/crates/phi-regs, sources named there): BAR indexes,
# SBOX offsets relative to SBOX_BASE inside BAR4, the POST code register
# as a raw BAR4 offset.
	.set APER_BAR, 0
	.set MMIO_BAR, 4
	.set SBOX_BASE, 0x10000
	.set POSTCODE, 0x242c
	.set SPAD0, 0xab20
	.set SPAD_DOWNLOAD_INFO, 2
	.set SPAD_PLATFORM_INFO, 4
	.set SPAD_FW_SIZE, 5
	.set APICICR0, 0xa9d0
	.set ICR_SEND, 0x2000
	.set BSP_INTERRUPT_VECTOR, 229
	.set RGCR, 0x4010
	.set RGCR_RESET, 1
	.set SICR0, 0x9004
	.set SICE0, 0x900c
	.set SICC0, 0x9010
	.set SIAC0, 0x9014
	.set MXAR0, 0x9044
	.set MSIXPBACR, 0x9084
	.set RDMASR0, 0xb180
	.set SDBIC0, 0xcc90
	.set SMPT00, 0x3100
	.set SMPT_COUNT, 32
	.set CURRENT_CLK_RATIO, 0x402c
	.set COREFREQ, 0x4100
	.set COREVOLT, 0x4104
	.set THERMAL_STATUS, 0x1018
	.set STATUS_FAN2, 0x1028
	.set BOARD_TEMP1, 0x1030
	.set BOARD_TEMP2, 0x1034
	.set CURRENT_DIE_TEMP0, 0x103c
	.set MAX_DIE_TEMP0, 0x1048
	.set DCR, 0xa280
	.set DMA_CHAN_BASE, 0xa000
	.set DMA_CHAN_STRIDE, 0x40
	.set DCAR, 0x00
	.set DHPR, 0x04
	.set DTPR, 0x08
	.set DRAR_LO, 0x14
	.set DRAR_HI, 0x18
	.set DSTAT, 0x20
	.set DCHERR, 0x2c
	.set DCHERRMSK, 0x30
	.set DCAR_IM0, 0x1000000
	.set DCAR_IM1, 0x2000000
	.set DRAR_HI_SYS, 0x4000000
	.set CORE_VCO_MHZ, 4000
	.set ICC_NOMINAL_DIVIDER, 20

# Card memory map (phi-regs memory.rs).
	.set GDDR_BYTES, 6 << 30
	.set SMPT_BASE_HI, 0x80
	.set SMPT_PAGE_SHIFT, 34
	.set RING_REGION_BASE, 0x10000000
	.set RING_REGION_BYTES, 16 << 20
	.set APERTURE_WRITE_CHUNK, 4096

# bzImage header (Documentation/arch/x86/boot.rst).
	.set OFF_SETUP_SECTS, 0x1f1
	.set OFF_BOOT_FLAG, 0x1fe
	.set OFF_HEADER, 0x202
	.set OFF_VERSION, 0x206
	.set OFF_TYPE_OF_LOADER, 0x210
	.set OFF_RAMDISK_IMAGE, 0x218
	.set OFF_RAMDISK_SIZE, 0x21c
	.set OFF_CMD_LINE_PTR, 0x228
	.set OFF_XLOADFLAGS, 0x236
	.set OFF_CMDLINE_SIZE, 0x238
	.set OFF_INIT_SIZE, 0x260
	.set MIN_IMAGE_LEN, 0x264

# Ring region layout (docs/spec/ring-protocol.md, phi-ring layout.rs).
	.set REGION_MAGIC, 0x52494850
	.set RING_MAGIC, 0x474e4952
	.set RING_VERSION, 1
	.set REGION_HDR_SIZE, 64
	.set CHANNEL_DESC_SIZE, 64
	.set RING_HDR_SIZE, 192
	.set LINE, 64
	.set DATA_ALIGN, 4096
	.set RH_MAGIC, 0
	.set RH_VERSION, 4
	.set RH_REGION_SIZE, 8
	.set RH_CHANNEL_COUNT, 12
	.set RH_HOST_EPOCH_NS, 16
	.set RH_CARD_BOOT_FLAGS, 24
	.set RH_HOSTMEM_ADDR, 32
	.set RH_HOSTMEM_SIZE, 40
	.set CD_KIND, 0
	.set CD_FLAGS, 4
	.set CD_H2C_OFFSET, 8
	.set CD_H2C_SIZE, 12
	.set CD_C2H_OFFSET, 16
	.set CD_C2H_SIZE, 20
	.set CD_DATA_OFFSET, 24
	.set CD_DATA_SIZE, 28
	.set RG_MAGIC, 0
	.set RG_SIZE, 4
	.set RG_HEAD, 64
	.set RG_TAIL, 128
	.set RG_DATA, 192
	.set KIND_CONSOLE, 1
	.set KIND_NETWORK, 2
	.set KIND_RPC, 3
	.set KIND_BLOCK, 4
	.set KIND_HOSTMEM, 5
	.set CARD_FLAG_INIT_REACHED, 1

# The rpc wire (host/crates/phi-rpc/src/lib.rs).
	.set MAX_FRAME, 1048576
	.set MAX_STRING, 65535
	.set MAX_LIST, 65535
	.set TAG_EXEC, 1
	.set TAG_STDIN, 2
	.set TAG_STDIN_EOF, 3
	.set TAG_STDOUT, 4
	.set TAG_STDERR, 5
	.set TAG_EXIT, 6
	.set TAG_PUT_OPEN, 7
	.set TAG_PUT_DATA, 8
	.set TAG_PUT_CLOSE, 9
	.set TAG_GET, 10
	.set TAG_GET_DATA, 11
	.set TAG_GET_END, 12
	.set TAG_ERROR, 13
	.set TAG_PING, 14
	.set TAG_PONG, 15
	.set TAG_SENSORS, 16
	.set TAG_SENSORS_REPLY, 17
	.set TAG_STAT, 18
	.set TAG_STAT_REPLY, 19
	.set TAG_TRAFFIC, 20
	.set TAG_TRAFFIC_REPLY, 21
	.set TAG_LAST, 21

# A frame decoder (wire.S): 0 head, 8 tail, 16 buffer, 24 capacity; the
# frame it returned: 32 tag, 40 body, 48 length. Its buffer holds the
# largest frame and one read.
	.set DEC_HEAD, 0
	.set DEC_TAIL, 8
	.set DEC_BUF, 16
	.set DEC_CAP, 24
	.set DEC_TAG, 32
	.set DEC_BODY, 40
	.set DEC_LEN, 48
	.set DEC_SIZE, 64
	.set DECCAP, 4 + 1048576 + 65536

# Cards (phi-vfio cards.rs).
	.set MAX_CARDS, 16
	.set FIRST_PORT, 2222

# Buffers.
	.set LOGCAP, 65536
	.set PATHCAP, 4096
	.set READ_CHUNK, 65536

# SYS nr: the system call with its arguments in rdi, rsi, rdx, r10, r8,
# r9; rax = result or -errno; rcx and r11 clobbered.
	.macro SYS nr
	movl $\nr, %eax
	syscall
	.endm

# A NUL-terminated string in .rodata, referenced as \name(%rip).
	.macro STR name, text
	.section .rodata
\name:	.asciz "\text"
	.text
	.endm
