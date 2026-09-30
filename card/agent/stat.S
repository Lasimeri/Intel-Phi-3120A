# stat.S: one `StatReply` for phitop, read from /proc and sysfs and
# written straight into the frame in wire order (phi-rpc's `put_stat`).
# Counters are cumulative; the host differences consecutive samples. The
# parsing rules are the Rust agent's (stat.md lists them); every path goes
# through read_text or open_dir, so --root can point the sampler at a
# fixture tree. See stat.md.
	.include "defs.inc"

	.set NCPU_MAX, 4096
	.set KM_MAX, 65536
	.set BIGCAP, 1048576
	.set SMALLCAP, 65536
	.set DIRCAP, 65536
	# hwmon temperature channels the driver registers, and the die ones
	# with a temp*_max (kernel patch 0027)
	.set TEMP_CHANNELS, 15
	.set DIE_CHANNELS, 9
	# kernel threads are re-read every this many samples
	.set KTHREAD_EVERY, 4
	# most processes in one sample (the Rust agent's MAX_PROCS)
	.set MAX_PROCS, 16384
	# PF_KTHREAD in /proc/[pid]/stat's flags (include/linux/sched.h)
	.set PF_KTHREAD, 0x00200000

	.bss
	.p2align 3
samples:	.skip 8
ncpu:	.skip 8
ncores:	.skip 8
hw_found:	.skip 8
hw_len:	.skip 8
hw_path:	.skip 512
# The kernel threads' ticks when last read, sorted by pid: two tables of
# 16-byte entries (pid, ticks), the previous sample's and the one being
# built, swapped after each sample.
km_old:	.skip 8
km_old_n:	.skip 8
km_new:	.skip 8
km_new_n:	.skip 8
km_unsorted:	.skip 8
st_nprocs:	.skip 8
st_nkthreads:	.skip 8
st_listed:	.skip 8
st_count_at:	.skip 8
st_refresh:	.skip 8
	.p2align 6
km_a:	.skip KM_MAX * 16
km_b:	.skip KM_MAX * 16
cpus:	.skip NCPU_MAX * 8
cores:	.skip NCPU_MAX * 2
bigbuf:	.skip BIGCAP
pbuf1:	.skip SMALLCAP
pbuf2:	.skip SMALLCAP
dirbuf:	.skip DIRCAP

	.section .rodata
p_uptime:	.asciz "/proc/uptime"
p_stat:	.asciz "/proc/stat"
p_meminfo:	.asciz "/proc/meminfo"
p_loadavg:	.asciz "/proc/loadavg"
p_diskstats:	.asciz "/proc/diskstats"
p_netdev:	.asciz "/proc/net/dev"
p_proc:	.asciz "/proc"
p_proc_slash:	.asciz "/proc/"
p_stat_tail:	.asciz "/stat"
p_status_tail:	.asciz "/status"
p_cpu:	.asciz "/sys/devices/system/cpu/cpu"
p_core_id:	.asciz "/topology/core_id"
p_hwmon:	.asciz "/sys/class/hwmon"
p_hwmon_slash:	.asciz "/sys/class/hwmon/"
p_name:	.asciz "/name"
p_temp:	.asciz "/temp"
p_input:	.asciz "_input"
p_max:	.asciz "_max"
p_in0:	.asciz "/in0_input"
p_core_mhz:	.asciz "/core_mhz"
s_zero:	.asciz "0"
s_cpu:	.asciz "cpu"
s_knc:	.asciz "knc"
s_phiblk:	.asciz "phiblk"
s_lo:	.asciz "lo"
s_dot:	.asciz "."
s_dotdot:	.asciz ".."
s_vmrss:	.asciz "VmRSS:"
k_memtotal:	.asciz "MemTotal:"
k_memavail:	.asciz "MemAvailable:"
k_buffers:	.asciz "Buffers:"
k_cached:	.asciz "Cached:"
k_swaptotal:	.asciz "SwapTotal:"
k_swapfree:	.asciz "SwapFree:"
	.p2align 3
mem_keys:
	.quad k_memtotal, k_memavail, k_buffers, k_cached, k_swaptotal, k_swapfree

	.text

	.globl stat_reply
# stat_reply: take one sample and send it.
stat_reply:
	movb $TAG_STAT_REPLY, %dil
	call f_begin
	call st_uptime
	call st_cpus
	call st_mem
	call st_load
	call st_sensors
	call st_disks
	call st_nets
	call st_procs
	jmp f_send

# room(rcx = bytes) -> CF set when an entry of that size would take the
# frame body past MAX_FRAME (with 16 bytes kept for what follows). The Rust
# agent relied on MAX_PROCS alone; long kernel-thread names could pass it.
room:
	leaq tx_buf+4+MAX_FRAME-16(%rip), %rax
	subq wp(%rip), %rax
	cmpq %rcx, %rax
	ret

# ------------------------------------------------------------ uptime

