//! Per-worker VM pool for parallel HTTP handler dispatch.

use crate::call_bridge::{clear_thread_vm_hook, install_thread_vm_hook};
use crate::{Vm, VmError};
use ahiru_core::AhiruResponse;
use niao_ast::Span;
use niao_bytecode::BytecodeModule;
use niao_runtime::{Value, ValueRef};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

pub struct VmHandlerJob {
    pub handler_id: u64,
    pub vm_index: Option<u32>,
    pub fields: HashMap<String, String>,
    pub result_tx: std::sync::mpsc::Sender<Result<AhiruResponse, String>>,
}

pub type FieldsToArgs = Arc<dyn Fn(&HashMap<String, String>) -> ValueRef + Send + Sync>;
pub type ResponseFromValue = Arc<dyn Fn(&Value) -> Result<AhiruResponse, String> + Send + Sync>;

pub struct VmHandlerPool {
    tx: std::sync::mpsc::Sender<VmHandlerJob>,
    _handles: Vec<thread::JoinHandle<()>>,
    rr: AtomicUsize,
    workers: usize,
}

impl VmHandlerPool {
    pub fn new(
        workers: usize,
        module: Arc<BytecodeModule>,
        base_dir: PathBuf,
        fields_to_args: FieldsToArgs,
        handler_index: Arc<dyn Fn(u64) -> Option<u32> + Send + Sync>,
        to_response: ResponseFromValue,
    ) -> Self {
        let workers = workers.max(1);
        let (tx, rx) = std::sync::mpsc::channel::<VmHandlerJob>();
        let rx = Arc::new(Mutex::new(rx));
        let mut handles = Vec::with_capacity(workers);
        for _ in 0..workers {
            let rx = Arc::clone(&rx);
            let module = Arc::clone(&module);
            let base_dir = base_dir.clone();
            let fields_to_args = Arc::clone(&fields_to_args);
            let handler_index = Arc::clone(&handler_index);
            let to_response = Arc::clone(&to_response);
            handles.push(thread::spawn(move || {
                let mut vm = Vm::new();
                if vm.init_module(&module, &base_dir).is_err() {
                    return;
                }
                install_thread_vm_hook(&mut vm);
                loop {
                    let job = {
                        let guard = rx.lock().unwrap();
                        guard.recv()
                    };
                    let Ok(job) = job else { break };
                    let span = Span::dummy();
                    let result = (|| -> Result<AhiruResponse, String> {
                        let req = fields_to_args(&job.fields);
                        let idx = job
                            .vm_index
                            .or_else(|| (handler_index)(job.handler_id))
                            .ok_or_else(|| "handler VM index not resolved".to_string())?
                            as usize;
                        let val = vm.call_at_index(idx, &[req], span).map_err(|e| match e {
                            VmError::Runtime(r) => r.to_string(),
                            other => other.to_string(),
                        })?;
                        let borrowed = val.borrow();
                        to_response(&borrowed)
                    })();
                    let _ = job.result_tx.send(result);
                }
                clear_thread_vm_hook();
            }));
        }
        Self {
            tx,
            _handles: handles,
            rr: AtomicUsize::new(0),
            workers,
        }
    }

    pub fn workers(&self) -> usize {
        self.workers
    }

    pub fn dispatch(
        &self,
        handler_id: u64,
        vm_index: Option<u32>,
        fields: HashMap<String, String>,
    ) -> Result<AhiruResponse, String> {
        let (result_tx, result_rx) = std::sync::mpsc::channel();
        self.tx
            .send(VmHandlerJob {
                handler_id,
                vm_index,
                fields,
                result_tx,
            })
            .map_err(|e| e.to_string())?;
        result_rx.recv().map_err(|e| e.to_string())?
    }

    pub fn next_worker_hint(&self) -> usize {
        self.rr.fetch_add(1, Ordering::Relaxed) % self.workers
    }
}
