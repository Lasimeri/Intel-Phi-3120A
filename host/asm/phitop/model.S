# model.S: from samples to what the screen shows (host/crates/phitop's
# model.rs): a StatReply and a TrafficReply decoded into a snapshot,
# and two consecutive snapshots into the derived numbers: per-thread
# load, the core layout, transfer rates, per-process CPU shares. See
# model.md.
	.include "defs.inc"

	.set TICKS_PER_SEC, 100
	.set FORGET_AFTER_US, 120000000

# A snapshot (layout in defs.inc, SN_*): fixed-size records so that a
# frame is drawn from memory the decoder filled, without allocation.

	.text

	.globl snapshot_decode
# snapshot_decode(rdi = snapshot, rsi = StatReply body, rdx = length):
# the sample in wire order (phi-rpc put_stat), lists clipped to the
# snapshot's capacities (the counts sent are kept for the totals).
snapshot_decode:
	pushq %rbx
	pushq %r12
	pushq %r13
	pushq %r14
	pushq %r15
	movq %rdi, %rbx
	movq %rsi, %r12		# cursor
	leaq (%rsi,%rdx), %r13	# end (validate checked the layout)
	movq (%r12), %rax
	movq %rax, SN_UPTIME(%rbx)
	addq $8, %r12
	# cpus
	movzwl (%r12), %r14d
	addq $2, %r12
	movq %r14, SN_NCPU_SENT(%rbx)
	xorl %r15d, %r15d
1:	cmpq %r14, %r15
	jae 3f
	cmpq $SN_CPU_CAP, %r15
	jae 2f
	movq %r15, %rax
	imulq $12, %rax, %rax
	leaq SN_CPUS(%rbx,%rax), %rdi
	movzwl (%r12), %eax
	movw %ax, 0(%rdi)
	movl 2(%r12), %eax
	movl %eax, 4(%rdi)
	movl 6(%r12), %eax
	movl %eax, 8(%rdi)
2:	addq $10, %r12
	incq %r15
	jmp 1b
3:	cmpq $SN_CPU_CAP, %r14
	jbe 31f
	movl $SN_CPU_CAP, %r14d
31:	movq %r14, SN_NCPU(%rbx)
	# memory: six u64
	movq 0(%r12), %rax
	movq %rax, SN_MEM+0(%rbx)
	movq 8(%r12), %rax
	movq %rax, SN_MEM+8(%rbx)
	movq 16(%r12), %rax
	movq %rax, SN_MEM+16(%rbx)
	movq 24(%r12), %rax
	movq %rax, SN_MEM+24(%rbx)
	movq 32(%r12), %rax
	movq %rax, SN_MEM+32(%rbx)
	movq 40(%r12), %rax
	movq %rax, SN_MEM+40(%rbx)
	addq $48, %r12
	movzwl 0(%r12), %eax
	movw %ax, SN_LOAD+0(%rbx)
	movzwl 2(%r12), %eax
	movw %ax, SN_LOAD+2(%rbx)
	movzwl 4(%r12), %eax
	movw %ax, SN_LOAD+4(%rbx)
	movl 6(%r12), %eax
	movl %eax, SN_RUNNING(%rbx)
	movl 10(%r12), %eax
	movl %eax, SN_TASKS(%rbx)
	addq $14, %r12
	# temperatures
	movzwl (%r12), %r14d
	addq $2, %r12
	xorl %r15d, %r15d
4:	cmpq %r14, %r15
	jae 6f
	cmpq $SN_TEMP_CAP, %r15
	jae 5f
	movzwl (%r12), %eax
	movw %ax, SN_TEMPS(%rbx,%r15,2)
5:	addq $2, %r12
	incq %r15
	jmp 4b
6:	cmpq $SN_TEMP_CAP, %r14
	jbe 61f
	movl $SN_TEMP_CAP, %r14d
