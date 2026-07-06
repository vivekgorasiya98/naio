//! OS thread spawn/join for Neko callbacks.

use super::common::{
    arity, arity_min, function_arg, handle_arg, ok_bool, ok_int, ok_nil, parallel_error,
    sendable_args_rest_or_err, sendable_result, ParallelResult,
};
use super::poll::{enqueue_poll_job, should_use_poll_mode, wait_poll_job, PollJobResult};
use super::registry::{store_callee, take_callee};
use super::sendable::{sendable_to_value_ref, value_to_sendable, SendableValue};
use crate::{call_neko_function, Value, ValueRef};
use neko_ast::Span;
use neko_errors::codes;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

enum ThreadState {
    Running,
    Finished(Result<SendableValue, String>),
    Detached,
}

struct ParallelThread {
    state: Arc<Mutex<ThreadState>>,
    join_handle: Mutex<Option<JoinHandle<()>>>,
}

thread_local! {
    static THREADS: RefCell<HashMap<u64, ParallelThread>> = RefCell::new(HashMap::new());
    static NEXT_THREAD: Cell<u64> = const { Cell::new(1) };
}

static THREAD_COUNTER: AtomicU64 = AtomicU64::new(0);

fn alloc_thread_id() -> u64 {
    NEXT_THREAD.with(|n| {
        let id = n.get();
        n.set(id + 1);
        id
    })
}

fn spawn_neko_thread(
    callee: ValueRef,
    sendable_args: Vec<SendableValue>,
    span: Span,
) -> ParallelThread {
    let state = Arc::new(Mutex::new(ThreadState::Running));
    let state_clone = Arc::clone(&state);
    let callee_id = store_callee(callee);
    let handle = thread::spawn(move || {
        let callee = take_callee(callee_id).unwrap_or_else(|| Value::Nil.ref_cell());
        let args: Vec<ValueRef> = sendable_args
            .into_iter()
            .map(sendable_to_value_ref)
            .collect();
        let result = match call_neko_function(callee, &args, span) {
            Ok(v) => value_to_sendable(&v.borrow()).map_err(|e| e),
            Err(e) => Err(e.to_string()),
        };
        if let Ok(mut guard) = state_clone.lock() {
            *guard = ThreadState::Finished(result);
        }
    });
    ParallelThread {
        state,
        join_handle: Mutex::new(Some(handle)),
    }
}

fn with_thread<F, R>(id: u64, name: &str, span: Span, f: F) -> Result<R, crate::RuntimeError>
where
    F: FnOnce(&ParallelThread) -> Result<R, ValueRef>,
{
    THREADS.with(|m| {
        let guard = m.borrow();
        let t = guard.get(&id).ok_or_else(|| {
            crate::RuntimeError::at(
                span,
                codes::E1505_PARALLEL_NOT_FOUND,
                format!("{name}(): thread {id} not found"),
            )
        })?;
        f(t).map_err(|v| {
            if let Value::Error(e) = &*v.borrow() {
                crate::RuntimeError::at(span, e.code, e.message.clone())
            } else {
                crate::RuntimeError::at(span, codes::E1505_PARALLEL_NOT_FOUND, "thread operation failed")
            }
        })
    })
}

pub fn parallel_thread_spawn(args: &[ValueRef], span: Span) -> ParallelResult {
    arity_min(args, 1, "parallel_thread_spawn", span)?;
    let callee = function_arg(args, 0, "parallel_thread_spawn", span)?;
    let sendable_args = sendable_args_rest_or_err(args, 1, "parallel_thread_spawn", span)?;

    if should_use_poll_mode() {
        let id = enqueue_poll_job(callee, sendable_args);
        return Ok(ok_int(id as i64));
    }

    let id = alloc_thread_id();
    let thread = spawn_neko_thread(callee, sendable_args, span);
    THREADS.with(|m| m.borrow_mut().insert(id, thread));
    Ok(ok_int(id as i64))
}

