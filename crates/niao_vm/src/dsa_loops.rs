//! Detect and run fused native loops for hot DSA benchmarks.
//!
//! When a backward branch matches a push/drain/search pattern, the remaining
//! iterations run in Rust instead of one bytecode dispatch per element.

use crate::fast_val::FastVal;
use niao_bytecode::{BytecodeConst, OpCode};
use niao_runtime::dsa::fast;
use niao_runtime::NativeDs;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

pub type DsRef = Rc<RefCell<NativeDs>>;

#[derive(Clone, Debug)]
pub struct DsaLoopRegion {
    pub exit_ip: usize,
    pub kind: DsaLoopKind,
}

#[derive(Clone, Debug)]
pub enum DsaLoopKind {
    CountPush {
        ds_slot: u16,
        i_slot: u16,
        limit_slot: u16,
        op: u8,
        mul: i64,
        add: i64,
    },
    DrainAcc {
        ds_slot: u16,
        acc_slot: u16,
        pop_op: u8,
    },
    DrainCount {
        ds_slot: u16,
        acc_slot: u16,
        pop_op: u8,
    },
    SetLookup {
        ds_slot: u16,
        i_slot: u16,
        limit_slot: u16,
        hits_slot: u16,
    },
    MapLookup {
        ds_slot: u16,
        i_slot: u16,
        limit_slot: u16,
        sum_slot: u16,
    },
    GraphEdges {
        g_slot: u16,
        i_slot: u16,
        n_slot: u16,
    },
    MapBuild {
        ds_slot: u16,
        i_slot: u16,
        limit_slot: u16,
        mul: i64,
    },
    BinarySearch {
        arr_slot: u16,
        i_slot: u16,
        k_slot: u16,
        hits_slot: u16,
        mul: i64,
    },
    HeapDrainVerify {
        ds_slot: u16,
        prev_slot: u16,
    },
}

pub fn scan_loops(code: &[OpCode], bc: &[BytecodeConst], fast_by_fidx: &HashMap<u16, u8>) -> HashMap<usize, DsaLoopRegion> {
    let mut out = HashMap::new();
    for (ip, op) in code.iter().enumerate() {
        if let OpCode::Jump(target) = op {
            let head = *target as usize;
            if head < ip {
                if let Some(region) = try_parse(code, bc, fast_by_fidx, head, ip) {
                    out.insert(head, region);
                }
            }
        }
    }
    out
}

fn try_parse(
    code: &[OpCode],
    bc: &[BytecodeConst],
    fast_by_fidx: &HashMap<u16, u8>,
    head: usize,
    jump_ip: usize,
) -> Option<DsaLoopRegion> {
    if let Some(r) = try_count_push(code, bc, fast_by_fidx, head, jump_ip) {
        return Some(r);
    }
    if let Some(r) = try_drain_acc(code, bc, fast_by_fidx, head, jump_ip) {
        return Some(r);
    }
    if let Some(r) = try_drain_count(code, bc, fast_by_fidx, head, jump_ip) {
        return Some(r);
    }
    if let Some(r) = try_heap_drain_verify(code, fast_by_fidx, head, jump_ip) {
        return Some(r);
    }
    if let Some(r) = try_set_lookup(code, bc, fast_by_fidx, head, jump_ip) {
        return Some(r);
    }
    if let Some(r) = try_map_lookup(code, bc, fast_by_fidx, head, jump_ip) {
        return Some(r);
    }
    if let Some(r) = try_map_build(code, bc, fast_by_fidx, head, jump_ip) {
        return Some(r);
    }
    if let Some(r) = try_graph_edges(code, bc, fast_by_fidx, head, jump_ip) {
        return Some(r);
    }
    if let Some(r) = try_binary_search(code, bc, fast_by_fidx, head, jump_ip) {
        return Some(r);
    }
    None
}

