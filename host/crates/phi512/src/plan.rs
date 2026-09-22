//! Planning a region's run on the card: which loop to split, what the
//! integer registers hold, and exactly which memory a phase touches.
//!
//! A phase is a stretch of the region run with one register file: the
//! prologue from the fault to a loop's head, the loop, the epilogue from
//! the loop's exit to the region's exits, or the whole region when there
//! is no loop to split. `Tracker` walks a phase's instructions in address
//! order carrying a value for each integer register: the frame's value,
//! an immediate, a copy, an address from `lea`, a stack slot read from
//! the program itself, or a range for an induction register (the loop's
//! own, or that of a loop nested in the phase, whose trip count follows
//! from its start value, step and exit condition). Every memory operand
//! whose address resolves becomes a range; one that does not makes the
//! phase unplanned, and the card runs it in demand mode instead.
//!
//! `find_loop` picks the loop to split: a single back edge, one exit (the
//! back edge's fall-through), an induction register stepped by a constant
//! and compared against a register or immediate, and `splittable` adds:
//! no register read before it is written in the body except the
//! induction (a carried accumulator makes the loop a reduction, which is
//! run whole so that the result stays bit-identical), every store
//! addressed through the induction, and no store range overlapping a
//! load range of a different form. See plan.md.

use std::collections::BTreeMap;

use iced_x86::{ConditionCode, FlowControl, Instruction, InstructionInfoFactory, Mnemonic, OpAccess, OpKind, Register};

pub type Insns = BTreeMap<u64, Instruction>;

/// What an integer register holds at a point of a phase.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Val {
    Known(u64),
    /// Between `lo` and `hi` inclusive, stepping by `step`: an induction
    /// register over its loop, or an address derived from one.
    Range {
        lo: u64,
        hi: u64,
        step: u64,
    },
    Unknown,
}

impl Val {
    fn add(self, d: i64) -> Val {
        match self {
            Val::Known(v) => Val::Known(v.wrapping_add(d as u64)),
            Val::Range { lo, hi, step } => Val::Range {
                lo: lo.wrapping_add(d as u64),
                hi: hi.wrapping_add(d as u64),
                step,
            },
            Val::Unknown => Val::Unknown,
        }
    }
    fn plus(self, o: Val) -> Val {
        match (self, o) {
            (Val::Known(a), Val::Known(b)) => Val::Known(a.wrapping_add(b)),
            (Val::Known(a), Val::Range { lo, hi, step }) | (Val::Range { lo, hi, step }, Val::Known(a)) => Val::Range {
                lo: lo.wrapping_add(a),
                hi: hi.wrapping_add(a),
                step,
            },
            _ => Val::Unknown,
        }
    }
    fn times(self, s: u32) -> Val {
        match self {
            Val::Known(v) => Val::Known(v.wrapping_mul(u64::from(s))),
            Val::Range { lo, hi, step } => Val::Range {
                lo: lo.wrapping_mul(u64::from(s)),
                hi: hi.wrapping_mul(u64::from(s)),
                step: step.wrapping_mul(u64::from(s)),
            },
            Val::Unknown => Val::Unknown,
        }
    }
    #[allow(dead_code)]
    fn is_known(self) -> bool {
        matches!(self, Val::Known(_))
    }
}

/// x86 encoding index of a general register, any width.
pub fn gpr_index(r: Register) -> Option<usize> {
    use Register::*;
    Some(match r.full_register() {
        RAX => 0,
        RCX => 1,
        RDX => 2,
        RBX => 3,
        RSP => 4,
        RBP => 5,
        RSI => 6,
        RDI => 7,
        R8 => 8,
        R9 => 9,
        R10 => 10,
        R11 => 11,
        R12 => 12,
        R13 => 13,
        R14 => 14,
        R15 => 15,
        _ => return Option::None,
    })
}

/// The loop bound the induction register is compared against.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bound {
    Reg(usize),
    Imm(u64),
}