# /proc/uptime: the first number, as milliseconds.
st_uptime:
	leaq p_uptime(%rip), %rdi
	leaq bigbuf(%rip), %rsi
	movl $BIGCAP, %edx
	call read_text
	leaq bigbuf(%rip), %rsi
	leaq (%rsi,%rax), %rdi
	call next_token
	testq %rax, %rax
	jnz 1f
	leaq s_zero(%rip), %rax
	movl $1, %edx
1:	movq %rax, %rsi
	call hundredths
	imulq $10, %rax, %rdi
	jmp w_u64

# ------------------------------------------------------------ cpus

# /proc/stat's cpuN lines, indexed by N (a gap stays zero): busy is user
# nice system irq softirq steal, idle is idle iowait (fs/proc/stat.c),
# each the low 32 bits. Core ids from sysfs, read again whenever more
# CPUs appear. N past NCPU_MAX is skipped.
st_cpus:
	pushq %rbx
	pushq %rbp
	pushq %r12
	pushq %r13
	pushq %r14
	pushq %r15
	subq $72, %rsp
	leaq p_stat(%rip), %rdi
	leaq bigbuf(%rip), %rsi
	movl $BIGCAP, %edx
	call read_text
	leaq bigbuf(%rip), %r12		# cursor
	leaq (%r12,%rax), %r13		# end
	movq $0, ncpu(%rip)
1:	movq %r12, %rsi
	movq %r13, %rdi
	call next_line
	movq %rsi, %r12
	testq %rax, %rax
	jz 20f
	leaq (%rax,%rdx), %r14		# line end
	movq %rax, %rsi
	movq %r14, %rdi
	call next_token
	testq %rax, %rax
	jz 1b
	movq %rsi, %r15			# past the name
	movq %rax, %rbx
	movq %rdx, %rbp
	movq %rax, %rsi
	leaq s_cpu(%rip), %rdi
	call starts_cstr
	testl %eax, %eax
	jz 1b
	leaq 3(%rbx), %rsi
	leaq -3(%rbp), %rdx
	call parse_u64
	testl %edx, %edx
	jz 1b
	cmpq $NCPU_MAX, %rax
	jae 1b
	movq %rax, 64(%rsp)		# n
	# eight values, 0 where missing or unparsable
	xorl %ebx, %ebx
2:	movq $0, (%rsp,%rbx,8)
	incl %ebx
	cmpl $8, %ebx
	jb 2b
	xorl %ebx, %ebx
3:	movq %r15, %rsi
	movq %r14, %rdi
	call next_token
	movq %rsi, %r15
	testq %rax, %rax
	jz 5f
	movq %rax, %rsi
	call parse_u64
	testl %edx, %edx
	jz 4f
	movq %rax, (%rsp,%rbx,8)
4:	incl %ebx
	cmpl $8, %ebx
	jb 3b
5:	movq 0(%rsp), %rax
	addq 8(%rsp), %rax
	addq 16(%rsp), %rax
	addq 40(%rsp), %rax
	addq 48(%rsp), %rax
	addq 56(%rsp), %rax
	movq 24(%rsp), %rcx
	addq 32(%rsp), %rcx
	movq 64(%rsp), %rdx		# n
	leaq cpus(%rip), %rdi
	# grow with zeros up to n
	movq ncpu(%rip), %r8
6:	cmpq %rdx, %r8
	ja 7f
	movq $0, (%rdi,%r8,8)
	incq %r8
	movq %r8, ncpu(%rip)
	jmp 6b
7:	movl %eax, (%rdi,%rdx,8)
	movl %ecx, 4(%rdi,%rdx,8)
	jmp 1b
20:	# core ids
	movq ncpu(%rip), %rax
	cmpq %rax, ncores(%rip)
	jae 25f
	xorl %ebx, %ebx
21:	cmpq ncpu(%rip), %rbx
	jae 24f
	call p_begin
	leaq p_cpu(%rip), %rsi
	call w_cstr
	movq %rbx, %rdi
	call w_dec
	leaq p_core_id(%rip), %rsi
	call w_cstr
	call p_end
	movq %rax, %rdi
	leaq pbuf2(%rip), %rsi
	movl $SMALLCAP, %edx
	call read_text
	leaq pbuf2(%rip), %rsi
	movq %rax, %rdx
	call trim
	call parse_u16
	testl %edx, %edx
	jnz 22f
	xorl %eax, %eax
22:	leaq cores(%rip), %rdi
	movw %ax, (%rdi,%rbx,2)
	incq %rbx
	jmp 21b
24:	movq ncpu(%rip), %rax
	movq %rax, ncores(%rip)
25:	movq ncpu(%rip), %rdi
	call w_u16
	xorl %ebx, %ebx
26:	cmpq ncpu(%rip), %rbx
	jae 29f
	leaq cores(%rip), %rax
	movzwl (%rax,%rbx,2), %edi
	call w_u16
	leaq cpus(%rip), %rax
	movl (%rax,%rbx,8), %edi
	call w_u32
	leaq cpus(%rip), %rax
	movl 4(%rax,%rbx,8), %edi
	call w_u32
	incq %rbx
	jmp 26b