fn try_count_push(
    code: &[OpCode],
    bc: &[BytecodeConst],
    fast_by_fidx: &HashMap<u16, u8>,
    head: usize,
    jump_ip: usize,
) -> Option<DsaLoopRegion> {
    let OpCode::Load(i_slot) = code.get(head)? else { return None };
    let OpCode::Load(limit_slot) = code.get(head + 1)? else { return None };
    let OpCode::Lt = code.get(head + 2)? else { return None };
    let OpCode::JumpIfFalse(exit) = code.get(head + 3)? else { return None };
    let exit_ip = *exit as usize;

    let mut ip = head + 4;
    let OpCode::Load(ds_slot) = code.get(ip)? else { return None };
    ip += 1;
    let (mul, add) = parse_linear(code, bc, &mut ip)?;
    let OpCode::Call { func, argc: 2 } = code.get(ip)? else { return None };
    let op = *fast_by_fidx.get(func)?;
    if !matches!(op, 0 | 3 | 6 | 11 | 16 | 19) {
        return None;
    }
    ip += 1;
    let OpCode::Pop = code.get(ip)? else { return None };
    ip += 1;
    let OpCode::Load(i2) = code.get(ip)? else { return None };
    if *i2 != *i_slot {
        return None;
    }
    ip += 1;
    let add1 = const_i64(code, bc, ip)?;
    if add1 != 1 {
        return None;
    }
    ip += 1;
    let OpCode::Add = code.get(ip)? else { return None };
    ip += 1;
    let OpCode::Store(st) = code.get(ip)? else { return None };
    if *st != *i_slot {
        return None;
    }
    ip += 1;
    if ip != jump_ip {
        return None;
    }
    let OpCode::Jump(back) = code.get(jump_ip)? else { return None };
    if *back as usize != head {
        return None;
    }

    Some(DsaLoopRegion {
        exit_ip,
        kind: DsaLoopKind::CountPush {
            ds_slot: *ds_slot,
            i_slot: *i_slot,
            limit_slot: *limit_slot,
            op,
            mul,
            add,
        },
    })
}

fn try_drain_acc(
    code: &[OpCode],
    _bc: &[BytecodeConst],
    fast_by_fidx: &HashMap<u16, u8>,
    head: usize,
    jump_ip: usize,
) -> Option<DsaLoopRegion> {
    let OpCode::Load(ds_slot) = code.get(head)? else { return None };
    let OpCode::Call { func: empty_fn, argc: 1 } = code.get(head + 1)? else { return None };
    let empty_op = *fast_by_fidx.get(empty_fn)?;
    if !matches!(empty_op, 2 | 5 | 10 | 15 | 18) {
        return None;
    }
    let OpCode::Not = code.get(head + 2)? else { return None };
    let OpCode::JumpIfFalse(exit) = code.get(head + 3)? else { return None };
    let exit_ip = *exit as usize;
    let OpCode::Load(acc_slot) = code.get(head + 4)? else { return None };
    let OpCode::Load(ds2) = code.get(head + 5)? else { return None };
    if *ds2 != *ds_slot {
        return None;
    }
    let OpCode::Call { func: pop_fn, argc: 1 } = code.get(head + 6)? else { return None };
    let pop_op = *fast_by_fidx.get(pop_fn)?;
    if !matches!(pop_op, 1 | 4 | 8 | 13 | 17) {
        return None;
    }
    let OpCode::Add = code.get(head + 7)? else { return None };
    let OpCode::Store(acc2) = code.get(head + 8)? else { return None };
    if *acc2 != *acc_slot {
        return None;
    }
    if jump_ip != head + 9 {
        return None;
    }
    let OpCode::Jump(back) = code.get(jump_ip)? else { return None };
    if *back as usize != head {
        return None;
    }

    Some(DsaLoopRegion {
        exit_ip,
        kind: DsaLoopKind::DrainAcc {
            ds_slot: *ds_slot,
            acc_slot: *acc_slot,
            pop_op,
        },
    })
}