/// A loop the region can be split on.
#[derive(Clone, Copy, Debug)]
pub struct Loop {
    pub head: u64,
    /// The back edge's address and the address after it: the exit.
    pub back: u64,
    pub exit: u64,
    pub ind: usize,
    pub step: i64,
    pub bound: Bound,
    pub cc: ConditionCode,
    /// The flags come from a `cmp ind, bound` (true) or from the `add`
    /// or `sub` that steps the register (false; the bound is then its
    /// immediate, compared against the value before the step).
    pub via_cmp: bool,
}

fn imm_of(insn: &Instruction, op: u32) -> Option<i64> {
    match insn.op_kind(op) {
        OpKind::Immediate8
        | OpKind::Immediate16
        | OpKind::Immediate32
        | OpKind::Immediate64
        | OpKind::Immediate8to16
        | OpKind::Immediate8to32
        | OpKind::Immediate8to64
        | OpKind::Immediate32to64 => Some(insn.immediate(op) as i64),
        _ => None,
    }
}

/// Registers an instruction writes, as x86 indices (full registers).
fn written_gprs(fac: &mut InstructionInfoFactory, insn: &Instruction) -> Vec<usize> {
    let info = fac.info(insn);
    let mut v = Vec::new();
    for r in info.used_registers() {
        if matches!(
            r.access(),
            OpAccess::Write | OpAccess::ReadWrite | OpAccess::CondWrite | OpAccess::ReadCondWrite
        ) {
            if let Some(i) = gpr_index(r.register()) {
                if !v.contains(&i) {
                    v.push(i);
                }
            }
        }
    }
    v
}

/// The induction step of a loop body for register `ind`: exactly one
/// `add`/`sub`/`inc`/`dec` by an immediate, and nothing else writing it.
fn induction_step(fac: &mut InstructionInfoFactory, body: &[&Instruction], ind: usize) -> Option<(i64, u64)> {
    let mut step: Option<(i64, u64)> = None;
    for insn in body {
        let writes = written_gprs(fac, insn);
        if !writes.contains(&ind) {
            continue;
        }
        let this = match insn.mnemonic() {
            Mnemonic::Add if gpr_index(insn.op0_register()) == Some(ind) => imm_of(insn, 1),
            Mnemonic::Sub if gpr_index(insn.op0_register()) == Some(ind) => imm_of(insn, 1).map(|i| -i),
            Mnemonic::Inc if gpr_index(insn.op0_register()) == Some(ind) => Some(1),
            Mnemonic::Dec if gpr_index(insn.op0_register()) == Some(ind) => Some(-1),
            _ => None,
        };
        match (this, step) {
            (Some(s), None) => step = Some((s, insn.ip())),
            _ => return None,
        }
    }
    step
}

/// The loop to split: the largest valid loop that contains `entry` or
/// starts after it, among the back edges in `insns`.
pub fn find_loop(insns: &Insns, entry: u64, exits: &[u64]) -> Option<Loop> {
    let mut fac = InstructionInfoFactory::new();
    let mut best: Option<Loop> = None;
    for (&addr, insn) in insns {
        if insn.flow_control() != FlowControl::ConditionalBranch {
            continue;
        }
        let head = insn.near_branch_target();
        if head > addr || !insns.contains_key(&head) {
            continue;
        }
        let end = insn.next_ip();
        if !(head <= entry && entry < end) && head < entry {
            continue;
        }
        if exits.iter().any(|&e| e >= head && e < end) {
            continue;
        }
        let body: Vec<&Instruction> = insns.range(head..end).map(|(_, i)| i).collect();
        // Every branch in the body stays inside it; the loop's only way out is this fall-through.
        let mut ok = true;
        for i in &body {
            match i.flow_control() {
                FlowControl::ConditionalBranch | FlowControl::UnconditionalBranch => {
                    let t = i.near_branch_target();
                    if t < head || t >= end {
                        ok = false;
                    }
                }
                FlowControl::Next => {}
                _ => ok = false,
            }
        }
        if !ok || body.len() < 2 {
            continue;
        }
        let prev = body[body.len() - 2];
        let cc = insn.condition_code();
        let (ind, bound, via_cmp) = match prev.mnemonic() {
            Mnemonic::Cmp => {
                let Some(i) = gpr_index(prev.op0_register()) else { continue };
                let b = if let Some(b) = imm_of(prev, 1) {
                    Bound::Imm(b as u64)
                } else if let Some(r) = gpr_index(prev.op1_register()) {
                    Bound::Reg(r)
                } else {
                    continue;
                };
                (i, b, true)
            }
            Mnemonic::Add | Mnemonic::Sub => {
                let Some(i) = gpr_index(prev.op0_register()) else { continue };
                let Some(imm) = imm_of(prev, 1) else { continue };
                (i, Bound::Imm(imm as u64), false)
            }
            _ => continue,
        };
        let Some((step, _)) = induction_step(&mut fac, &body, ind) else {
            continue;
        };
        if let Bound::Reg(r) = bound {
            if body.iter().any(|i| written_gprs(&mut fac, i).contains(&r)) {
                continue;
            }
        }
        let lp = Loop {
            head,
            back: addr,
            exit: end,
            ind,
            step,
            bound,
            cc,
            via_cmp,
        };
        if best.is_none_or(|b| (b.exit - b.head) < (end - head)) {
            best = Some(lp);
        }
    }
    best
}

