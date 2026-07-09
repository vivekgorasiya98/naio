//! Turbo tier: compiles pure integer bytecode loops into register micro-ops.
//!
//! When the VM executes a backward `Jump` (a loop backedge), it checks whether
//! the loop body was pre-compiled into a `LoopRegion`. If so — and all locals
//! the loop touches are currently ints — the remaining iterations run in a
//! tight native loop over an `i64` register file: no operand stack, no enum
//! tagging, and division/modulo by constants strength-reduced via magic
//! multiplication (the same trick JIT compilers use).
//!
//! Semantics stay exact: every arithmetic op is overflow-checked. On overflow
//! (BigInt promotion needed), division by zero, or any other case the turbo
//! tier can't handle, it restores the register snapshot taken at the last
//! loop-head crossing and resumes the generic interpreter from that point.
//! Replay is safe because regions never contain side effects (no calls, no
//! array writes, no prints).

use crate::fast_val::FastVal;
use niao_bytecode::OpCode;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

/// Strength-reduced signed division by a compile-time constant.
/// Computed with the algorithm from Hacker's Delight (2nd ed., 10-4).
#[derive(Clone, Copy, Debug)]
struct MagicDiv {
    m: i64,
    s: u32,
    add_n: bool,
    sub_n: bool,
    d: i64,
}

impl MagicDiv {
    /// Requires |d| >= 2.
    fn new(d: i64) -> MagicDiv {
        debug_assert!(d.unsigned_abs() >= 2);
        let ad = d.unsigned_abs();
        let two63 = 1u64 << 63;
        let t = two63 + ((d as u64) >> 63);
        let anc = t - 1 - t % ad;
        let mut p = 63u32;
        let mut q1 = two63 / anc;
        let mut r1 = two63 - q1 * anc;
        let mut q2 = two63 / ad;
        let mut r2 = two63 - q2 * ad;
        loop {
            p += 1;
            q1 *= 2;
            r1 *= 2;
            if r1 >= anc {
                q1 += 1;
                r1 -= anc;
            }
            q2 *= 2;
            r2 *= 2;
            if r2 >= ad {
                q2 += 1;
                r2 -= ad;
            }
            let delta = ad - r2;
            if !(q1 < delta || (q1 == delta && r1 == 0)) {
                break;
            }
        }
        let mut m = (q2 + 1) as i64;
        if d < 0 {
            m = -m;
        }
        MagicDiv {
            m,
            s: p - 64,
            add_n: d > 0 && m < 0,
            sub_n: d < 0 && m > 0,
            d,
        }
    }

    /// Truncating division `n / d` without a hardware divide.
    #[inline(always)]
    fn div(&self, n: i64) -> i64 {
        let mut q = (((n as i128) * (self.m as i128)) >> 64) as i64;
        if self.add_n {
            q = q.wrapping_add(n);
        }
        if self.sub_n {
            q = q.wrapping_sub(n);
        }
        q >>= self.s;
        q + ((q as u64) >> 63) as i64
    }

    /// `n % d` derived from the quotient (exact, cannot overflow).
    #[inline(always)]
    fn rem(&self, n: i64) -> i64 {
        n.wrapping_sub(self.div(n).wrapping_mul(self.d))
    }
}

/// Register-based micro-op. Register fields index a flat `i64` array laid out
/// as `[loop locals..., constants..., expression temps...]`.
#[derive(Clone, Copy, Debug)]
enum UOp {
    Add(u16, u16, u16),
    Sub(u16, u16, u16),
    Mul(u16, u16, u16),
    Div(u16, u16, u16),
    Mod(u16, u16, u16),
    /// dst, src, magic index
    DivC(u16, u16, u16),
    ModC(u16, u16, u16),
    /// Fused `(a op b) % const` — dst, a, b, magic index.
    AddModC(u16, u16, u16, u16),
    SubModC(u16, u16, u16, u16),
    MulModC(u16, u16, u16, u16),
    Neg(u16, u16),
    Mov(u16, u16),
    Jump(u32),
    /// Loop backedge: snapshot locals, then jump. Fields: uop target, resume bytecode ip.
    BackJump(u32, u32),
    /// Fused backedge + loop condition: snapshot, compare, then branch to
    /// `taken` (loop exit) or `fall` (body start).
    /// Fields: kind, a, b, taken uop, fall uop, resume bytecode ip.
    BackJumpBr(CmpKind, u16, u16, u32, u32, u32),
    /// Fused comparison + JumpIfFalse. `BrGe(a, b, t)` = branch to t if a >= b
    /// (i.e. the original `Lt` comparison was false).
    BrLt(u16, u16, u32),
    BrLe(u16, u16, u32),
    BrGt(u16, u16, u32),
    BrGe(u16, u16, u32),
    BrEq(u16, u16, u32),
    BrNe(u16, u16, u32),
    /// JumpIfFalse on a plain int value (0 = falsy).
    BrZero(u16, u32),
    /// Leave the region; resume generic dispatch at this bytecode ip.
    Exit(u32),
}

/// Sentinel resume ip: the interpreted loop is hot, switch to native code.
const PROMOTE: usize = usize::MAX;
/// Backedges interpreted (in one run) before a loop is considered JIT-worthy.
const PROMOTE_AFTER: u32 = 128;
/// Region entries before a loop is JIT-compiled eagerly (short hot loops).
const ENTRY_PROMOTE: u32 = 4;

enum JitState {
    Untried,
    Failed,
    Ready(jit::JitLoop),
}

pub(crate) struct LoopRegion {
    head_bc: u32,
    /// Local slots the region reads/writes; register i holds slot slots[i].
    slots: Vec<u16>,
    /// Constant register initial values (registers slots.len()..slots.len()+consts.len()).
    consts: Vec<i64>,
    total_regs: usize,
    uops: Vec<UOp>,
    magics: Vec<MagicDiv>,
    /// Lazily-compiled native code for this region (Cranelift).
    jit: RefCell<JitState>,
    /// How many times this region has been entered (JIT heat).
    entries: Cell<u32>,
}