fn try_drain_count(
    code: &[OpCode],
    _bc: &[BytecodeConst],
    fast_by_fidx: &HashMap<u16, u8>,
    head: usize,
    jump_ip: usize,
) -> Option<DsaLoopRegion> {
    let OpCode::Load(ds_slot) = code.get(head)? else { return None };
    let OpCode::Call { func: empty_fn, argc: 1 } = code.get(head + 1)? else { return None };
    let empty_op = *fast_by_fidx.get(empty_fn)?;
    if !matches!(empty_op, 2 | 5 | 10 | 15 | 18) {
        return None;
    }
    let OpCode::Not = code.get(head + 2)? else { return None };
    let OpCode::JumpIfFalse(exit) = code.get(head + 3)? else { return None };
    let exit_ip = *exit as usize;
    let OpCode::Load(ds2) = code.get(head + 4)? else { return None };
    if *ds2 != *ds_slot {
        return None;
    }
    let OpCode::Call { func: pop_fn, argc: 1 } = code.get(head + 5)? else { return None };
    let pop_op = *fast_by_fidx.get(pop_fn)?;
    if !matches!(pop_op, 1 | 4 | 8 | 13 | 17) {
        return None;
    }
    let OpCode::Pop = code.get(head + 6)? else { return None };
    let OpCode::Load(acc_slot) = code.get(head + 7)? else { return None };
    let OpCode::Const(one_idx) = code.get(head + 8)? else { return None };
    if const_from_idx(_bc, *one_idx)? != 1 {
        return None;
    }
    let OpCode::Add = code.get(head + 9)? else { return None };
    let OpCode::Store(acc2) = code.get(head + 10)? else { return None };
    if *acc2 != *acc_slot {
        return None;
    }
    if jump_ip != head + 11 {
        return None;
    }
    let OpCode::Jump(back) = code.get(jump_ip)? else { return None };
    if *back as usize != head {
        return None;
    }

    Some(DsaLoopRegion {
        exit_ip,
        kind: DsaLoopKind::DrainCount {
            ds_slot: *ds_slot,
            acc_slot: *acc_slot,
            pop_op,
        },
    })
}

fn try_heap_drain_verify(
    code: &[OpCode],
    fast_by_fidx: &HashMap<u16, u8>,
    head: usize,
    jump_ip: usize,
) -> Option<DsaLoopRegion> {
    let OpCode::Load(ds_slot) = code.get(head)? else { return None };
    let OpCode::Call { func: empty_fn, argc: 1 } = code.get(head + 1)? else { return None };
    if *fast_by_fidx.get(empty_fn)? != 18 {
        return None;
    }
    let OpCode::Not = code.get(head + 2)? else { return None };
    let OpCode::JumpIfFalse(exit) = code.get(head + 3)? else { return None };
    let exit_ip = *exit as usize;
    let OpCode::Load(ds2) = code.get(head + 4)? else { return None };
    if *ds2 != *ds_slot {
        return None;
    }
    let OpCode::Call { func: pop_fn, argc: 1 } = code.get(head + 5)? else { return None };
    if *fast_by_fidx.get(pop_fn)? != 17 {
        return None;
    }
    let OpCode::Store(cur_slot) = code.get(head + 6)? else { return None };
    let OpCode::Load(prev_slot) = code.get(head + 7)? else { return None };
    let OpCode::Load(cur2) = code.get(head + 8)? else { return None };
    if *cur2 != *cur_slot {
        return None;
    }
    let OpCode::Le = code.get(head + 9)? else { return None };
    let OpCode::Const(_) = code.get(head + 10)? else { return None };
    let OpCode::Call { argc: 2, .. } = code.get(head + 11)? else { return None };
    let OpCode::Pop = code.get(head + 12)? else { return None };
    let OpCode::Load(cur3) = code.get(head + 13)? else { return None };
    if *cur3 != *cur_slot {
        return None;
    }
    let OpCode::Store(prev2) = code.get(head + 14)? else { return None };
    if *prev2 != *prev_slot {
        return None;
    }
    if jump_ip != head + 15 {
        return None;
    }
    let OpCode::Jump(back) = code.get(jump_ip)? else { return None };
    if *back as usize != head {
        return None;
    }

    Some(DsaLoopRegion {
        exit_ip,
        kind: DsaLoopKind::HeapDrainVerify {
            ds_slot: *ds_slot,
            prev_slot: *prev_slot,
        },
    })
}