/// Does the loop go on, given the compared values and the condition?
fn continues(a: u64, b: u64, cc: ConditionCode) -> Option<bool> {
    Some(match cc {
        ConditionCode::l => (a as i64) < (b as i64),
        ConditionCode::le => (a as i64) <= (b as i64),
        ConditionCode::g => (a as i64) > (b as i64),
        ConditionCode::ge => (a as i64) >= (b as i64),
        ConditionCode::b => a < b,
        ConditionCode::be => a <= b,
        ConditionCode::a => a > b,
        ConditionCode::ae => a >= b,
        ConditionCode::ne => a != b,
        ConditionCode::e => a == b,
        _ => return None,
    })
}

/// How many times the body runs from induction value `i0` (a do-while:
/// the body runs, the register steps, the condition decides).
pub fn iterations(lp: &Loop, i0: u64, bound: u64) -> Option<u64> {
    let mut i = i0;
    let mut n: u64 = 1;
    loop {
        let next = i.wrapping_add(lp.step as u64);
        let go = if lp.via_cmp {
            continues(next, bound, lp.cc)?
        } else if lp.step < 0 {
            // flags from `sub ind, imm`: the comparison is of the value before the step
            continues(i, bound, lp.cc)?
        } else {
            // flags from `add ind, imm`: the result against zero
            continues(next, 0, lp.cc)?
        };
        if !go {
            return Some(n);
        }
        i = next;
        n += 1;
        if n > 1 << 27 {
            return None;
        }
    }
}

/// A range of memory a phase touches, with the form of the address that
/// produced it (for telling one array from another when checking that a
/// split loop's stores stay clear of its loads).
#[derive(Clone, Copy, Debug)]
pub struct MemRange {
    pub addr: u64,
    pub len: u64,
    pub write: bool,
    pub form: (Register, Register, u32, u64),
    /// The register whose value was a range when this address was taken,
    /// when exactly one was: the induction variable the access walks
    /// with. `None` for a fixed address, and for an address two ranges
    /// contributed to (ambiguous, so treated as neither).
    pub from_ind: Option<usize>,
    /// The access steps through the range by at most its own size: every
    /// byte of the range is touched.
    pub dense: bool,
}

/// Values through a phase.
pub struct Tracker {
    pub gpr: [Val; 16],
    pub ranges: Vec<MemRange>,
    /// Every address resolved; the phase can run in ranges mode.
    pub resolved: bool,
    /// Ranges stored to so far, so a later load of a stack slot is not
    /// read from the program's memory as it was before the phase.
    stored: Vec<(u64, u64)>,
    fac: InstructionInfoFactory,
    /// Reads program memory (a stack slot the code reloads a pointer from).
    read_mem: fn(u64, usize) -> Option<u64>,
}

