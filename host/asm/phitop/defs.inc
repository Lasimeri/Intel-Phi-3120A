# defs.inc: phitop's constants on top of phictl's (../phictl/defs.inc,
# included here so the shared sources assemble identically). See
# defs.md.
	.include "../phictl/defs.inc"

# Terminal (asm-generic/termbits.h, asm-generic/ioctls.h): the kernel's
# struct termios is 36 bytes (four u32, c_line, c_cc[19]).
	.set TCGETS, 0x5401
	.set TCSETS, 0x5402
	.set TIOCGWINSZ, 0x5413
	.set ICANON, 2
	.set ECHO, 8
	.set VTIME, 5
	.set VMIN, 6
	.set TERMIOS_SIZE, 36
	.set SIGINT, 2
	.set SIGTERM, 15
	.set SA_RESTORER, 0x04000000

# A device entry of a snapshot: name (length, bytes), read, written.
	.set DEV_NAME_LEN, 0
	.set DEV_NAME, 4
	.set DEV_NAME_CAP, 60
	.set DEV_READ, 64
	.set DEV_WRITTEN, 72
	.set DEV_SIZE, 80
# A process entry.
	.set PR_PID, 0
	.set PR_STATE, 4
	.set PR_KTHREAD, 5
	.set PR_THREADS, 8
	.set PR_COMM_LEN, 12
	.set PR_TICKS, 16
	.set PR_RSS, 24
	.set PR_COMM, 32
	.set PR_COMM_CAP, 64
	.set PR_SIZE, 96
# Capacities.
	.set SN_CPU_CAP, 4096
	.set SN_TEMP_CAP, 64
	.set SN_DEV_CAP, 16
	.set SN_PROC_CAP, 16384
# A snapshot.
	.set SN_AT_US, 0
	.set SN_UPTIME, 8
	.set SN_NCPU, 16
	.set SN_MEM, 24
	.set SN_LOAD, 72
	.set SN_RUNNING, 80
	.set SN_TASKS, 84
	.set SN_NTEMPS, 88
	.set SN_TEMPS, 96
	.set SN_PEAK, 224
	.set SN_VCORE, 232
	.set SN_MHZ, 240
	.set SN_NDISKS, 248
	.set SN_NNETS, 256
	.set SN_NLISTED, 264
	.set SN_NPROCS, 272
	.set SN_NKTHREADS, 276
	.set SN_TRAFFIC, 280
	.set SN_NCPU_SENT, 328
	.set SN_CPUS, 336
	.set SN_DISKS, SN_CPUS + SN_CPU_CAP * 12
	.set SN_NETS, SN_DISKS + SN_DEV_CAP * DEV_SIZE
	.set SN_PROCS, SN_NETS + SN_DEV_CAP * DEV_SIZE
# The co-processor worker's stats line (Intel-Phi-AVX512's
# card/vpu/vpu_proto.h, struct vpu_stats at VPU_OFF_STATS of the card's
# host-memory window): its counters since it started, copied whole into
# the snapshot when the window is mapped (SN_VPU_HAVE 1).
	.set VPU_OFF_STATS, 320
	.set VS_REQUESTS, 0
	.set VS_ERRORS, 8
	.set VS_BUSY_NS, 16
	.set VS_COMPUTE_NS, 24
	.set VS_PULL_NS, 32
	.set VS_PUSH_NS, 40
	.set VS_LAST_KERNEL, 48
	.set VS_LAST_THREADS, 52
	.set VS_LAST_SEQ, 56
	.set VS_SIZE, 64
	.set SN_VPU, SN_PROCS + SN_PROC_CAP * PR_SIZE
	.set SN_VPU_HAVE, SN_VPU + VS_SIZE
	.set SN_SIZE, SN_VPU_HAVE + 8
# A rate entry.
	.set RATE_NAME_LEN, 0
	.set RATE_NAME, 4
	.set RATE_A, 64
	.set RATE_B, 72
	.set RATE_SIZE, 80
# A core entry.
	.set CORE_ID, 0
	.set CORE_N, 2
	.set CORE_CPU, 4
	.set CORE_THREADS, 8
	.set CORE_SIZE, 24
	.set CORE_CAP, 1024
# The derived record.
	.set DV_INTERVAL_US, 0
	.set DV_MEAN, 8
	.set DV_DMA_TO, 16
	.set DV_DMA_FROM, 24
	.set DV_APER_TO, 32
	.set DV_APER_FROM, 40
	.set DV_NCORES, 48
	.set DV_MAXTHREADS, 56
	.set DV_NDISKS, 64
	.set DV_NNETS, 72
	.set DV_NCPU, 80
	.set DV_CPU_LOAD, 96
	.set DV_CORES, DV_CPU_LOAD + SN_CPU_CAP * 4
	.set DV_DISKS, DV_CORES + CORE_CAP * CORE_SIZE
	.set DV_NETS, DV_DISKS + SN_DEV_CAP * RATE_SIZE
	.set DV_PROC_CPU, DV_NETS + SN_DEV_CAP * RATE_SIZE
# The worker's rates over the interval (derive_vpu): requests a second and
# busy percent in tenths, each stage's time per request, then what the
# line carries as it is (the last request's kind and threads, the counts
# since the worker started).
	.set DV_VPU_HAVE, DV_PROC_CPU + SN_PROC_CAP * 4
	.set DV_VPU_REQ_TENTHS, DV_VPU_HAVE + 8
	.set DV_VPU_BUSY_TENTHS, DV_VPU_HAVE + 16
	.set DV_VPU_PULL_NS, DV_VPU_HAVE + 24
	.set DV_VPU_COMPUTE_NS, DV_VPU_HAVE + 32
	.set DV_VPU_PUSH_NS, DV_VPU_HAVE + 40
	.set DV_VPU_TOTAL_NS, DV_VPU_HAVE + 48
	.set DV_VPU_ERRORS, DV_VPU_HAVE + 56
	.set DV_VPU_KERNEL, DV_VPU_HAVE + 64
	.set DV_VPU_THREADS, DV_VPU_HAVE + 72
	.set DV_VPU_REQUESTS, DV_VPU_HAVE + 80
	.set DV_SIZE, DV_VPU_HAVE + 88
# The history table.
	.set SEEN_BITS, 16
	.set SEEN_CAP, 1 << SEEN_BITS
	.set SEEN_SIZE, 24
# A slot: one card on the screen.
	.set SL_INDEX, 0
	.set SL_HAVE_PREV, 8
	.set SL_HAVE_DERIVED, 16
	.set SL_CUR, 24
	.set SL_PREV, 32
	.set SL_ERROR_LEN, 40
	.set SL_NAME, 48
	.set SL_BDF, 64
	.set SL_SOCKET, 80
	.set SL_ERROR, 592
	.set SL_DERIVED, 1024
	.set SL_SEEN, SL_DERIVED + DV_SIZE
	.set SL_SNAP_A, SL_SEEN + SEEN_CAP * SEEN_SIZE
	.set SL_SNAP_B, SL_SNAP_A + SN_SIZE
	.set SL_SIZE, SL_SNAP_B + SN_SIZE
	.set MAX_SLOTS, 16
# Frame building.
	.set LINECAP, 16384
	.set FRAMECAP, 1 << 20
	.set LABEL, 7