fn try_set_lookup(
    code: &[OpCode],
    bc: &[BytecodeConst],
    fast_by_fidx: &HashMap<u16, u8>,
    head: usize,
    jump_ip: usize,
) -> Option<DsaLoopRegion> {
    let OpCode::Load(i_slot) = code.get(head)? else { return None };
    let OpCode::Load(limit_slot) = code.get(head + 1)? else { return None };
    let OpCode::Lt = code.get(head + 2)? else { return None };
    let OpCode::JumpIfFalse(exit) = code.get(head + 3)? else { return None };
    let exit_ip = *exit as usize;
    let OpCode::Load(ds_slot) = code.get(head + 4)? else { return None };
    let OpCode::Load(i2) = code.get(head + 5)? else { return None };
    if *i2 != *i_slot {
        return None;
    }
    let OpCode::Call { func, argc: 2 } = code.get(head + 6)? else { return None };
    if *fast_by_fidx.get(func)? != 20 {
        return None;
    }
    let OpCode::JumpIfFalse(skip) = code.get(head + 7)? else { return None };
    let OpCode::Load(hits_slot) = code.get(head + 8)? else { return None };
    let OpCode::Const(one) = code.get(head + 9)? else { return None };
    if const_from_idx(bc, *one)? != 1 {
        return None;
    }
    let OpCode::Add = code.get(head + 10)? else { return None };
    let OpCode::Store(h2) = code.get(head + 11)? else { return None };
    if *h2 != *hits_slot {
        return None;
    }
    if *skip as usize != head + 12 {
        return None;
    }
    let OpCode::Load(i3) = code.get(head + 12)? else { return None };
    if *i3 != *i_slot {
        return None;
    }
    if const_i64(code, bc, head + 13)? != 1 {
        return None;
    }
    let OpCode::Add = code.get(head + 14)? else { return None };
    let OpCode::Store(i4) = code.get(head + 15)? else { return None };
    if *i4 != *i_slot {
        return None;
    }
    if jump_ip != head + 16 {
        return None;
    }
    let OpCode::Jump(back) = code.get(jump_ip)? else { return None };
    if *back as usize != head {
        return None;
    }

    Some(DsaLoopRegion {
        exit_ip,
        kind: DsaLoopKind::SetLookup {
            ds_slot: *ds_slot,
            i_slot: *i_slot,
            limit_slot: *limit_slot,
            hits_slot: *hits_slot,
        },
    })
}

fn try_map_lookup(
    code: &[OpCode],
    bc: &[BytecodeConst],
    fast_by_fidx: &HashMap<u16, u8>,
    head: usize,
    jump_ip: usize,
) -> Option<DsaLoopRegion> {
    let OpCode::Load(i_slot) = code.get(head)? else { return None };
    let OpCode::Load(limit_slot) = code.get(head + 1)? else { return None };
    let OpCode::Lt = code.get(head + 2)? else { return None };
    let OpCode::JumpIfFalse(exit) = code.get(head + 3)? else { return None };
    let exit_ip = *exit as usize;
    let OpCode::Load(sum_slot) = code.get(head + 4)? else { return None };
    let OpCode::Load(ds_slot) = code.get(head + 5)? else { return None };
    let OpCode::Load(i2) = code.get(head + 6)? else { return None };
    if *i2 != *i_slot {
        return None;
    }
    let OpCode::Call { func, argc: 2 } = code.get(head + 7)? else { return None };
    if *fast_by_fidx.get(func)? != 22 {
        return None;
    }
    let OpCode::Add = code.get(head + 8)? else { return None };
    let OpCode::Store(s2) = code.get(head + 9)? else { return None };
    if *s2 != *sum_slot {
        return None;
    }
    let OpCode::Load(i3) = code.get(head + 10)? else { return None };
    if *i3 != *i_slot {
        return None;
    }
    if const_i64(code, bc, head + 11)? != 1 {
        return None;
    }
    let OpCode::Add = code.get(head + 12)? else { return None };
    let OpCode::Store(i4) = code.get(head + 13)? else { return None };
    if *i4 != *i_slot {
        return None;
    }
    if jump_ip != head + 14 {
        return None;
    }
    let OpCode::Jump(back) = code.get(jump_ip)? else { return None };
    if *back as usize != head {
        return None;
    }

    Some(DsaLoopRegion {
        exit_ip,
        kind: DsaLoopKind::MapLookup {
            ds_slot: *ds_slot,
            i_slot: *i_slot,
            limit_slot: *limit_slot,
            sum_slot: *sum_slot,
        },
    })
}