impl Tracker {
    pub fn new(gpr: [u64; 16], read_mem: fn(u64, usize) -> Option<u64>) -> Tracker {
        Tracker {
            gpr: gpr.map(Val::Known),
            ranges: Vec::new(),
            resolved: true,
            stored: Vec::new(),
            fac: InstructionInfoFactory::new(),
            read_mem,
        }
    }

    fn val(&self, r: Register) -> Val {
        if r == Register::None {
            return Val::Known(0);
        }
        match gpr_index(r) {
            Some(i) => match (self.gpr[i], r.size()) {
                (Val::Known(v), 4) => Val::Known(v & 0xffff_ffff),
                (v, 8) => v,
                (Val::Known(v), 2) => Val::Known(v & 0xffff),
                (Val::Known(v), 1) => Val::Known(v & 0xff),
                _ => Val::Unknown,
            },
            None => Val::Unknown,
        }
    }

    fn set(&mut self, r: Register, v: Val) {
        if let Some(i) = gpr_index(r) {
            self.gpr[i] = match (r.size(), v) {
                (8, v) => v,
                (4, Val::Known(x)) => Val::Known(x & 0xffff_ffff),
                (4, Val::Range { lo, hi, step }) if hi <= 0xffff_ffff => Val::Range { lo, hi, step },
                _ => Val::Unknown,
            };
        }
    }

    /// The address of a memory operand: base + index * scale + disp.
    fn address(&self, insn: &Instruction, base: Register, index: Register, scale: u32, disp: u64) -> Val {
        if base == Register::RIP {
            return Val::Known(insn.memory_displacement64());
        }
        if insn.segment_prefix() != Register::None
            && !matches!(insn.segment_prefix(), Register::CS | Register::DS | Register::ES | Register::SS)
        {
            return Val::Unknown;
        }
        self.val(base).plus(self.val(index).times(scale)).add(disp as i64)
    }

    /// Which register made this address a range: exactly one of the base
    /// and the index holding a `Val::Range`, else none.
    fn range_source(&self, base: Register, index: Register) -> Option<usize> {
        let mut found = None;
        for r in [base, index] {
            if let Some(i) = gpr_index(r) {
                if matches!(self.gpr[i], Val::Range { .. }) {
                    if found.is_some() {
                        return None; /* two ranges in one address: ambiguous */
                    }
                    found = Some(i);
                }
            }
        }
        found
    }

    fn note(&mut self, addr: Val, size: u64, write: bool, form: (Register, Register, u32, u64), from_ind: Option<usize>) {
        match addr {
            Val::Known(a) => self.ranges.push(MemRange {
                addr: a,
                len: size,
                write,
                form,
                from_ind: None,
                dense: true,
            }),
            Val::Range { lo, hi, step } => self.ranges.push(MemRange {
                addr: lo,
                len: hi.wrapping_sub(lo).wrapping_add(size),
                write,
                form,
                from_ind,
                dense: step <= size,
            }),
            Val::Unknown => self.resolved = false,
        }
        if write {
            if let Val::Known(a) = addr {
                self.stored.push((a, size));
            } else if let Val::Range { lo, hi, .. } = addr {
                self.stored.push((lo, hi.wrapping_sub(lo).wrapping_add(size)));
            }
        }
    }