29:	addq $72, %rsp
	popq %r15
	popq %r14
	popq %r13
	popq %r12
	popq %rbp
	popq %rbx
	ret

# ------------------------------------------------------------ memory

# /proc/meminfo: six fields in kB, 0 when absent.
st_mem:
	pushq %rbx
	pushq %r12
	pushq %r13
	pushq %r14
	pushq %r15
	subq $64, %rsp
	xorl %ebx, %ebx
1:	movq $0, (%rsp,%rbx,8)
	incl %ebx
	cmpl $6, %ebx
	jb 1b
	leaq p_meminfo(%rip), %rdi
	leaq bigbuf(%rip), %rsi
	movl $BIGCAP, %edx
	call read_text
	leaq bigbuf(%rip), %r12
	leaq (%r12,%rax), %r13
2:	movq %r12, %rsi
	movq %r13, %rdi
	call next_line
	movq %rsi, %r12
	testq %rax, %rax
	jz 8f
	leaq (%rax,%rdx), %r14
	movq %rax, %rsi
	movq %r14, %rdi
	call next_token
	testq %rax, %rax
	jz 2b
	movq %rax, 48(%rsp)		# key
	movq %rdx, 56(%rsp)
	movq %r14, %rdi
	call next_token
	testq %rax, %rax
	jz 2b
	movq %rax, %rsi
	call parse_u64
	testl %edx, %edx
	jnz 3f
	xorl %eax, %eax
3:	movq %rax, %r15
	xorl %ebx, %ebx
4:	movq 48(%rsp), %rsi
	movq 56(%rsp), %rdx
	leaq mem_keys(%rip), %rax
	movq (%rax,%rbx,8), %rdi
	call eq_cstr
	testl %eax, %eax
	jz 5f
	movq %r15, (%rsp,%rbx,8)
	jmp 2b
5:	incl %ebx
	cmpl $6, %ebx
	jb 4b
	jmp 2b
8:	xorl %ebx, %ebx
9:	movq (%rsp,%rbx,8), %rdi
	call w_u64
	incl %ebx
	cmpl $6, %ebx
	jb 9b
	addq $64, %rsp
	popq %r15
	popq %r14
	popq %r13
	popq %r12
	popq %rbx
	ret

# ------------------------------------------------------------ load

# /proc/loadavg: three averages in hundredths (capped at the wire's u16),
# then "running/total".
st_load:
	pushq %rbx
	pushq %r12
	pushq %r13
	subq $64, %rsp
	leaq p_loadavg(%rip), %rdi
	leaq bigbuf(%rip), %rsi
	movl $BIGCAP, %edx
	call read_text
	leaq bigbuf(%rip), %r12
	leaq (%r12,%rax), %r13
	# up to four tokens: (ptr, len) at 0(%rsp) .. 56(%rsp)
	xorl %ebx, %ebx
1:	movq $0, (%rsp,%rbx,8)
	incl %ebx
	cmpl $8, %ebx
	jb 1b
	xorl %ebx, %ebx
2:	movq %r12, %rsi
	movq %r13, %rdi
	call next_token
	movq %rsi, %r12
	testq %rax, %rax
	jz 3f
	movq %rbx, %rcx
	shlq $4, %rcx
	movq %rax, (%rsp,%rcx)
	movq %rdx, 8(%rsp,%rcx)
	incl %ebx
	cmpl $4, %ebx
	jb 2b
3:	xorl %ebx, %ebx
4:	movq %rbx, %rcx
	shlq $4, %rcx
	movq (%rsp,%rcx), %rsi
	xorl %eax, %eax
	testq %rsi, %rsi
	jz 5f
	movq 8(%rsp,%rcx), %rdx
	call hundredths
	cmpq $0xffff, %rax
	jbe 5f
	movl $0xffff, %eax
5:	movl %eax, %edi
	call w_u16
	incl %ebx
	cmpl $3, %ebx
	jb 4b
	# running/total
	xorl %r12d, %r12d
	xorl %r13d, %r13d
	movq 48(%rsp), %rsi
	testq %rsi, %rsi
	jz 9f
	movq 56(%rsp), %rdx
	xorl %ecx, %ecx
6:	cmpq %rdx, %rcx
	jae 9f
	cmpb $'/', (%rsi,%rcx)
	je 7f
	incq %rcx
	jmp 6b
7:	leaq 1(%rsi,%rcx), %rax
	subq %rcx, %rdx
	decq %rdx
	pushq %rax
	pushq %rdx
	movq %rcx, %rdx
	call parse_u32
	testl %edx, %edx
	jz 8f
	movq %rax, %r12
8:	popq %rdx
	popq %rsi
	call parse_u32
	testl %edx, %edx
	jz 9f
	movq %rax, %r13