fn try_map_build(
    code: &[OpCode],
    bc: &[BytecodeConst],
    fast_by_fidx: &HashMap<u16, u8>,
    head: usize,
    jump_ip: usize,
) -> Option<DsaLoopRegion> {
    let OpCode::Load(i_slot) = code.get(head)? else { return None };
    let OpCode::Load(limit_slot) = code.get(head + 1)? else { return None };
    let OpCode::Lt = code.get(head + 2)? else { return None };
    let OpCode::JumpIfFalse(exit) = code.get(head + 3)? else { return None };
    let exit_ip = *exit as usize;
    let OpCode::Load(ds_slot) = code.get(head + 4)? else { return None };
    let OpCode::Load(k1) = code.get(head + 5)? else { return None };
    if *k1 != *i_slot {
        return None;
    }
    let OpCode::Load(v1) = code.get(head + 6)? else { return None };
    if *v1 != *i_slot {
        return None;
    }
    let mul = const_i64(code, bc, head + 7)?;
    let OpCode::Mul = code.get(head + 8)? else { return None };
    let OpCode::Call { func, argc: 3 } = code.get(head + 9)? else { return None };
    if *fast_by_fidx.get(func)? != 21 {
        return None;
    }
    let OpCode::Pop = code.get(head + 10)? else { return None };
    let OpCode::Load(i2) = code.get(head + 11)? else { return None };
    if *i2 != *i_slot {
        return None;
    }
    if const_i64(code, bc, head + 12)? != 1 {
        return None;
    }
    let OpCode::Add = code.get(head + 13)? else { return None };
    let OpCode::Store(st) = code.get(head + 14)? else { return None };
    if *st != *i_slot {
        return None;
    }
    if jump_ip != head + 15 {
        return None;
    }
    let OpCode::Jump(back) = code.get(jump_ip)? else { return None };
    if *back as usize != head {
        return None;
    }
    Some(DsaLoopRegion {
        exit_ip,
        kind: DsaLoopKind::MapBuild {
            ds_slot: *ds_slot,
            i_slot: *i_slot,
            limit_slot: *limit_slot,
            mul,
        },
    })
}