    /// Walk the instructions of `[from, to)` in address order. `outer`
    /// gives the phase's own induction register and its value bounds;
    /// loops nested in the phase are found and resolved here.
    pub fn run(&mut self, insns: &Insns, from: u64, to: u64, outer: Option<(usize, u64, u64, u64)>) {
        if let Some((ind, lo, hi, step)) = outer {
            self.gpr[ind] = Val::Range { lo, hi, step };
        }
        // Loops nested in the phase, by their back edge; and intervals a
        // forward branch can skip, whose writes cannot be trusted after.
        let mut inner: Vec<Loop> = Vec::new();
        let mut skips: Vec<(u64, u64)> = Vec::new();
        for (&addr, insn) in insns.range(from..to) {
            if matches!(
                insn.flow_control(),
                FlowControl::ConditionalBranch | FlowControl::UnconditionalBranch
            ) {
                let t = insn.near_branch_target();
                if t <= addr {
                    if let Some(lp) = find_loop(&insns.range(from..to).map(|(a, i)| (*a, *i)).collect(), t, &[]) {
                        if lp.head == t && lp.back == addr {
                            inner.push(lp);
                        }
                    }
                } else if t < to {
                    skips.push((addr, t));
                }
            }
        }
        // A loop nested in this one, or that this phase wraps: the phase's
        // own back edge is not "inner".
        inner.retain(|lp| !(outer.is_some() && lp.head == from));
        let mut leave_at: Vec<(u64, usize, Val)> = Vec::new(); // (exit address, register, value after the loop)
        for (&addr, insn) in insns.range(from..to) {
            for &(exit, reg, v) in &leave_at {
                if addr == exit {
                    self.gpr[reg] = v;
                }
            }
            if let Some(lp) = inner.iter().find(|lp| lp.head == addr) {
                // Entering a nested loop: its induction becomes a range, from
                // its value here over its trip count; everything else the
                // body writes is unknown from here on.
                let Val::Known(i0) = self.gpr[lp.ind] else {
                    self.resolved = false;
                    continue;
                };
                let bound = match lp.bound {
                    Bound::Imm(b) => Some(b),
                    Bound::Reg(r) => match self.gpr[r] {
                        Val::Known(b) => Some(b),
                        _ => None,
                    },
                };
                let n = bound.and_then(|b| iterations(lp, i0, b));
                let Some(n) = n else {
                    self.resolved = false;
                    continue;
                };
                let last = i0.wrapping_add((lp.step as u64).wrapping_mul(n - 1));
                let (lo, hi) = if lp.step > 0 { (i0, last) } else { (last, i0) };
                self.gpr[lp.ind] = Val::Range {
                    lo,
                    hi,
                    step: lp.step.unsigned_abs(),
                };
                leave_at.push((lp.exit, lp.ind, Val::Known(i0.wrapping_add((lp.step as u64).wrapping_mul(n)))));
                let body: Vec<&Instruction> = insns.range(lp.head..lp.exit).map(|(_, i)| i).collect();
                for b in &body {
                    for w in written_gprs(&mut self.fac, b) {
                        if w != lp.ind {
                            self.gpr[w] = Val::Unknown;
                        }
                    }
                }
            }
            self.step(insn, &skips, &inner);
        }
    }