9:	movl %r12d, %edi
	call w_u32
	movl %r13d, %edi
	call w_u32
	addq $64, %rsp
	popq %r13
	popq %r12
	popq %rbx
	ret

# ------------------------------------------------------------ sensors

# hw_file(rsi = "/tail") -> rax = hw_path + tail, NUL-terminated.
hw_file:
	pushq %rsi
	call p_begin
	leaq hw_path(%rip), %rsi
	movq hw_len(%rip), %rdx
	call w_bytes
	popq %rsi
	call w_cstr
	jmp p_end

# read_small(rax = path) -> rsi, rdx: the file's text, trimmed.
read_small:
	movq %rax, %rdi
	leaq pbuf2(%rip), %rsi
	movl $SMALLCAP, %edx
	call read_text
	leaq pbuf2(%rip), %rsi
	movq %rax, %rdx
	jmp trim

# millideg(edi = channel, rsi = "_input" or "_max") -> eax: the reading in
# whole degrees as an i16 (truncated, as Rust's `(v / 1000) as i16`), -1
# when unreadable.
millideg:
	pushq %rbx
	pushq %r12
	subq $8, %rsp
	movl %edi, %ebx
	movq %rsi, %r12
	call p_begin
	leaq hw_path(%rip), %rsi
	movq hw_len(%rip), %rdx
	call w_bytes
	leaq p_temp(%rip), %rsi
	call w_cstr
	movl %ebx, %edi
	call w_dec
	movq %r12, %rsi
	call w_cstr
	call p_end
	call read_small
	call parse_i64
	testl %edx, %edx
	jz 1f
	cqto
	movl $1000, %ecx
	idivq %rcx
	movswl %ax, %eax
	jmp 2f
1:	movl $-1, %eax
2:	addq $8, %rsp
	popq %r12
	popq %rbx
	ret

# hw_scan: the hwmon directory whose name reads "knc", looked for until
# found.
hw_scan:
	pushq %rbx
	pushq %r12
	pushq %r13
	pushq %r14
	subq $8, %rsp
	leaq p_hwmon(%rip), %rdi
	call open_dir
	testq %rax, %rax
	js 9f
	movl %eax, %ebx
1:	movl %ebx, %edi
	leaq dirbuf(%rip), %rsi
	movl $DIRCAP, %edx
	SYS SYS_getdents64
	testq %rax, %rax
	jle 8f
	movq %rax, %r13
	xorl %r12d, %r12d
2:	cmpq %r13, %r12
	jae 1b
	leaq dirbuf(%rip), %r14
	addq %r12, %r14
	movzwl 16(%r14), %eax
	addq %rax, %r12
	addq $19, %r14			# d_name
	movq %r14, %rsi
	call cstr_len
	movq %r14, %rsi
	leaq s_dot(%rip), %rdi
	pushq %rdx
	call eq_cstr
	popq %rdx
	testl %eax, %eax
	jnz 2b
	movq %r14, %rsi
	leaq s_dotdot(%rip), %rdi
	call eq_cstr
	testl %eax, %eax
	jnz 2b
	# hw_path = "/sys/class/hwmon/" name; its "name" file
	movq wp(%rip), %rax
	pushq %rax
	leaq hw_path(%rip), %rax
	movq %rax, wp(%rip)
	leaq p_hwmon_slash(%rip), %rsi
	call w_cstr
	movq %r14, %rsi
	call w_cstr
	movq wp(%rip), %rax
	leaq hw_path(%rip), %rcx
	subq %rcx, %rax
	movq %rax, hw_len(%rip)
	popq %rax
	movq %rax, wp(%rip)
	leaq p_name(%rip), %rsi
	call hw_file
	call read_small
	leaq s_knc(%rip), %rdi
	call eq_cstr
	testl %eax, %eax
	jz 2b
	movq $1, hw_found(%rip)
8:	movl %ebx, %edi
	SYS SYS_close
9:	addq $8, %rsp
	popq %r14
	popq %r13
	popq %r12
	popq %rbx
	ret

# The "knc" hwmon device (kernel patch 0027): fifteen temperatures, the
# highest die maximum, core voltage and clock. Without the device the
# sample carries none, with a peak of 0, as the Rust agent's default.
st_sensors:
	pushq %rbx
	pushq %r12
	subq $8, %rsp
	cmpq $0, hw_found(%rip)
	jne 1f
	call hw_scan
	cmpq $0, hw_found(%rip)
	jne 1f
	xorl %edi, %edi
	call w_u16
	xorl %edi, %edi
	call w_u16
	xorl %edi, %edi
	call w_u32
	xorl %edi, %edi
	call w_u32
	jmp 9f
1:	movl $TEMP_CHANNELS, %edi
	call w_u16
	movl $1, %ebx
2:	movl %ebx, %edi
	leaq p_input(%rip), %rsi
	call millideg
	movl %eax, %edi
	call w_u16
	incl %ebx
	cmpl $TEMP_CHANNELS, %ebx
	jbe 2b
	movl $1, %ebx
	movl $-32768, %r12d
