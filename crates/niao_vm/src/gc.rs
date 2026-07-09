//! Mark-and-compact garbage collection for the VM object and native arenas.

use super::fast_val::FastVal;
use niao_runtime::{NativeDs, Value, ValueRef};
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::sync::Arc;

/// Collect after this many arena allocations since the last collection.
pub(crate) const GC_INTERVAL: u32 = 8192;
/// Initial combined heap + native slot threshold before a collection is considered.
pub(crate) const GC_THRESHOLD_INITIAL: usize = 16384;
/// Maximum memoization entries per `@memoize` function.
pub(crate) const MEMO_CACHE_CAP: usize = 65536;

/// Bounded FIFO memo cache for recursive functions.
pub(crate) struct MemoCache {
    pub(crate) map: HashMap<i64, FastVal>,
    order: VecDeque<i64>,
}

impl MemoCache {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    pub fn get(&self, key: &i64) -> Option<&FastVal> {
        self.map.get(key)
    }

    pub fn insert(&mut self, key: i64, val: FastVal) {
        if self.map.contains_key(&key) {
            self.map.insert(key, val);
            return;
        }
        while self.map.len() >= MEMO_CACHE_CAP {
            if let Some(old) = self.order.pop_front() {
                self.map.remove(&old);
            } else {
                break;
            }
        }
        self.order.push_back(key);
        self.map.insert(key, val);
    }
}

impl super::Vm {
    pub(crate) fn reset_gc_state(&mut self) {
        self.alloc_since_gc = 0;
        self.gc_threshold = GC_THRESHOLD_INITIAL;
    }

    pub(crate) fn alloc_heap(&mut self, value: ValueRef) -> u32 {
        let idx = self.heap.len() as u32;
        self.heap.push(value);
        self.alloc_since_gc += 1;
        idx
    }

    pub(crate) fn alloc_native(&mut self, ds: Rc<RefCell<NativeDs>>) -> FastVal {
        let idx = self.native_ds.len() as u32;
        Arc::make_mut(&mut self.native_refs).push(Value::Native(Rc::clone(&ds)).ref_cell());
        self.native_ds.push(ds);
        self.alloc_since_gc += 1;
        FastVal::Native(idx)
    }

    pub(crate) fn maybe_collect(&mut self) {
        if self.gc_defer > 0 {
            return;
        }
        if self.alloc_since_gc >= GC_INTERVAL
            || self.heap.len() + self.native_ds.len() >= self.gc_threshold
        {
            if !self.frames.is_empty() || !self.stack.is_empty() {
                self.collect();
            }
        }
    }

    fn collect(&mut self) {
        let total_before = self.heap.len() + self.native_ds.len();

        let mut marked_heap = vec![false; self.heap.len()];
        let mut marked_native = vec![false; self.native_ds.len()];

        for v in &self.stack {
            mark_fastval(*v, &mut marked_heap, &mut marked_native);
        }
        for frame in &self.frames {
            for v in &frame.locals {
                mark_fastval(*v, &mut marked_heap, &mut marked_native);
            }
        }
        for v in &self.constants {
            mark_fastval(*v, &mut marked_heap, &mut marked_native);
        }
        for locals in &self.frame_pool {
            for v in locals {
                mark_fastval(*v, &mut marked_heap, &mut marked_native);
            }
        }
        for cache in &self.memo_caches {
            if let Some(cache) = cache {
                for v in cache.map.values() {
                    mark_fastval(*v, &mut marked_heap, &mut marked_native);
                }
            }
        }

        let heap_map = compact_vec(&mut self.heap, &marked_heap);
        let native_map = compact_vec(&mut self.native_ds, &marked_native);
        compact_vec(Arc::make_mut(&mut self.native_refs), &marked_native);

        remap_fastvals(&mut self.stack, &heap_map, &native_map);
        for frame in &mut self.frames {
            remap_fastvals(&mut frame.locals, &heap_map, &native_map);
        }
        remap_fastvals(&mut self.constants, &heap_map, &native_map);
        for locals in &mut self.frame_pool {
            remap_fastvals(locals, &heap_map, &native_map);
        }
        for cache in &mut self.memo_caches {
            if let Some(cache) = cache {
                for v in cache.map.values_mut() {
                    *v = remap_one(*v, &heap_map, &native_map);
                }
            }
        }

        let total_after = self.heap.len() + self.native_ds.len();
        if total_before > 0 {
            let live_ratio = total_after as f64 / total_before as f64;
            if live_ratio < 0.5 {
                self.gc_threshold = (self.gc_threshold / 2).max(512);
            } else if total_after > self.gc_threshold * 3 / 4 {
                self.gc_threshold = (self.gc_threshold * 2).min(1_048_576);
            }
        }
        self.alloc_since_gc = 0;
    }

