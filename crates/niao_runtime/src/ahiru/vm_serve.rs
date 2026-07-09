//! VM serve mode flag — set by CLI before `start_pending_server`.

use std::sync::Mutex;

static VM_SERVE_ACTIVE: Mutex<bool> = Mutex::new(false);

pub fn set_vm_serve_active(active: bool) {
    *VM_SERVE_ACTIVE.lock().unwrap() = active;
}

pub fn vm_serve_active() -> bool {
    *VM_SERVE_ACTIVE.lock().unwrap()
}

pub fn clear_vm_serve() {
    *VM_SERVE_ACTIVE.lock().unwrap() = false;
}