3:	movl %ebx, %edi
	leaq p_max(%rip), %rsi
	call millideg
	cmpl %r12d, %eax
	jle 4f
	movl %eax, %r12d
4:	incl %ebx
	cmpl $DIE_CHANNELS, %ebx
	jbe 3b
	movl %r12d, %edi
	call w_u16
	leaq p_in0(%rip), %rsi
	call hw_file
	call read_small
	call parse_u32
	testl %edx, %edx
	jnz 5f
	xorl %eax, %eax
5:	movl %eax, %edi
	call w_u32
	leaq p_core_mhz(%rip), %rsi
	call hw_file
	call read_small
	call parse_u32
	testl %edx, %edx
	jnz 6f
	xorl %eax, %eax
6:	movl %eax, %edi
	call w_u32
9:	addq $8, %rsp
	popq %r12
	popq %rbx
	ret

# ------------------------------------------------------------ devices

# tokens(rsi = cursor, rdi = end, rdx = table, ecx = most) -> eax = how
# many (ptr, len) pairs were stored.
tokens:
	pushq %rbx
	pushq %r12
	pushq %r13
	pushq %r14
	subq $8, %rsp
	movq %rdi, %r12
	movq %rdx, %r13
	movl %ecx, %r14d
	xorl %ebx, %ebx
1:	cmpl %r14d, %ebx
	jae 2f
	movq %r12, %rdi
	call next_token
	testq %rax, %rax
	jz 2f
	movq %rbx, %rcx
	shlq $4, %rcx
	movq %rax, (%r13,%rcx)
	movq %rdx, 8(%r13,%rcx)
	incl %ebx
	jmp 1b
2:	movl %ebx, %eax
	addq $8, %rsp
	popq %r14
	popq %r13
	popq %r12
	popq %rbx
	ret

# dev_entry(rsi = name, rdx = name length, r8 = first count, r9 = second):
# one device, if the list and the frame have room; counts it.
dev_entry:
	cmpq $MAX_LIST, st_listed(%rip)
	jae 9f
	leaq 18(%rdx), %rcx
	pushq %rsi
	pushq %rdx
	call room
	popq %rdx
	popq %rsi
	jb 9f
	pushq %r9
	pushq %r8
	call w_str
	popq %rdi
	call w_u64
	popq %rdi
	call w_u64
	incq st_listed(%rip)
9:	ret

# list_begin / list_end: a u16 count written before its items are known.
list_begin:
	movq wp(%rip), %rax
	movq %rax, st_count_at(%rip)
	movq $0, st_listed(%rip)
	xorl %edi, %edi
	jmp w_u16

list_end:
	movq st_count_at(%rip), %rax
	movq st_listed(%rip), %rcx
	movw %cx, (%rax)
	ret

# /proc/diskstats: the phiblk devices the ring serves, bytes read and
# written (fields 6 and 10, sectors of 512 bytes,
# Documentation/admin-guide/iostats.rst). A line whose counters do not
# parse is left out.
st_disks:
	pushq %rbx
	pushq %r12
	pushq %r13
	subq $176, %rsp
	call list_begin
	leaq p_diskstats(%rip), %rdi
	leaq bigbuf(%rip), %rsi
	movl $BIGCAP, %edx
	call read_text
	leaq bigbuf(%rip), %r12
	leaq (%r12,%rax), %r13
1:	movq %r12, %rsi
	movq %r13, %rdi
	call next_line
	movq %rsi, %r12
	testq %rax, %rax
	jz 9f
	leaq (%rax,%rdx), %rdi
	movq %rax, %rsi
	movq %rsp, %rdx
	movl $10, %ecx
	call tokens
	movl %eax, %ebx
	cmpl $3, %ebx
	jb 1b
	movq 32(%rsp), %rsi
	movq 40(%rsp), %rdx
	leaq s_phiblk(%rip), %rdi
	call starts_cstr
	testl %eax, %eax
	jz 1b
	cmpl $6, %ebx
	jb 1b
	movq 80(%rsp), %rsi
	movq 88(%rsp), %rdx
	call parse_u64
	testl %edx, %edx
	jz 1b
	shlq $9, %rax
	movq %rax, 160(%rsp)
	cmpl $10, %ebx
	jb 1b
	movq 144(%rsp), %rsi
	movq 152(%rsp), %rdx
	call parse_u64
	testl %edx, %edx
	jz 1b
	shlq $9, %rax
	movq %rax, %r9
	movq 160(%rsp), %r8
	movq 32(%rsp), %rsi
	movq 40(%rsp), %rdx
	call dev_entry
	jmp 1b
9:	call list_end
	addq $176, %rsp
	popq %r13
	popq %r12
	popq %rbx
	ret