61:	movq %r14, SN_NTEMPS(%rbx)
	movswq (%r12), %rax
	movq %rax, SN_PEAK(%rbx)
	movl 2(%r12), %eax
	movq %rax, SN_VCORE(%rbx)
	movl 6(%r12), %eax
	movq %rax, SN_MHZ(%rbx)
	addq $10, %r12
	# devices
	leaq SN_DISKS(%rbx), %rdi
	call decode_devs
	movq %rax, SN_NDISKS(%rbx)
	leaq SN_NETS(%rbx), %rdi
	call decode_devs
	movq %rax, SN_NNETS(%rbx)
	# processes
	movzwl (%r12), %r14d
	addq $2, %r12
	xorl %r15d, %r15d
7:	cmpq %r14, %r15
	jae 9f
	cmpq $SN_PROC_CAP, %r15
	jae 8f
	movq %r15, %rax
	imulq $PR_SIZE, %rax, %rax
	leaq SN_PROCS(%rbx,%rax), %rdi
	movl 0(%r12), %eax
	movl %eax, PR_PID(%rdi)
	movb 4(%r12), %al
	movb %al, PR_STATE(%rdi)
	movb 5(%r12), %al
	movb %al, PR_KTHREAD(%rdi)
	movq 6(%r12), %rax
	movq %rax, PR_TICKS(%rdi)
	movq 14(%r12), %rax
	movq %rax, PR_RSS(%rdi)
	movl 22(%r12), %eax
	movl %eax, PR_THREADS(%rdi)
	movzwl 26(%r12), %ecx
	movl %ecx, %edx
	cmpl $PR_COMM_CAP - 1, %edx
	jbe 71f
	movl $PR_COMM_CAP - 1, %edx
71:	movl %edx, PR_COMM_LEN(%rdi)
	pushq %rcx
	leaq PR_COMM(%rdi), %rdi
	leaq 28(%r12), %rsi
	movl %edx, %ecx
	rep movsb
	movb $0, (%rdi)
	popq %rcx
	leaq 28(%r12,%rcx), %r12
	incq %r15
	jmp 7b
8:	movzwl 26(%r12), %ecx
	leaq 28(%r12,%rcx), %r12
	incq %r15
	jmp 7b
9:	cmpq $SN_PROC_CAP, %r14
	jbe 91f
	movl $SN_PROC_CAP, %r14d
91:	movq %r14, SN_NLISTED(%rbx)
	movl 0(%r12), %eax
	movl %eax, SN_NPROCS(%rbx)
	movl 4(%r12), %eax
	movl %eax, SN_NKTHREADS(%rbx)
	popq %r15
	popq %r14
	popq %r13
	popq %r12
	popq %rbx
	ret

# decode_devs(rdi = the table, r12 = cursor) -> rax = entries kept, r12
# advanced: u16 count; name string, u64 read, u64 written.
decode_devs:
	pushq %rbx
	pushq %r14
	pushq %r15
	movq %rdi, %rbx
	movzwl (%r12), %r14d
	addq $2, %r12
	xorl %r15d, %r15d
1:	cmpq %r14, %r15
	jae 4f
	movzwl (%r12), %ecx
	addq $2, %r12
	cmpq $SN_DEV_CAP, %r15
	jae 3f
	movq %r15, %rax
	imulq $DEV_SIZE, %rax, %rax
	leaq (%rbx,%rax), %rdi
	movl %ecx, %edx
	cmpl $DEV_NAME_CAP - 1, %edx
	jbe 2f
	movl $DEV_NAME_CAP - 1, %edx
2:	movl %edx, DEV_NAME_LEN(%rdi)
	pushq %rcx
	pushq %rdi
	leaq DEV_NAME(%rdi), %rdi
	movq %r12, %rsi
	movl %edx, %ecx
	rep movsb
	movb $0, (%rdi)
	popq %rdi
	popq %rcx
	movq (%r12,%rcx), %rax
	movq %rax, DEV_READ(%rdi)
	movq 8(%r12,%rcx), %rax
	movq %rax, DEV_WRITTEN(%rdi)