pub fn parallel_thread_join(args: &[ValueRef], span: Span) -> ParallelResult {
    arity(args, 1, "parallel_thread_join", span)?;
    let id = handle_arg(args, 0, "parallel_thread_join", span)?;

    if should_use_poll_mode() || !THREADS.with(|m| m.borrow().contains_key(&id)) {
        return match wait_poll_job(id) {
            PollJobResult::Done(Ok(v)) => Ok(sendable_result(v)),
            PollJobResult::Done(Err(msg)) => Ok(parallel_error(
                span,
                codes::E1505_PARALLEL_NOT_FOUND,
                msg,
            )),
            PollJobResult::Pending => Ok(parallel_error(
                span,
                codes::E1505_PARALLEL_NOT_FOUND,
                format!("thread {id} still pending — call parallel_poll()"),
            )),
        };
    }

    let thread = THREADS.with(|m| m.borrow_mut().remove(&id));
    let Some(thread) = thread else {
        return Err(crate::RuntimeError::at(
            span,
            codes::E1505_PARALLEL_NOT_FOUND,
            format!("parallel_thread_join(): thread {id} not found"),
        ));
    };

    if let Some(handle) = thread.join_handle.lock().unwrap().take() {
        let _ = handle.join();
    }

    let state = thread.state.lock().unwrap();
    match &*state {
        ThreadState::Finished(Ok(v)) => Ok(sendable_result(v.clone())),
        ThreadState::Finished(Err(msg)) => Ok(parallel_error(
            span,
            codes::E1505_PARALLEL_NOT_FOUND,
            msg.clone(),
        )),
        ThreadState::Detached => Ok(parallel_error(
            span,
            codes::E1505_PARALLEL_NOT_FOUND,
            "cannot join detached thread",
        )),
        ThreadState::Running => Ok(parallel_error(
            span,
            codes::E1505_PARALLEL_NOT_FOUND,
            "thread still running",
        )),
    }
}

pub fn parallel_thread_detach(args: &[ValueRef], span: Span) -> ParallelResult {
    arity(args, 1, "parallel_thread_detach", span)?;
    let id = handle_arg(args, 0, "parallel_thread_detach", span)?;
    with_thread(id, "parallel_thread_detach", span, |t| {
        if let Ok(mut guard) = t.state.lock() {
            *guard = ThreadState::Detached;
        }
        *t.join_handle.lock().unwrap() = None;
        Ok(ok_nil())
    })
}

pub fn parallel_thread_is_alive(args: &[ValueRef], span: Span) -> ParallelResult {
    arity(args, 1, "parallel_thread_is_alive", span)?;
    let id = handle_arg(args, 0, "parallel_thread_is_alive", span)?;

    if !THREADS.with(|m| m.borrow().contains_key(&id)) {
        return Ok(ok_bool(false));
    }

    with_thread(id, "parallel_thread_is_alive", span, |t| {
        let alive = matches!(*t.state.lock().unwrap(), ThreadState::Running);
        Ok(ok_bool(alive))
    })
}

pub fn parallel_thread_id(_args: &[ValueRef], _span: Span) -> ParallelResult {
    let id = THREAD_COUNTER.fetch_add(1, Ordering::Relaxed);
    Ok(ok_int(id as i64))
}

pub fn parallel_thread_yield(_args: &[ValueRef], _span: Span) -> ParallelResult {
    thread::yield_now();
    Ok(ok_nil())
}

pub fn parallel_thread_sleep(args: &[ValueRef], span: Span) -> ParallelResult {
    arity(args, 1, "parallel_thread_sleep", span)?;
    let ms = super::common::int_arg(args, 0, "parallel_thread_sleep", span)?;
    if ms < 0 {
        return Ok(parallel_error(
            span,
            codes::E1500_PARALLEL_ARITY,
            "parallel_thread_sleep() expects non-negative milliseconds",
        ));
    }
    thread::sleep(std::time::Duration::from_millis(ms as u64));
    Ok(ok_nil())
}

pub fn parallel_cpu_count(_args: &[ValueRef], _span: Span) -> ParallelResult {
    let n = thread::available_parallelism()
        .map(|n| n.get() as i64)
        .unwrap_or(1);
    Ok(ok_int(n))
}