# /proc/net/dev: every interface but the loopback, bytes received and sent
# (the first and ninth counters after "name:").
st_nets:
	pushq %rbx
	pushq %r12
	pushq %r13
	pushq %r14
	pushq %r15
	subq $160, %rsp
	call list_begin
	leaq p_netdev(%rip), %rdi
	leaq bigbuf(%rip), %rsi
	movl $BIGCAP, %edx
	call read_text
	leaq bigbuf(%rip), %r12
	leaq (%r12,%rax), %r13
1:	movq %r12, %rsi
	movq %r13, %rdi
	call next_line
	movq %rsi, %r12
	testq %rax, %rax
	jz 9f
	leaq (%rax,%rdx), %r14		# line end
	movq %rax, %r15			# line start
	movq %rax, %rcx
2:	cmpq %r14, %rcx
	jae 1b
	cmpb $':', (%rcx)
	je 3f
	incq %rcx
	jmp 2b
3:	leaq 1(%rcx), %rbx		# after the colon
	movq %r15, %rsi
	movq %rcx, %rdx
	subq %r15, %rdx
	call trim
	movq %rsi, 144(%rsp)
	movq %rdx, 152(%rsp)
	leaq s_lo(%rip), %rdi
	call eq_cstr
	testl %eax, %eax
	jnz 1b
	movq %rbx, %rsi
	movq %r14, %rdi
	movq %rsp, %rdx
	movl $9, %ecx
	call tokens
	movl %eax, %ebx
	testl %ebx, %ebx
	jz 1b
	movq 0(%rsp), %rsi
	movq 8(%rsp), %rdx
	call parse_u64
	testl %edx, %edx
	jz 1b
	movq %rax, %r15
	cmpl $9, %ebx
	jb 1b
	movq 128(%rsp), %rsi
	movq 136(%rsp), %rdx
	call parse_u64
	testl %edx, %edx
	jz 1b
	movq %rax, %r9
	movq %r15, %r8
	movq 144(%rsp), %rsi
	movq 152(%rsp), %rdx
	call dev_entry
	jmp 1b
9:	call list_end
	addq $160, %rsp
	popq %r15
	popq %r14
	popq %r13
	popq %r12
	popq %rbx
	ret

# ------------------------------------------------------------ processes

# km_find(edi = pid) -> rax = its entry in the previous table, or 0.
km_find:
	movq km_old(%rip), %r8
	xorl %ecx, %ecx			# lo
	movq km_old_n(%rip), %rdx	# hi
1:	cmpq %rdx, %rcx
	jae 8f
	leaq (%rcx,%rdx), %rax
	shrq $1, %rax
	movq %rax, %r9
	shlq $4, %r9
	movl (%r8,%r9), %r10d
	cmpl %edi, %r10d
	je 9f
	jb 2f
	movq %rax, %rdx
	jmp 1b
2:	leaq 1(%rax), %rcx
	jmp 1b
8:	xorl %eax, %eax
	ret
9:	leaq (%r8,%r9), %rax
	ret

# km_add(edi = pid, rsi = ticks): record a kernel thread in the table being
# built.
km_add:
	movq km_new_n(%rip), %rcx
	cmpq $KM_MAX, %rcx
	jae 9f
	movq km_new(%rip), %r8
	testq %rcx, %rcx
	jz 1f
	movq %rcx, %rax
	shlq $4, %rax
	cmpl -16(%r8,%rax), %edi
	ja 1f
	movq $1, km_unsorted(%rip)
1:	movq %rcx, %rax
	shlq $4, %rax
	movl %edi, (%r8,%rax)
	movq %rsi, 8(%r8,%rax)
	incq km_new_n(%rip)
9:	ret

# km_sort: insertion sort of the new table by pid, needed only if /proc
# ever lists pids out of order (it lists them ascending).
km_sort:
	movq km_new(%rip), %r8
	movq km_new_n(%rip), %r9
	movl $1, %ecx
1:	cmpq %r9, %rcx
	jae 9f
	movq %rcx, %rax
	shlq $4, %rax
	movl (%r8,%rax), %r10d
	movq 8(%r8,%rax), %r11
	movq %rcx, %rdx
2:	testq %rdx, %rdx
	jz 3f
	movq %rdx, %rax
	shlq $4, %rax
	cmpl %r10d, -16(%r8,%rax)
	jbe 3f
	movq -16(%r8,%rax), %rsi
	movq %rsi, (%r8,%rax)
	movq -8(%r8,%rax), %rsi
	movq %rsi, 8(%r8,%rax)
	decq %rdx
	jmp 2b
3:	movq %rdx, %rax
	shlq $4, %rax
	movl %r10d, (%r8,%rax)
	movq %r11, 8(%r8,%rax)
	incq %rcx
	jmp 1b
9:	ret

# Every user process, and each kernel thread whose CPU time moved since the
# previous sample; all of them counted. Known kernel threads are opened
# only every KTHREAD_EVERY samples (1500 stat reads a second cost a sixth
# of a hardware thread, measured 2026-09-17).
st_procs:
	pushq %rbx
	pushq %rbp
	pushq %r12
	pushq %r13
	pushq %r14
	pushq %r15
	subq $88, %rsp
	cmpq $0, km_old(%rip)
	jne 1f
	leaq km_a(%rip), %rax
	movq %rax, km_old(%rip)
	leaq km_b(%rip), %rax
	movq %rax, km_new(%rip)