3:	leaq 16(%r12,%rcx), %r12
	incq %r15
	jmp 1b
4:	cmpq $SN_DEV_CAP, %r14
	jbe 5f
	movl $SN_DEV_CAP, %r14d
5:	movq %r14, %rax
	popq %r15
	popq %r14
	popq %rbx
	ret

	.globl traffic_decode
# traffic_decode(rdi = snapshot, rsi = TrafficReply body): six u64.
traffic_decode:
	movq 0(%rsi), %rax
	movq %rax, SN_TRAFFIC+0(%rdi)
	movq 8(%rsi), %rax
	movq %rax, SN_TRAFFIC+8(%rdi)
	movq 16(%rsi), %rax
	movq %rax, SN_TRAFFIC+16(%rdi)
	movq 24(%rsi), %rax
	movq %rax, SN_TRAFFIC+24(%rdi)
	movq 32(%rsi), %rax
	movq %rax, SN_TRAFFIC+32(%rdi)
	movq 40(%rsi), %rax
	movq %rax, SN_TRAFFIC+40(%rdi)
	ret

# ------------------------------------------------------------ history

# The per-slot history of processes: pid -> (ticks, at) in an open
# addressing table of SEEN_CAP entries (pid u32 at 0, 0 = empty; ticks at
# 8; at_us at 16), so a share is computed since the pid was last
# reported (a kernel thread the agent left out while idle gets its burst
# averaged over the gap; a new process shows zero until its second
# report). Entries unseen for two minutes are forgotten.

	.globl seen_find
# seen_find(rdi = table, esi = pid) -> rax = the entry (existing or the
# empty slot for it).
seen_find:
	movl %esi, %eax
	imull $2654435761, %eax, %eax
	shrl $32 - SEEN_BITS, %eax
1:	movq %rax, %rcx
	imulq $SEEN_SIZE, %rcx, %rcx
	leaq (%rdi,%rcx), %rcx
	movl (%rcx), %edx
	testl %edx, %edx
	jz 2f
	cmpl %esi, %edx
	je 2f
	incl %eax
	andl $SEEN_CAP - 1, %eax
	jmp 1b
2:	movq %rcx, %rax
	ret

# seen_remember(rdi = table, rsi = snapshot): every listed process
# recorded with its ticks and the sample's time; stale entries dropped
# (a tombstone would break the probe chains, so a stale entry is
# emptied and every entry after it in its chain reinserted).
seen_remember:
	pushq %rbx
	pushq %r12
	pushq %r13
	pushq %r14
	movq %rdi, %rbx
	movq %rsi, %r12
	xorl %r13d, %r13d
1:	cmpq SN_NLISTED(%r12), %r13
	jae 3f
	movq %r13, %rax
	imulq $PR_SIZE, %rax, %rax
	leaq SN_PROCS(%r12,%rax), %r14
	movq %rbx, %rdi
	movl PR_PID(%r14), %esi
	call seen_find
	movl PR_PID(%r14), %ecx
	movl %ecx, (%rax)
	movq PR_TICKS(%r14), %rcx
	movq %rcx, 8(%rax)
	movq SN_AT_US(%r12), %rcx
	movq %rcx, 16(%rax)
	incq %r13
	jmp 1b
3:	# forget the stale: walk every entry; a stale one is emptied and the
	# chain after it reinserted
	xorl %r13d, %r13d
4:	cmpq $SEEN_CAP, %r13
	jae 9f
	movq %r13, %rax
	imulq $SEEN_SIZE, %rax, %rax
	leaq (%rbx,%rax), %r14
	cmpl $0, (%r14)
	je 8f
	movq SN_AT_US(%r12), %rax
	subq 16(%r14), %rax
	cmpq $FORGET_AFTER_US, %rax
	jb 8f
	movl $0, (%r14)
	# reinsert the entries that follow in the chain
	movq %r13, %rcx
