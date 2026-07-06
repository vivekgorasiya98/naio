//! Bridge for invoking Neko user functions from native HTTP handlers while the VM runs.

use crate::{fast_val::HeapMut, fast_val::value_to_fast, Vm, VmError};
use neko_ast::Span;
use neko_bytecode::BytecodeModule;
use neko_runtime::{NekoResult, RuntimeError as RtError, Value, ValueRef};
use std::cell::RefCell;
use std::path::Path;
use std::sync::Arc;

thread_local! {
    static ACTIVE_VM: RefCell<Option<*mut Vm>> = const { RefCell::new(None) };
}

/// Run bytecode and register a VM call hook for the duration (current thread only).
pub fn run_with_handler_hook(
    vm: &mut Vm,
    module: &BytecodeModule,
    base_dir: &Path,
) -> Result<(), VmError> {
    install_thread_vm_hook(vm);
    let result = vm.run(module, base_dir);
    clear_thread_vm_hook();
    result
}

/// Install thread-local VM hook so `call_neko_function` dispatches on this thread.
pub fn install_thread_vm_hook(vm: &mut Vm) {
    let vm_ptr = vm as *mut Vm;
    ACTIVE_VM.with(|slot| *slot.borrow_mut() = Some(vm_ptr));
    neko_runtime::set_neko_vm_call_hook(Some(Arc::new(|callee, args, span| {
        ACTIVE_VM.with(|slot| {
            let ptr = slot.borrow().ok_or_else(|| {
                RtError::at(span, neko_errors::codes::E1404_NET_HTTP, "VM call hook not active")
            })?;
            // SAFETY: hook is only used on the thread that set ACTIVE_VM.
            unsafe { (*ptr).invoke_handler(callee, args, span) }
        })
    })));
}

/// Clear thread-local VM hook (call after serve startup on the primary thread).
pub fn clear_thread_vm_hook() {
    neko_runtime::set_neko_vm_call_hook(None);
    ACTIVE_VM.with(|slot| *slot.borrow_mut() = None);
}

impl Vm {
    /// Resolve a function name to its bytecode index.
    pub fn function_index(&self, name: &str) -> Option<usize> {
        self.functions.iter().position(|f| f.name == name)
    }

    /// Invoke a handler by bytecode index (skips name lookup).
    pub fn call_at_index(
        &mut self,
        idx: usize,
        args: &[ValueRef],
        _span: Span,
    ) -> Result<ValueRef, VmError> {
        if idx >= self.functions.len() {
            return Err(VmError::UnknownFunction(format!("index {idx}")));
        }
        for arg in args {
            let fast = {
                let mut heap = HeapMut { vm: self };
                value_to_fast(&arg.borrow(), &mut heap)
            };
            self.stack.push(fast);
        }
        self.enter_frame(idx, args.len())?;
        self.dispatch()?;
        let ret = self.stack.pop().ok_or(VmError::StackUnderflow)?;
        Ok(ret.to_value_ref(&self.heap, &self.native_refs))
    }

    /// Invoke a user `Value::Function` handler while the VM module is loaded.
    pub fn invoke_handler(
        &mut self,
        callee: ValueRef,
        args: &[ValueRef],
        span: Span,
    ) -> NekoResult<ValueRef> {
        let name = match &*callee.borrow() {
            Value::Function(f) => f.def.name.clone(),
            other => {
                return Err(RtError::at(
                    span,
                    neko_errors::codes::E2003_TYPE_ERROR,
                    format!("handler must be a function, got {}", other.type_name()),
                ));
            }
        };
        let idx = self.function_index(&name).ok_or_else(|| {
            RtError::at(
                span,
                neko_errors::codes::E2003_TYPE_ERROR,
                format!("unknown function: {name}"),
            )
        })?;
        self.call_at_index(idx, args, span).map_err(|e| match e {
            VmError::Runtime(r) => r,
            other => RtError::at(span, neko_errors::codes::E2003_TYPE_ERROR, other.to_string()),
        })
    }
}