1:	incl samples(%rip)
	movl samples(%rip), %eax
	xorl %edx, %edx
	movl $KTHREAD_EVERY, %ecx
	divl %ecx
	xorl %eax, %eax
	cmpl $1, %edx
	sete %al
	movq %rax, st_refresh(%rip)
	movq $0, st_nprocs(%rip)
	movq $0, st_nkthreads(%rip)
	movq $0, km_new_n(%rip)
	movq $0, km_unsorted(%rip)
	call list_begin
	leaq p_proc(%rip), %rdi
	call open_dir
	testq %rax, %rax
	js 90f				# no /proc: an empty list, the table kept
	movl %eax, %ebx
2:	movl %ebx, %edi
	leaq dirbuf(%rip), %rsi
	movl $DIRCAP, %edx
	SYS SYS_getdents64
	testq %rax, %rax
	jle 80f
	movq %rax, %r13			# bytes
	xorl %r12d, %r12d		# offset
3:	cmpq %r13, %r12
	jae 2b
	leaq dirbuf(%rip), %r14
	addq %r12, %r14
	movzwl 16(%r14), %eax
	addq %rax, %r12
	addq $19, %r14			# the name
	movq %r14, %rsi
	call cstr_len
	movq %rdx, %r15			# its length
	movq %r14, %rsi
	call parse_u32
	testl %edx, %edx
	jz 3b
	movl %eax, %ebp			# pid
	# between refreshes a known kernel thread is counted, not opened
	cmpq $0, st_refresh(%rip)
	jne 4f
	movl %ebp, %edi
	call km_find
	testq %rax, %rax
	jz 4f
	incq st_nprocs(%rip)
	incq st_nkthreads(%rip)
	movq 8(%rax), %rsi
	movl %ebp, %edi
	call km_add
	jmp 3b
4:	# /proc/<pid>/stat
	call p_begin
	leaq p_proc_slash(%rip), %rsi
	call w_cstr
	movq %r14, %rsi
	call w_cstr
	leaq p_stat_tail(%rip), %rsi
	call w_cstr
	call p_end
	movq %rax, %rdi
	leaq pbuf1(%rip), %rsi
	movl $SMALLCAP, %edx
	call read_text
	leaq pbuf1(%rip), %rsi
	movq %rax, %rdx
	movq %rsp, %rdi
	call parse_stat
	testl %eax, %eax
	jz 3b
	incq st_nprocs(%rip)
	cmpq $0, 32(%rsp)		# kthread
	jne 6f
	# The stat line's rss is the mm counter's global part, and with 228
	# CPUs the per-CPU batch is 456 pages, so a small process reads 0
	# there (measured 2026-09-17); status sums the parts.
	call p_begin
	leaq p_proc_slash(%rip), %rsi
	call w_cstr
	movq %r14, %rsi
	call w_cstr
	leaq p_status_tail(%rip), %rsi
	call w_cstr
	call p_end
	movq %rax, %rdi
	leaq pbuf2(%rip), %rsi
	movl $SMALLCAP, %edx
	call read_text
	leaq pbuf2(%rip), %rsi
	movq %rax, %rdx
	call vm_rss
	testl %edx, %edx
	jz 7f
	movq %rax, 16(%rsp)
	jmp 7f
6:	incq st_nkthreads(%rip)
	movl %ebp, %edi
	call km_find
	movq %rax, 80(%rsp)
	movl %ebp, %edi
	movq 8(%rsp), %rsi
	call km_add
	movq 80(%rsp), %rax
	testq %rax, %rax
	jz 7f
	movq 8(%rax), %rax
	cmpq 8(%rsp), %rax
	je 3b				# a kernel thread that did not run
7:	# listed, while the list and the frame have room
	cmpq $MAX_PROCS, st_listed(%rip)
	jae 3b
	movq 48(%rsp), %rcx
	addq $28, %rcx
	call room
	jb 3b
	movl %ebp, %edi
	call w_u32
	movl 0(%rsp), %edi
	call w_byte
	movl 32(%rsp), %edi
	call w_byte
	movq 8(%rsp), %rdi
	call w_u64
	movq 16(%rsp), %rdi
	call w_u64
	movl 24(%rsp), %edi
	call w_u32
	movq 40(%rsp), %rsi
	movq 48(%rsp), %rdx
	call w_str
	incq st_listed(%rip)
	jmp 3b
80:	movl %ebx, %edi
	SYS SYS_close
	cmpq $0, km_unsorted(%rip)
	je 81f
	call km_sort
81:	movq km_old(%rip), %rax
	movq km_new(%rip), %rcx
	movq %rcx, km_old(%rip)
	movq %rax, km_new(%rip)
	movq km_new_n(%rip), %rax
	movq %rax, km_old_n(%rip)