5:	incq %rcx
	andq $SEEN_CAP - 1, %rcx
	movq %rcx, %rax
	imulq $SEEN_SIZE, %rax, %rax
	leaq (%rbx,%rax), %rdi
	cmpl $0, (%rdi)
	je 8f
	movl (%rdi), %esi
	movq 8(%rdi), %r8
	movq 16(%rdi), %r9
	movl $0, (%rdi)
	pushq %rcx
	pushq %r8
	pushq %r9
	pushq %rsi
	movq %rbx, %rdi
	call seen_find
	popq %rsi
	popq %r9
	popq %r8
	popq %rcx
	movl %esi, (%rax)
	movq %r8, 8(%rax)
	movq %r9, 16(%rax)
	jmp 5b
8:	incq %r13
	jmp 4b
9:	popq %r14
	popq %r13
	popq %r12
	popq %rbx
	ret

# ------------------------------------------------------------ deriving

	.globl model_update
# model_update(rdi = slot): the slot's incoming snapshot becomes the
# current one; with a previous one, the derived numbers are computed and
# the slot's derived flag set. The history is updated last.
model_update:
	pushq %rbx
	pushq %r12
	pushq %r13
	movq %rdi, %rbx
	movq SL_CUR(%rbx), %r12		# the snapshot just filled
	movq SL_PREV(%rbx), %r13
	cmpq $0, SL_HAVE_PREV(%rbx)
	je 1f
	movq %rbx, %rdi
	movq %r13, %rsi
	movq %r12, %rdx
	call derive
	movq $1, SL_HAVE_DERIVED(%rbx)
1:	leaq SL_SEEN(%rbx), %rdi
	movq %r12, %rsi
	call seen_remember
	# swap: the current becomes the previous, the other buffer receives next
	movq %r12, SL_PREV(%rbx)
	movq %r13, SL_CUR(%rbx)
	movq $1, SL_HAVE_PREV(%rbx)
	popq %r13
	popq %r12
	popq %rbx
	ret

# derive(rdi = slot, rsi = previous snapshot, rdx = current snapshot):
# fills the slot's derived record.
derive:
	pushq %rbx
	pushq %r12
	pushq %r13
	pushq %r14
	pushq %r15
	subq $24, %rsp
	movq %rdi, %rbx
	movq %rsi, %r12		# prev
	movq %rdx, %r13		# cur
	leaq SL_DERIVED(%rbx), %r14
	# interval, at least 1 ms
	movq SN_AT_US(%r13), %rax
	subq SN_AT_US(%r12), %rax
	cmpq $1000, %rax
	jae 1f
	movl $1000, %eax
1:	movq %rax, DV_INTERVAL_US(%r14)
	# per-CPU load in 1/10000: busy / (busy + idle)
	movq SN_NCPU(%r13), %rax
	movq %rax, DV_NCPU(%r14)
	xorl %r15d, %r15d
	xorl %r8d, %r8d		# sum of loads
	movq %r8, 0(%rsp)
2:	cmpq DV_NCPU(%r14), %r15
	jae 5f
	movq %r15, %rax
	imulq $12, %rax, %rax
	leaq SN_CPUS(%r13,%rax), %rcx
	movl 4(%rcx), %esi	# busy now
	movl 8(%rcx), %edi	# idle now
	xorl %r8d, %r8d
	xorl %r9d, %r9d
	cmpq SN_NCPU(%r12), %r15
	jae 3f
	leaq SN_CPUS(%r12,%rax), %rcx
	movl 4(%rcx), %r8d
	movl 8(%rcx), %r9d
3:	subl %r8d, %esi		# busy delta (wrapping)
	subl %r9d, %edi		# idle delta
	movl %esi, %eax
	addl %edi, %eax		# total
	jz 4f
	movl %esi, %ecx
	imulq $10000, %rcx, %rcx
	movq %rcx, %r10
	movl %eax, %ecx
	movq %r10, %rax
	xorl %edx, %edx
	divq %rcx
	jmp 41f
