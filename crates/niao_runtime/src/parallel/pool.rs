//! Fixed-size worker pool for parallel Niao tasks.

use super::common::{
    arity, arity_min, function_arg, handle_arg, ok_int, ok_nil, parallel_error,
    sendable_args_rest_or_err, sendable_result, ParallelResult,
};
use super::poll::{enqueue_poll_job, should_use_poll_mode, wait_poll_job, PollJobResult};
use super::registry::{store_callee, take_callee};
use super::sendable::{sendable_to_value_ref, value_to_sendable, SendableValue};
use crate::{call_niao_function, Value, ValueRef};
use niao_ast::Span;
use niao_errors::codes;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;

struct PoolJob {
    id: u64,
    callee_id: u64,
    args: Vec<SendableValue>,
}

enum PoolTaskState {
    Pending,
    Running,
    Done(Result<SendableValue, String>),
}

struct PoolInner {
    workers: usize,
    queue: VecDeque<PoolJob>,
    shutdown: bool,
    active: usize,
    tasks: HashMap<u64, PoolTaskState>,
}

struct ParallelPool {
    inner: Arc<(Mutex<PoolInner>, Condvar)>,
    worker_handles: Mutex<Vec<thread::JoinHandle<()>>>,
}

thread_local! {
    static POOLS: RefCell<HashMap<u64, ParallelPool>> = RefCell::new(HashMap::new());
    static NEXT_POOL: Cell<u64> = const { Cell::new(1) };
}

static NEXT_POOL_TASK: AtomicU64 = AtomicU64::new(1);

fn alloc_pool_id() -> u64 {
    NEXT_POOL.with(|n| {
        let id = n.get();
        n.set(id + 1);
        id
    })
}

fn run_pool_job(job: PoolJob, span: Span) -> Result<SendableValue, String> {
    let callee = take_callee(job.callee_id).unwrap_or_else(|| Value::Nil.ref_cell());
    let args: Vec<ValueRef> = job.args.into_iter().map(sendable_to_value_ref).collect();
    match call_niao_function(callee, &args, span) {
        Ok(v) => value_to_sendable(&v.borrow()).map_err(|e| e),
        Err(e) => Err(e.to_string()),
    }
}

fn spawn_pool_workers(pool: &ParallelPool, span: Span) {
    let arc = Arc::clone(&pool.inner);
    let mut handles = Vec::new();
    let worker_count = {
        let (lock, _) = &*pool.inner;
        lock.lock().unwrap().workers
    };
    for _ in 0..worker_count {
        let arc = Arc::clone(&arc);
        handles.push(thread::spawn(move || {
            loop {
                let job = {
                    let (lock, cv) = &*arc;
                    let mut guard = lock.lock().unwrap();
                    while guard.queue.is_empty() && !guard.shutdown {
                        guard = cv.wait(guard).unwrap();
                    }
                    if guard.shutdown && guard.queue.is_empty() {
                        break;
                    }
                    let job = guard.queue.pop_front();
                    if job.is_some() {
                        guard.active += 1;
                    }
                    job
                };
                let Some(job) = job else {
                    continue;
                };
                let job_id = job.id;
                {
                    let (lock, _) = &*arc;
                    let mut guard = lock.lock().unwrap();
                    guard.tasks.insert(job_id, PoolTaskState::Running);
                }
                let result = run_pool_job(job, span);
                let (lock, cv) = &*arc;
                let mut guard = lock.lock().unwrap();
                guard.tasks.insert(job_id, PoolTaskState::Done(result));
                guard.active = guard.active.saturating_sub(1);
                cv.notify_all();
            }
        }));
    }
    *pool.worker_handles.lock().unwrap() = handles;
}

fn with_pool<F, R>(id: u64, name: &str, span: Span, f: F) -> Result<R, crate::RuntimeError>
where
    F: FnOnce(&ParallelPool) -> Result<R, ValueRef>,
{
    POOLS.with(|m| {
        let guard = m.borrow();
        let pool = guard.get(&id).ok_or_else(|| {
            crate::RuntimeError::at(
                span,
                codes::E1503_PARALLEL_INVALID_HANDLE,
                format!("{name}(): invalid pool handle {id}"),
            )
        })?;
        f(pool).map_err(|v| {
            if let Value::Error(e) = &*v.borrow() {
                crate::RuntimeError::at(span, e.code, e.message.clone())
            } else {
                crate::RuntimeError::at(span, codes::E1505_PARALLEL_NOT_FOUND, "pool operation failed")
            }
        })
    })
}

pub fn parallel_pool_new(args: &[ValueRef], span: Span) -> ParallelResult {
    arity(args, 1, "parallel_pool_new", span)?;
    let workers = super::common::int_arg(args, 0, "parallel_pool_new", span)?;
    if workers <= 0 {
        return Ok(parallel_error(
            span,
            codes::E1500_PARALLEL_ARITY,
            "parallel_pool_new() workers must be positive",
        ));
    }
    let id = alloc_pool_id();
    let inner = Arc::new((
        Mutex::new(PoolInner {
            workers: workers as usize,
            queue: VecDeque::new(),
            shutdown: false,
            active: 0,
            tasks: HashMap::new(),
        }),
        Condvar::new(),
    ));
    let pool = ParallelPool {
        inner,
        worker_handles: Mutex::new(Vec::new()),
    };
    if !should_use_poll_mode() {
        spawn_pool_workers(&pool, span);
    }
    POOLS.with(|m| m.borrow_mut().insert(id, pool));
    Ok(ok_int(id as i64))
}