90:	call list_end
	movl st_nprocs(%rip), %edi
	call w_u32
	movl st_nkthreads(%rip), %edi
	call w_u32
	addq $88, %rsp
	popq %r15
	popq %r14
	popq %r13
	popq %r12
	popq %rbp
	popq %rbx
	ret

# parse_stat(rsi = text, rdx = length, rdi = out) -> eax 1 with out filled:
# 0 state, 8 ticks, 16 rss_kb, 24 threads, 32 kthread, 40 comm pointer, 48
# comm length. The command name is in parentheses and may hold spaces and
# parentheses, so the fields are counted from the last ')'
# (proc_pid_stat(5)); a missing or unparsable field reads 0.
parse_stat:
	pushq %rbx
	pushq %r12
	pushq %r13
	pushq %r14
	pushq %r15
	subq $64, %rsp
	movq %rdi, %rbx
	leaq (%rsi,%rdx), %r12		# end
	# first '('
	movq %rsi, %rcx
1:	cmpq %r12, %rcx
	jae 90f
	cmpb $'(', (%rcx)
	je 2f
	incq %rcx
	jmp 1b
2:	# last ')'
	movq %r12, %rax
3:	cmpq %rsi, %rax
	jbe 90f
	decq %rax
	cmpb $')', (%rax)
	jne 3b
	leaq 1(%rcx), %rdx
	cmpq %rdx, %rax
	jb 90f
	movq %rdx, 40(%rbx)
	movq %rax, %rcx
	subq %rdx, %rcx
	movq %rcx, 48(%rbx)
	leaq 1(%rax), %r13		# the fields
	# the fields we need, by index after comm: 0 state, 6 flags, 11 utime,
	# 12 stime, 17 num_threads, 21 rss; 0 for each by default
	xorl %eax, %eax
	movq %rax, 0(%rsp)		# flags
	movq %rax, 8(%rsp)		# utime
	movq %rax, 16(%rsp)		# stime
	movq %rax, 24(%rsp)		# threads
	movq %rax, 32(%rsp)		# rss pages
	movq $'?', 0(%rbx)
	xorl %r14d, %r14d		# index
4:	cmpl $22, %r14d
	jae 7f
	movq %r13, %rsi
	movq %r12, %rdi
	call next_token
	movq %rsi, %r13
	testq %rax, %rax
	jz 7f
	testl %r14d, %r14d
	jnz 5f
	movzbl (%rax), %ecx
	movq %rcx, 0(%rbx)
	jmp 6f
5:	movq %rax, %rsi
	movl %r14d, %r15d
	call parse_u64
	testl %edx, %edx
	jz 6f
	cmpl $6, %r15d
	jne 51f
	movq %rax, 0(%rsp)
51:	cmpl $11, %r15d
	jne 52f
	movq %rax, 8(%rsp)
52:	cmpl $12, %r15d
	jne 53f
	movq %rax, 16(%rsp)
53:	cmpl $17, %r15d
	jne 54f
	movq %rax, 24(%rsp)
54:	cmpl $21, %r15d
	jne 6f
	movq %rax, 32(%rsp)
6:	incl %r14d
	jmp 4b
7:	movq 8(%rsp), %rax
	addq 16(%rsp), %rax
	movq %rax, 8(%rbx)
	movq 32(%rsp), %rax
	imulq page_kb(%rip), %rax
	movq %rax, 16(%rbx)
	movl 24(%rsp), %eax
	movq %rax, 24(%rbx)
	xorl %eax, %eax
	testq $PF_KTHREAD, 0(%rsp)
	setnz %al
	movq %rax, 32(%rbx)
	movl $1, %eax
	jmp 99f
90:	xorl %eax, %eax
99:	addq $64, %rsp
	popq %r15
	popq %r14
	popq %r13
	popq %r12
	popq %rbx
	ret

# vm_rss(rsi = /proc/<pid>/status text, rdx = length) -> rax = VmRSS in kB,
# edx = 1, or edx = 0: the first "VmRSS:" line decides.
vm_rss:
	pushq %r12
	pushq %r13
	subq $8, %rsp
	leaq (%rsi,%rdx), %r13
	movq %rsi, %r12
1:	movq %r12, %rsi
	movq %r13, %rdi
	call next_line
	movq %rsi, %r12
	testq %rax, %rax
	jz 8f
	movq %rax, %rsi
	pushq %rax
	pushq %rdx
	leaq s_vmrss(%rip), %rdi
	call starts_cstr
	popq %rdx
	popq %rsi
	testl %eax, %eax
	jz 1b
	leaq (%rsi,%rdx), %rdi
	addq $6, %rsi
	call next_token
	testq %rax, %rax
	jz 8f
	movq %rax, %rsi
	call parse_u64
	jmp 9f
8:	xorl %eax, %eax
	xorl %edx, %edx
9:	addq $8, %rsp
	popq %r13
	popq %r12
	ret