4:	xorl %eax, %eax
41:	movl %eax, DV_CPU_LOAD(%r14,%r15,4)
	addq %rax, 0(%rsp)
	incq %r15
	jmp 2b
5:	# the mean
	movq DV_NCPU(%r14), %rcx
	testq %rcx, %rcx
	jnz 6f
	movl $1, %ecx
6:	movq 0(%rsp), %rax
	xorl %edx, %edx
	divq %rcx
	movq %rax, DV_MEAN(%r14)
	# cores: cpus grouped by core id, cores in id order (insertion into a
	# sorted table; at most a few hundred cores)
	movq $0, DV_NCORES(%r14)
	movq $0, DV_MAXTHREADS(%r14)
	xorl %r15d, %r15d
7:	cmpq DV_NCPU(%r14), %r15
	jae 12f
	movq %r15, %rax
	imulq $12, %rax, %rax
	movzwl SN_CPUS(%r13,%rax), %esi	# core id
	# find it
	xorl %ecx, %ecx
8:	cmpq DV_NCORES(%r14), %rcx
	jae 9f
	movq %rcx, %rax
	imulq $CORE_SIZE, %rax, %rax
	leaq DV_CORES(%r14,%rax), %rdi
	cmpw %si, CORE_ID(%rdi)
	je 11f
	ja 10f
	incq %rcx
	jmp 8b
9:	# append at the end
	cmpq $CORE_CAP, DV_NCORES(%r14)
	jae 111f
	movq %rcx, %rax
	imulq $CORE_SIZE, %rax, %rax
	leaq DV_CORES(%r14,%rax), %rdi
	movw %si, CORE_ID(%rdi)
	movw $0, CORE_N(%rdi)
	incq DV_NCORES(%r14)
	jmp 11f
10:	# insert before rcx: shift the rest up
	cmpq $CORE_CAP, DV_NCORES(%r14)
	jae 111f
	movq DV_NCORES(%r14), %rax
	pushq %rcx
	pushq %rsi
101:	cmpq %rcx, %rax
	jbe 102f
	decq %rax
	movq %rax, %rdx
	imulq $CORE_SIZE, %rdx, %rdx
	leaq DV_CORES(%r14,%rdx), %rsi
	leaq CORE_SIZE(%rsi), %rdi
	pushq %rcx
	movl $CORE_SIZE, %ecx
	rep movsb
	popq %rcx
	jmp 101b
102:	popq %rsi
	popq %rcx
	movq %rcx, %rax
	imulq $CORE_SIZE, %rax, %rax
	leaq DV_CORES(%r14,%rax), %rdi
	movw %si, CORE_ID(%rdi)
	movw $0, CORE_N(%rdi)
	incq DV_NCORES(%r14)
11:	# add the cpu to core rdi
	movzwl CORE_N(%rdi), %eax
	cmpl $CORE_THREADS, %eax
	jae 111f
	movw %r15w, CORE_CPU(%rdi,%rax,2)
	incl %eax
	movw %ax, CORE_N(%rdi)
	cmpq DV_MAXTHREADS(%r14), %rax
	jbe 111f
	movq %rax, DV_MAXTHREADS(%r14)
111:	incq %r15
	jmp 7b
