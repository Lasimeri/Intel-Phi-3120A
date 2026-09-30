# defs.inc: constants and macros every source of the agent includes. See
# defs.md for where each number comes from.

	.section .note.GNU-stack,"",@progbits

# System calls, x86-64 (arch/x86/entry/syscalls/syscall_64.tbl). The card's
# kernel is x86-64 Linux, so the numbers are the host's.
	.set SYS_read, 0
	.set SYS_write, 1
	.set SYS_open, 2
	.set SYS_close, 3
	.set SYS_poll, 7
	.set SYS_rt_sigaction, 13
	.set SYS_rt_sigprocmask, 14
	.set SYS_madvise, 28
	.set SYS_dup2, 33
	.set SYS_fork, 57
	.set SYS_execve, 59
	.set SYS_exit, 60
	.set SYS_wait4, 61
	.set SYS_fcntl, 72
	.set SYS_fsync, 74
	.set SYS_chdir, 80
	.set SYS_fchmod, 91
	.set SYS_getdents64, 217
	.set SYS_clock_gettime, 228
	.set SYS_exit_group, 231
	.set SYS_pipe2, 293
	.set SYS_pidfd_open, 434

# open(2) and fcntl(2) (include/uapi/asm-generic/fcntl.h).
	.set O_RDONLY, 0x0
	.set O_WRONLY, 0x1
	.set O_RDWR, 0x2
	.set O_CREAT, 0x40
	.set O_TRUNC, 0x200
	.set O_NONBLOCK, 0x800
	.set O_DIRECTORY, 0x10000
	.set O_CLOEXEC, 0x80000
	.set F_SETFD, 2
	.set F_GETFD, 1
	.set F_GETFL, 3
	.set F_SETFL, 4

# poll(2) (include/uapi/asm-generic/poll.h).
	.set POLLIN, 0x1
	.set POLLOUT, 0x4
	.set POLLERR, 0x8
	.set POLLHUP, 0x10

# Signals, wait, clocks, errors (include/uapi/asm-generic/signal.h,
# include/uapi/linux/wait.h, include/uapi/linux/time.h,
# include/uapi/asm-generic/errno-base.h and errno.h).
	.set SIGPIPE, 13
	.set SIG_SETMASK, 2
	.set SIG_DFL, 0
	.set SIG_IGN, 1
	.set WNOHANG, 1
	.set MADV_DONTNEED, 4
	.set CLOCK_MONOTONIC, 1
	.set AT_PAGESZ, 6
	.set ENOENT, 2
	.set EINTR, 4
	.set EBADF, 9
	.set EAGAIN, 11
	.set EACCES, 13
	.set ENOTDIR, 20
	.set ENAMETOOLONG, 36

# The wire (host/crates/phi-rpc/src/lib.rs): frame limit, string and list
# limits, message tags.
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

# The agent's own limits, unchanged from the Rust agent (agent.md).
	.set CHUNK, 32768
	.set CHILD_POLL_MS, 20
	.set OUTPUT_GRACE_MS, 300
	.set INPUT_BACKLOG, 1048576
	.set READ_CHUNK, 65536

# Buffers. RXCAP holds the longest frame plus one read; INCAP holds the
# backlog plus everything the decoder can still hand over once the device
# is no longer read (under RXCAP); TXCAP holds the largest frame, which
# stat.S keeps under MAX_FRAME, with room to spare.
	.set RXCAP, 4 + MAX_FRAME + READ_CHUNK
	.set TXCAP, 2 * MAX_FRAME
	.set INCAP, 4 * 1048576
	.set ARENACAP, 8 * 1048576
	.set LOGCAP, 8192
	.set PATHCAP, 8192

# SYS nr: the system call `nr` with its arguments already in rdi, rsi, rdx,
# r10, r8, r9. Returns in rax (-errno on failure); clobbers rcx and r11.
	.macro SYS nr
	movl $\nr, %eax
	syscall
	.endm
