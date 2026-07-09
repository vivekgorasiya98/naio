//! Shared background task pool for `io_*` and `net_*` async builtins.



use crate::{error_value, NiaoResult, RuntimeError, Value, ValueRef};

use niao_ast::Span;

use niao_errors::codes;

use std::collections::HashMap;

use std::sync::atomic::{AtomicU64, Ordering};

use std::sync::{mpsc, Arc, Condvar, Mutex, OnceLock};

use std::thread;



/// Result payload for a completed background task (all variants are `Send`).

#[derive(Clone)]

pub enum AsyncValue {

    Nil,

    Int(i64),

    Bool(bool),

    String(String),

    IntArray(Vec<i64>),

    ByteArray(Vec<u8>),

    Float(f64),

    Array(Vec<AsyncValue>),

    Object(HashMap<String, AsyncValue>),

}



impl AsyncValue {

    pub fn nil() -> Self {

        Self::Nil

    }



    pub fn int(n: i64) -> Self {

        Self::Int(n)

    }



    pub fn to_value(self) -> Value {

        match self {

            Self::Nil => Value::Nil,

            Self::Int(n) => Value::Int(n),

            Self::Bool(b) => Value::Bool(b),

            Self::String(s) => Value::String(s),

            Self::IntArray(v) => Value::IntArray(v),

            Self::ByteArray(v) => Value::ByteArray(v),

            Self::Float(f) => Value::Float(f),

            Self::Array(items) => {

                Value::Array(items.into_iter().map(|v| v.to_value().ref_cell()).collect())

            }

            Self::Object(map) => {

                let mut out = HashMap::with_capacity(map.len());

                for (k, v) in map {

                    out.insert(k, v.to_value().ref_cell());

                }

                Value::Object(out)

            }

        }

    }

}



pub(crate) enum AsyncState {

    Pending,

    Done(Result<AsyncValue, String>),

    Cancelled,

}



struct AsyncTaskInner {

    state: Mutex<AsyncState>,

    done: Condvar,

}



pub(crate) struct AsyncTask {

    inner: Arc<AsyncTaskInner>,

    #[allow(dead_code)]

    cancel: Option<mpsc::Sender<()>>,

}



struct ThreadPool {

    sender: mpsc::Sender<Box<dyn FnOnce() + Send + 'static>>,

}



static ASYNC_TASKS: OnceLock<Mutex<HashMap<u64, AsyncTask>>> = OnceLock::new();

static NEXT_ASYNC_TASK: AtomicU64 = AtomicU64::new(1);

static THREAD_POOL: OnceLock<ThreadPool> = OnceLock::new();

static TOKIO_RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

fn tokio_runtime() -> &'static tokio::runtime::Runtime {
    TOKIO_RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("niao-async")
            .build()
            .expect("failed to create niao async tokio runtime")
    })
}

pub(crate) fn async_tasks() -> &'static Mutex<HashMap<u64, AsyncTask>> {
    ASYNC_TASKS.get_or_init(|| Mutex::new(HashMap::new()))
}



fn thread_pool() -> &'static ThreadPool {

    THREAD_POOL.get_or_init(|| {

        let (sender, receiver) = mpsc::channel::<Box<dyn FnOnce() + Send + 'static>>();

        let receiver = Arc::new(Mutex::new(receiver));

        let workers = thread::available_parallelism()

            .map(|n| n.get())

            .unwrap_or(4)

            .clamp(2, 16);

        for _ in 0..workers {

            let receiver = Arc::clone(&receiver);

            thread::spawn(move || loop {

                let job = receiver.lock().unwrap().recv();

                match job {

                    Ok(job) => job(),

                    Err(_) => break,

                }

            });

        }

        ThreadPool { sender }

    })

}



fn finish_task(inner: &AsyncTaskInner, state: AsyncState) {

    let mut guard = inner.state.lock().unwrap();

    if matches!(*guard, AsyncState::Pending) {

        *guard = state;

        inner.done.notify_all();

    }

}



pub fn spawn_async<F>(work: F) -> u64

where

    F: FnOnce() -> Result<AsyncValue, String> + Send + 'static,

