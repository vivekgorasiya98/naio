//! Native DSA standard library: linked list, stack, queue, deque, heap,
//! set, map, graph, and array algorithms — all implemented in Rust for speed.
//!
//! Every structure is exposed to Neko programs as prefixed free functions
//! (`stack_push`, `heap_pop`, ...) registered as builtins, so they run at
//! native speed on both the bytecode VM and the tree-walking interpreter.

pub use crate::dsa_storage::{
    key_to_value, value_to_key, AnyMap, AnySet, DsKey, MapData, MapData as NativeMapData, SeqData,
    SeqData as NativeSeqData, SetData, SetData as NativeSetData, StackData, StackData as NativeStackData,
};

use crate::{apply_binop, values_equal, NativeFn, NekoResult, RuntimeError, Value, ValueRef};
use neko_ast::{BinOp, Span};
use num_bigint::BigInt;
use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashSet, VecDeque};
use std::rc::Rc;

pub type DsRef = Rc<RefCell<NativeDs>>;

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum HeapNum {
    Int(i64),
    Float(f64),
}

impl HeapNum {
    fn as_f64(self) -> f64 {
        match self {
            HeapNum::Int(n) => n as f64,
            HeapNum::Float(f) => f,
        }
    }

    fn to_value(self) -> Value {
        match self {
            HeapNum::Int(n) => Value::Int(n),
            HeapNum::Float(f) => Value::Float(f),
        }
    }

    fn order(&self, other: &HeapNum) -> Ordering {
        match (self, other) {
            (HeapNum::Int(a), HeapNum::Int(b)) => a.cmp(b),
            (a, b) => a.as_f64().total_cmp(&b.as_f64()),
        }
    }
}

/// Heap entry that flips its ordering for min-heaps, since `BinaryHeap` is a
/// max-heap. All entries in one heap share the same `min` flag.
struct HeapEntry {
    num: HeapNum,
    min: bool,
}

impl Ord for HeapEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        let ord = self.num.order(&other.num);
        if self.min {
            ord.reverse()
        } else {
            ord
        }
    }
}

impl PartialOrd for HeapEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for HeapEntry {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for HeapEntry {}

pub struct HeapDs {
    min: bool,
    /// Int-only fast path (JS-style array heap).
    ints: Vec<i64>,
    /// Promoted when a float is pushed.
    generic: Option<BinaryHeap<HeapEntry>>,
}

impl HeapDs {
    fn len(&self) -> usize {
        if let Some(g) = &self.generic {
            g.len()
        } else {
            self.ints.len()
        }
    }

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn push_int(&mut self, n: i64) {
        if self.generic.is_some() {
            let min = self.min;
            let g = self.generic.as_mut().unwrap();
            g.push(HeapEntry {
                num: HeapNum::Int(n),
                min,
            });
            return;
        }
        crate::int_heap::push(&mut self.ints, n, self.min);
    }

    fn push_num(&mut self, num: HeapNum) {
        match num {
            HeapNum::Int(n) if self.generic.is_none() => self.push_int(n),
            _ => {
                self.promote_generic();
                let min = self.min;
                self.generic.as_mut().unwrap().push(HeapEntry { num, min });
            }
        }
    }

    fn promote_generic(&mut self) {
        if self.generic.is_some() {
            return;
        }
        let min = self.min;
        let mut g = BinaryHeap::new();
        for n in self.ints.drain(..) {
            g.push(HeapEntry {
                num: HeapNum::Int(n),
                min,
            });
        }
        self.generic = Some(g);
    }

    fn pop_num(&mut self) -> Option<HeapNum> {
        if let Some(g) = &mut self.generic {
            return g.pop().map(|e| e.num);
        }
        crate::int_heap::pop(&mut self.ints, self.min).map(HeapNum::Int)
    }

    fn peek_num(&self) -> Option<HeapNum> {
        if let Some(g) = &self.generic {
            return g.peek().map(|e| e.num);
        }
        self.ints.first().copied().map(HeapNum::Int)
    }

    fn drain_count_int(&mut self) -> Option<i64> {
        if self.generic.is_some() {
            return None;
        }
        let n = self.ints.len() as i64;
        self.ints.clear();
        Some(n)
    }
}

pub struct GraphDs {
    pub n: usize,
    pub directed: bool,
    pub edges: usize,
    pub adj: Vec<Vec<(u32, i64)>>,
}

impl GraphDs {
    pub fn edge_list_f32(&self) -> Vec<(u32, u32, f32)> {
        let mut out = Vec::new();
        for (u, nbrs) in self.adj.iter().enumerate() {
            for &(v, w) in nbrs {
                out.push((u as u32, v, w as f32));
            }
        }
        out
    }
}

pub enum NativeDs {
    List(SeqData),
    Stack(StackData),
    Queue(SeqData),
    Deque(SeqData),
    Heap(HeapDs),
    Set(SetData),
    Map(MapData),
    Graph(GraphDs),
}

impl NativeDs {
    pub fn kind_name(&self) -> &'static str {
        match self {
            NativeDs::List(_) => "list",
            NativeDs::Stack(_) => "stack",
            NativeDs::Queue(_) => "queue",
            NativeDs::Deque(_) => "deque",
            NativeDs::Heap(_) => "heap",
            NativeDs::Set(_) => "set",
            NativeDs::Map(_) => "map",
            NativeDs::Graph(_) => "graph",
        }
    }