impl LoopRegion {
    /// Build the register file from frame locals. Returns None when any local
    /// the region touches is not currently an int (turbo doesn't apply).
    pub(crate) fn enter(&self, locals: &[FastVal]) -> Option<Vec<i64>> {
        for &slot in &self.slots {
            match locals.get(slot as usize) {
                Some(FastVal::Int(_)) => {}
                _ => return None,
            }
        }
        let mut regs = vec![0i64; self.total_regs];
        for (i, &slot) in self.slots.iter().enumerate() {
            if let Some(FastVal::Int(v)) = locals.get(slot as usize) {
                regs[i] = *v;
            }
        }
        for (i, &c) in self.consts.iter().enumerate() {
            regs[self.slots.len() + i] = c;
        }
        Some(regs)
    }

    pub(crate) fn write_back(&self, regs: &[i64], locals: &mut [FastVal]) {
        for (i, &slot) in self.slots.iter().enumerate() {
            locals[slot as usize] = FastVal::Int(regs[i]);
        }
    }

    /// Execute the region: interpret first, then promote hot loops to native
    /// code (Cranelift) once they cross a heat threshold. Returns the bytecode
    /// ip to resume generic dispatch at.
    pub(crate) fn execute(&self, regs: &mut [i64]) -> usize {
        assert_eq!(regs.len(), self.total_regs);
        let entries = self.entries.get().saturating_add(1);
        self.entries.set(entries);

        if let JitState::Ready(j) = &*self.jit.borrow() {
            // Safety: code was generated for exactly this register layout.
            return unsafe { j.call(regs.as_mut_ptr()) };
        }
        let untried = matches!(&*self.jit.borrow(), JitState::Untried);
        if untried && entries >= ENTRY_PROMOTE {
            // Short loop entered many times (e.g. inner loop): compile eagerly.
            jit::ensure_compiled(self);
            if let JitState::Ready(j) = &*self.jit.borrow() {
                return unsafe { j.call(regs.as_mut_ptr()) };
            }
        }
        let resume = self.run(regs, untried);
        if resume != PROMOTE {
            return resume;
        }
        // Hot loop: regs are at the loop head, safe to switch tiers mid-run.
        jit::ensure_compiled(self);
        if let JitState::Ready(j) = &*self.jit.borrow() {
            unsafe { j.call(regs.as_mut_ptr()) }
        } else {
            self.run(regs, false)
        }
    }

    /// Execute micro-ops until the region exits or bails. Returns the bytecode
    /// ip where generic dispatch should resume (or PROMOTE when the loop is
    /// hot and `allow_promote` is set). On bail, `regs` holds the snapshot
    /// from the last loop-head crossing so generic replay is exact.
    ///
    /// The hot loop uses unchecked indexing. Safety: `compile_region` validates
    /// that every register operand is `< total_regs`, every branch target is
    /// `< uops.len()`, and every magic index is `< magics.len()`; the assert
    /// below pins `regs.len()` to `total_regs`.
    fn run(&self, regs: &mut [i64], allow_promote: bool) -> usize {
        assert_eq!(regs.len(), self.total_regs);
        let ops = self.uops.as_ptr();
        let magics = self.magics.as_ptr();
        let rp = regs.as_mut_ptr();
        let n_slots = self.slots.len();
        let mut snap: Vec<i64> = regs[..n_slots].to_vec();
        let sp = snap.as_mut_ptr();
        let mut snap_bc = self.head_bc;
        let mut pc = 0usize;
        // Snapshot every 64 backedges: bail replay is bounded and cheap, and
        // the copy stays off the per-iteration hot path.
        let mut backedges = 0u32;

        macro_rules! r {
            ($i:expr) => {
                unsafe { *rp.add($i as usize) }
            };
        }
        macro_rules! w {
            ($i:expr, $v:expr) => {{
                // Evaluate the value first so nested r!/bail! unsafe blocks
                // stay outside this one.
                let val = $v;
                unsafe { *rp.add($i as usize) = val }
            }};
        }
        macro_rules! magic {
            ($i:expr) => {
                unsafe { &*magics.add($i as usize) }
            };
        }
        macro_rules! bail {
            () => {{
                #[allow(unused_unsafe)]
                unsafe {
                    std::ptr::copy_nonoverlapping(sp, rp, n_slots)
                };
                return snap_bc as usize;
            }};
        }
        macro_rules! checked {
            ($e:expr) => {
                match $e {
                    Some(v) => v,
                    None => bail!(),
                }
            };
        }

        loop {
            let op = unsafe { *ops.add(pc) };
            match op {
                UOp::Add(d, a, b) => {
                    w!(d, checked!(r!(a).checked_add(r!(b))));
                    pc += 1;
                }
                UOp::Sub(d, a, b) => {
                    w!(d, checked!(r!(a).checked_sub(r!(b))));
                    pc += 1;
                }
                UOp::Mul(d, a, b) => {
                    w!(d, checked!(r!(a).checked_mul(r!(b))));
                    pc += 1;
                }
                UOp::Div(d, a, b) => {
                    let (x, y) = (r!(a), r!(b));
                    if y == 0 || (x == i64::MIN && y == -1) {
                        bail!();
                    }
                    w!(d, x / y);
                    pc += 1;
                }
                UOp::Mod(d, a, b) => {
                    let (x, y) = (r!(a), r!(b));
                    if y == 0 || (x == i64::MIN && y == -1) {
                        bail!();
                    }
                    w!(d, x % y);
                    pc += 1;
                }
                UOp::DivC(d, a, m) => {
                    w!(d, magic!(m).div(r!(a)));
                    pc += 1;
                }
                UOp::ModC(d, a, m) => {
                    w!(d, magic!(m).rem(r!(a)));
                    pc += 1;
                }
                UOp::AddModC(d, a, b, m) => {
                    let t = checked!(r!(a).checked_add(r!(b)));
                    w!(d, magic!(m).rem(t));
                    pc += 1;
                }
                UOp::SubModC(d, a, b, m) => {
                    let t = checked!(r!(a).checked_sub(r!(b)));
                    w!(d, magic!(m).rem(t));
                    pc += 1;
                }
                UOp::MulModC(d, a, b, m) => {
                    let t = checked!(r!(a).checked_mul(r!(b)));
                    w!(d, magic!(m).rem(t));
                    pc += 1;
                }
                UOp::Neg(d, a) => {
                    w!(d, checked!(r!(a).checked_neg()));
                    pc += 1;
                }
                UOp::Mov(d, a) => {
                    w!(d, r!(a));
                    pc += 1;
                }
                UOp::Jump(t) => pc = t as usize,
                UOp::BackJump(t, bc) => {
                    backedges = backedges.wrapping_add(1);
                    if backedges & 63 == 0 {
                        unsafe { std::ptr::copy_nonoverlapping(rp, sp, n_slots) };
                        snap_bc = bc;
                        if allow_promote && backedges >= PROMOTE_AFTER && bc == self.head_bc {
                            return PROMOTE;
                        }
                    }
                    pc = t as usize;
                }
                UOp::BackJumpBr(kind, a, b, taken, fall, bc) => {
                    backedges = backedges.wrapping_add(1);
                    if backedges & 63 == 0 {
                        unsafe { std::ptr::copy_nonoverlapping(rp, sp, n_slots) };
                        snap_bc = bc;
                        if allow_promote && backedges >= PROMOTE_AFTER && bc == self.head_bc {
                            // Regs are at the loop head; the JIT re-evaluates
                            // the condition on entry.
                            return PROMOTE;
                        }
                    }
                    pc = if kind.eval(r!(a), r!(b)) {
                        taken as usize
                    } else {
                        fall as usize
                    };
                }
                UOp::BrLt(a, b, t) => {
                    pc = if r!(a) < r!(b) { t as usize } else { pc + 1 };
                }
                UOp::BrLe(a, b, t) => {
                    pc = if r!(a) <= r!(b) { t as usize } else { pc + 1 };
                }
                UOp::BrGt(a, b, t) => {
                    pc = if r!(a) > r!(b) { t as usize } else { pc + 1 };
                }
                UOp::BrGe(a, b, t) => {
                    pc = if r!(a) >= r!(b) { t as usize } else { pc + 1 };
                }
                UOp::BrEq(a, b, t) => {
                    pc = if r!(a) == r!(b) { t as usize } else { pc + 1 };
                }
                UOp::BrNe(a, b, t) => {
                    pc = if r!(a) != r!(b) { t as usize } else { pc + 1 };
                }
                UOp::BrZero(a, t) => {
                    pc = if r!(a) == 0 { t as usize } else { pc + 1 };
                }
                UOp::Exit(bc) => return bc as usize,
            }
        }
    }
}