{

    let id = NEXT_ASYNC_TASK.fetch_add(1, Ordering::Relaxed);

    let inner = Arc::new(AsyncTaskInner {

        state: Mutex::new(AsyncState::Pending),

        done: Condvar::new(),

    });

    let (cancel_tx, cancel_rx) = mpsc::channel();

    async_tasks().lock().unwrap().insert(

        id,

        AsyncTask {

            inner: Arc::clone(&inner),

            cancel: Some(cancel_tx),

        },

    );



    let job_inner = Arc::clone(&inner);

    let _ = thread_pool().sender.send(Box::new(move || {

        if cancel_rx.try_recv().is_ok() {

            finish_task(&job_inner, AsyncState::Cancelled);

            return;

        }

        let result = work();

        finish_task(&job_inner, AsyncState::Done(result));

    }));



    id

}



/// Spawn an I/O-bound future on the shared Tokio runtime (no thread-pool cap).
pub fn spawn_tokio<F>(future: F) -> u64
where
    F: std::future::Future<Output = Result<AsyncValue, String>> + Send + 'static,
{
    let id = NEXT_ASYNC_TASK.fetch_add(1, Ordering::Relaxed);
    let inner = Arc::new(AsyncTaskInner {
        state: Mutex::new(AsyncState::Pending),
        done: Condvar::new(),
    });

    async_tasks().lock().unwrap().insert(
        id,
        AsyncTask {
            inner: Arc::clone(&inner),
            cancel: None,
        },
    );

    let job_inner = Arc::clone(&inner);
    tokio_runtime().spawn(async move {
        let result = future.await;
        finish_task(&job_inner, AsyncState::Done(result));
    });

    id
}



pub fn with_task<F>(

    id: u64,

    name: &str,

    span: Span,

    task_not_found_code: u32,

    _cancelled_msg: &str,

    _error_factory: impl Fn(Span, String) -> ValueRef,

    f: F,

) -> NiaoResult<ValueRef>

where

    F: FnOnce(&AsyncState) -> NiaoResult<ValueRef>,

{

    let guard = async_tasks().lock().unwrap();

    let task = guard.get(&id).ok_or_else(|| {

        RuntimeError::at(

            span,

            task_not_found_code,

            format!("{name}(): task {id} not found"),

        )

    })?;

    let state = task.inner.state.lock().unwrap();

    f(&state)

}



pub fn task_result_value(

    state: &AsyncState,

    span: Span,

    cancelled_msg: &str,

    error_factory: impl Fn(Span, String) -> ValueRef,

) -> ValueRef {

    match state {

        AsyncState::Pending => Value::Nil.ref_cell(),

        AsyncState::Cancelled => error_factory(span, cancelled_msg.into()),

        AsyncState::Done(Ok(v)) => v.clone().to_value().ref_cell(),

        AsyncState::Done(Err(msg)) => error_factory(span, msg.clone()),

    }

}



pub fn cancel_task(id: u64, span: Span, task_not_found_code: u32) -> NiaoResult<bool> {

    let mut guard = async_tasks().lock().unwrap();

    let task = guard.get_mut(&id).ok_or_else(|| {

        RuntimeError::at(

            span,

            task_not_found_code,

            format!("task_cancel(): task {id} not found"),

        )

    })?;

    let mut state = task.inner.state.lock().unwrap();

    if matches!(*state, AsyncState::Pending) {

        if let Some(tx) = task.cancel.take() {

            let _ = tx.send(());

        }

        *state = AsyncState::Cancelled;

        task.inner.done.notify_all();

        Ok(true)

    } else {

        Ok(false)

    }

}



pub fn task_done(state: &AsyncState) -> bool {

    matches!(state, AsyncState::Done(_) | AsyncState::Cancelled)

}



/// Block until all background tasks complete.
pub fn task_wait_all(ids: &[u64]) {
    for &id in ids {
        task_wait_loop(id);
    }
}



/// Block until a background task completes (Condvar — no busy spin).

pub fn task_wait_loop(id: u64) {

    let inner = {

        let guard = async_tasks().lock().unwrap();

        guard.get(&id).map(|t| Arc::clone(&t.inner))

    };

    let Some(inner) = inner else {

        return;

    };

    let mut state = inner.state.lock().unwrap();

    while !task_done(&state) {

        state = inner.done.wait(state).unwrap();

    }

}



/// Convert a recoverable async failure into an error value (io style).

pub fn async_io_error(span: Span, msg: impl Into<String>) -> ValueRef {

    error_value(

        codes::E1201_IO_ERROR,

        "io_error",

        msg.into(),

        span,

    )

}