    pub fn len(&self) -> usize {
        match self {
            NativeDs::List(v) => v.len(),
            NativeDs::Stack(v) => v.len(),
            NativeDs::Queue(v) => v.len(),
            NativeDs::Deque(v) => v.len(),
            NativeDs::Heap(h) => h.len(),
            NativeDs::Set(s) => s.len(),
            NativeDs::Map(m) => m.len(),
            NativeDs::Graph(g) => g.n,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn display(&self) -> String {
        const CAP: usize = 32;
        match self {
            NativeDs::List(v) => {
                let refs = v.iter_refs();
                seq_display("list", refs.iter(), refs.len(), CAP)
            }
            NativeDs::Stack(v) => {
                let refs = v.iter_refs();
                seq_display("stack", refs.iter(), refs.len(), CAP)
            }
            NativeDs::Queue(v) => {
                let refs = v.iter_refs();
                seq_display("queue", refs.iter(), refs.len(), CAP)
            }
            NativeDs::Deque(v) => {
                let refs = v.iter_refs();
                seq_display("deque", refs.iter(), refs.len(), CAP)
            }
            NativeDs::Heap(h) => {
                let name = if h.min { "min_heap" } else { "max_heap" };
                if h.len() > CAP {
                    return format!("{name}[{} items]", h.len());
                }
                let mut nums: Vec<HeapNum> = Vec::with_capacity(h.len());
                if let Some(g) = &h.generic {
                    nums.extend(g.iter().map(|e| e.num));
                } else {
                    nums.extend(h.ints.iter().copied().map(HeapNum::Int));
                }
                nums.sort_by(|a, b| {
                    let ord = a.order(b);
                    if h.min {
                        ord
                    } else {
                        ord.reverse()
                    }
                });
                let parts: Vec<String> = nums.iter().map(|n| n.to_value().to_string()).collect();
                format!("{name}[{}]", parts.join(", "))
            }
            NativeDs::Set(s) => {
                if s.len() > CAP {
                    return format!("set{{{} items}}", s.len());
                }
                let parts: Vec<String> = s
                    .iter_values()
                    .iter()
                    .map(|v| v.borrow().to_string())
                    .collect();
                format!("set{{{}}}", parts.join(", "))
            }
            NativeDs::Map(m) => {
                if m.len() > CAP {
                    return format!("map{{{} items}}", m.len());
                }
                let (keys, vals) = m.keys_values();
                let parts: Vec<String> = keys
                    .iter()
                    .zip(vals.iter())
                    .map(|(k, v)| format!("{}: {}", k.borrow().to_string(), v.borrow().to_string()))
                    .collect();
                format!("map{{{}}}", parts.join(", "))
            }
            NativeDs::Graph(g) => {
                format!("graph(nodes={}, edges={})", g.n, g.edges)
            }
        }
    }
}

fn seq_display<'a>(
    name: &str,
    items: impl Iterator<Item = &'a ValueRef>,
    len: usize,
    cap: usize,
) -> String {
    if len > cap {
        return format!("{name}[{len} items]");
    }
    let parts: Vec<String> = items.map(|v| v.borrow().to_string()).collect();
    format!("{name}[{}]", parts.join(", "))
}

// ---------------------------------------------------------------------------
// Argument helpers
// ---------------------------------------------------------------------------

fn type_err(span: Span, msg: impl Into<String>) -> RuntimeError {
    RuntimeError::TypeError {
        message: msg.into(),
        line: span.line,
        col: span.col,
    }
}

fn arity(args: &[ValueRef], n: usize, name: &str, span: Span) -> NekoResult<()> {
    if args.len() != n {
        return Err(RuntimeError::at(
            span,
            1100,
            format!("{name}() expects {n} argument(s), got {}", args.len()),
        ));
    }
    Ok(())
}

fn ds_arg(args: &[ValueRef], idx: usize, name: &str, expected: &str, span: Span) -> NekoResult<DsRef> {
    match &*args[idx].borrow() {
        Value::Native(ds) => Ok(Rc::clone(ds)),
        other => Err(type_err(
            span,
            format!(
                "{name}() expects a {expected} as argument {}, got {}",
                idx + 1,
                other.type_name()
            ),
        )),
    }
}

fn int_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> NekoResult<i64> {
    match &*args[idx].borrow() {
        Value::Int(n) => Ok(*n),
        other => Err(type_err(
            span,
            format!(
                "{name}() expects an int as argument {}, got {}",
                idx + 1,
                other.type_name()
            ),
        )),
    }
}

fn size_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> NekoResult<usize> {
    let n = int_arg(args, idx, name, span)?;
    if n < 0 {
        return Err(type_err(
            span,
            format!("{name}() expects a non-negative int as argument {}", idx + 1),
        ));
    }
    Ok(n as usize)
}

fn key_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> NekoResult<DsKey> {
    value_to_key(&args[idx].borrow()).ok_or_else(|| {
        type_err(
            span,
            format!("{name}() keys must be int, float, string, or bool"),
        )
    })
}

fn num_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> NekoResult<HeapNum> {
    match &*args[idx].borrow() {
        Value::Int(n) => Ok(HeapNum::Int(*n)),
        Value::Float(f) => Ok(HeapNum::Float(*f)),
        other => Err(type_err(
            span,
            format!(
                "{name}() expects a number as argument {}, got {}",
                idx + 1,
                other.type_name()
            ),
        )),
    }
}

macro_rules! with_ds {
    ($fn_name:ident, $variant:ident, $ty:ty, $kind:expr) => {
        fn $fn_name<T>(
            args: &[ValueRef],
            idx: usize,
            name: &str,
            span: Span,
            f: impl FnOnce(&mut $ty) -> NekoResult<T>,
        ) -> NekoResult<T> {
            let rc = ds_arg(args, idx, name, $kind, span)?;
            let mut b = rc.borrow_mut();
            match &mut *b {
                NativeDs::$variant(inner) => f(inner),
                other => Err(type_err(
                    span,
                    format!("{name}() expects a {}, got {}", $kind, other.kind_name()),
                )),
            }
        }
    };
}

with_ds!(with_list, List, SeqData, "list");
with_ds!(with_stack, Stack, StackData, "stack");
with_ds!(with_queue, Queue, SeqData, "queue");
with_ds!(with_deque, Deque, SeqData, "deque");
with_ds!(with_heap, Heap, HeapDs, "heap");
with_ds!(with_set, Set, SetData, "set");
with_ds!(with_map, Map, MapData, "map");
with_ds!(with_graph, Graph, GraphDs, "graph");

fn new_ds(ds: NativeDs) -> ValueRef {
    Value::Native(Rc::new(RefCell::new(ds))).ref_cell()
}

fn nil() -> ValueRef {
    Value::Nil.ref_cell()
}

fn opt_val(v: Option<ValueRef>) -> ValueRef {
    v.unwrap_or_else(nil)
}

fn oob(span: Span, name: &str, idx: i64, len: usize) -> RuntimeError {
    RuntimeError::at(
        span,
        1101,
        format!("{name}() index {idx} out of bounds (len {len})"),
    )
}

/// Snapshot the elements of an array argument as `ValueRef`s.
fn array_elems(args: &[ValueRef], idx: usize, name: &str, span: Span) -> NekoResult<Vec<ValueRef>> {
    match &*args[idx].borrow() {
        Value::IntArray(v) => Ok(v.iter().map(|n| Value::Int(*n).ref_cell()).collect()),
        Value::Array(items) => Ok(items.iter().map(Rc::clone).collect()),
        other => Err(type_err(
            span,
            format!(
                "{name}() expects an array as argument {}, got {}",
                idx + 1,
                other.type_name()
            ),
        )),
    }
}

// ---------------------------------------------------------------------------
// Linked list (VecDeque-backed: O(1) at both ends, cache friendly)
// ---------------------------------------------------------------------------

fn list_new(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 0, "list_new", span)?;
    Ok(new_ds(NativeDs::List(SeqData::default())))
}

fn list_from_array(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "list_from_array", span)?;
    let elems = array_elems(args, 0, "list_from_array", span)?;
    Ok(new_ds(NativeDs::List(SeqData::from_elems(elems))))
}

fn list_push_front(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "list_push_front", span)?;
    let v = Rc::clone(&args[1]);
    with_list(args, 0, "list_push_front", span, |l| {
        l.push_front(v);
        Ok(())
    })?;
    Ok(nil())
}

fn list_push_back(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "list_push_back", span)?;
    let v = Rc::clone(&args[1]);
    with_list(args, 0, "list_push_back", span, |l| {
        l.push_back(v);
        Ok(())
    })?;
    Ok(nil())
}

fn list_pop_front(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "list_pop_front", span)?;
    with_list(args, 0, "list_pop_front", span, |l| Ok(opt_val(l.pop_front())))
}

fn list_pop_back(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "list_pop_back", span)?;
    with_list(args, 0, "list_pop_back", span, |l| Ok(opt_val(l.pop_back())))
}

fn list_front(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "list_front", span)?;
    with_list(args, 0, "list_front", span, |l| Ok(opt_val(l.front())))
}

fn list_back(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "list_back", span)?;
    with_list(args, 0, "list_back", span, |l| Ok(opt_val(l.back())))
}

fn list_get(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "list_get", span)?;
    let i = int_arg(args, 1, "list_get", span)?;
    with_list(args, 0, "list_get", span, |l| {
        if i < 0 || i as usize >= l.len() {
            return Err(oob(span, "list_get", i, l.len()));
        }
        Ok(l.get(i as usize).unwrap())
    })
}

fn list_set(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 3, "list_set", span)?;
    let i = int_arg(args, 1, "list_set", span)?;
    let v = Rc::clone(&args[2]);
    with_list(args, 0, "list_set", span, |l| {
        if i < 0 || i as usize >= l.len() {
            return Err(oob(span, "list_set", i, l.len()));
        }
        l.set(i as usize, v);
        Ok(())
    })?;
    Ok(nil())
}

fn list_insert(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 3, "list_insert", span)?;
    let i = int_arg(args, 1, "list_insert", span)?;
    let v = Rc::clone(&args[2]);
    with_list(args, 0, "list_insert", span, |l| {
        if i < 0 || i as usize > l.len() {
            return Err(oob(span, "list_insert", i, l.len()));
        }
        l.insert(i as usize, v);
        Ok(())
    })?;
    Ok(nil())
}

fn list_remove(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "list_remove", span)?;
    let i = int_arg(args, 1, "list_remove", span)?;
    with_list(args, 0, "list_remove", span, |l| {
        if i < 0 || i as usize >= l.len() {
            return Err(oob(span, "list_remove", i, l.len()));
        }
        Ok(l.remove(i as usize))
    })
}

fn list_contains(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "list_contains", span)?;
    let target = Rc::clone(&args[1]);
    with_list(args, 0, "list_contains", span, |l| {
        let refs = l.iter_refs();
        let found = refs.iter().any(|v| values_equal(&v.borrow(), &target.borrow()));
        Ok(Value::Bool(found).ref_cell())
    })
}

fn list_index_of(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "list_index_of", span)?;
    let target = Rc::clone(&args[1]);
    with_list(args, 0, "list_index_of", span, |l| {
        let refs = l.iter_refs();
        let idx = refs
            .iter()
            .position(|v| values_equal(&v.borrow(), &target.borrow()))
            .map(|i| i as i64)
            .unwrap_or(-1);
        Ok(Value::Int(idx).ref_cell())
    })
}

fn list_reverse(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "list_reverse", span)?;
    with_list(args, 0, "list_reverse", span, |l| {
        l.make_contiguous_reverse();
        Ok(())
    })?;
    Ok(nil())
}

