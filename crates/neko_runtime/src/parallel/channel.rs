//! Channel primitives for message passing between threads.

use super::common::{
    arity, arity_range, handle_arg, ok_bool, ok_int, ok_nil, parallel_error, sendable_arg_or_err,
    sendable_result, ParallelResult,
};
use super::sendable::SendableValue;
use crate::{Value, ValueRef};
use neko_ast::Span;
use neko_errors::codes;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::mpsc::{self, RecvTimeoutError, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

enum ChannelKind {
    Unbounded {
        tx: mpsc::Sender<SendableValue>,
        rx: Arc<Mutex<mpsc::Receiver<SendableValue>>>,
    },
    Bounded {
        tx: mpsc::SyncSender<SendableValue>,
        rx: Arc<Mutex<mpsc::Receiver<SendableValue>>>,
        #[allow(dead_code)]
        capacity: usize,
    },
}

struct ParallelChannel {
    kind: ChannelKind,
    closed: Cell<bool>,
}

thread_local! {
    static CHANNELS: RefCell<HashMap<u64, ParallelChannel>> = RefCell::new(HashMap::new());
    static NEXT_CHANNEL: Cell<u64> = const { Cell::new(1) };
}

fn alloc_channel(capacity: Option<usize>) -> u64 {
    let id = NEXT_CHANNEL.with(|n| {
        let id = n.get();
        n.set(id + 1);
        id
    });
    let kind = if let Some(cap) = capacity {
        let (tx, rx) = mpsc::sync_channel(cap);
        ChannelKind::Bounded {
            tx,
            rx: Arc::new(Mutex::new(rx)),
            capacity: cap,
        }
    } else {
        let (tx, rx) = mpsc::channel();
        ChannelKind::Unbounded {
            tx,
            rx: Arc::new(Mutex::new(rx)),
        }
    };
    CHANNELS.with(|m| {
        m.borrow_mut().insert(
            id,
            ParallelChannel {
                kind,
                closed: Cell::new(false),
            },
        );
    });
    id
}

fn with_channel<F, R>(id: u64, name: &str, span: Span, f: F) -> Result<R, crate::RuntimeError>
where
    F: FnOnce(&ParallelChannel) -> Result<R, ValueRef>,
{
    CHANNELS.with(|m| {
        let guard = m.borrow();
        let ch = guard.get(&id).ok_or_else(|| {
            crate::RuntimeError::at(
                span,
                codes::E1503_PARALLEL_INVALID_HANDLE,
                format!("{name}(): invalid channel handle {id}"),
            )
        })?;
        f(ch).map_err(|v| {
            if let Value::Error(e) = &*v.borrow() {
                crate::RuntimeError::at(span, e.code, e.message.clone())
            } else {
                crate::RuntimeError::at(span, codes::E1502_PARALLEL_CHANNEL, "channel operation failed")
            }
        })
    })
}

fn channel_tx_unbounded(ch: &ParallelChannel) -> Option<&mpsc::Sender<SendableValue>> {
    match &ch.kind {
        ChannelKind::Unbounded { tx, .. } => Some(tx),
        ChannelKind::Bounded { .. } => None,
    }
}

fn channel_tx_bounded(ch: &ParallelChannel) -> Option<&mpsc::SyncSender<SendableValue>> {
    match &ch.kind {
        ChannelKind::Bounded { tx, .. } => Some(tx),
        ChannelKind::Unbounded { .. } => None,
    }
}

fn channel_rx(ch: &ParallelChannel) -> Arc<Mutex<mpsc::Receiver<SendableValue>>> {
    match &ch.kind {
        ChannelKind::Unbounded { rx, .. } | ChannelKind::Bounded { rx, .. } => Arc::clone(rx),
    }
}

pub fn parallel_channel_new(args: &[ValueRef], span: Span) -> ParallelResult {
    arity_range(args, 0, 1, "parallel_channel_new", span)?;
    let capacity = if args.is_empty() {
        None
    } else {
        let n = super::common::int_arg(args, 0, "parallel_channel_new", span)?;
        if n <= 0 {
            return Ok(parallel_error(
                span,
                codes::E1500_PARALLEL_ARITY,
                "parallel_channel_new() capacity must be positive",
            ));
        }
        Some(n as usize)
    };
    Ok(ok_int(alloc_channel(capacity) as i64))
}

pub fn parallel_channel_send(args: &[ValueRef], span: Span) -> ParallelResult {
    arity(args, 2, "parallel_channel_send", span)?;
    let id = handle_arg(args, 0, "parallel_channel_send", span)?;
    let val = sendable_arg_or_err(args, 1, "parallel_channel_send", span)?;
    with_channel(id, "parallel_channel_send", span, |ch| {
        if ch.closed.get() {
            return Ok(parallel_error(
                span,
                codes::E1502_PARALLEL_CHANNEL,
                "channel is closed",
            ));
        }
        let send_ok = if let Some(tx) = channel_tx_unbounded(ch) {
            tx.send(val).is_ok()
        } else if let Some(tx) = channel_tx_bounded(ch) {
            tx.send(val).is_ok()
        } else {
            false
        };
        if send_ok {
            Ok(ok_nil())
        } else {
            ch.closed.set(true);
            Ok(parallel_error(
                span,
                codes::E1502_PARALLEL_CHANNEL,
                "channel is closed",
            ))
        }
    })
}

pub fn parallel_channel_recv(args: &[ValueRef], span: Span) -> ParallelResult {
    arity(args, 1, "parallel_channel_recv", span)?;
    let id = handle_arg(args, 0, "parallel_channel_recv", span)?;
    with_channel(id, "parallel_channel_recv", span, |ch| {
        let rx = channel_rx(ch);
        let guard = rx.lock().map_err(|e| {
            parallel_error(
                span,
                codes::E1502_PARALLEL_CHANNEL,
                format!("channel receiver poisoned: {e}"),
            )
        })?;
        match guard.recv() {
            Ok(v) => Ok(sendable_result(v)),
            Err(_) => {
                ch.closed.set(true);
                Ok(parallel_error(
                    span,
                    codes::E1502_PARALLEL_CHANNEL,
                    "channel is closed",
                ))
            }
        }
    })
}

pub fn parallel_channel_try_recv(args: &[ValueRef], span: Span) -> ParallelResult {
    arity(args, 1, "parallel_channel_try_recv", span)?;
    let id = handle_arg(args, 0, "parallel_channel_try_recv", span)?;
    with_channel(id, "parallel_channel_try_recv", span, |ch| {
        let rx = channel_rx(ch);
        let guard = rx.lock().map_err(|e| {
            parallel_error(
                span,
                codes::E1502_PARALLEL_CHANNEL,
                format!("channel receiver poisoned: {e}"),
            )
        })?;
        match guard.try_recv() {
            Ok(v) => Ok(sendable_result(v)),
            Err(TryRecvError::Empty) => Ok(ok_nil()),
            Err(TryRecvError::Disconnected) => {
                ch.closed.set(true);
                Ok(parallel_error(
                    span,
                    codes::E1502_PARALLEL_CHANNEL,
                    "channel is closed",
                ))
            }
        }
    })
}

pub fn parallel_channel_close(args: &[ValueRef], span: Span) -> ParallelResult {
    arity(args, 1, "parallel_channel_close", span)?;
    let id = handle_arg(args, 0, "parallel_channel_close", span)?;
    CHANNELS.with(|m| {
        if let Some(ch) = m.borrow_mut().remove(&id) {
            ch.closed.set(true);
            let _ = channel_tx_unbounded(&ch);
            let _ = channel_tx_bounded(&ch);
            Ok(ok_nil())
        } else {
            Err(crate::RuntimeError::at(
                span,
                codes::E1503_PARALLEL_INVALID_HANDLE,
                format!("parallel_channel_close(): invalid channel handle {id}"),
            ))
        }
    })
}

pub fn parallel_channel_recv_timeout(args: &[ValueRef], span: Span) -> ParallelResult {
    arity(args, 2, "parallel_channel_recv_timeout", span)?;
    let id = handle_arg(args, 0, "parallel_channel_recv_timeout", span)?;
    let ms = super::common::int_arg(args, 1, "parallel_channel_recv_timeout", span)?;
    if ms < 0 {
        return Ok(parallel_error(
            span,
            codes::E1500_PARALLEL_ARITY,
            "parallel_channel_recv_timeout() timeout must be non-negative",
        ));
    }
    with_channel(id, "parallel_channel_recv_timeout", span, |ch| {
        let rx = channel_rx(ch);
        let guard = rx.lock().map_err(|e| {
            parallel_error(
                span,
                codes::E1502_PARALLEL_CHANNEL,
                format!("channel receiver poisoned: {e}"),
            )
        })?;
        match guard.recv_timeout(Duration::from_millis(ms as u64)) {
            Ok(v) => Ok(sendable_result(v)),
            Err(RecvTimeoutError::Timeout) => Ok(ok_nil()),
            Err(RecvTimeoutError::Disconnected) => {
                ch.closed.set(true);
                Ok(parallel_error(
                    span,
                    codes::E1502_PARALLEL_CHANNEL,
                    "channel is closed",
                ))
            }
        }
    })
}

pub fn parallel_channel_is_closed(args: &[ValueRef], span: Span) -> ParallelResult {
    arity(args, 1, "parallel_channel_is_closed", span)?;
    let id = handle_arg(args, 0, "parallel_channel_is_closed", span)?;
    with_channel(id, "parallel_channel_is_closed", span, |ch| Ok(ok_bool(ch.closed.get())))
}