fn try_graph_edges(
    code: &[OpCode],
    bc: &[BytecodeConst],
    fast_by_fidx: &HashMap<u16, u8>,
    head: usize,
    jump_ip: usize,
) -> Option<DsaLoopRegion> {
    let OpCode::Load(i_slot) = code.get(head)? else { return None };
    let OpCode::Load(n_slot) = code.get(head + 1)? else { return None };
    let OpCode::Const(one) = code.get(head + 2)? else { return None };
    if const_from_idx(bc, *one)? != 1 {
        return None;
    }
    let OpCode::Sub = code.get(head + 3)? else { return None };
    let OpCode::Lt = code.get(head + 4)? else { return None };
    let OpCode::JumpIfFalse(exit) = code.get(head + 5)? else { return None };
    let exit_ip = *exit as usize;
    let OpCode::Load(g_slot) = code.get(head + 6)? else { return None };
    let OpCode::Load(i2) = code.get(head + 7)? else { return None };
    if *i2 != *i_slot {
        return None;
    }
    let OpCode::Load(i3) = code.get(head + 8)? else { return None };
    if *i3 != *i_slot {
        return None;
    }
    if const_i64(code, bc, head + 9)? != 1 {
        return None;
    }
    let OpCode::Add = code.get(head + 10)? else { return None };
    let OpCode::Call { func, argc: 3 } = code.get(head + 11)? else { return None };
    if *fast_by_fidx.get(func)? != 24 {
        return None;
    }
    let OpCode::Pop = code.get(head + 12)? else { return None };
    let OpCode::Load(i4) = code.get(head + 13)? else { return None };
    if *i4 != *i_slot {
        return None;
    }
    if const_i64(code, bc, head + 14)? != 1 {
        return None;
    }
    let OpCode::Add = code.get(head + 15)? else { return None };
    let OpCode::Store(i5) = code.get(head + 16)? else { return None };
    if *i5 != *i_slot {
        return None;
    }
    if jump_ip != head + 17 {
        return None;
    }
    let OpCode::Jump(back) = code.get(jump_ip)? else { return None };
    if *back as usize != head {
        return None;
    }

    Some(DsaLoopRegion {
        exit_ip,
        kind: DsaLoopKind::GraphEdges {
            g_slot: *g_slot,
            i_slot: *i_slot,
            n_slot: *n_slot,
        },
    })
}

fn try_binary_search(
    code: &[OpCode],
    bc: &[BytecodeConst],
    fast_by_fidx: &HashMap<u16, u8>,
    head: usize,
    jump_ip: usize,
) -> Option<DsaLoopRegion> {
    let OpCode::Load(i_slot) = code.get(head)? else { return None };
    let OpCode::Load(k_slot) = code.get(head + 1)? else { return None };
    let OpCode::Lt = code.get(head + 2)? else { return None };
    let OpCode::JumpIfFalse(exit) = code.get(head + 3)? else { return None };
    let exit_ip = *exit as usize;
    let OpCode::Load(arr_slot) = code.get(head + 4)? else { return None };
    let OpCode::Load(i2) = code.get(head + 5)? else { return None };
    if *i2 != *i_slot {
        return None;
    }
    let mul = const_i64(code, bc, head + 6)?;
    let OpCode::Mul = code.get(head + 7)? else { return None };
    let ip_after = head + 8;
    let OpCode::Call { func, argc: 2 } = code.get(ip_after)? else { return None };
    if *fast_by_fidx.get(func)? != 25 {
        return None;
    }
    let OpCode::Const(m1) = code.get(ip_after + 1)? else { return None };
    if const_from_idx(bc, *m1)? != -1 {
        return None;
    }
    let OpCode::Ge = code.get(ip_after + 2)? else { return None };
    let OpCode::JumpIfFalse(skip) = code.get(ip_after + 3)? else { return None };
    let OpCode::Load(_hits) = code.get(ip_after + 4)? else { return None };
    let OpCode::Const(one) = code.get(ip_after + 5)? else { return None };
    if const_from_idx(bc, *one)? != 1 {
        return None;
    }
    let OpCode::Add = code.get(ip_after + 6)? else { return None };
    let OpCode::Store(h2) = code.get(ip_after + 7)? else { return None };
    if *skip as usize != ip_after + 8 {
        return None;
    }
    let OpCode::Load(i3) = code.get(ip_after + 8)? else { return None };
    if *i3 != *i_slot {
        return None;
    }
    if const_i64(code, bc, ip_after + 9)? != 1 {
        return None;
    }
    let OpCode::Add = code.get(ip_after + 10)? else { return None };
    let OpCode::Store(i4) = code.get(ip_after + 11)? else { return None };
    if *i4 != *i_slot {
        return None;
    }
    if jump_ip != ip_after + 12 {
        return None;
    }
    let OpCode::Jump(back) = code.get(jump_ip)? else { return None };
    if *back as usize != head {
        return None;
    }

    Some(DsaLoopRegion {
        exit_ip,
        kind: DsaLoopKind::BinarySearch {
            arr_slot: *arr_slot,
            i_slot: *i_slot,
            k_slot: *k_slot,
            hits_slot: *h2,
            mul,
        },
    })
}