fn list_to_array(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "list_to_array", span)?;
    with_list(args, 0, "list_to_array", span, |l| {
        Ok(Value::Array(l.iter_refs()).ref_cell())
    })
}

fn list_clear(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "list_clear", span)?;
    with_list(args, 0, "list_clear", span, |l| {
        l.clear();
        Ok(())
    })?;
    Ok(nil())
}

fn list_is_empty(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "list_is_empty", span)?;
    with_list(args, 0, "list_is_empty", span, |l| {
        Ok(Value::Bool(l.is_empty()).ref_cell())
    })
}

// ---------------------------------------------------------------------------
// Stack (Vec-backed LIFO)
// ---------------------------------------------------------------------------

fn stack_new(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 0, "stack_new", span)?;
    Ok(new_ds(NativeDs::Stack(StackData::default())))
}

fn stack_push(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "stack_push", span)?;
    let v = Rc::clone(&args[1]);
    with_stack(args, 0, "stack_push", span, |s| {
        s.push(v);
        Ok(())
    })?;
    Ok(nil())
}

fn stack_pop(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "stack_pop", span)?;
    with_stack(args, 0, "stack_pop", span, |s| Ok(opt_val(s.pop())))
}

fn stack_peek(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "stack_peek", span)?;
    with_stack(args, 0, "stack_peek", span, |s| Ok(opt_val(s.last())))
}

fn stack_is_empty(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "stack_is_empty", span)?;
    with_stack(args, 0, "stack_is_empty", span, |s| {
        Ok(Value::Bool(s.is_empty()).ref_cell())
    })
}

fn stack_clear(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "stack_clear", span)?;
    with_stack(args, 0, "stack_clear", span, |s| {
        s.clear();
        Ok(())
    })?;
    Ok(nil())
}

fn stack_to_array(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "stack_to_array", span)?;
    with_stack(args, 0, "stack_to_array", span, |s| {
        Ok(Value::Array(s.iter_refs()).ref_cell())
    })
}

// ---------------------------------------------------------------------------
// Queue (FIFO)
// ---------------------------------------------------------------------------

fn queue_new(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 0, "queue_new", span)?;
    Ok(new_ds(NativeDs::Queue(SeqData::default())))
}

fn queue_push(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "queue_push", span)?;
    let v = Rc::clone(&args[1]);
    with_queue(args, 0, "queue_push", span, |q| {
        q.push_back(v);
        Ok(())
    })?;
    Ok(nil())
}

fn queue_pop(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "queue_pop", span)?;
    with_queue(args, 0, "queue_pop", span, |q| Ok(opt_val(q.pop_front())))
}

fn queue_front(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "queue_front", span)?;
    with_queue(args, 0, "queue_front", span, |q| Ok(opt_val(q.front())))
}

fn queue_back(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "queue_back", span)?;
    with_queue(args, 0, "queue_back", span, |q| Ok(opt_val(q.back())))
}

fn queue_is_empty(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "queue_is_empty", span)?;
    with_queue(args, 0, "queue_is_empty", span, |q| {
        Ok(Value::Bool(q.is_empty()).ref_cell())
    })
}

fn queue_clear(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "queue_clear", span)?;
    with_queue(args, 0, "queue_clear", span, |q| {
        q.clear();
        Ok(())
    })?;
    Ok(nil())
}

fn queue_to_array(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "queue_to_array", span)?;
    with_queue(args, 0, "queue_to_array", span, |q| {
        Ok(Value::Array(q.iter_refs()).ref_cell())
    })
}

// ---------------------------------------------------------------------------
// Deque (double-ended queue)
// ---------------------------------------------------------------------------

fn deque_new(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 0, "deque_new", span)?;
    Ok(new_ds(NativeDs::Deque(SeqData::default())))
}

fn deque_push_front(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "deque_push_front", span)?;
    let v = Rc::clone(&args[1]);
    with_deque(args, 0, "deque_push_front", span, |d| {
        d.push_front(v);
        Ok(())
    })?;
    Ok(nil())
}

fn deque_push_back(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "deque_push_back", span)?;
    let v = Rc::clone(&args[1]);
    with_deque(args, 0, "deque_push_back", span, |d| {
        d.push_back(v);
        Ok(())
    })?;
    Ok(nil())
}

fn deque_pop_front(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "deque_pop_front", span)?;
    with_deque(args, 0, "deque_pop_front", span, |d| Ok(opt_val(d.pop_front())))
}

fn deque_pop_back(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "deque_pop_back", span)?;
    with_deque(args, 0, "deque_pop_back", span, |d| Ok(opt_val(d.pop_back())))
}

fn deque_front(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "deque_front", span)?;
    with_deque(args, 0, "deque_front", span, |d| Ok(opt_val(d.front())))
}

fn deque_back(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "deque_back", span)?;
    with_deque(args, 0, "deque_back", span, |d| Ok(opt_val(d.back())))
}

fn deque_is_empty(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "deque_is_empty", span)?;
    with_deque(args, 0, "deque_is_empty", span, |d| {
        Ok(Value::Bool(d.is_empty()).ref_cell())
    })
}

fn deque_clear(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "deque_clear", span)?;
    with_deque(args, 0, "deque_clear", span, |d| {
        d.clear();
        Ok(())
    })?;
    Ok(nil())
}

fn deque_to_array(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "deque_to_array", span)?;
    with_deque(args, 0, "deque_to_array", span, |d| {
        Ok(Value::Array(d.iter_refs()).ref_cell())
    })
}

// ---------------------------------------------------------------------------
// Heap / priority queue (binary heap)
// ---------------------------------------------------------------------------

fn heap_new_min(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 0, "heap_new_min", span)?;
    Ok(new_ds(NativeDs::Heap(HeapDs {
        min: true,
        ints: Vec::new(),
        generic: None,
    })))
}

fn heap_new_max(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 0, "heap_new_max", span)?;
    Ok(new_ds(NativeDs::Heap(HeapDs {
        min: false,
        ints: Vec::new(),
        generic: None,
    })))
}

fn heap_push(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "heap_push", span)?;
    let num = num_arg(args, 1, "heap_push", span)?;
    with_heap(args, 0, "heap_push", span, |h| {
        h.push_num(num);
        Ok(())
    })?;
    Ok(nil())
}

fn heap_pop(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "heap_pop", span)?;
    with_heap(args, 0, "heap_pop", span, |h| {
        Ok(h
            .pop_num()
            .map(|e| e.to_value().ref_cell())
            .unwrap_or_else(nil))
    })
}

fn heap_peek(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "heap_peek", span)?;
    with_heap(args, 0, "heap_peek", span, |h| {
        Ok(h
            .peek_num()
            .map(|e| e.to_value().ref_cell())
            .unwrap_or_else(nil))
    })
}

fn heap_is_empty(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "heap_is_empty", span)?;
    with_heap(args, 0, "heap_is_empty", span, |h| {
        Ok(Value::Bool(h.is_empty()).ref_cell())
    })
}

// ---------------------------------------------------------------------------
// Hash set (insertion-ordered for deterministic output)
// ---------------------------------------------------------------------------

fn set_new(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 0, "set_new", span)?;
    Ok(new_ds(NativeDs::Set(SetData::default())))
}

fn set_from_array(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "set_from_array", span)?;
    let elems = array_elems(args, 0, "set_from_array", span)?;
    Ok(new_ds(NativeDs::Set(SetData::from_elems(&elems))))
}

fn set_add(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "set_add", span)?;
    let key = key_arg(args, 1, "set_add", span)?;
    with_set(args, 0, "set_add", span, |s| {
        Ok(Value::Bool(s.insert_key(key)).ref_cell())
    })
}

fn set_remove(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "set_remove", span)?;
    let key = key_arg(args, 1, "set_remove", span)?;
    with_set(args, 0, "set_remove", span, |s| {
        Ok(Value::Bool(s.shift_remove(&key)).ref_cell())
    })
}

fn set_contains(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "set_contains", span)?;
    let key = key_arg(args, 1, "set_contains", span)?;
    with_set(args, 0, "set_contains", span, |s| {
        Ok(Value::Bool(s.contains_key(&key)).ref_cell())
    })
}

fn set_is_empty(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "set_is_empty", span)?;
    with_set(args, 0, "set_is_empty", span, |s| {
        Ok(Value::Bool(s.is_empty()).ref_cell())
    })
}