/// Find every loop (backward-jump target) in a function and try to compile it.
/// Returns `(jump_ip, region)` pairs: the VM intercepts those Jump instructions.
pub(crate) fn find_regions(
    code: &[OpCode],
    constants: &[FastVal],
) -> Vec<(usize, Rc<LoopRegion>)> {
    let mut heads: HashMap<usize, usize> = HashMap::new();
    let mut back_jumps: Vec<(usize, usize)> = Vec::new();
    for (j, op) in code.iter().enumerate() {
        if let OpCode::Jump(t) = op {
            let t = *t as usize;
            if t <= j {
                let end = heads.entry(t).or_insert(j);
                *end = (*end).max(j);
                back_jumps.push((j, t));
            }
        }
    }

    let mut out = Vec::new();
    for (&head, &end) in &heads {
        if let Some(region) = compile_region(code, head, end, constants) {
            let rc = Rc::new(region);
            for &(j, t) in &back_jumps {
                if t == head {
                    out.push((j, Rc::clone(&rc)));
                }
            }
        }
    }
    out
}

#[derive(Clone, Copy, Debug)]
enum CmpKind {
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
}

impl CmpKind {
    #[inline(always)]
    fn eval(self, a: i64, b: i64) -> bool {
        match self {
            CmpKind::Lt => a < b,
            CmpKind::Le => a <= b,
            CmpKind::Gt => a > b,
            CmpKind::Ge => a >= b,
            CmpKind::Eq => a == b,
            CmpKind::Ne => a != b,
        }
    }
}

#[derive(Clone, Copy)]
enum VDesc {
    Reg(u16),
    Cmp(CmpKind, u16, u16),
}

enum BrTarget {
    UOp(u32),
    /// Forward branch inside the region, target bytecode ip not compiled yet.
    PendingBc(u32),
    /// Branch out of the region: resume generic dispatch at this bytecode ip.
    ExitBc(u32),
}

struct RegionBuilder {
    slot_map: HashMap<u16, u16>,
    slots: Vec<u16>,
    const_map: HashMap<i64, u16>,
    consts: Vec<i64>,
    magic_map: HashMap<i64, u16>,
    magics: Vec<MagicDiv>,
    uops: Vec<UOp>,
    vstack: Vec<VDesc>,
    max_depth: usize,
    /// uop index for each bytecode ip where the virtual stack was empty.
    uop_at_bc: HashMap<u32, u32>,
    /// (uop index, target) fixups resolved after emission.
    patches: Vec<(usize, BrTarget)>,
}

impl RegionBuilder {
    fn slot_reg(&mut self, slot: u16) -> u16 {
        if let Some(&r) = self.slot_map.get(&slot) {
            return r;
        }
        let r = self.slots.len() as u16;
        self.slot_map.insert(slot, r);
        self.slots.push(slot);
        r
    }

    /// Returns a provisional register (CONST_BASE + index), remapped at the end.
    fn const_reg(&mut self, value: i64) -> Option<u16> {
        if let Some(&r) = self.const_map.get(&value) {
            return Some(r);
        }
        let idx = self.consts.len();
        let r = CONST_BASE as usize + idx;
        if r >= TEMP_BASE as usize {
            return None;
        }
        self.const_map.insert(value, r as u16);
        self.consts.push(value);
        Some(r as u16)
    }

    fn magic_idx(&mut self, d: i64) -> Option<u16> {
        if let Some(&i) = self.magic_map.get(&d) {
            return Some(i);
        }
        let idx = self.magics.len();
        if idx > u16::MAX as usize {
            return None;
        }
        self.magic_map.insert(d, idx as u16);
        self.magics.push(MagicDiv::new(d));
        Some(idx as u16)
    }

    /// Register index for the value at virtual-stack position `pos`.
    /// Resolved to absolute registers after slot/const counts are final,
    /// so temps use a large provisional base fixed up at the end.
    fn temp_reg(&mut self, pos: usize) -> Option<u16> {
        self.max_depth = self.max_depth.max(pos + 1);
        let r = TEMP_BASE as usize + pos;
        if r > u16::MAX as usize {
            return None;
        }
        Some(r as u16)
    }
}

/// Provisional register namespaces used during compilation. Slot registers are
/// final from the start (0..n_slots); constants and temps are numbered in
/// these high ranges and remapped once slot/const counts are known.
const CONST_BASE: u16 = 0x4000;
const TEMP_BASE: u16 = 0x8000;

fn is_temp(r: u16) -> bool {
    r >= TEMP_BASE
}

