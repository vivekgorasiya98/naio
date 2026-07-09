//! Mutex primitives for shared sendable state across threads.

use super::common::{
    arity, arity_range, sendable_arg_or_err, function_arg, handle_arg, ok_bool, ok_int, ok_nil,
    parallel_error, sendable_result, ParallelResult,
};
use super::sendable::{sendable_to_value_ref, value_to_sendable, SendableValue};
use crate::{Value, ValueRef};
use niao_ast::Span;
use niao_errors::codes;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};

struct ParallelMutex {
    data: Arc<Mutex<SendableValue>>,
}

thread_local! {
    static MUTEXES: RefCell<HashMap<u64, ParallelMutex>> = RefCell::new(HashMap::new());
    static MUTEX_GUARDS: RefCell<HashMap<u64, (Arc<Mutex<SendableValue>>, MutexGuard<'static, SendableValue>)>> =
        RefCell::new(HashMap::new());
    static NEXT_MUTEX: Cell<u64> = const { Cell::new(1) };
}

fn alloc_mutex(initial: SendableValue) -> u64 {
    let id = NEXT_MUTEX.with(|n| {
        let id = n.get();
        n.set(id + 1);
        id
    });
    MUTEXES.with(|m| {
        m.borrow_mut().insert(
            id,
            ParallelMutex {
                data: Arc::new(Mutex::new(initial)),
            },
        );
    });
    id
}

fn with_mutex_entry<F, R>(id: u64, name: &str, span: Span, f: F) -> Result<R, crate::RuntimeError>
where
    F: FnOnce(&ParallelMutex) -> Result<R, ValueRef>,
{
    MUTEXES.with(|m| {
        let guard = m.borrow();
        let mutex = guard.get(&id).ok_or_else(|| {
            crate::RuntimeError::at(
                span,
                codes::E1503_PARALLEL_INVALID_HANDLE,
                format!("{name}(): invalid mutex handle {id}"),
            )
        })?;
        f(mutex).map_err(|v| {
            if let Value::Error(e) = &*v.borrow() {
                crate::RuntimeError::at(span, e.code, e.message.clone())
            } else {
                crate::RuntimeError::at(span, codes::E1501_PARALLEL_LOCK, "mutex operation failed")
            }
        })
    })
}

fn lock_guard(id: u64, arc: Arc<Mutex<SendableValue>>, span: Span) -> Result<(), ValueRef> {
  match arc.lock() {
        Ok(guard) => {
            // SAFETY: guard is stored with its owning Arc in thread-local storage until unlock.
            let guard = unsafe {
                std::mem::transmute::<MutexGuard<'_, SendableValue>, MutexGuard<'static, SendableValue>>(
                    guard,
                )
            };
            MUTEX_GUARDS.with(|m| m.borrow_mut().insert(id, (Arc::clone(&arc), guard)));
            Ok(())
        }
        Err(e) => Err(parallel_error(
            span,
            codes::E1501_PARALLEL_LOCK,
            format!("mutex lock poisoned: {e}"),
        )),
    }
}

fn is_locked(id: u64) -> bool {
    MUTEX_GUARDS.with(|m| m.borrow().contains_key(&id))
}

pub fn parallel_mutex_new(args: &[ValueRef], span: Span) -> ParallelResult {
    arity_range(args, 0, 1, "parallel_mutex_new", span)?;
    let initial = if args.is_empty() {
        SendableValue::Nil
    } else {
        sendable_arg_or_err(args, 0, "parallel_mutex_new", span)?
    };
    Ok(ok_int(alloc_mutex(initial) as i64))
}

pub fn parallel_mutex_lock(args: &[ValueRef], span: Span) -> ParallelResult {
    arity(args, 1, "parallel_mutex_lock", span)?;
    let id = handle_arg(args, 0, "parallel_mutex_lock", span)?;
    if is_locked(id) {
        return Ok(parallel_error(
            span,
            codes::E1501_PARALLEL_LOCK,
            "mutex already locked on this thread",
        ));
    }
    with_mutex_entry(id, "parallel_mutex_lock", span, |mutex| {
        lock_guard(id, Arc::clone(&mutex.data), span)?;
        Ok(ok_nil())
    })
}

pub fn parallel_mutex_unlock(args: &[ValueRef], span: Span) -> ParallelResult {
    arity(args, 1, "parallel_mutex_unlock", span)?;
    let id = handle_arg(args, 0, "parallel_mutex_unlock", span)?;
    let removed = MUTEX_GUARDS.with(|m| m.borrow_mut().remove(&id).is_some());
    if removed {
        Ok(ok_nil())
    } else {
        Ok(parallel_error(
            span,
            codes::E1501_PARALLEL_LOCK,
            "mutex unlock without matching lock",
        ))
    }
}

pub fn parallel_mutex_try_lock(args: &[ValueRef], span: Span) -> ParallelResult {
    arity(args, 1, "parallel_mutex_try_lock", span)?;
    let id = handle_arg(args, 0, "parallel_mutex_try_lock", span)?;
    if is_locked(id) {
        return Ok(ok_bool(true));
    }
    with_mutex_entry(id, "parallel_mutex_try_lock", span, |mutex| {
        match mutex.data.try_lock() {
            Ok(guard) => {
                let guard = unsafe {
                    std::mem::transmute::<
                        MutexGuard<'_, SendableValue>,
                        MutexGuard<'static, SendableValue>,
                    >(guard)
                };
                MUTEX_GUARDS.with(|m| {
                    m.borrow_mut()
                        .insert(id, (Arc::clone(&mutex.data), guard))
                });
                Ok(ok_bool(true))
            }
            Err(TryLockError::WouldBlock) => Ok(ok_bool(false)),
            Err(TryLockError::Poisoned(e)) => Ok(parallel_error(
                span,
                codes::E1501_PARALLEL_LOCK,
                format!("mutex lock poisoned: {e}"),
            )),
        }
    })
}

pub fn parallel_mutex_get(args: &[ValueRef], span: Span) -> ParallelResult {
    arity(args, 1, "parallel_mutex_get", span)?;
    let id = handle_arg(args, 0, "parallel_mutex_get", span)?;
    if is_locked(id) {
        return MUTEX_GUARDS.with(|m| {
            let guard = m.borrow();
            let (_, holder) = guard.get(&id).ok_or_else(|| {
                crate::RuntimeError::at(
                    span,
                    codes::E1501_PARALLEL_LOCK,
                    "mutex guard missing",
                )
            })?;
            Ok(sendable_result((*holder).clone()))
        });
    }
    with_mutex_entry(id, "parallel_mutex_get", span, |mutex| {
        let data = mutex.data.lock().map_err(|e| {
            parallel_error(
                span,
                codes::E1501_PARALLEL_LOCK,
                format!("mutex lock poisoned: {e}"),
            )
        })?;
        Ok(sendable_result(data.clone()))
    })
}

pub fn parallel_mutex_set(args: &[ValueRef], span: Span) -> ParallelResult {
    arity(args, 2, "parallel_mutex_set", span)?;
    let id = handle_arg(args, 0, "parallel_mutex_set", span)?;
    let val = sendable_arg_or_err(args, 1, "parallel_mutex_set", span)?;
    if is_locked(id) {
        return MUTEX_GUARDS.with(|m| {
            let mut guard = m.borrow_mut();
            let (_, holder) = guard.get_mut(&id).ok_or_else(|| {
                crate::RuntimeError::at(
                    span,
                    codes::E1501_PARALLEL_LOCK,
                    "mutex guard missing",
                )
            })?;
            **holder = val;
            Ok(ok_nil())
        });
    }
    with_mutex_entry(id, "parallel_mutex_set", span, |mutex| {
        let mut data = mutex.data.lock().map_err(|e| {
            parallel_error(
                span,
                codes::E1501_PARALLEL_LOCK,
                format!("mutex lock poisoned: {e}"),
            )
        })?;
        *data = val;
        Ok(ok_nil())
    })
}

pub fn parallel_mutex_run(args: &[ValueRef], span: Span) -> ParallelResult {
    arity(args, 2, "parallel_mutex_run", span)?;
    let _id = handle_arg(args, 0, "parallel_mutex_run", span)?;
    let _fn_val = function_arg(args, 1, "parallel_mutex_run", span)?;
    parallel_mutex_lock(&[args[0].clone()], span)?;
    let result = crate::call_niao_function(args[1].clone(), &[], span);
    let _ = parallel_mutex_unlock(&[args[0].clone()], span);
    match result {
        Ok(v) => match value_to_sendable(&v.borrow()) {
            Ok(s) => Ok(sendable_to_value_ref(s)),
            Err(msg) => Ok(parallel_error(
                span,
                codes::E1504_PARALLEL_NOT_SENDABLE,
                format!("mutex run result: {msg}"),
            )),
        },
        Err(e) => Err(e),
    }
}