fn set_clear(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "set_clear", span)?;
    with_set(args, 0, "set_clear", span, |s| {
        s.clear();
        Ok(())
    })?;
    Ok(nil())
}

fn set_to_array(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "set_to_array", span)?;
    with_set(args, 0, "set_to_array", span, |s| {
        Ok(Value::Array(s.iter_values()).ref_cell())
    })
}

/// Fetch both set arguments for binary set operations, handling aliasing
/// (e.g. `set_union(s, s)`) without a double borrow.
fn set_pair(
    args: &[ValueRef],
    name: &str,
    span: Span,
) -> NekoResult<(AnySet, AnySet)> {
    let a = ds_arg(args, 0, name, "set", span)?;
    let b = ds_arg(args, 1, name, "set", span)?;
    let sa = match &*a.borrow() {
        NativeDs::Set(s) => s.clone_keys(),
        other => {
            return Err(type_err(
                span,
                format!("{name}() expects a set, got {}", other.kind_name()),
            ))
        }
    };
    let sb = if Rc::ptr_eq(&a, &b) {
        sa.clone()
    } else {
        match &*b.borrow() {
            NativeDs::Set(s) => s.clone_keys(),
            other => {
                return Err(type_err(
                    span,
                    format!("{name}() expects a set, got {}", other.kind_name()),
                ))
            }
        }
    };
    Ok((sa, sb))
}

fn set_union(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "set_union", span)?;
    let (mut a, b) = set_pair(args, "set_union", span)?;
    for k in b {
        a.insert(k);
    }
    Ok(new_ds(NativeDs::Set(SetData::from_any(a))))
}

fn set_intersect(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "set_intersect", span)?;
    let (a, b) = set_pair(args, "set_intersect", span)?;
    let out: AnySet = a.into_iter().filter(|k| b.contains(k)).collect();
    Ok(new_ds(NativeDs::Set(SetData::from_any(out))))
}

fn set_diff(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "set_diff", span)?;
    let (a, b) = set_pair(args, "set_diff", span)?;
    let out: AnySet = a.into_iter().filter(|k| !b.contains(k)).collect();
    Ok(new_ds(NativeDs::Set(SetData::from_any(out))))
}

// ---------------------------------------------------------------------------
// Hash map (insertion-ordered, any hashable key)
// ---------------------------------------------------------------------------

fn map_new(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 0, "map_new", span)?;
    Ok(new_ds(NativeDs::Map(MapData::default())))
}

fn map_set(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 3, "map_set", span)?;
    let key = key_arg(args, 1, "map_set", span)?;
    let v = Rc::clone(&args[2]);
    with_map(args, 0, "map_set", span, |m| {
        m.insert(key, v);
        Ok(())
    })?;
    Ok(nil())
}

fn map_get(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "map_get", span)?;
    let key = key_arg(args, 1, "map_get", span)?;
    with_map(args, 0, "map_get", span, |m| Ok(opt_val(m.get(&key))))
}

fn map_has(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "map_has", span)?;
    let key = key_arg(args, 1, "map_has", span)?;
    with_map(args, 0, "map_has", span, |m| {
        Ok(Value::Bool(m.contains_key(&key)).ref_cell())
    })
}

fn map_remove(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "map_remove", span)?;
    let key = key_arg(args, 1, "map_remove", span)?;
    with_map(args, 0, "map_remove", span, |m| {
        Ok(Value::Bool(m.shift_remove(&key).is_some()).ref_cell())
    })
}

fn map_keys(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "map_keys", span)?;
    with_map(args, 0, "map_keys", span, |m| {
        let (keys, _) = m.keys_values();
        Ok(Value::Array(keys).ref_cell())
    })
}

fn map_values(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "map_values", span)?;
    with_map(args, 0, "map_values", span, |m| {
        let (_, vals) = m.keys_values();
        Ok(Value::Array(vals).ref_cell())
    })
}

fn map_is_empty(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "map_is_empty", span)?;
    with_map(args, 0, "map_is_empty", span, |m| {
        Ok(Value::Bool(m.is_empty()).ref_cell())
    })
}

fn map_clear(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "map_clear", span)?;
    with_map(args, 0, "map_clear", span, |m| {
        m.clear();
        Ok(())
    })?;
    Ok(nil())
}

// ---------------------------------------------------------------------------
// Graph (adjacency list) + BFS / DFS / Dijkstra / topological sort
// ---------------------------------------------------------------------------

fn make_graph(n: usize, directed: bool) -> ValueRef {
    new_ds(NativeDs::Graph(GraphDs {
        n,
        directed,
        edges: 0,
        adj: vec![Vec::new(); n],
    }))
}

fn graph_new(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "graph_new", span)?;
    let n = size_arg(args, 0, "graph_new", span)?;
    Ok(make_graph(n, false))
}

fn graph_new_directed(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "graph_new_directed", span)?;
    let n = size_arg(args, 0, "graph_new_directed", span)?;
    Ok(make_graph(n, true))
}

fn check_node(g: &GraphDs, node: i64, name: &str, span: Span) -> NekoResult<usize> {
    if node < 0 || node as usize >= g.n {
        return Err(RuntimeError::at(
            span,
            1102,
            format!("{name}(): node {node} out of range (graph has {} nodes)", g.n),
        ));
    }
    Ok(node as usize)
}

fn add_edge_impl(args: &[ValueRef], name: &str, span: Span, w: i64) -> NekoResult<ValueRef> {
    let u = int_arg(args, 1, name, span)?;
    let v = int_arg(args, 2, name, span)?;
    with_graph(args, 0, name, span, |g| {
        let u = check_node(g, u, name, span)?;
        let v = check_node(g, v, name, span)?;
        g.adj[u].push((v as u32, w));
        if !g.directed && u != v {
            g.adj[v].push((u as u32, w));
        }
        g.edges += 1;
        Ok(())
    })?;
    Ok(nil())
}

fn graph_add_edge(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 3, "graph_add_edge", span)?;
    add_edge_impl(args, "graph_add_edge", span, 1)
}

fn graph_add_edge_w(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 4, "graph_add_edge_w", span)?;
    let w = int_arg(args, 3, "graph_add_edge_w", span)?;
    add_edge_impl(args, "graph_add_edge_w", span, w)
}

fn graph_neighbors(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "graph_neighbors", span)?;
    let u = int_arg(args, 1, "graph_neighbors", span)?;
    with_graph(args, 0, "graph_neighbors", span, |g| {
        let u = check_node(g, u, "graph_neighbors", span)?;
        let out: Vec<i64> = g.adj[u].iter().map(|(v, _)| *v as i64).collect();
        Ok(Value::IntArray(out).ref_cell())
    })
}

fn graph_node_count(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "graph_node_count", span)?;
    with_graph(args, 0, "graph_node_count", span, |g| {
        Ok(Value::Int(g.n as i64).ref_cell())
    })
}

fn graph_edge_count(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "graph_edge_count", span)?;
    with_graph(args, 0, "graph_edge_count", span, |g| {
        Ok(Value::Int(g.edges as i64).ref_cell())
    })
}

fn graph_bfs(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "graph_bfs", span)?;
    let src = int_arg(args, 1, "graph_bfs", span)?;
    with_graph(args, 0, "graph_bfs", span, |g| {
        let src = check_node(g, src, "graph_bfs", span)?;
        let mut visited = vec![false; g.n];
        let mut order = Vec::with_capacity(g.n);
        let mut queue = VecDeque::new();
        visited[src] = true;
        queue.push_back(src as u32);
        while let Some(u) = queue.pop_front() {
            order.push(u as i64);
            for &(v, _) in &g.adj[u as usize] {
                if !visited[v as usize] {
                    visited[v as usize] = true;
                    queue.push_back(v);
                }
            }
        }
        Ok(Value::IntArray(order).ref_cell())
    })
}

fn graph_dfs(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "graph_dfs", span)?;
    let src = int_arg(args, 1, "graph_dfs", span)?;
    with_graph(args, 0, "graph_dfs", span, |g| {
        let src = check_node(g, src, "graph_dfs", span)?;
        let mut visited = vec![false; g.n];
        let mut order = Vec::with_capacity(g.n);
        let mut stack = vec![src as u32];
        while let Some(u) = stack.pop() {
            if visited[u as usize] {
                continue;
            }
            visited[u as usize] = true;
            order.push(u as i64);
            // Reverse push so the first-listed neighbor is visited first.
            for &(v, _) in g.adj[u as usize].iter().rev() {
                if !visited[v as usize] {
                    stack.push(v);
                }
            }
        }
        Ok(Value::IntArray(order).ref_cell())
    })
}