fn is_const(r: u16) -> bool {
    (CONST_BASE..TEMP_BASE).contains(&r)
}

fn uop_dst_mut(op: &mut UOp) -> Option<&mut u16> {
    match op {
        UOp::Add(d, ..)
        | UOp::Sub(d, ..)
        | UOp::Mul(d, ..)
        | UOp::Div(d, ..)
        | UOp::Mod(d, ..)
        | UOp::DivC(d, ..)
        | UOp::ModC(d, ..)
        | UOp::AddModC(d, ..)
        | UOp::SubModC(d, ..)
        | UOp::MulModC(d, ..)
        | UOp::Neg(d, ..)
        | UOp::Mov(d, ..) => Some(d),
        _ => None,
    }
}

fn compile_region(
    code: &[OpCode],
    head: usize,
    end: usize,
    constants: &[FastVal],
) -> Option<LoopRegion> {
    let mut b = RegionBuilder {
        slot_map: HashMap::new(),
        slots: Vec::new(),
        const_map: HashMap::new(),
        consts: Vec::new(),
        magic_map: HashMap::new(),
        magics: Vec::new(),
        uops: Vec::new(),
        vstack: Vec::new(),
        max_depth: 0,
        uop_at_bc: HashMap::new(),
        patches: Vec::new(),
    };

    for bc in head..=end {
        if b.vstack.is_empty() {
            b.uop_at_bc.insert(bc as u32, b.uops.len() as u32);
        }
        let op = code[bc].clone();
        match op {
            OpCode::Const(idx) => {
                let FastVal::Int(v) = *constants.get(idx as usize)? else {
                    return None;
                };
                let r = b.const_reg(v)?;
                b.vstack.push(VDesc::Reg(r));
            }
            OpCode::Load(slot) => {
                let r = b.slot_reg(slot);
                b.vstack.push(VDesc::Reg(r));
            }
            OpCode::Store(slot) => {
                let dst = b.slot_reg(slot);
                match b.vstack.pop()? {
                    VDesc::Reg(r) => {
                        let mut fused = false;
                        if is_temp(r) {
                            if let Some(last) = b.uops.last_mut() {
                                if let Some(d) = uop_dst_mut(last) {
                                    if *d == r {
                                        *d = dst;
                                        fused = true;
                                    }
                                }
                            }
                        }
                        if !fused && r != dst {
                            b.uops.push(UOp::Mov(dst, r));
                        }
                    }
                    VDesc::Cmp(..) => return None,
                }
            }
            OpCode::Add | OpCode::Sub | OpCode::Mul | OpCode::Mod => {
                let VDesc::Reg(rb) = b.vstack.pop()? else {
                    return None;
                };
                let VDesc::Reg(ra) = b.vstack.pop()? else {
                    return None;
                };
                let dst = b.temp_reg(b.vstack.len())?;
                let const_rhs = const_value_of(&b, rb);
                match op {
                    OpCode::Add => b.uops.push(UOp::Add(dst, ra, rb)),
                    OpCode::Sub => b.uops.push(UOp::Sub(dst, ra, rb)),
                    OpCode::Mul => b.uops.push(UOp::Mul(dst, ra, rb)),
                    OpCode::Mod => match const_rhs {
                        Some(1) | Some(-1) => {
                            let zero = b.const_reg(0)?;
                            b.vstack.push(VDesc::Reg(zero));
                            continue;
                        }
                        Some(d) if d != 0 => {
                            let m = b.magic_idx(d)?;
                            // Fuse `(a op b) % const` when `a` is the result of
                            // the immediately preceding add/sub/mul.
                            let mut fused = false;
                            if is_temp(ra) {
                                if let Some(last) = b.uops.last_mut() {
                                    let replacement = match *last {
                                        UOp::Add(d0, a0, b0) if d0 == ra => {
                                            Some(UOp::AddModC(dst, a0, b0, m))
                                        }
                                        UOp::Sub(d0, a0, b0) if d0 == ra => {
                                            Some(UOp::SubModC(dst, a0, b0, m))
                                        }
                                        UOp::Mul(d0, a0, b0) if d0 == ra => {
                                            Some(UOp::MulModC(dst, a0, b0, m))
                                        }
                                        _ => None,
                                    };
                                    if let Some(rep) = replacement {
                                        *last = rep;
                                        fused = true;
                                    }
                                }
                            }
                            if !fused {
                                b.uops.push(UOp::ModC(dst, ra, m));
                            }
                        }
                        _ => b.uops.push(UOp::Mod(dst, ra, rb)),
                    },
                    _ => unreachable!(),
                }
                b.vstack.push(VDesc::Reg(dst));
            }
            OpCode::Div => return None,
            OpCode::FloorDiv => {
                let VDesc::Reg(rb) = b.vstack.pop()? else {
                    return None;
                };
                let VDesc::Reg(ra) = b.vstack.pop()? else {
                    return None;
                };
                let dst = b.temp_reg(b.vstack.len())?;
                let const_rhs = const_value_of(&b, rb);
                match const_rhs {
                    Some(1) => {
                        b.vstack.push(VDesc::Reg(ra));
                        continue;
                    }
                    Some(-1) => b.uops.push(UOp::Neg(dst, ra)),
                    Some(d) if d != 0 => {
                        let m = b.magic_idx(d)?;
                        b.uops.push(UOp::DivC(dst, ra, m));
                    }
                    _ => b.uops.push(UOp::Div(dst, ra, rb)),
                }
                b.vstack.push(VDesc::Reg(dst));
            }
            OpCode::Neg => {
                let VDesc::Reg(ra) = b.vstack.pop()? else {
                    return None;
                };
                let dst = b.temp_reg(b.vstack.len())?;
                b.uops.push(UOp::Neg(dst, ra));
                b.vstack.push(VDesc::Reg(dst));
            }
            OpCode::Lt | OpCode::Le | OpCode::Gt | OpCode::Ge | OpCode::Eq | OpCode::Ne => {
                let VDesc::Reg(rb) = b.vstack.pop()? else {
                    return None;
                };
                let VDesc::Reg(ra) = b.vstack.pop()? else {
                    return None;
                };
                let kind = match op {
                    OpCode::Lt => CmpKind::Lt,
                    OpCode::Le => CmpKind::Le,
                    OpCode::Gt => CmpKind::Gt,
                    OpCode::Ge => CmpKind::Ge,
                    OpCode::Eq => CmpKind::Eq,
                    OpCode::Ne => CmpKind::Ne,
                    _ => unreachable!(),
                };
                b.vstack.push(VDesc::Cmp(kind, ra, rb));
            }
            OpCode::JumpIfFalse(target) => {
                let desc = b.vstack.pop()?;
                let tgt = branch_target(&b, head, end, bc, target as usize)?;
                let uop_idx = b.uops.len();
                // Branch when the condition is FALSE: invert the comparison.
                let placeholder = u32::MAX;
                let uop = match desc {
                    VDesc::Cmp(CmpKind::Lt, a, r) => UOp::BrGe(a, r, placeholder),
                    VDesc::Cmp(CmpKind::Le, a, r) => UOp::BrGt(a, r, placeholder),
                    VDesc::Cmp(CmpKind::Gt, a, r) => UOp::BrLe(a, r, placeholder),
                    VDesc::Cmp(CmpKind::Ge, a, r) => UOp::BrLt(a, r, placeholder),
                    VDesc::Cmp(CmpKind::Eq, a, r) => UOp::BrNe(a, r, placeholder),
                    VDesc::Cmp(CmpKind::Ne, a, r) => UOp::BrEq(a, r, placeholder),
                    VDesc::Reg(r) => UOp::BrZero(r, placeholder),
                };
                b.uops.push(uop);
                b.patches.push((uop_idx, tgt));
            }
            OpCode::Jump(target) => {
                if !b.vstack.is_empty() {
                    return None;
                }
                let t = target as usize;
                if t >= head && t <= end {
                    if t <= bc {
                        // Backedge: snapshot locals for exact bail replay.
                        let uop_target = *b.uop_at_bc.get(&(t as u32))?;
                        b.uops.push(UOp::BackJump(uop_target, t as u32));
                    } else {
                        let idx = b.uops.len();
                        b.uops.push(UOp::Jump(u32::MAX));
                        b.patches.push((idx, BrTarget::PendingBc(t as u32)));
                    }
                } else {
                    b.uops.push(UOp::Exit(t as u32));
                }
            }
            OpCode::Pop => {
                b.vstack.pop()?;
            }
            // Calls, arrays, strings, bools, returns: not turbo material.
            _ => return None,
        }
    }

    if !b.vstack.is_empty() {
        return None;
    }

    // Resolve pending branch targets; out-of-region targets become Exit stubs.
    let mut stub_at: HashMap<u32, u32> = HashMap::new();
    for (uop_idx, tgt) in std::mem::take(&mut b.patches) {
        let resolved: u32 = match tgt {
            BrTarget::UOp(u) => u,
            BrTarget::PendingBc(bc) => *b.uop_at_bc.get(&bc)?,
            BrTarget::ExitBc(bc) => match stub_at.get(&bc) {
                Some(&u) => u,
                None => {
                    let u = b.uops.len() as u32;
                    b.uops.push(UOp::Exit(bc));
                    stub_at.insert(bc, u);
                    u
                }
            },
        };
        match &mut b.uops[uop_idx] {
            UOp::Jump(t)
            | UOp::BrLt(_, _, t)
            | UOp::BrLe(_, _, t)
            | UOp::BrGt(_, _, t)
            | UOp::BrGe(_, _, t)
            | UOp::BrEq(_, _, t)
            | UOp::BrNe(_, _, t)
            | UOp::BrZero(_, t) => *t = resolved,
            _ => return None,
        }
    }

    // Peephole: fuse each backedge with the loop condition it jumps to, so one
    // dispatch handles "jump to head + compare + branch".
    for i in 0..b.uops.len() {
        if let UOp::BackJump(t, bc) = b.uops[i] {
            let fall = t + 1;
            if fall as usize >= b.uops.len() || i as u32 == t {
                continue;
            }
            let fused = match b.uops[t as usize] {
                UOp::BrLt(a, x, tk) => Some(UOp::BackJumpBr(CmpKind::Lt, a, x, tk, fall, bc)),
                UOp::BrLe(a, x, tk) => Some(UOp::BackJumpBr(CmpKind::Le, a, x, tk, fall, bc)),
                UOp::BrGt(a, x, tk) => Some(UOp::BackJumpBr(CmpKind::Gt, a, x, tk, fall, bc)),
                UOp::BrGe(a, x, tk) => Some(UOp::BackJumpBr(CmpKind::Ge, a, x, tk, fall, bc)),
                UOp::BrEq(a, x, tk) => Some(UOp::BackJumpBr(CmpKind::Eq, a, x, tk, fall, bc)),
                UOp::BrNe(a, x, tk) => Some(UOp::BackJumpBr(CmpKind::Ne, a, x, tk, fall, bc)),
                _ => None,
            };
            if let Some(f) = fused {
                b.uops[i] = f;
            }
        }
    }

    // Remap provisional const/temp registers to their final indices.
    let n_slots = b.slots.len();
    if n_slots >= CONST_BASE as usize {
        return None;
    }
    let temp_base = n_slots + b.consts.len();
    let total_regs = temp_base + b.max_depth;
    if total_regs >= CONST_BASE as usize {
        return None;
    }
    let remap = |r: &mut u16| {
        if is_temp(*r) {
            *r = (temp_base + (*r - TEMP_BASE) as usize) as u16;
        } else if is_const(*r) {
            *r = (n_slots + (*r - CONST_BASE) as usize) as u16;
        }
    };
    for op in &mut b.uops {
        match op {
            UOp::Add(d, a, x)
            | UOp::Sub(d, a, x)
            | UOp::Mul(d, a, x)
            | UOp::Div(d, a, x)
            | UOp::Mod(d, a, x) => {
                remap(d);
                remap(a);
                remap(x);
            }
            UOp::AddModC(d, a, x, _) | UOp::SubModC(d, a, x, _) | UOp::MulModC(d, a, x, _) => {
                remap(d);
                remap(a);
                remap(x);
            }
            UOp::DivC(d, a, _) | UOp::ModC(d, a, _) | UOp::Neg(d, a) | UOp::Mov(d, a) => {
                remap(d);
                remap(a);
            }
            UOp::BrLt(a, x, _)
            | UOp::BrLe(a, x, _)
            | UOp::BrGt(a, x, _)
            | UOp::BrGe(a, x, _)
            | UOp::BrEq(a, x, _)
            | UOp::BrNe(a, x, _) => {
                remap(a);
                remap(x);
            }
            UOp::BrZero(a, _) => remap(a),
            UOp::BackJumpBr(_, a, x, ..) => {
                remap(a);
                remap(x);
            }
            UOp::Jump(_) | UOp::BackJump(..) | UOp::Exit(_) => {}
        }
    }

    // Validate every register and branch target (executor indexes unchecked-by-logic).
    let n_uops = b.uops.len() as u32;
    for op in &b.uops {
        let regs_ok = |rs: &[u16]| rs.iter().all(|&r| (r as usize) < total_regs);
        let tgt_ok = |t: u32| t < n_uops;
        let ok = match *op {
            UOp::Add(d, a, x)
            | UOp::Sub(d, a, x)
            | UOp::Mul(d, a, x)
            | UOp::Div(d, a, x)
            | UOp::Mod(d, a, x) => regs_ok(&[d, a, x]),
            UOp::AddModC(d, a, x, m) | UOp::SubModC(d, a, x, m) | UOp::MulModC(d, a, x, m) => {
                regs_ok(&[d, a, x]) && (m as usize) < b.magics.len()
            }
            UOp::DivC(d, a, m) | UOp::ModC(d, a, m) => {
                regs_ok(&[d, a]) && (m as usize) < b.magics.len()
            }
            UOp::Neg(d, a) | UOp::Mov(d, a) => regs_ok(&[d, a]),
            UOp::Jump(t) => tgt_ok(t),
            UOp::BackJump(t, _) => tgt_ok(t),
            UOp::BackJumpBr(_, a, x, taken, fall, _) => {
                regs_ok(&[a, x]) && tgt_ok(taken) && tgt_ok(fall)
            }
            UOp::BrLt(a, x, t)
            | UOp::BrLe(a, x, t)
            | UOp::BrGt(a, x, t)
            | UOp::BrGe(a, x, t)
            | UOp::BrEq(a, x, t)
            | UOp::BrNe(a, x, t) => regs_ok(&[a, x]) && tgt_ok(t),
            UOp::BrZero(a, t) => regs_ok(&[a]) && tgt_ok(t),
            UOp::Exit(_) => true,
        };
        if !ok {
            return None;
        }
    }

    Some(LoopRegion {
        head_bc: head as u32,
        slots: b.slots,
        consts: b.consts,
        total_regs,
        uops: b.uops,
        magics: b.magics,
        jit: RefCell::new(JitState::Untried),
        entries: Cell::new(0),
    })
}

