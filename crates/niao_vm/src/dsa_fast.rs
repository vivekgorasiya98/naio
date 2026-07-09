//! Inline fast paths for hot DSA builtins in the bytecode VM.

use crate::fast_val::FastVal;
use niao_runtime::dsa::fast;
use niao_runtime::{NativeDs, Value, ValueRef};
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Clone, Copy)]
pub struct DsaFastPath(pub u8);

impl DsaFastPath {
    pub fn from_name(name: &str) -> Option<Self> {
        fast::path_id(name).map(Self)
    }

    pub fn execute(
        self,
        stack: &mut Vec<FastVal>,
        heap: &[ValueRef],
        native_ds: &[Rc<RefCell<NativeDs>>],
        argc: usize,
    ) -> Option<FastVal> {
        if stack.len() < argc {
            return None;
        }
        let base = stack.len() - argc;
        let args = &stack[base..];

        let out = match self.0 {
            0 if argc == 2 => {
                let (Some(ds), Some(n)) = (native_idx(args[0], heap, native_ds), arg_int(args[1])) else {
                    return None;
                };
                if !fast::queue_push_int(&native_ds[ds], n) {
                    return None;
                }
                FastVal::Nil
            }
            1 if argc == 1 => {
                let ds = native_idx(args[0], heap, native_ds)?;
                FastVal::Int(fast::queue_pop_int(&native_ds[ds])?)
            }
            2 if argc == 1 => {
                let ds = native_idx(args[0], heap, native_ds)?;
                FastVal::Bool(fast::queue_is_empty(&native_ds[ds])?)
            }
            3 if argc == 2 => {
                let (Some(ds), Some(n)) = (native_idx(args[0], heap, native_ds), arg_int(args[1])) else {
                    return None;
                };
                if !fast::stack_push_int(&native_ds[ds], n) {
                    return None;
                }
                FastVal::Nil
            }
            4 if argc == 1 => {
                let ds = native_idx(args[0], heap, native_ds)?;
                FastVal::Int(fast::stack_pop_int(&native_ds[ds])?)
            }
            5 if argc == 1 => {
                let ds = native_idx(args[0], heap, native_ds)?;
                FastVal::Bool(fast::stack_is_empty(&native_ds[ds])?)
            }
            6 if argc == 2 => {
                let (Some(ds), Some(n)) = (native_idx(args[0], heap, native_ds), arg_int(args[1])) else {
                    return None;
                };
                if !fast::deque_push_back_int(&native_ds[ds], n) {
                    return None;
                }
                FastVal::Nil
            }
            7 if argc == 2 => {
                let (Some(ds), Some(n)) = (native_idx(args[0], heap, native_ds), arg_int(args[1])) else {
                    return None;
                };
                if !fast::deque_push_front_int(&native_ds[ds], n) {
                    return None;
                }
                FastVal::Nil
            }
            8 if argc == 1 => {
                let ds = native_idx(args[0], heap, native_ds)?;
                FastVal::Int(fast::deque_pop_front_int(&native_ds[ds])?)
            }
            9 if argc == 1 => {
                let ds = native_idx(args[0], heap, native_ds)?;
                FastVal::Int(fast::deque_pop_back_int(&native_ds[ds])?)
            }
            10 if argc == 1 => {
                let ds = native_idx(args[0], heap, native_ds)?;
                FastVal::Bool(fast::deque_is_empty(&native_ds[ds])?)
            }
            11 if argc == 2 => {
                let (Some(ds), Some(n)) = (native_idx(args[0], heap, native_ds), arg_int(args[1])) else {
                    return None;
                };
                if !fast::list_push_back_int(&native_ds[ds], n) {
                    return None;
                }
                FastVal::Nil
            }
            12 if argc == 2 => {
                let (Some(ds), Some(n)) = (native_idx(args[0], heap, native_ds), arg_int(args[1])) else {
                    return None;
                };
                if !fast::list_push_front_int(&native_ds[ds], n) {
                    return None;
                }
                FastVal::Nil
            }
            13 if argc == 1 => {
                let ds = native_idx(args[0], heap, native_ds)?;
                FastVal::Int(fast::list_pop_front_int(&native_ds[ds])?)
            }
            14 if argc == 1 => {
                let ds = native_idx(args[0], heap, native_ds)?;
                FastVal::Int(fast::list_pop_back_int(&native_ds[ds])?)
            }
            15 if argc == 1 => {
                let ds = native_idx(args[0], heap, native_ds)?;
                FastVal::Bool(fast::list_is_empty(&native_ds[ds])?)
            }
            16 if argc == 2 => {
                let (Some(ds), Some(n)) = (native_idx(args[0], heap, native_ds), arg_int(args[1])) else {
                    return None;
                };
                if !fast::heap_push_int(&native_ds[ds], n) {
                    return None;
                }
                FastVal::Nil
            }
            17 if argc == 1 => {
                let ds = native_idx(args[0], heap, native_ds)?;
                FastVal::Int(fast::heap_pop_int(&native_ds[ds])?)
            }
            18 if argc == 1 => {
                let ds = native_idx(args[0], heap, native_ds)?;
                FastVal::Bool(fast::heap_is_empty(&native_ds[ds])?)
            }
            19 if argc == 2 => {
                let (Some(ds), Some(n)) = (native_idx(args[0], heap, native_ds), arg_int(args[1])) else {
                    return None;
                };
                FastVal::Bool(fast::set_add_int(&native_ds[ds], n)?)
            }
            20 if argc == 2 => {
                let (Some(ds), Some(n)) = (native_idx(args[0], heap, native_ds), arg_int(args[1])) else {
                    return None;
                };
                FastVal::Bool(fast::set_contains_int(&native_ds[ds], n)?)
            }
            21 if argc == 3 => {
                let (Some(ds), Some(k), Some(v)) = (
                    native_idx(args[0], heap, native_ds),
                    arg_int(args[1]),
                    arg_int(args[2]),
                ) else {
                    return None;
                };
                fast::map_set_int_int(&native_ds[ds], k, v);
                FastVal::Nil
            }
            22 if argc == 2 => {
                let (Some(ds), Some(k)) = (native_idx(args[0], heap, native_ds), arg_int(args[1])) else {
                    return None;
                };
                match fast::map_get_int(&native_ds[ds], k)? {
                    Some(n) => FastVal::Int(n),
                    None => FastVal::Nil,
                }
            }
            23 if argc == 2 => {
                let (Some(ds), Some(k)) = (native_idx(args[0], heap, native_ds), arg_int(args[1])) else {
                    return None;
                };
                FastVal::Bool(fast::map_has_int(&native_ds[ds], k)?)
            }
            24 if argc == 3 => {
                let (Some(ds), Some(u), Some(v)) = (
                    native_idx(args[0], heap, native_ds),
                    arg_int(args[1]),
                    arg_int(args[2]),
                ) else {
                    return None;
                };
                fast::graph_add_edge_int(&native_ds[ds], u, v);
                FastVal::Nil
            }
            25 if argc == 2 => {
                let (Some(arr), Some(target)) = (arg_heap(args[0], heap), arg_int(args[1])) else {
                    return None;
                };
                FastVal::Int(fast::binary_search_int(&arr, target)?)
            }
            26 if argc == 1 => match args[0] {
                FastVal::Native(i) => FastVal::Int(fast::native_len(&native_ds[i as usize])?),
                FastVal::Heap(idx) => {
                    let rc = &heap[idx as usize];
                    if let Some(ds) = fast::native_from(rc) {
                        if let Some(i) = native_ds.iter().position(|d| Rc::ptr_eq(d, &ds)) {
                            FastVal::Int(fast::native_len(&native_ds[i])?)
                        } else {
                            FastVal::Int(fast::native_len(&ds)?)
                        }
                    } else {
                        match &*rc.borrow() {
                            Value::IntArray(v) => FastVal::Int(v.len() as i64),
                            Value::FloatArray(v) => FastVal::Int(v.len() as i64),
                            Value::BoolArray(v) => FastVal::Int(v.len() as i64),
                            Value::Array(v) => FastVal::Int(v.len() as i64),
                            Value::String(s) => FastVal::Int(s.len() as i64),
                            _ => return None,
                        }
                    }
                }
                _ => return None,
            },
            _ => return None,
        };

        stack.truncate(base);
        Some(out)
    }
}

#[inline]
fn arg_int(v: FastVal) -> Option<i64> {
    match v {
        FastVal::Int(n) => Some(n),
        _ => None,
    }
}

#[inline]
fn arg_heap(v: FastVal, heap: &[ValueRef]) -> Option<ValueRef> {
    match v {
        FastVal::Heap(idx) => Some(Rc::clone(&heap[idx as usize])),
        _ => None,
    }
}

#[inline]
fn native_idx(v: FastVal, heap: &[ValueRef], native_ds: &[Rc<RefCell<NativeDs>>]) -> Option<usize> {
    match v {
        FastVal::Native(i) => Some(i as usize),
        FastVal::Heap(h) => {
            let rc = &heap[h as usize];
            if let Some(ds) = fast::native_from(rc) {
                native_ds.iter().position(|d| Rc::ptr_eq(d, &ds))
            } else {
                None
            }
        }
        _ => None,
    }
}