fn graph_dijkstra(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "graph_dijkstra", span)?;
    let src = int_arg(args, 1, "graph_dijkstra", span)?;
    with_graph(args, 0, "graph_dijkstra", span, |g| {
        let src = check_node(g, src, "graph_dijkstra", span)?;
        const INF: i64 = i64::MAX;
        let mut dist = vec![INF; g.n];
        dist[src] = 0;
        // Min-heap via Reverse ordering on (dist, node).
        let mut heap: BinaryHeap<std::cmp::Reverse<(i64, u32)>> = BinaryHeap::new();
        heap.push(std::cmp::Reverse((0, src as u32)));
        while let Some(std::cmp::Reverse((d, u))) = heap.pop() {
            if d > dist[u as usize] {
                continue;
            }
            for &(v, w) in &g.adj[u as usize] {
                if w < 0 {
                    return Err(type_err(
                        span,
                        "graph_dijkstra() requires non-negative edge weights",
                    ));
                }
                let nd = d.saturating_add(w);
                if nd < dist[v as usize] {
                    dist[v as usize] = nd;
                    heap.push(std::cmp::Reverse((nd, v)));
                }
            }
        }
        let out: Vec<i64> = dist.into_iter().map(|d| if d == INF { -1 } else { d }).collect();
        Ok(Value::IntArray(out).ref_cell())
    })
}

fn graph_edge_list(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "graph_edge_list", span)?;
    with_graph(args, 0, "graph_edge_list", span, |g| {
        let edges = g.edge_list_f32();
        let rows: Vec<i64> = edges.iter().map(|(r, _, _)| *r as i64).collect();
        let cols: Vec<i64> = edges.iter().map(|(_, c, _)| *c as i64).collect();
        let vals: Vec<f64> = edges.iter().map(|(_, _, v)| *v as f64).collect();
        let mut map = std::collections::HashMap::new();
        map.insert("rows".to_string(), Value::IntArray(rows).ref_cell());
        map.insert("cols".to_string(), Value::IntArray(cols).ref_cell());
        map.insert("weights".to_string(), Value::FloatArray(vals).ref_cell());
        map.insert("n".to_string(), Value::Int(g.n as i64).ref_cell());
        Ok(Value::Object(map).ref_cell())
    })
}

fn graph_adjacency_coo(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    graph_edge_list(args, span)
}

fn graph_topo_sort(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "graph_topo_sort", span)?;
    with_graph(args, 0, "graph_topo_sort", span, |g| {
        if !g.directed {
            return Err(type_err(
                span,
                "graph_topo_sort() requires a directed graph (use graph_new_directed)",
            ));
        }
        // Kahn's algorithm.
        let mut indeg = vec![0usize; g.n];
        for u in 0..g.n {
            for &(v, _) in &g.adj[u] {
                indeg[v as usize] += 1;
            }
        }
        let mut queue: VecDeque<u32> = (0..g.n as u32).filter(|&u| indeg[u as usize] == 0).collect();
        let mut order = Vec::with_capacity(g.n);
        while let Some(u) = queue.pop_front() {
            order.push(u as i64);
            for &(v, _) in &g.adj[u as usize] {
                indeg[v as usize] -= 1;
                if indeg[v as usize] == 0 {
                    queue.push_back(v);
                }
            }
        }
        if order.len() < g.n {
            return Ok(nil()); // cycle
        }
        Ok(Value::IntArray(order).ref_cell())
    })
}

// ---------------------------------------------------------------------------
// Array algorithms (work on IntArray and Array)
// ---------------------------------------------------------------------------

fn array_type_err(span: Span, name: &str, other: &Value) -> RuntimeError {
    type_err(
        span,
        format!("{name}() expects an array, got {}", other.type_name()),
    )
}

fn push(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "push", span)?;
    let elem_int: Option<i64> = if Rc::ptr_eq(&args[0], &args[1]) {
        None
    } else {
        match &*args[1].borrow() {
            Value::Int(n) => Some(*n),
            _ => None,
        }
    };
    let mut cell = args[0].borrow_mut();
    let converted = match &mut *cell {
        Value::IntArray(v) => {
            if let Some(n) = elem_int {
                v.push(n);
                None
            } else {
                // Non-int element: promote to a generic array.
                let mut items: Vec<ValueRef> =
                    v.drain(..).map(|n| Value::Int(n).ref_cell()).collect();
                items.push(Rc::clone(&args[1]));
                Some(items)
            }
        }
        Value::Array(items) => {
            items.push(Rc::clone(&args[1]));
            None
        }
        other => return Err(array_type_err(span, "push", other)),
    };
    if let Some(items) = converted {
        *cell = Value::Array(items);
    }
    Ok(nil())
}

fn pop(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "pop", span)?;
    let mut cell = args[0].borrow_mut();
    match &mut *cell {
        Value::IntArray(v) => Ok(v.pop().map(Value::Int).unwrap_or(Value::Nil).ref_cell()),
        Value::Array(items) => Ok(items.pop().unwrap_or_else(nil)),
        other => Err(array_type_err(span, "pop", other)),
    }
}

fn cmp_vals(a: &Value, b: &Value, name: &str, span: Span) -> NekoResult<Ordering> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(x.cmp(y)),
        (Value::Int(x), Value::Float(y)) => Ok((*x as f64).total_cmp(y)),
        (Value::Float(x), Value::Int(y)) => Ok(x.total_cmp(&(*y as f64))),
        (Value::Float(x), Value::Float(y)) => Ok(x.total_cmp(y)),
        (Value::String(x), Value::String(y)) => Ok(x.cmp(y)),
        _ => Err(type_err(
            span,
            format!(
                "{name}(): elements must be all numbers or all strings (got {} and {})",
                a.type_name(),
                b.type_name()
            ),
        )),
    }
}

fn sort_impl(args: &[ValueRef], name: &str, span: Span, desc: bool) -> NekoResult<ValueRef> {
    arity(args, 1, name, span)?;
    let mut cell = args[0].borrow_mut();
    match &mut *cell {
        Value::IntArray(v) => {
            v.sort_unstable();
            if desc {
                v.reverse();
            }
        }
        Value::Array(items) => {
            let mut vals: Vec<Value> = items.iter().map(|r| r.borrow().clone()).collect();
            if vals.iter().all(|v| matches!(v, Value::Int(_))) {
                // Fast path: unbox, sort ints, rebox.
                let mut ints: Vec<i64> = vals
                    .iter()
                    .map(|v| match v {
                        Value::Int(n) => *n,
                        _ => unreachable!(),
                    })
                    .collect();
                ints.sort_unstable();
                if desc {
                    ints.reverse();
                }
                for (slot, n) in items.iter().zip(ints) {
                    *slot.borrow_mut() = Value::Int(n);
                }
            } else {
                let mut sort_err: Option<RuntimeError> = None;
                vals.sort_by(|a, b| match cmp_vals(a, b, name, span) {
                    Ok(o) => o,
                    Err(e) => {
                        if sort_err.is_none() {
                            sort_err = Some(e);
                        }
                        Ordering::Equal
                    }
                });
                if let Some(e) = sort_err {
                    return Err(e);
                }
                if desc {
                    vals.reverse();
                }
                for (slot, v) in items.iter().zip(vals) {
                    *slot.borrow_mut() = v;
                }
            }
        }
        other => return Err(array_type_err(span, name, other)),
    }
    Ok(nil())
}

fn sort(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    sort_impl(args, "sort", span, false)
}

fn sort_desc(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    sort_impl(args, "sort_desc", span, true)
}

fn binary_search(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "binary_search", span)?;
    let target = args[1].borrow().clone();
    let idx: i64 = match &*args[0].borrow() {
        Value::IntArray(v) => {
            let Value::Int(t) = target else {
                return Err(type_err(
                    span,
                    "binary_search() target must be an int for int arrays",
                ));
            };
            crate::int_algos::sorted_index(v, t)
        }
        Value::Array(items) => {
            let mut lo: usize = 0;
            let mut hi: usize = items.len();
            let mut found: i64 = -1;
            while lo < hi {
                let mid = lo + (hi - lo) / 2;
                let ord = cmp_vals(&items[mid].borrow(), &target, "binary_search", span)?;
                match ord {
                    Ordering::Less => lo = mid + 1,
                    _ => {
                        if ord == Ordering::Equal {
                            found = mid as i64;
                        }
                        hi = mid;
                    }
                }
            }
            found
        }
        other => return Err(array_type_err(span, "binary_search", other)),
    };
    Ok(Value::Int(idx).ref_cell())
}

