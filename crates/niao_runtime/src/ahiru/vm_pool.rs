//! VM worker pool dispatch slot (pool implementation lives in `niao_vm::ahiru_pool`).

use ahiru_core::AhiruResponse;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub type VmPoolDispatchFn = Arc<
    dyn Fn(u64, Option<u32>, HashMap<String, String>, bool) -> Result<AhiruResponse, String>
        + Send
        + Sync,
>;

static VM_POOL: Mutex<Option<VmPoolDispatchFn>> = Mutex::new(None);

pub fn install_vm_pool(dispatch: Option<VmPoolDispatchFn>) {
    *VM_POOL.lock().unwrap() = dispatch;
}

pub fn vm_pool_active() -> bool {
    VM_POOL.lock().unwrap().is_some()
}

pub fn vm_pool_dispatch(
    handler_id: u64,
    vm_index: Option<u32>,
    fields: HashMap<String, String>,
    quiet: bool,
) -> Result<AhiruResponse, String> {
    let pool = VM_POOL
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| "VM pool not installed".to_string())?;
    (pool)(handler_id, vm_index, fields, quiet)
}

pub fn clear_vm_pool() {
    *VM_POOL.lock().unwrap() = None;
}
