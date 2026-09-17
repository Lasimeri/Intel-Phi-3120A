# model.rs: from samples to the screen's numbers

`Model::update` takes each `Snapshot` (a `Stat` from the card, a
`Traffic` from the daemon, and the host time they arrived) and, once it
holds two, derives:

- **Per-CPU load**: busy ticks over busy plus idle ticks between the two
  samples, 0 to 1. Ticks are USER_HZ (100 per second); the host's clock
  is not needed for this ratio.
- **Cores**: CPUs grouped by the `core_id` the agent read from sysfs,
  cores sorted by id, threads in CPU order. On the 3120A CPU 0 sits on
  core 56 with CPUs 225 to 227 (the boot CPU is the last core), every
  other core holds four consecutive CPUs.
- **Rates**: counter differences over the host interval, for the four
  PCIe counters, the block devices and the interfaces.
- **Process shares**: ticks since the process was last reported, over the
  time since then, as a percentage of one hardware thread. The agent
  leaves idle kernel threads out of a sample, so a kernel thread that
  wakes after a quiet spell is compared against its last report, however
  old (up to two minutes, then it is forgotten). A process seen for the
  first time shows zero until its next report.

The test drives three CPUs on two cores through two samples two seconds
apart and checks the load, the core grouping, a DMA rate and a process
share.