/// If `reg` is a (provisional) constant register, return its value.
fn const_value_of(b: &RegionBuilder, reg: u16) -> Option<i64> {
    if is_const(reg) {
        b.consts.get((reg - CONST_BASE) as usize).copied()
    } else {
        None
    }
}

fn branch_target(
    b: &RegionBuilder,
    head: usize,
    end: usize,
    bc: usize,
    target: usize,
) -> Option<BrTarget> {
    if target >= head && target <= end {
        if target <= bc {
            Some(BrTarget::UOp(*b.uop_at_bc.get(&(target as u32))?))
        } else {
            Some(BrTarget::PendingBc(target as u32))
        }
    } else {
        Some(BrTarget::ExitBc(target as u32))
    }
}

/// Cranelift-based native compilation of loop regions. Micro-ops translate
/// 1:1 into IR; loop-carried values live in machine registers, so the region
/// runs at the same speed as statically compiled code.
mod jit {
    use super::{CmpKind, JitState, LoopRegion, MagicDiv, UOp};
    use cranelift_codegen::ir::condcodes::IntCC;
    use cranelift_codegen::ir::{
        types, AbiParam, InstBuilder, MemFlagsData, StackSlotData, StackSlotKind,
    };
    use cranelift_codegen::settings::{self, Configurable};
    use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
    use cranelift_jit::{JITBuilder, JITModule};
    use cranelift_module::{Linkage, Module};