12:	# PCIe rates: bytes per second from the traffic counters
	leaq SN_TRAFFIC(%r13), %rsi
	leaq SN_TRAFFIC(%r12), %rdi
	movq 0(%rsi), %rax
	subq 0(%rdi), %rax
	call per_second
	movq %rax, DV_DMA_TO(%r14)
	movq 8(%rsi), %rax
	subq 8(%rdi), %rax
	call per_second
	movq %rax, DV_DMA_FROM(%r14)
	movq 16(%rsi), %rax
	subq 16(%rdi), %rax
	call per_second
	movq %rax, DV_APER_TO(%r14)
	movq 24(%rsi), %rax
	subq 24(%rdi), %rax
	call per_second
	movq %rax, DV_APER_FROM(%r14)
	# device rates, sorted by name
	leaq SN_DISKS(%r13), %rdi
	movq SN_NDISKS(%r13), %rsi
	leaq SN_DISKS(%r12), %rdx
	movq SN_NDISKS(%r12), %rcx
	leaq DV_DISKS(%r14), %r8
	call dev_rates
	movq %rax, DV_NDISKS(%r14)
	leaq SN_NETS(%r13), %rdi
	movq SN_NNETS(%r13), %rsi
	leaq SN_NETS(%r12), %rdx
	movq SN_NNETS(%r12), %rcx
	leaq DV_NETS(%r14), %r8
	call dev_rates
	movq %rax, DV_NNETS(%r14)
	# process shares in tenths of a percent of one thread: ticks since the
	# pid was last reported, over that span
	xorl %r15d, %r15d
13:	cmpq SN_NLISTED(%r13), %r15
	jae 16f
	movq %r15, %rax
	imulq $PR_SIZE, %rax, %rax
	leaq SN_PROCS(%r13,%rax), %rcx
	movq %rcx, 8(%rsp)
	leaq SL_SEEN(%rbx), %rdi
	movl PR_PID(%rcx), %esi
	call seen_find
	movq 8(%rsp), %rcx
	cmpl $0, (%rax)
	je 15f
	movq PR_TICKS(%rcx), %rdx
	cmpq 8(%rax), %rdx
	jb 15f
	movq SN_AT_US(%r13), %rsi
	cmpq 16(%rax), %rsi
	jbe 15f
	subq 8(%rax), %rdx		# tick delta
	subq 16(%rax), %rsi		# span us
	cmpq $1000, %rsi
	jae 14f
	movl $1000, %esi
14:	# tenths of percent = delta * 100 * 10 / (100 ticks/s) * 1e6 / span
	# = delta * 1e7 / span
	movq %rdx, %rax
	imulq $10000000, %rax, %rax
	xorl %edx, %edx
	divq %rsi
	movl $0xffffffff, %ecx
	cmpq %rcx, %rax
	jbe 141f
	movl %ecx, %eax
141:	jmp 151f
15:	xorl %eax, %eax
151:	movl %eax, DV_PROC_CPU(%r14,%r15,4)
	incq %r15
	jmp 13b
16:	call derive_vpu
	addq $24, %rsp
	popq %r15
	popq %r14
	popq %r13
	popq %r12
	popq %rbx
	ret