fn reverse(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "reverse", span)?;
    let mut cell = args[0].borrow_mut();
    match &mut *cell {
        Value::IntArray(v) => v.reverse(),
        Value::Array(items) => items.reverse(),
        other => return Err(array_type_err(span, "reverse", other)),
    }
    Ok(nil())
}

fn sum(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "sum", span)?;
    match &*args[0].borrow() {
        Value::IntArray(v) => {
            let total: i128 = v.iter().map(|&n| n as i128).sum();
            if let Ok(n) = i64::try_from(total) {
                Ok(Value::Int(n).ref_cell())
            } else {
                Ok(Value::BigInt(BigInt::from(total)).ref_cell())
            }
        }
        Value::Array(items) => {
            let mut acc = Value::Int(0);
            for item in items {
                let v = item.borrow();
                match &*v {
                    Value::Int(_) | Value::BigInt(_) | Value::Float(_) => {
                        acc = apply_binop(BinOp::Add, &acc, &v, span)?;
                    }
                    other => {
                        return Err(type_err(
                            span,
                            format!("sum() requires numeric elements, got {}", other.type_name()),
                        ))
                    }
                }
            }
            Ok(acc.ref_cell())
        }
        other => Err(array_type_err(span, "sum", other)),
    }
}

fn min_max_impl(args: &[ValueRef], name: &str, span: Span, want_max: bool) -> NekoResult<ValueRef> {
    arity(args, 1, name, span)?;
    match &*args[0].borrow() {
        Value::IntArray(v) => {
            let best = if want_max {
                v.iter().max()
            } else {
                v.iter().min()
            };
            Ok(best.map(|&n| Value::Int(n)).unwrap_or(Value::Nil).ref_cell())
        }
        Value::Array(items) => {
            if items.is_empty() {
                return Ok(nil());
            }
            let mut best = items[0].borrow().clone();
            for item in &items[1..] {
                let v = item.borrow();
                let ord = cmp_vals(&v, &best, name, span)?;
                let better = if want_max {
                    ord == Ordering::Greater
                } else {
                    ord == Ordering::Less
                };
                if better {
                    best = v.clone();
                }
            }
            Ok(best.ref_cell())
        }
        other => Err(array_type_err(span, name, other)),
    }
}

fn min(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    min_max_impl(args, "min", span, false)
}

fn max(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    min_max_impl(args, "max", span, true)
}

fn index_of_impl(args: &[ValueRef], name: &str, span: Span) -> NekoResult<i64> {
    arity(args, 2, name, span)?;
    let target = args[1].borrow().clone();
    match &*args[0].borrow() {
        Value::IntArray(v) => {
            let Value::Int(t) = target else {
                return Ok(-1);
            };
            Ok(v.iter().position(|&x| x == t).map(|i| i as i64).unwrap_or(-1))
        }
        Value::Array(items) => Ok(items
            .iter()
            .position(|item| values_equal(&item.borrow(), &target))
            .map(|i| i as i64)
            .unwrap_or(-1)),
        other => Err(array_type_err(span, name, other)),
    }
}

fn index_of(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    Ok(Value::Int(index_of_impl(args, "index_of", span)?).ref_cell())
}

fn contains(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    Ok(Value::Bool(index_of_impl(args, "contains", span)? >= 0).ref_cell())
}

fn unique(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "unique", span)?;
    match &*args[0].borrow() {
        Value::IntArray(v) => Ok(Value::IntArray(crate::int_algos::unique_int(v)).ref_cell()),
        Value::Array(items) => {
            let mut seen: HashSet<DsKey> = HashSet::with_capacity(items.len());
            let mut unhashable: Vec<ValueRef> = Vec::new();
            let mut out: Vec<ValueRef> = Vec::with_capacity(items.len());
            for item in items {
                let key = value_to_key(&item.borrow());
                match key {
                    Some(k) => {
                        if seen.insert(k) {
                            out.push(Rc::clone(item));
                        }
                    }
                    None => {
                        // Fall back to linear equality for unhashable values.
                        let dup = unhashable
                            .iter()
                            .any(|u| values_equal(&u.borrow(), &item.borrow()));
                        if !dup {
                            unhashable.push(Rc::clone(item));
                            out.push(Rc::clone(item));
                        }
                    }
                }
            }
            Ok(Value::Array(out).ref_cell())
        }
        other => Err(array_type_err(span, "unique", other)),
    }
}

// ---------------------------------------------------------------------------
// VM fast paths (unboxed int args, no per-call ValueRef allocation)
// ---------------------------------------------------------------------------

pub mod fast {
    use super::*;
    use std::rc::Rc;

    #[derive(Clone, Copy, Debug)]
    pub enum FastOut {
        Int(i64),
        Bool(bool),
        Nil,
    }

    #[inline]
    pub fn native_from(rc: &ValueRef) -> Option<DsRef> {
        match &*rc.borrow() {
            Value::Native(ds) => Some(Rc::clone(ds)),
            _ => None,
        }
    }

    #[inline]
    pub fn int_array_get(rc: &ValueRef, i: usize) -> Option<i64> {
        match &*rc.borrow() {
            Value::IntArray(v) => v.get(i).copied(),
            _ => None,
        }
    }

    pub fn queue_push_int(ds: &DsRef, n: i64) -> bool {
        let mut b = ds.borrow_mut();
        match &mut *b {
            NativeDs::Queue(q) => {
                q.push_back_int(n);
                true
            }
            _ => false,
        }
    }

    pub fn queue_pop_int(ds: &DsRef) -> Option<i64> {
        let mut b = ds.borrow_mut();
        match &mut *b {
            NativeDs::Queue(q) => q.pop_front_int(),
            _ => None,
        }
    }

    pub fn queue_is_empty(ds: &DsRef) -> Option<bool> {
        let b = ds.borrow();
        match &*b {
            NativeDs::Queue(q) => Some(q.is_empty()),
            _ => None,
        }
    }

    pub fn stack_push_int(ds: &DsRef, n: i64) -> bool {
        let mut b = ds.borrow_mut();
        match &mut *b {
            NativeDs::Stack(s) => {
                s.push_int(n);
                true
            }
            _ => false,
        }
    }

    pub fn stack_pop_int(ds: &DsRef) -> Option<i64> {
        let mut b = ds.borrow_mut();
        match &mut *b {
            NativeDs::Stack(s) => s.pop_int(),
            _ => None,
        }
    }

    pub fn stack_is_empty(ds: &DsRef) -> Option<bool> {
        let b = ds.borrow();
        match &*b {
            NativeDs::Stack(s) => Some(s.is_empty()),
            _ => None,
        }
    }

    pub fn deque_push_back_int(ds: &DsRef, n: i64) -> bool {
        let mut b = ds.borrow_mut();
        match &mut *b {
            NativeDs::Deque(d) => {
                d.push_back_int(n);
                true
            }
            _ => false,
        }
    }

    pub fn deque_push_front_int(ds: &DsRef, n: i64) -> bool {
        let mut b = ds.borrow_mut();
        match &mut *b {
            NativeDs::Deque(d) => {
                d.push_front_int(n);
                true
            }
            _ => false,
        }
    }

    pub fn deque_pop_front_int(ds: &DsRef) -> Option<i64> {
        let mut b = ds.borrow_mut();
        match &mut *b {
            NativeDs::Deque(d) => d.pop_front_int(),
            _ => None,
        }
    }

    pub fn deque_pop_back_int(ds: &DsRef) -> Option<i64> {
        let mut b = ds.borrow_mut();
        match &mut *b {
            NativeDs::Deque(d) => d.pop_back_int(),
            _ => None,
        }
    }

    pub fn deque_is_empty(ds: &DsRef) -> Option<bool> {
        let b = ds.borrow();
        match &*b {
            NativeDs::Deque(d) => Some(d.is_empty()),
            _ => None,
        }
    }

    pub fn list_push_back_int(ds: &DsRef, n: i64) -> bool {
        let mut b = ds.borrow_mut();
        match &mut *b {
            NativeDs::List(l) => {
                l.push_back_int(n);
                true
            }
            _ => false,
        }
    }

    pub fn list_push_front_int(ds: &DsRef, n: i64) -> bool {
        let mut b = ds.borrow_mut();
        match &mut *b {
            NativeDs::List(l) => {
                l.push_front_int(n);
                true
            }
            _ => false,
        }
    }