    type LoopFn = unsafe extern "C" fn(*mut i64) -> i64;

    pub(super) struct JitLoop {
        module: Option<JITModule>,
        func: LoopFn,
    }

    // The VM is single-threaded; JitLoop is only used from the owning thread.

    impl JitLoop {
        /// Safety: `regs` must point to `total_regs` i64s of the region this
        /// code was compiled for.
        pub(super) unsafe fn call(&self, regs: *mut i64) -> usize {
            (self.func)(regs) as usize
        }
    }

    impl Drop for JitLoop {
        fn drop(&mut self) {
            if let Some(module) = self.module.take() {
                // Safety: the function pointer is never called after drop.
                unsafe { module.free_memory() };
            }
        }
    }

    fn jit_disabled() -> bool {
        static DISABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *DISABLED.get_or_init(|| std::env::var_os("NIAO_NO_JIT").is_some())
    }

    fn cmp_cc(kind: CmpKind) -> IntCC {
        match kind {
            CmpKind::Lt => IntCC::SignedLessThan,
            CmpKind::Le => IntCC::SignedLessThanOrEqual,
            CmpKind::Gt => IntCC::SignedGreaterThan,
            CmpKind::Ge => IntCC::SignedGreaterThanOrEqual,
            CmpKind::Eq => IntCC::Equal,
            CmpKind::Ne => IntCC::NotEqual,
        }
    }

    fn compile(region: &LoopRegion) -> Option<JitLoop> {
        if jit_disabled() {
            return None;
        }

        let mut flag_builder = settings::builder();
        flag_builder.set("use_colocated_libcalls", "false").ok()?;
        flag_builder.set("is_pic", "false").ok()?;
        flag_builder.set("opt_level", "speed").ok()?;
        let isa_builder = cranelift_native::builder().ok()?;
        let isa = isa_builder
            .finish(settings::Flags::new(flag_builder))
            .ok()?;
        let mut module = JITModule::new(JITBuilder::with_isa(
            isa,
            cranelift_module::default_libcall_names(),
        ));

        let ptr_ty = module.target_config().pointer_type();
        let mut sig = module.make_signature();
        sig.params.push(AbiParam::new(ptr_ty));
        sig.returns.push(AbiParam::new(types::I64));
        let func_id = module
            .declare_function("turbo_loop", Linkage::Export, &sig)
            .ok()?;

        let mut ctx = module.make_context();
        ctx.func.signature = sig;
        let mut fb_ctx = FunctionBuilderContext::new();
        {
            let mut b = FunctionBuilder::new(&mut ctx.func, &mut fb_ctx);
            build_body(&mut b, region)?;
            b.seal_all_blocks();
            b.finalize();
        }

        module.define_function(func_id, &mut ctx).ok()?;
        module.clear_context(&mut ctx);
        module.finalize_definitions().ok()?;
        let code = module.get_finalized_function(func_id);
        // Safety: the finalized code has exactly the declared signature.
        let func: LoopFn = unsafe { std::mem::transmute(code) };
        Some(JitLoop {
            module: Some(module),
            func,
        })
    }