    fn step(&mut self, insn: &Instruction, skips: &[(u64, u64)], inner: &[Loop]) {
        let addr = insn.ip();
        // Memory operands first: their addresses use the values before the instruction.
        let info = self.fac.info(insn);
        let mems: Vec<(Register, Register, u32, u64, bool, u64)> = info
            .used_memory()
            .iter()
            .map(|m| {
                let write = matches!(
                    m.access(),
                    OpAccess::Write | OpAccess::ReadWrite | OpAccess::CondWrite | OpAccess::ReadCondWrite
                );
                (
                    m.base(),
                    m.index(),
                    m.scale(),
                    m.displacement(),
                    write,
                    m.memory_size().size() as u64,
                )
            })
            .collect();
        let mut loaded: Option<u64> = None;
        for (base, index, scale, disp, write, size) in mems {
            let size = if size == 0 { 64 } else { size };
            let a = self.address(insn, base, index, scale, disp);
            if !write && insn.mnemonic() == Mnemonic::Mov && insn.op0_kind() == OpKind::Register && size <= 8 {
                if let Val::Known(x) = a {
                    let clobbered = self.stored.iter().any(|&(s, l)| x < s + l && s < x + size);
                    if !clobbered {
                        loaded = (self.read_mem)(x, size as usize);
                    }
                }
            }
            let src = self.range_source(base, index);
            self.note(a, size, write, (base, index, scale, disp), src);
        }
        // Inside a nested loop's body, or an interval a branch can skip,
        // writes are not trusted; the loop's induction is already a range.
        let in_inner = inner.iter().find(|lp| addr >= lp.head && addr < lp.exit);
        let skipped = skips.iter().any(|&(j, t)| addr > j && addr < t);
        let writes = written_gprs(&mut self.fac, insn);
        if let Some(lp) = in_inner {
            for w in writes {
                if w != lp.ind {
                    self.gpr[w] = Val::Unknown;
                }
            }
            return;
        }
        if skipped {
            for w in writes {
                self.gpr[w] = Val::Unknown;
            }
            return;
        }
        let dst = insn.op0_register();
        let new = match insn.mnemonic() {
            Mnemonic::Mov => match insn.op1_kind() {
                OpKind::Register => Some(self.val(insn.op1_register())),
                OpKind::Memory => Some(loaded.map_or(Val::Unknown, Val::Known)),
                _ => imm_of(insn, 1).map(|i| Val::Known(i as u64)),
            },
            Mnemonic::Lea => Some(self.address(
                insn,
                insn.memory_base(),
                insn.memory_index(),
                insn.memory_index_scale(),
                insn.memory_displacement64(),
            )),
            Mnemonic::Add | Mnemonic::Sub => {
                let sign = if insn.mnemonic() == Mnemonic::Add { 1 } else { -1 };
                Some(match insn.op1_kind() {
                    OpKind::Register => {
                        let o = self.val(insn.op1_register());
                        if sign > 0 {
                            self.val(dst).plus(o)
                        } else if let Val::Known(x) = o {
                            self.val(dst).add(-(x as i64))
                        } else {
                            Val::Unknown
                        }
                    }
                    _ => imm_of(insn, 1).map_or(Val::Unknown, |i| self.val(dst).add(sign * i)),
                })
            }
            Mnemonic::Inc => Some(self.val(dst).add(1)),
            Mnemonic::Dec => Some(self.val(dst).add(-1)),
            Mnemonic::Xor if insn.op1_kind() == OpKind::Register && insn.op1_register() == dst => Some(Val::Known(0)),
            Mnemonic::And => match (self.val(dst), imm_of(insn, 1)) {
                (Val::Known(x), Some(i)) => Some(Val::Known(x & i as u64)),
                _ => Some(Val::Unknown),
            },
            Mnemonic::Shl => match (self.val(dst), imm_of(insn, 1)) {
                (Val::Known(x), Some(i)) => Some(Val::Known(x << (i as u32 & 63))),
                _ => Some(Val::Unknown),
            },
            Mnemonic::Shr => match (self.val(dst), imm_of(insn, 1)) {
                (Val::Known(x), Some(i)) => Some(Val::Known(x >> (i as u32 & 63))),
                _ => Some(Val::Unknown),
            },
            Mnemonic::Cmp | Mnemonic::Test => None,
            Mnemonic::Push => {
                self.gpr[4] = self.gpr[4].add(-8);
                None
            }
            Mnemonic::Pop => {
                self.gpr[4] = self.gpr[4].add(8);
                if let Some(i) = gpr_index(dst) {
                    self.gpr[i] = Val::Unknown;
                }
                None
            }
            _ => Some(Val::Unknown),
        };
        if let Some(v) = new {
            if insn.op0_kind() == OpKind::Register {
                self.set(dst, v);
            }
            // Anything else the instruction writes (an implicit rdx, say).
            for w in writes {
                if Some(w) != gpr_index(dst) {
                    self.gpr[w] = Val::Unknown;
                }
            }
        }
    }

    /// The ranges, merged and sorted, as (addr, len, written, read, dense):
    /// two that touch or share a page become one; a merged range is dense
    /// only if every part was.
    pub fn merged(&self) -> Vec<(u64, u64, bool, bool, bool)> {
        let mut v: Vec<(u64, u64, bool, bool, bool)> = self.ranges.iter().map(|r| (r.addr, r.len, r.write, !r.write, r.dense)).collect();
        v.sort();
        let mut out: Vec<(u64, u64, bool, bool, bool)> = Vec::new();
        for (a, l, w, rd, d) in v {
            if let Some(last) = out.last_mut() {
                if a <= last.0 + last.1 + 4095 {
                    let end = (last.0 + last.1).max(a + l);
                    last.1 = end - last.0;
                    last.2 |= w;
                    last.3 |= rd;
                    last.4 &= d;
                    continue;
                }
            }
            out.push((a, l, w, rd, d));
        }
        out
    }
}