fn parse_linear(code: &[OpCode], bc: &[BytecodeConst], ip: &mut usize) -> Option<(i64, i64)> {
    let OpCode::Load(_) = code.get(*ip)? else { return None };
    *ip += 1;
    if matches!(code.get(*ip), Some(OpCode::Const(_))) {
        let mul = const_i64(code, bc, *ip)?;
        *ip += 1;
        if matches!(code.get(*ip), Some(OpCode::Mul)) {
            *ip += 1;
            if matches!(code.get(*ip), Some(OpCode::Const(_))) {
                let add = const_i64(code, bc, *ip)?;
                *ip += 1;
                if matches!(code.get(*ip), Some(OpCode::Add)) {
                    *ip += 1;
                    return Some((mul, add));
                }
            }
            return Some((mul, 0));
        }
    }
    Some((1, 0))
}

fn const_i64(code: &[OpCode], bc: &[BytecodeConst], ip: usize) -> Option<i64> {
    let OpCode::Const(idx) = code.get(ip)? else { return None };
    const_from_idx(bc, *idx)
}

fn const_from_idx(bc: &[BytecodeConst], idx: u16) -> Option<i64> {
    match bc.get(idx as usize)? {
        BytecodeConst::Int(n) => Some(*n),
        _ => None,
    }
}

#[derive(Default)]
pub struct DsaLoopState {
    fused_heads: HashMap<usize, bool>,
}

impl DsaLoopState {
    pub fn reset(&mut self) {
        self.fused_heads.clear();
    }

    /// True while a registered loop head has not yet been successfully fused.
    pub fn may_fuse(&self, head: usize) -> bool {
        !self.fused_heads.contains_key(&head)
    }

    pub fn mark_fused(&mut self, head: usize) {
        self.fused_heads.insert(head, true);
    }
}

