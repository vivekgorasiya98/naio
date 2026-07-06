//! Global registry for Neko function handles used by worker threads.
//! Stores `Rc` pointers as `usize` so the static table remains `Sync`.

use crate::ValueRef;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

static CALLEE_REGISTRY: OnceLock<Mutex<HashMap<u64, usize>>> = OnceLock::new();
static NEXT_CALLEE_ID: AtomicU64 = AtomicU64::new(1);

fn registry() -> &'static Mutex<HashMap<u64, usize>> {
    CALLEE_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn store_callee(callee: ValueRef) -> u64 {
    let id = NEXT_CALLEE_ID.fetch_add(1, Ordering::Relaxed);
    let ptr = Rc::into_raw(callee) as usize;
    registry().lock().unwrap().insert(id, ptr);
    id
}

pub fn take_callee(id: u64) -> Option<ValueRef> {
    let ptr = registry().lock().unwrap().remove(&id)?;
    // SAFETY: pointer came from `Rc::into_raw` in `store_callee`.
    Some(unsafe { Rc::from_raw(ptr as *const RefCell<crate::Value>) })
}