/// Can the loop's iterations run on several threads at once? No register
/// (integer, vector or mask) read before written in the body except the
/// induction; every store indexed by the induction; no store range
/// overlapping a load range of a different form.
pub fn splittable(insns: &Insns, lp: &Loop, tracker: &Tracker) -> Result<(), &'static str> {
    let mut fac = InstructionInfoFactory::new();
    // Each instruction's registers as (register, reads, writes), body order.
    let uses: Vec<Vec<(Register, bool, bool)>> = insns
        .range(lp.head..lp.exit)
        .map(|(_, insn)| {
            fac.info(insn)
                .used_registers()
                .iter()
                .map(|r| {
                    let reads = matches!(
                        r.access(),
                        OpAccess::Read | OpAccess::CondRead | OpAccess::ReadWrite | OpAccess::ReadCondWrite
                    );
                    let writes = matches!(
                        r.access(),
                        OpAccess::Write | OpAccess::ReadWrite | OpAccess::CondWrite | OpAccess::ReadCondWrite
                    );
                    (r.register().full_register(), reads, writes)
                })
                .collect()
        })
        .collect();
    let body_writes = |reg: Register| uses.iter().flatten().any(|&(r, _, w)| r == reg && w);
    let mut written: Vec<Register> = Vec::new();
    for insn_uses in &uses {
        for &(reg, reads, writes) in insn_uses {
            if reg == Register::RIP || reg == Register::RSP || reg == Register::RBP {
                continue;
            }
            let is_ind = gpr_index(reg) == Some(lp.ind);
            // Read before written in this body, and written somewhere in it:
            // carried across iterations.
            if reads && !is_ind && !written.contains(&reg) && body_writes(reg) {
                return Err("a register is carried across iterations (a reduction, or a running pointer)");
            }
            if writes && !written.contains(&reg) {
                written.push(reg);
            }
        }
    }
    for r in &tracker.ranges {
        // A store must walk with THIS loop's induction register. One that
        // walks with a nested loop's counter, or sits at a fixed address,
        // is written by every thread at the same place.
        if r.write && r.from_ind != Some(lp.ind) {
            return Err("a store not indexed by this loop's induction register (every thread would write the same address)");
        }
    }
    for w in tracker.ranges.iter().filter(|r| r.write) {
        for l in tracker.ranges.iter().filter(|r| !r.write) {
            let overlap = w.addr < l.addr + l.len && l.addr < w.addr + w.len;
            if overlap && w.form != l.form {
                return Err("a store range overlaps a load range of another form");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced_x86::{Decoder, DecoderOptions};

    fn decode(bytes: &[u8], ip: u64) -> Insns {
        let mut d = Decoder::with_ip(64, bytes, ip, DecoderOptions::NONE);
        let mut m = Insns::new();
        while d.can_decode() {
            let i = d.decode();
            m.insert(i.ip(), i);
        }
        m
    }

    fn no_mem(_: u64, _: usize) -> Option<u64> {
        None
    }

    /// gcc's integer loop from the seamless test: vmovdqa64 (%r10,%rax,4),%zmm1
    /// ... vmovdqa32 %zmm0,(%r11,%rax,4); add $0x10,%rax; cmp %r12,%rax; jl head
    const INTS: &[u8] = &[
        0x62, 0xd1, 0xfd, 0x48, 0x6f, 0x0c, 0x82, // vmovdqa64 (%r10,%rax,4),%zmm1
        0x62, 0xf1, 0x6d, 0x48, 0xdb, 0xc1, // vpandd %zmm1,%zmm2,%zmm0
        0x62, 0xf3, 0x7d, 0x48, 0x1f, 0xca, 0x00, // vpcmpeqd %zmm2,%zmm0,%k1
        0x62, 0xf1, 0x5d, 0x48, 0xfe, 0xc1, // vpaddd %zmm1,%zmm4,%zmm0
        0x62, 0xf2, 0x75, 0x49, 0x40, 0xc3, // vpmulld %zmm3,%zmm1,%zmm0{%k1}
        0x62, 0xd1, 0x7d, 0x48, 0x7f, 0x04, 0x83, // vmovdqa32 %zmm0,(%r11,%rax,4)
        0x48, 0x83, 0xc0, 0x10, // add $0x10,%rax
        0x4c, 0x39, 0xe0, // cmp %r12,%rax
        0x7c, 0xd0, // jl head
        0x90,
    ];

    #[test]
    fn the_integer_loop_is_found_and_splittable() {
        let insns = decode(INTS, 0x1000);
        let lp = find_loop(&insns, 0x1000, &[]).expect("a loop");
        assert_eq!(lp.head, 0x1000);
        assert_eq!(lp.ind, 0, "rax");
        assert_eq!(lp.step, 16);
        assert_eq!(lp.bound, Bound::Reg(12), "r12");
        assert_eq!(iterations(&lp, 0, 65536), Some(4096));
        let mut gpr = [0u64; 16];
        gpr[10] = 0x10000; // r10 = in
        gpr[11] = 0x80000; // r11 = out
        gpr[12] = 65536; // r12 = n
        let mut t = Tracker::new(gpr, no_mem);
        t.run(&insns, lp.head, lp.exit, Some((0, 0, 65536 - 16, 16)));
        assert!(t.resolved);
        let m = t.merged();
        assert_eq!(
            m,
            vec![(0x10000, 65536 * 4, false, true, true), (0x80000, 65536 * 4, true, false, true)],
            "the output is dense and write-only"
        );
        assert!(splittable(&insns, &lp, &t).is_ok());
    }

    #[test]
    fn a_reduction_is_not_split() {
        // vmovaps (%r15,%rax,4),%zmm5; vfmadd231ps (%r13,%rax,4),%zmm5,%zmm0; add $0x10,%rax; cmp %r12,%rax; jl
        let bytes = [
            0x62, 0xd1, 0x7c, 0x48, 0x28, 0x2c, 0x87, 0x62, 0xd2, 0x55, 0x48, 0xb8, 0x44, 0x85, 0x00, 0x48, 0x83, 0xc0, 0x10, 0x4c, 0x39,
            0xe0, 0x7c, 0xe8, 0x90,
        ];
        let insns = decode(&bytes, 0x2000);
        let lp = find_loop(&insns, 0x2000, &[]).expect("a loop");
        let mut gpr = [0u64; 16];
        gpr[15] = 0x10000;
        gpr[13] = 0x20000;
        gpr[12] = 1024;
        let mut t = Tracker::new(gpr, no_mem);
        t.run(&insns, lp.head, lp.exit, Some((0, 0, 1024 - 16, 16)));
        assert!(t.resolved);
        assert!(splittable(&insns, &lp, &t).is_err(), "zmm0 accumulates");
    }

    #[test]
    fn a_count_down_inner_loop_resolves_its_range() {
        // mov $0x1d,%eax; L: vfmadd213ps (%r9,%rax,4){1to16},%zmm1,%zmm0; sub $1,%rax; jae L; nop
        let bytes = [
            0xb8, 0x1d, 0x00, 0x00, 0x00, 0x62, 0xd2, 0x75, 0x58, 0xa8, 0x04, 0x81, 0x48, 0x83, 0xe8, 0x01, 0x73, 0xf3, 0x90,
        ];
        let insns = decode(&bytes, 0x3000);
        let mut gpr = [0u64; 16];
        gpr[9] = 0x50000;
        let mut t = Tracker::new(gpr, no_mem);
        t.run(&insns, 0x3000, 0x3013, None);
        assert!(t.resolved, "{:?}", t.ranges);
        assert_eq!(t.merged(), vec![(0x50000, 30 * 4, false, true, true)]);
        assert_eq!(t.gpr[0], Val::Known(u64::MAX), "rax after the loop is -1");
    }
}
