//! Worker thread pool for parallel Niao handler dispatch.
#![allow(dead_code)]

use ahiru_core::AhiruResponse;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

type InvokeFn = Arc<dyn Fn(u64, HashMap<String, String>) -> Result<AhiruResponse, String> + Send + Sync>;

struct PoolJob {
    handler_id: u64,
    fields: HashMap<String, String>,
    result_tx: std::sync::mpsc::Sender<Result<AhiruResponse, String>>,
}

pub struct HandlerWorkerPool {
    tx: std::sync::mpsc::Sender<PoolJob>,
    _handles: Vec<thread::JoinHandle<()>>,
    rr: AtomicUsize,
    workers: usize,
}

impl HandlerWorkerPool {
    pub fn new(workers: usize, invoke: InvokeFn) -> Self {
        let workers = workers.max(1);
        let (tx, rx) = std::sync::mpsc::channel::<PoolJob>();
        let rx = Arc::new(Mutex::new(rx));
        let mut handles = Vec::with_capacity(workers);
        for _ in 0..workers {
            let rx = Arc::clone(&rx);
            let invoke = Arc::clone(&invoke);
            handles.push(thread::spawn(move || {
                loop {
                    let job = {
                        let guard = rx.lock().unwrap();
                        guard.recv()
                    };
                    let Ok(job) = job else { break };
                    let result = (invoke)(job.handler_id, job.fields);
                    let _ = job.result_tx.send(result);
                }
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
        fields: HashMap<String, String>,
    ) -> Result<AhiruResponse, String> {
        let (result_tx, result_rx) = std::sync::mpsc::channel();
        self.tx
            .send(PoolJob {
                handler_id,
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

static GLOBAL_POOL: Mutex<Option<Arc<HandlerWorkerPool>>> = Mutex::new(None);

pub fn install_pool(pool: Arc<HandlerWorkerPool>) {
    *GLOBAL_POOL.lock().unwrap() = Some(pool);
}

pub fn global_pool() -> Option<Arc<HandlerWorkerPool>> {
    GLOBAL_POOL.lock().unwrap().clone()
}

pub fn clear_pool() {
    *GLOBAL_POOL.lock().unwrap() = None;
}