    pub fn list_pop_front_int(ds: &DsRef) -> Option<i64> {
        let mut b = ds.borrow_mut();
        match &mut *b {
            NativeDs::List(l) => l.pop_front_int(),
            _ => None,
        }
    }

    pub fn list_pop_back_int(ds: &DsRef) -> Option<i64> {
        let mut b = ds.borrow_mut();
        match &mut *b {
            NativeDs::List(l) => l.pop_back_int(),
            _ => None,
        }
    }

    pub fn list_is_empty(ds: &DsRef) -> Option<bool> {
        let b = ds.borrow();
        match &*b {
            NativeDs::List(l) => Some(l.is_empty()),
            _ => None,
        }
    }

    pub fn heap_push_int(ds: &DsRef, n: i64) -> bool {
        let mut b = ds.borrow_mut();
        match &mut *b {
            NativeDs::Heap(h) => {
                h.push_int(n);
                true
            }
            _ => false,
        }
    }

    pub fn heap_pop_int(ds: &DsRef) -> Option<i64> {
        let mut b = ds.borrow_mut();
        match &mut *b {
            NativeDs::Heap(h) => match h.pop_num()? {
                HeapNum::Int(n) => Some(n),
                HeapNum::Float(f) => Some(f as i64),
            },
            _ => None,
        }
    }

    pub fn heap_is_empty(ds: &DsRef) -> Option<bool> {
        let b = ds.borrow();
        match &*b {
            NativeDs::Heap(h) => Some(h.is_empty()),
            _ => None,
        }
    }

    pub fn set_add_int(ds: &DsRef, n: i64) -> Option<bool> {
        let mut b = ds.borrow_mut();
        match &mut *b {
            NativeDs::Set(s) => Some(s.insert_int(n)),
            _ => None,
        }
    }

    pub fn set_contains_int(ds: &DsRef, n: i64) -> Option<bool> {
        let b = ds.borrow();
        match &*b {
            NativeDs::Set(s) => Some(s.contains_int(n)),
            _ => None,
        }
    }

    pub fn map_set_int_int(ds: &DsRef, k: i64, v: i64) -> bool {
        let mut b = ds.borrow_mut();
        match &mut *b {
            NativeDs::Map(m) => {
                m.insert_int_int(k, v);
                true
            }
            _ => false,
        }
    }

    pub fn map_get_int(ds: &DsRef, k: i64) -> Option<Option<i64>> {
        let b = ds.borrow();
        match &*b {
            NativeDs::Map(m) => Some(m.get_int(k)),
            _ => None,
        }
    }

    pub fn map_has_int(ds: &DsRef, k: i64) -> Option<bool> {
        let b = ds.borrow();
        match &*b {
            NativeDs::Map(m) => Some(m.contains_key(&DsKey::Int(k))),
            _ => None,
        }
    }

    pub fn graph_add_edge_int(ds: &DsRef, u: i64, v: i64) -> bool {
        let mut b = ds.borrow_mut();
        match &mut *b {
            NativeDs::Graph(g) => {
                if u < 0 || v < 0 {
                    return false;
                }
                let u = u as usize;
                let v = v as usize;
                if u >= g.n || v >= g.n {
                    return false;
                }
                g.adj[u].push((v as u32, 1));
                if !g.directed && u != v {
                    g.adj[v].push((u as u32, 1));
                }
                g.edges += 1;
                true
            }
            _ => false,
        }
    }

    pub fn native_len(ds: &DsRef) -> Option<i64> {
        let b = ds.borrow();
        Some(b.len() as i64)
    }

    /// Fused count-push loop: one borrow for the entire range.
    pub fn fuse_count_push(
        ds: &DsRef,
        op: u8,
        start: i64,
        limit: i64,
        mul: i64,
        add: i64,
    ) -> Option<i64> {
        let mut b = ds.borrow_mut();
        match &mut *b {
            NativeDs::Queue(q) if op == 0 => q.push_back_range_int(start, limit, mul, add).then_some(limit),
            NativeDs::Stack(s) if op == 3 => s.push_range_int(start, limit, mul, add).then_some(limit),
            NativeDs::Deque(d) if op == 6 => d.push_back_range_int(start, limit, mul, add).then_some(limit),
            NativeDs::List(l) if op == 11 => l.push_back_range_int(start, limit, mul, add).then_some(limit),
            NativeDs::Heap(h) if op == 16 => {
                if h.generic.is_some() {
                    return None;
                }
                let min = h.min;
                let mut i = start;
                while i < limit {
                    crate::int_heap::push(&mut h.ints, i.wrapping_mul(mul).wrapping_add(add), min);
                    i += 1;
                }
                Some(limit)
            }
            NativeDs::Set(s) if op == 19 => {
                let SetData::Int { set, order } = s else {
                    return None;
                };
                let mut i = start;
                while i < limit {
                    let k = i.wrapping_mul(mul).wrapping_add(add);
                    if set.insert(k) {
                        order.push(k);
                    }
                    i += 1;
                }
                Some(limit)
            }
            NativeDs::Map(m) if op == 21 => {
                if start == 0 {
                    let values = crate::int_algos::map_build_dense(start, limit, mul);
                    *m = MapData::Dense(values);
                    return Some(limit);
                }
                let MapData::IntInt { map, keys } = m else {
                    return None;
                };
                let count = (limit - start).max(0) as usize;
                map.reserve(map.len() + count);
                keys.reserve(keys.len() + count);
                let mut i = start;
                use std::collections::hash_map::Entry;
                while i < limit {
                    let v = i.wrapping_mul(mul);
                    match map.entry(i) {
                        Entry::Vacant(e) => {
                            keys.push(i);
                            e.insert(v);
                        }
                        Entry::Occupied(mut e) => {
                            e.insert(v);
                        }
                    }
                    i += 1;
                }
                Some(limit)
            }
            _ => None,
        }
    }

    /// Fused drain-and-accumulate loop: one borrow for the entire drain.
    pub fn fuse_drain_acc(ds: &DsRef, pop_op: u8) -> Option<i64> {
        let mut b = ds.borrow_mut();
        match (&mut *b, pop_op) {
            (NativeDs::Queue(q), 1) => q.drain_sum_front(),
            (NativeDs::Stack(s), 4) => s.drain_sum(),
            (NativeDs::Deque(d), 8) => d.drain_sum_front(),
            (NativeDs::List(l), 13) => l.drain_sum_front(),
            _ => None,
        }
    }

    /// Fused drain-and-count loop: pop until empty, return prior acc + count.
    pub fn fuse_drain_count(ds: &DsRef, pop_op: u8, acc: i64) -> Option<i64> {
        let mut b = ds.borrow_mut();
        match (&mut *b, pop_op) {
            (NativeDs::Stack(s), 4) => s.drain_count().map(|n| acc + n),
            (NativeDs::Heap(h), 17) => h.drain_count_int().map(|n| acc + n),
            _ => None,
        }
    }

    pub fn fuse_set_lookup(ds: &DsRef, start: i64, limit: i64, hits: i64) -> Option<(i64, i64)> {
        let b = ds.borrow();
        let NativeDs::Set(s) = &*b else {
            return None;
        };
        let SetData::Int { set, .. } = s else {
            return None;
        };
        let mut i = start;
        let mut h = hits;
        while i < limit {
            if set.contains(&i) {
                h += 1;
            }
            i += 1;
        }
        Some((i, h))
    }

    pub fn fuse_map_lookup(ds: &DsRef, start: i64, limit: i64, sum: i64) -> Option<(i64, i64)> {
        let b = ds.borrow();
        let NativeDs::Map(m) = &*b else {
            return None;
        };
        let s = match m {
            MapData::Dense(values) => sum + crate::int_algos::map_lookup_dense_sum(values, start, limit),
            MapData::IntInt { map, .. } => {
                let mut s = sum;
                let mut i = start;
                while i < limit {
                    if let Some(v) = map.get(&i) {
                        s += *v;
                    }
                    i += 1;
                }
                s
            }
            MapData::Any(_) => return None,
        };
        Some((limit, s))
    }

    pub fn fuse_map_build(ds: &DsRef, start: i64, limit: i64, mul: i64) -> Option<i64> {
        let mut b = ds.borrow_mut();
        let NativeDs::Map(m) = &mut *b else {
            return None;
        };
        if start == 0 && m.is_empty() {
            *m = MapData::Dense(crate::int_algos::map_build_dense(start, limit, mul));
            return Some(limit);
        }
        drop(b);
        fuse_count_push(ds, 21, start, limit, mul, 0)
    }