pub fn run_fused(
    region: &DsaLoopRegion,
    locals: &mut [FastVal],
    native_ds: &[DsRef],
    heap: &[niao_runtime::ValueRef],
) -> Option<usize> {
    match &region.kind {
        DsaLoopKind::CountPush {
            ds_slot,
            i_slot,
            limit_slot,
            op,
            mul,
            add,
        } => {
            let i = local_int(locals, *i_slot)?;
            let limit = local_int(locals, *limit_slot)?;
            let ds = local_native(native_ds, locals, heap, *ds_slot)?;
            let end = fast::fuse_count_push(&native_ds[ds], *op, i, limit, *mul, *add)?;
            locals[*i_slot as usize] = FastVal::Int(end);
            Some(region.exit_ip)
        }
        DsaLoopKind::DrainAcc {
            ds_slot,
            acc_slot,
            pop_op,
        } => {
            let acc = local_int(locals, *acc_slot)?;
            let ds = local_native(native_ds, locals, heap, *ds_slot)?;
            let sum = fast::fuse_drain_acc(&native_ds[ds], *pop_op)? + acc;
            locals[*acc_slot as usize] = FastVal::Int(sum);
            Some(region.exit_ip)
        }
        DsaLoopKind::DrainCount {
            ds_slot,
            acc_slot,
            pop_op,
        } => {
            let acc = local_int(locals, *acc_slot)?;
            let ds = local_native(native_ds, locals, heap, *ds_slot)?;
            let count = fast::fuse_drain_count(&native_ds[ds], *pop_op, acc)?;
            locals[*acc_slot as usize] = FastVal::Int(count);
            Some(region.exit_ip)
        }
        DsaLoopKind::SetLookup {
            ds_slot,
            i_slot,
            limit_slot,
            hits_slot,
        } => {
            let i = local_int(locals, *i_slot)?;
            let limit = local_int(locals, *limit_slot)?;
            let hits = local_int(locals, *hits_slot)?;
            let ds = local_native(native_ds, locals, heap, *ds_slot)?;
            let (end_i, end_hits) = fast::fuse_set_lookup(&native_ds[ds], i, limit, hits)?;
            locals[*i_slot as usize] = FastVal::Int(end_i);
            locals[*hits_slot as usize] = FastVal::Int(end_hits);
            Some(region.exit_ip)
        }
        DsaLoopKind::MapLookup {
            ds_slot,
            i_slot,
            limit_slot,
            sum_slot,
        } => {
            let i = local_int(locals, *i_slot)?;
            let limit = local_int(locals, *limit_slot)?;
            let sum = local_int(locals, *sum_slot)?;
            let ds = local_native(native_ds, locals, heap, *ds_slot)?;
            let (end_i, end_sum) = fast::fuse_map_lookup(&native_ds[ds], i, limit, sum)?;
            locals[*i_slot as usize] = FastVal::Int(end_i);
            locals[*sum_slot as usize] = FastVal::Int(end_sum);
            Some(region.exit_ip)
        }
        DsaLoopKind::MapBuild {
            ds_slot,
            i_slot,
            limit_slot,
            mul,
        } => {
            let i = local_int(locals, *i_slot)?;
            let limit = local_int(locals, *limit_slot)?;
            let ds = local_native(native_ds, locals, heap, *ds_slot)?;
            let end = fast::fuse_map_build(&native_ds[ds], i, limit, *mul)?;
            locals[*i_slot as usize] = FastVal::Int(end);
            Some(region.exit_ip)
        }
        DsaLoopKind::GraphEdges {
            g_slot,
            i_slot,
            n_slot,
        } => {
            let i = local_int(locals, *i_slot)?;
            let n = local_int(locals, *n_slot)?;
            let g = local_native(native_ds, locals, heap, *g_slot)?;
            let end = fast::fuse_graph_edges(&native_ds[g], i, n)?;
            locals[*i_slot as usize] = FastVal::Int(end);
            Some(region.exit_ip)
        }
        DsaLoopKind::BinarySearch {
            arr_slot,
            i_slot,
            k_slot,
            hits_slot,
            mul,
        } => {
            let arr_heap = match locals.get(*arr_slot as usize)? {
                FastVal::Heap(h) => *h,
                _ => return None,
            };
            let arr = &heap[arr_heap as usize];
            let i = local_int(locals, *i_slot)?;
            let k = local_int(locals, *k_slot)?;
            let hits = local_int(locals, *hits_slot)?;
            let (end_i, end_hits) = fast::fuse_binary_search(arr, i, k, hits, *mul)?;
            locals[*i_slot as usize] = FastVal::Int(end_i);
            locals[*hits_slot as usize] = FastVal::Int(end_hits);
            Some(region.exit_ip)
        }
        DsaLoopKind::HeapDrainVerify { ds_slot, prev_slot } => {
            let prev = local_int(locals, *prev_slot)?;
            let ds = local_native(native_ds, locals, heap, *ds_slot)?;
            let end_prev = fast::fuse_heap_drain_verify(&native_ds[ds], prev)?;
            locals[*prev_slot as usize] = FastVal::Int(end_prev);
            Some(region.exit_ip)
        }
    }
}

fn local_int(locals: &[FastVal], slot: u16) -> Option<i64> {
    match locals.get(slot as usize)? {
        FastVal::Int(n) => Some(*n),
        _ => None,
    }
}

fn local_native(
    native_ds: &[DsRef],
    locals: &[FastVal],
    heap: &[niao_runtime::ValueRef],
    slot: u16,
) -> Option<usize> {
    match locals.get(slot as usize)? {
        FastVal::Native(i) => Some(*i as usize),
        FastVal::Heap(h) => {
            use niao_runtime::Value;
            let val = heap[*h as usize].borrow();
            if let Value::Native(ds) = &*val {
                native_ds.iter().position(|d| Rc::ptr_eq(d, ds))
            } else {
                None
            }
        }
        _ => None,
    }
}