    fn build_body(b: &mut FunctionBuilder, region: &LoopRegion) -> Option<()> {
        let uops = &region.uops;
        let magics = &region.magics;
        let n_slots = region.slots.len();
        let n_consts = region.consts.len();
        let flags = MemFlagsData::trusted();

        // One variable per turbo register + one for the bail resume ip.
        let vars: Vec<Variable> = (0..region.total_regs)
            .map(|_| b.declare_var(types::I64))
            .collect();
        let snap_bc = b.declare_var(types::I64);

        let entry = b.create_block();
        b.append_block_params_for_function_params(entry);
        b.switch_to_block(entry);
        let regs_ptr = b.block_params(entry)[0];

        // Snapshot buffer for exact bail replay (written at every backedge).
        let snap_size = (n_slots.max(1) * 8) as u32;
        let ss = b.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            snap_size,
            3,
        ));

        for i in 0..(n_slots + n_consts) {
            let v = b.ins().load(types::I64, flags, regs_ptr, (i * 8) as i32);
            b.def_var(vars[i], v);
        }
        for i in (n_slots + n_consts)..region.total_regs {
            let zero = b.ins().iconst(types::I64, 0);
            b.def_var(vars[i], zero);
        }
        for i in 0..n_slots {
            let v = b.use_var(vars[i]);
            b.ins().stack_store(v, ss, (i * 8) as i32);
        }
        let head = b.ins().iconst(types::I64, region.head_bc as i64);
        b.def_var(snap_bc, head);

        let blocks: Vec<_> = uops.iter().map(|_| b.create_block()).collect();
        let bail_block = b.create_block();
        b.ins().jump(blocks[0], &[]);

        // Shared bail path: restore the snapshot into regs, resume at snap_bc.
        b.switch_to_block(bail_block);
        for i in 0..n_slots {
            let v = b.ins().stack_load(types::I64, ss, (i * 8) as i32);
            b.ins().store(flags, v, regs_ptr, (i * 8) as i32);
        }
        let resume = b.use_var(snap_bc);
        b.ins().return_(&[resume]);

        // n = numerator variable value; returns the magic quotient.
        macro_rules! magic_div {
            ($b:expr, $m:expr, $n:expr) => {{
                let m: &MagicDiv = $m;
                let mval = $b.ins().iconst(types::I64, m.m);
                let mut q = $b.ins().smulhi($n, mval);
                if m.add_n {
                    q = $b.ins().iadd(q, $n);
                }
                if m.sub_n {
                    q = $b.ins().isub(q, $n);
                }
                if m.s > 0 {
                    q = $b.ins().sshr_imm(q, m.s as i64);
                }
                let sign = $b.ins().ushr_imm(q, 63);
                $b.ins().iadd(q, sign)
            }};
        }
        macro_rules! magic_rem {
            ($b:expr, $m:expr, $n:expr) => {{
                let m: &MagicDiv = $m;
                let q = magic_div!($b, m, $n);
                let prod = $b.ins().imul_imm(q, m.d);
                $b.ins().isub($n, prod)
            }};
        }

        for (i, op) in uops.iter().enumerate() {
            b.switch_to_block(blocks[i]);
            let next = blocks.get(i + 1).copied();

            // Checked ops branch to bail on overflow, else continue in a fresh block.
            macro_rules! guard {
                ($ovf:expr) => {{
                    let cont = b.create_block();
                    b.ins().brif($ovf, bail_block, &[], cont, &[]);
                    b.switch_to_block(cont);
                }};
            }
            macro_rules! finish {
                () => {{
                    b.ins().jump(next?, &[]);
                }};
            }

            match *op {
                UOp::Add(d, x, y) => {
                    let a = b.use_var(vars[x as usize]);
                    let c = b.use_var(vars[y as usize]);
                    let r = b.ins().iadd(a, c);
                    let t1 = b.ins().bxor(a, r);
                    let t2 = b.ins().bxor(c, r);
                    let t3 = b.ins().band(t1, t2);
                    let ovf = b.ins().icmp_imm(IntCC::SignedLessThan, t3, 0);
                    guard!(ovf);
                    b.def_var(vars[d as usize], r);
                    finish!();
                }
                UOp::Sub(d, x, y) => {
                    let a = b.use_var(vars[x as usize]);
                    let c = b.use_var(vars[y as usize]);
                    let r = b.ins().isub(a, c);
                    let t1 = b.ins().bxor(a, c);
                    let t2 = b.ins().bxor(a, r);
                    let t3 = b.ins().band(t1, t2);
                    let ovf = b.ins().icmp_imm(IntCC::SignedLessThan, t3, 0);
                    guard!(ovf);
                    b.def_var(vars[d as usize], r);
                    finish!();
                }
                UOp::Mul(d, x, y) => {
                    let a = b.use_var(vars[x as usize]);
                    let c = b.use_var(vars[y as usize]);
                    let lo = b.ins().imul(a, c);
                    let hi = b.ins().smulhi(a, c);
                    let sign = b.ins().sshr_imm(lo, 63);
                    let ovf = b.ins().icmp(IntCC::NotEqual, hi, sign);
                    guard!(ovf);
                    b.def_var(vars[d as usize], lo);
                    finish!();
                }
                UOp::Div(d, x, y) | UOp::Mod(d, x, y) => {
                    let a = b.use_var(vars[x as usize]);
                    let c = b.use_var(vars[y as usize]);
                    let zero = b.ins().icmp_imm(IntCC::Equal, c, 0);
                    guard!(zero);
                    let is_min = b.ins().icmp_imm(IntCC::Equal, a, i64::MIN);
                    let is_m1 = b.ins().icmp_imm(IntCC::Equal, c, -1);
                    let both = b.ins().band(is_min, is_m1);
                    guard!(both);
                    let r = if matches!(op, UOp::Div(..)) {
                        b.ins().sdiv(a, c)
                    } else {
                        b.ins().srem(a, c)
                    };
                    b.def_var(vars[d as usize], r);
                    finish!();
                }
                UOp::DivC(d, x, m) => {
                    let a = b.use_var(vars[x as usize]);
                    let q = magic_div!(b, &magics[m as usize], a);
                    b.def_var(vars[d as usize], q);
                    finish!();
                }
                UOp::ModC(d, x, m) => {
                    let a = b.use_var(vars[x as usize]);
                    let r = magic_rem!(b, &magics[m as usize], a);
                    b.def_var(vars[d as usize], r);
                    finish!();
                }
                UOp::AddModC(d, x, y, m) | UOp::SubModC(d, x, y, m) | UOp::MulModC(d, x, y, m) => {
                    let a = b.use_var(vars[x as usize]);
                    let c = b.use_var(vars[y as usize]);
                    let (r, ovf) = match op {
                        UOp::AddModC(..) => {
                            let r = b.ins().iadd(a, c);
                            let t1 = b.ins().bxor(a, r);
                            let t2 = b.ins().bxor(c, r);
                            let t3 = b.ins().band(t1, t2);
                            (r, b.ins().icmp_imm(IntCC::SignedLessThan, t3, 0))
                        }
                        UOp::SubModC(..) => {
                            let r = b.ins().isub(a, c);
                            let t1 = b.ins().bxor(a, c);
                            let t2 = b.ins().bxor(a, r);
                            let t3 = b.ins().band(t1, t2);
                            (r, b.ins().icmp_imm(IntCC::SignedLessThan, t3, 0))
                        }
                        _ => {
                            let lo = b.ins().imul(a, c);
                            let hi = b.ins().smulhi(a, c);
                            let sign = b.ins().sshr_imm(lo, 63);
                            (lo, b.ins().icmp(IntCC::NotEqual, hi, sign))
                        }
                    };
                    guard!(ovf);
                    let rem = magic_rem!(b, &magics[m as usize], r);
                    b.def_var(vars[d as usize], rem);
                    finish!();
                }
                UOp::Neg(d, x) => {
                    let a = b.use_var(vars[x as usize]);
                    let ovf = b.ins().icmp_imm(IntCC::Equal, a, i64::MIN);
                    guard!(ovf);
                    let r = b.ins().ineg(a);
                    b.def_var(vars[d as usize], r);
                    finish!();
                }
                UOp::Mov(d, x) => {
                    let a = b.use_var(vars[x as usize]);
                    b.def_var(vars[d as usize], a);
                    finish!();
                }
                UOp::Jump(t) => {
                    b.ins().jump(blocks[t as usize], &[]);
                }
                UOp::BackJump(t, bc) => {
                    for k in 0..n_slots {
                        let v = b.use_var(vars[k]);
                        b.ins().stack_store(v, ss, (k * 8) as i32);
                    }
                    let bcv = b.ins().iconst(types::I64, bc as i64);
                    b.def_var(snap_bc, bcv);
                    b.ins().jump(blocks[t as usize], &[]);
                }
                UOp::BackJumpBr(kind, x, y, taken, fall, bc) => {
                    for k in 0..n_slots {
                        let v = b.use_var(vars[k]);
                        b.ins().stack_store(v, ss, (k * 8) as i32);
                    }
                    let bcv = b.ins().iconst(types::I64, bc as i64);
                    b.def_var(snap_bc, bcv);
                    let a = b.use_var(vars[x as usize]);
                    let c = b.use_var(vars[y as usize]);
                    let cond = b.ins().icmp(cmp_cc(kind), a, c);
                    b.ins()
                        .brif(cond, blocks[taken as usize], &[], blocks[fall as usize], &[]);
                }
                UOp::BrLt(x, y, t)
                | UOp::BrLe(x, y, t)
                | UOp::BrGt(x, y, t)
                | UOp::BrGe(x, y, t)
                | UOp::BrEq(x, y, t)
                | UOp::BrNe(x, y, t) => {
                    let kind = match op {
                        UOp::BrLt(..) => CmpKind::Lt,
                        UOp::BrLe(..) => CmpKind::Le,
                        UOp::BrGt(..) => CmpKind::Gt,
                        UOp::BrGe(..) => CmpKind::Ge,
                        UOp::BrEq(..) => CmpKind::Eq,
                        _ => CmpKind::Ne,
                    };
                    let a = b.use_var(vars[x as usize]);
                    let c = b.use_var(vars[y as usize]);
                    let cond = b.ins().icmp(cmp_cc(kind), a, c);
                    b.ins().brif(cond, blocks[t as usize], &[], next?, &[]);
                }
                UOp::BrZero(x, t) => {
                    let a = b.use_var(vars[x as usize]);
                    let cond = b.ins().icmp_imm(IntCC::Equal, a, 0);
                    b.ins().brif(cond, blocks[t as usize], &[], next?, &[]);
                }
                UOp::Exit(bc) => {
                    for k in 0..n_slots {
                        let v = b.use_var(vars[k]);
                        b.ins().store(flags, v, regs_ptr, (k * 8) as i32);
                    }
                    let r = b.ins().iconst(types::I64, bc as i64);
                    b.ins().return_(&[r]);
                }
            }
        }
        Some(())
    }

    /// Compile a region and cache the result in its `jit` cell.
    pub(super) fn ensure_compiled(region: &LoopRegion) {
        if matches!(&*region.jit.borrow(), JitState::Untried) {
            let compiled = compile(region);
            *region.jit.borrow_mut() = match compiled {
                Some(j) => JitState::Ready(j),
                None => JitState::Failed,
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magic_div_matches_hardware() {
        let divisors = [
            2i64,
            3,
            4,
            5,
            7,
            10,
            997,
            1_000_000_007,
            i64::MAX,
            -2,
            -3,
            -997,
            -1_000_000_007,
            1 << 40,
        ];
        let dividends = [
            0i64,
            1,
            -1,
            2,
            -2,
            996,
            997,
            998,
            12_345,
            -12_345,
            1_000_000_006,
            1_000_000_007,
            1_000_000_008,
            i64::MAX,
            i64::MAX - 1,
            i64::MIN,
            i64::MIN + 1,
            123_456_789_012_345,
            -123_456_789_012_345,
        ];
        for &d in &divisors {
            let m = MagicDiv::new(d);
            for &n in &dividends {
                assert_eq!(m.div(n), n / d, "div {n} / {d}");
                assert_eq!(m.rem(n), n % d, "rem {n} % {d}");
            }
        }
    }
}