# derive_vpu (r12 = previous snapshot, r13 = current, r14 = derived):
# the co-processor worker's rates over the interval from the two copies
# of its stats line: requests a second and busy percent in tenths, each
# stage's time per request (0 without a request), and the line's own
# counts. DV_VPU_HAVE stays 0 when either sample lacks the line or a
# counter went backwards (a worker restarted between the samples).
derive_vpu:
	pushq %rbx
	movq $0, DV_VPU_HAVE(%r14)
	cmpq $0, SN_VPU_HAVE(%r13)
	je 9f
	cmpq $0, SN_VPU_HAVE(%r12)
	je 9f
	movq SN_VPU+VS_REQUESTS(%r13), %rbx
	subq SN_VPU+VS_REQUESTS(%r12), %rbx	# requests in the interval
	jc 9f
	movq SN_VPU+VS_REQUESTS(%r13), %rax
	movq %rax, DV_VPU_REQUESTS(%r14)
	movq SN_VPU+VS_ERRORS(%r13), %rax
	movq %rax, DV_VPU_ERRORS(%r14)
	movl SN_VPU+VS_LAST_KERNEL(%r13), %eax
	movq %rax, DV_VPU_KERNEL(%r14)
	movl SN_VPU+VS_LAST_THREADS(%r13), %eax
	movq %rax, DV_VPU_THREADS(%r14)
	# requests a second in tenths: n * 1e7 / interval_us
	movq %rbx, %rax
	imulq $10000000, %rax, %rax
	xorl %edx, %edx
	divq DV_INTERVAL_US(%r14)
	movq %rax, DV_VPU_REQ_TENTHS(%r14)
	# busy percent in tenths: busy_ns * 1000 / (interval_us * 1000)
	movq SN_VPU+VS_BUSY_NS(%r13), %rax
	subq SN_VPU+VS_BUSY_NS(%r12), %rax
	jc 9f
	xorl %edx, %edx
	divq DV_INTERVAL_US(%r14)
	movq %rax, DV_VPU_BUSY_TENTHS(%r14)
	# per request: the stage sums' deltas over the requests
	movq $0, DV_VPU_TOTAL_NS(%r14)
	movq $0, DV_VPU_COMPUTE_NS(%r14)
	movq $0, DV_VPU_PULL_NS(%r14)
	movq $0, DV_VPU_PUSH_NS(%r14)
	testq %rbx, %rbx
	jz 8f
	movq SN_VPU+VS_BUSY_NS(%r13), %rax
	subq SN_VPU+VS_BUSY_NS(%r12), %rax
	xorl %edx, %edx
	divq %rbx
	movq %rax, DV_VPU_TOTAL_NS(%r14)
	movq SN_VPU+VS_COMPUTE_NS(%r13), %rax
	subq SN_VPU+VS_COMPUTE_NS(%r12), %rax
	jc 9f
	xorl %edx, %edx
	divq %rbx
	movq %rax, DV_VPU_COMPUTE_NS(%r14)
	movq SN_VPU+VS_PULL_NS(%r13), %rax
	subq SN_VPU+VS_PULL_NS(%r12), %rax
	jc 9f
	xorl %edx, %edx
	divq %rbx
	movq %rax, DV_VPU_PULL_NS(%r14)
	movq SN_VPU+VS_PUSH_NS(%r13), %rax
	subq SN_VPU+VS_PUSH_NS(%r12), %rax
	jc 9f
	xorl %edx, %edx
	divq %rbx
	movq %rax, DV_VPU_PUSH_NS(%r14)
8:	movq $1, DV_VPU_HAVE(%r14)
9:	popq %rbx
	ret

# per_second(rax = a byte delta; r14 = derived) -> rax = bytes per
# second over the interval; rsi and rdi kept.
per_second:
	pushq %rdx
	pushq %rcx
	movq DV_INTERVAL_US(%r14), %rcx
	xorl %edx, %edx
	imulq $1000000, %rax, %rax
	divq %rcx
	popq %rcx
	popq %rdx
	ret

# dev_rates(rdi = now table, rsi = now count, rdx = before table, rcx =
# before count, r8 = rates out) -> rax = count: read and write bytes per
# second per device (a device new since the last sample rates as 0),
# sorted by name (insertion).
dev_rates:
	pushq %rbx
	pushq %r12
	pushq %r13
	pushq %r15
	pushq %rbp
	movq %rdi, %rbx
	movq %rsi, %r12
	movq %rdx, %r13
	movq %rcx, %r15
	movq %r8, %rbp
	xorl %r9d, %r9d
1:	cmpq %r12, %r9
	jae 9f
	movq %r9, %rax
	imulq $DEV_SIZE, %rax, %rax
	leaq (%rbx,%rax), %rsi		# now entry
	# the matching before entry
	xorl %r10d, %r10d
2:	cmpq %r15, %r10
	jae 4f
	movq %r10, %rax
	imulq $DEV_SIZE, %rax, %rax
	leaq (%r13,%rax), %rdi
	movl DEV_NAME_LEN(%rsi), %ecx
	cmpl DEV_NAME_LEN(%rdi), %ecx
	jne 3f
	pushq %rsi
	pushq %rdi
	leaq DEV_NAME(%rsi), %rsi
	leaq DEV_NAME(%rdi), %rdi
	repe cmpsb
	popq %rdi
	popq %rsi
	je 5f