    pub fn fuse_heap_drain_verify(ds: &DsRef, mut prev: i64) -> Option<i64> {
        let mut b = ds.borrow_mut();
        let NativeDs::Heap(h) = &mut *b else {
            return None;
        };
        if h.generic.is_some() {
            return None;
        }
        let min = h.min;
        while !h.ints.is_empty() {
            let cur = crate::int_heap::pop(&mut h.ints, min)?;
            if cur < prev {
                return None;
            }
            prev = cur;
        }
        Some(prev)
    }

    pub fn fuse_graph_edges(ds: &DsRef, start: i64, n: i64) -> Option<i64> {
        let mut b = ds.borrow_mut();
        let NativeDs::Graph(g) = &mut *b else {
            return None;
        };
        let mut i = start;
        while i < n - 1 {
            let u = i as usize;
            let v = (i + 1) as usize;
            if v >= g.n {
                return None;
            }
            g.adj[u].push((v as u32, 1));
            if !g.directed && u != v {
                g.adj[v].push((u as u32, 1));
            }
            g.edges += 1;
            i += 1;
        }
        Some(i)
    }

    pub fn fuse_binary_search(
        arr: &ValueRef,
        start: i64,
        k: i64,
        hits: i64,
        mul: i64,
    ) -> Option<(i64, i64)> {
        let arr_ref = arr.borrow();
        let Value::IntArray(v) = &*arr_ref else {
            return None;
        };
        Some(crate::int_algos::binary_search_hits(v, start, k, mul, hits))
    }

    pub fn binary_search_int(arr: &ValueRef, target: i64) -> Option<i64> {
        match &*arr.borrow() {
            Value::IntArray(v) => Some(crate::int_algos::sorted_index(v, target)),
            _ => None,
        }
    }

    /// Map builtin name to a fast-path id for the VM.
    pub fn path_id(name: &str) -> Option<u8> {
        Some(match name {
            "queue_push" => 0,
            "queue_pop" => 1,
            "queue_is_empty" => 2,
            "stack_push" => 3,
            "stack_pop" => 4,
            "stack_is_empty" => 5,
            "deque_push_back" => 6,
            "deque_push_front" => 7,
            "deque_pop_front" => 8,
            "deque_pop_back" => 9,
            "deque_is_empty" => 10,
            "list_push_back" => 11,
            "list_push_front" => 12,
            "list_pop_front" => 13,
            "list_pop_back" => 14,
            "list_is_empty" => 15,
            "heap_push" => 16,
            "heap_pop" => 17,
            "heap_is_empty" => 18,
            "set_add" => 19,
            "set_contains" => 20,
            "map_set" => 21,
            "map_get" => 22,
            "map_has" => 23,
            "graph_add_edge" => 24,
            "binary_search" => 25,
            "len" => 26,
            _ => return None,
        })
    }
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

/// All dsa builtins in registration order. Extend here; names propagate to
/// the interpreter, the VM, and the bytecode compiler automatically.
pub fn builtins() -> Vec<(&'static str, NativeFn)> {
    let table: Vec<(&'static str, NativeFn)> = vec![
        // linked list
        ("list_new", Rc::new(list_new)),
        ("list_from_array", Rc::new(list_from_array)),
        ("list_push_front", Rc::new(list_push_front)),
        ("list_push_back", Rc::new(list_push_back)),
        ("list_pop_front", Rc::new(list_pop_front)),
        ("list_pop_back", Rc::new(list_pop_back)),
        ("list_front", Rc::new(list_front)),
        ("list_back", Rc::new(list_back)),
        ("list_get", Rc::new(list_get)),
        ("list_set", Rc::new(list_set)),
        ("list_insert", Rc::new(list_insert)),
        ("list_remove", Rc::new(list_remove)),
        ("list_contains", Rc::new(list_contains)),
        ("list_index_of", Rc::new(list_index_of)),
        ("list_reverse", Rc::new(list_reverse)),
        ("list_to_array", Rc::new(list_to_array)),
        ("list_clear", Rc::new(list_clear)),
        ("list_is_empty", Rc::new(list_is_empty)),
        // stack
        ("stack_new", Rc::new(stack_new)),
        ("stack_push", Rc::new(stack_push)),
        ("stack_pop", Rc::new(stack_pop)),
        ("stack_peek", Rc::new(stack_peek)),
        ("stack_is_empty", Rc::new(stack_is_empty)),
        ("stack_clear", Rc::new(stack_clear)),
        ("stack_to_array", Rc::new(stack_to_array)),
        // queue
        ("queue_new", Rc::new(queue_new)),
        ("queue_push", Rc::new(queue_push)),
        ("queue_pop", Rc::new(queue_pop)),
        ("queue_front", Rc::new(queue_front)),
        ("queue_back", Rc::new(queue_back)),
        ("queue_is_empty", Rc::new(queue_is_empty)),
        ("queue_clear", Rc::new(queue_clear)),
        ("queue_to_array", Rc::new(queue_to_array)),
        // deque
        ("deque_new", Rc::new(deque_new)),
        ("deque_push_front", Rc::new(deque_push_front)),
        ("deque_push_back", Rc::new(deque_push_back)),
        ("deque_pop_front", Rc::new(deque_pop_front)),
        ("deque_pop_back", Rc::new(deque_pop_back)),
        ("deque_front", Rc::new(deque_front)),
        ("deque_back", Rc::new(deque_back)),
        ("deque_is_empty", Rc::new(deque_is_empty)),
        ("deque_clear", Rc::new(deque_clear)),
        ("deque_to_array", Rc::new(deque_to_array)),
        // heap / priority queue
        ("heap_new_min", Rc::new(heap_new_min)),
        ("heap_new_max", Rc::new(heap_new_max)),
        ("heap_push", Rc::new(heap_push)),
        ("heap_pop", Rc::new(heap_pop)),
        ("heap_peek", Rc::new(heap_peek)),
        ("heap_is_empty", Rc::new(heap_is_empty)),
        // set
        ("set_new", Rc::new(set_new)),
        ("set_from_array", Rc::new(set_from_array)),
        ("set_add", Rc::new(set_add)),
        ("set_remove", Rc::new(set_remove)),
        ("set_contains", Rc::new(set_contains)),
        ("set_is_empty", Rc::new(set_is_empty)),
        ("set_clear", Rc::new(set_clear)),
        ("set_to_array", Rc::new(set_to_array)),
        ("set_union", Rc::new(set_union)),
        ("set_intersect", Rc::new(set_intersect)),
        ("set_diff", Rc::new(set_diff)),
        // map
        ("map_new", Rc::new(map_new)),
        ("map_set", Rc::new(map_set)),
        ("map_get", Rc::new(map_get)),
        ("map_has", Rc::new(map_has)),
        ("map_remove", Rc::new(map_remove)),
        ("map_keys", Rc::new(map_keys)),
        ("map_values", Rc::new(map_values)),
        ("map_is_empty", Rc::new(map_is_empty)),
        ("map_clear", Rc::new(map_clear)),
        // graph
        ("graph_new", Rc::new(graph_new)),
        ("graph_new_directed", Rc::new(graph_new_directed)),
        ("graph_add_edge", Rc::new(graph_add_edge)),
        ("graph_add_edge_w", Rc::new(graph_add_edge_w)),
        ("graph_neighbors", Rc::new(graph_neighbors)),
        ("graph_node_count", Rc::new(graph_node_count)),
        ("graph_edge_count", Rc::new(graph_edge_count)),
        ("graph_bfs", Rc::new(graph_bfs)),
        ("graph_dfs", Rc::new(graph_dfs)),
        ("graph_dijkstra", Rc::new(graph_dijkstra)),
        ("graph_edge_list", Rc::new(graph_edge_list)),
        ("graph_adjacency_coo", Rc::new(graph_adjacency_coo)),
        ("graph_topo_sort", Rc::new(graph_topo_sort)),
        // array algorithms
        ("push", Rc::new(push)),
        ("pop", Rc::new(pop)),
        ("sort", Rc::new(sort)),
        ("sort_desc", Rc::new(sort_desc)),
        ("binary_search", Rc::new(binary_search)),
        ("reverse", Rc::new(reverse)),
        ("sum", Rc::new(sum)),
        ("min", Rc::new(min)),
        ("max", Rc::new(max)),
        ("index_of", Rc::new(index_of)),
        ("contains", Rc::new(contains)),
        ("unique", Rc::new(unique)),
    ];
    table
}