    /// Test-only accessor for heap arena size.
    #[cfg(test)]
    pub fn heap_len(&self) -> usize {
        self.heap.len()
    }

    /// Test-only accessor for native arena size.
    #[cfg(test)]
    pub fn native_len(&self) -> usize {
        self.native_ds.len()
    }
}

fn mark_fastval(v: FastVal, marked_heap: &mut [bool], marked_native: &mut [bool]) {
    match v {
        FastVal::Heap(i) => {
            let i = i as usize;
            if i < marked_heap.len() {
                marked_heap[i] = true;
            }
        }
        FastVal::Native(i) => {
            let i = i as usize;
            if i < marked_native.len() {
                marked_native[i] = true;
            }
        }
        _ => {}
    }
}

fn compact_vec<T: Clone>(vec: &mut Vec<T>, marked: &[bool]) -> Vec<Option<u32>> {
    let mut map = vec![None; vec.len()];
    let mut next = 0u32;
    let mut new_vec = Vec::with_capacity(marked.iter().filter(|&&m| m).count());
    for (i, item) in vec.iter().enumerate() {
        if marked.get(i).copied().unwrap_or(false) {
            map[i] = Some(next);
            new_vec.push(item.clone());
            next += 1;
        }
    }
    *vec = new_vec;
    map
}

fn remap_one(v: FastVal, heap_map: &[Option<u32>], native_map: &[Option<u32>]) -> FastVal {
    match v {
        FastVal::Heap(i) => heap_map
            .get(i as usize)
            .and_then(|x| *x)
            .map(FastVal::Heap)
            .unwrap_or(FastVal::Nil),
        FastVal::Native(i) => native_map
            .get(i as usize)
            .and_then(|x| *x)
            .map(FastVal::Native)
            .unwrap_or(FastVal::Nil),
        other => other,
    }
}

fn remap_fastvals(vals: &mut [FastVal], heap_map: &[Option<u32>], native_map: &[Option<u32>]) {
    for v in vals.iter_mut() {
        *v = remap_one(*v, heap_map, native_map);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Vm;
    use niao_bytecode::compile_to_bytecode;
    use niao_parser::parse;
    use std::path::Path;

    #[test]
    fn gc_compacts_unreachable_heap_slots() {
        let src = r#"
fn main() {
    let i = 0
    while i < 100000 {
        let a = [i, i + 1, i + 2]
        i = i + 1
    }
}
"#;
        let program = parse(src).unwrap();
        let bytecode = compile_to_bytecode(&program).unwrap();
        niao_runtime::set_quiet_output(true);
        let mut vm = Vm::new();
        vm.run(&bytecode, Path::new(".")).unwrap();
        niao_runtime::set_quiet_output(false);
        assert!(
            vm.heap_len() < 10_000,
            "heap should stay bounded under GC, got {}",
            vm.heap_len()
        );
    }

    #[test]
    fn memo_cache_evicts_at_cap() {
        let mut cache = MemoCache::new();
        for i in 0..(MEMO_CACHE_CAP + 100) as i64 {
            cache.insert(i, FastVal::Int(i));
        }
        assert!(cache.map.len() <= MEMO_CACHE_CAP);
        assert!(cache.get(&(MEMO_CACHE_CAP as i64 + 99)).is_some());
        assert!(cache.get(&0).is_none());
    }
}