3:	incq %r10
	jmp 2b
4:	movq %rsi, %rdi			# none: the rate is against itself, 0
5:	# the out entry, inserted in name order
	movq %r9, %rax
	imulq $RATE_SIZE, %rax, %rax
	leaq (%rbp,%rax), %r11
	pushq %r9
	pushq %rsi
	pushq %rdi
	pushq %r11
	# name
	movl DEV_NAME_LEN(%rsi), %ecx
	movl %ecx, RATE_NAME_LEN(%r11)
	pushq %rcx
	leaq RATE_NAME(%r11), %rdi
	leaq DEV_NAME(%rsi), %rsi
	rep movsb
	movb $0, (%rdi)
	popq %rcx
	movq 8(%rsp), %rdi
	movq 16(%rsp), %rsi
	movq DEV_READ(%rsi), %rax
	subq DEV_READ(%rdi), %rax
	pushq %rsi
	pushq %rdi
	call per_second
	popq %rdi
	popq %rsi
	movq (%rsp), %r11
	movq %rax, RATE_A(%r11)
	movq DEV_WRITTEN(%rsi), %rax
	subq DEV_WRITTEN(%rdi), %rax
	call per_second
	movq (%rsp), %r11
	movq %rax, RATE_B(%r11)
	popq %r11
	popq %rdi
	popq %rsi
	popq %r9
	incq %r9
	jmp 1b
9:	# sort by name: insertion over at most 16 entries
	movl $1, %r10d
10:	cmpq %r12, %r10
	jae 19f
	movq %r10, %r11
11:	testq %r11, %r11
	jz 12f
	movq %r11, %rax
	imulq $RATE_SIZE, %rax, %rax
	leaq (%rbp,%rax), %rsi
	leaq -RATE_SIZE(%rsi), %rdi
	call rate_name_cmp		# eax < 0 when rsi's name sorts before rdi's
	testl %eax, %eax
	jns 12f
	# swap rsi and rdi entries (RATE_SIZE bytes) through the stack
	subq $RATE_SIZE, %rsp
	pushq %rsi
	pushq %rdi
	movq %rsi, %rdx
	leaq 16(%rsp), %rdi
	movl $RATE_SIZE, %ecx
	movq %rdx, %rsi
	rep movsb
	movq (%rsp), %rsi
	movq 8(%rsp), %rdi
	movl $RATE_SIZE, %ecx
	rep movsb
	movq (%rsp), %rdi
	leaq 16(%rsp), %rsi
	movl $RATE_SIZE, %ecx
	rep movsb
	addq $16 + RATE_SIZE, %rsp
	decq %r11
	jmp 11b
12:	incq %r10
	jmp 10b
19:	movq %r12, %rax
	popq %rbp
	popq %r15
	popq %r13
	popq %r12
	popq %rbx
	ret

# rate_name_cmp(rsi, rdi = rate entries) -> eax: bytewise, shorter first.
rate_name_cmp:
	movl RATE_NAME_LEN(%rsi), %ecx
	movl RATE_NAME_LEN(%rdi), %edx
	movl %ecx, %r8d
	cmpl %edx, %r8d
	jbe 1f
	movl %edx, %r8d
1:	leaq RATE_NAME(%rsi), %r9
	leaq RATE_NAME(%rdi), %r10
2:	testl %r8d, %r8d
	jz 3f
	movzbl (%r9), %eax
	movzbl (%r10), %r11d
	subl %r11d, %eax
	jnz 4f
	incq %r9
	incq %r10
	decl %r8d
	jmp 2b
3:	movl %ecx, %eax
	subl %edx, %eax
4:	ret