pub fn parallel_pool_submit(args: &[ValueRef], span: Span) -> ParallelResult {
    arity_min(args, 2, "parallel_pool_submit", span)?;
    let pool_id = handle_arg(args, 0, "parallel_pool_submit", span)?;
    let callee = function_arg(args, 1, "parallel_pool_submit", span)?;
    let sendable_args = sendable_args_rest_or_err(args, 2, "parallel_pool_submit", span)?;
    let task_id = NEXT_POOL_TASK.fetch_add(1, Ordering::Relaxed);

    if should_use_poll_mode() {
        let poll_id = enqueue_poll_job(callee, sendable_args);
        return Ok(ok_int(poll_id as i64));
    }

    with_pool(pool_id, "parallel_pool_submit", span, |pool| {
        let (lock, cv) = &*pool.inner;
        let mut guard = lock.lock().unwrap();
        if guard.shutdown {
            return Ok(parallel_error(
                span,
                codes::E1505_PARALLEL_NOT_FOUND,
                "pool is shut down",
            ));
        }
        guard.tasks.insert(task_id, PoolTaskState::Pending);
        let callee_id = store_callee(callee);
        guard.queue.push_back(PoolJob {
            id: task_id,
            callee_id,
            args: sendable_args,
        });
        cv.notify_one();
        Ok(ok_int(task_id as i64))
    })
}

pub fn parallel_pool_wait(args: &[ValueRef], span: Span) -> ParallelResult {
    arity(args, 2, "parallel_pool_wait", span)?;
    let pool_id = handle_arg(args, 0, "parallel_pool_wait", span)?;
    let task_id = handle_arg(args, 1, "parallel_pool_wait", span)?;

    if should_use_poll_mode() || !POOLS.with(|m| m.borrow().contains_key(&pool_id)) {
        return match wait_poll_job(task_id) {
            PollJobResult::Done(Ok(v)) => Ok(sendable_result(v)),
            PollJobResult::Done(Err(msg)) => Ok(parallel_error(
                span,
                codes::E1505_PARALLEL_NOT_FOUND,
                msg,
            )),
            PollJobResult::Pending => Ok(parallel_error(
                span,
                codes::E1505_PARALLEL_NOT_FOUND,
                format!("task {task_id} pending — call parallel_poll()"),
            )),
        };
    }

    let pool = POOLS.with(|m| {
        m.borrow()
            .get(&pool_id)
            .map(|p| Arc::clone(&p.inner))
    });
    let Some(arc) = pool else {
        return Err(crate::RuntimeError::at(
            span,
            codes::E1503_PARALLEL_INVALID_HANDLE,
            format!("parallel_pool_wait(): invalid pool handle {pool_id}"),
        ));
    };

    loop {
        let done = {
            let (lock, _) = &*arc;
            let guard = lock.lock().unwrap();
            match guard.tasks.get(&task_id) {
                Some(PoolTaskState::Done(Ok(v))) => Some(Ok(v.clone())),
                Some(PoolTaskState::Done(Err(e))) => Some(Err(e.clone())),
                Some(PoolTaskState::Pending) | Some(PoolTaskState::Running) => None,
                None => {
                    return Ok(parallel_error(
                        span,
                        codes::E1505_PARALLEL_NOT_FOUND,
                        format!("task {task_id} not found"),
                    ));
                }
            }
        };
        if let Some(result) = done {
            return match result {
                Ok(v) => Ok(sendable_result(v)),
                Err(msg) => Ok(parallel_error(span, codes::E1505_PARALLEL_NOT_FOUND, msg)),
            };
        }
        thread::yield_now();
    }
}

pub fn parallel_pool_shutdown(args: &[ValueRef], span: Span) -> ParallelResult {
    arity(args, 1, "parallel_pool_shutdown", span)?;
    let pool_id = handle_arg(args, 0, "parallel_pool_shutdown", span)?;

    if should_use_poll_mode() {
        return Ok(ok_nil());
    }

    let pool = POOLS.with(|m| m.borrow_mut().remove(&pool_id));
    let Some(pool) = pool else {
        return Err(crate::RuntimeError::at(
            span,
            codes::E1503_PARALLEL_INVALID_HANDLE,
            format!("parallel_pool_shutdown(): invalid pool handle {pool_id}"),
        ));
    };

    {
        let (lock, cv) = &*pool.inner;
        let mut guard = lock.lock().unwrap();
        guard.shutdown = true;
        cv.notify_all();
    }

    let handles = pool.worker_handles.lock().unwrap().drain(..).collect::<Vec<_>>();
    for h in handles {
        let _ = h.join();
    }
    Ok(ok_nil())
}

pub fn parallel_pool_active(args: &[ValueRef], span: Span) -> ParallelResult {
    arity(args, 1, "parallel_pool_active", span)?;
    let pool_id = handle_arg(args, 0, "parallel_pool_active", span)?;
    with_pool(pool_id, "parallel_pool_active", span, |pool| {
        let (lock, _) = &*pool.inner;
        let guard = lock.lock().unwrap();
        Ok(ok_int(guard.active as i64))
    })
}

pub fn parallel_poll(args: &[ValueRef], span: Span) -> ParallelResult {
    arity(args, 0, "parallel_poll", span)?;
    let ran = super::poll::poll_one(span);
    Ok(super::common::ok_bool(ran))
}

pub fn parallel_poll_all(args: &[ValueRef], span: Span) -> ParallelResult {
    arity(args, 0, "parallel_poll_all", span)?;
    super::poll::poll_all(span);
    Ok(ok_nil())
}
