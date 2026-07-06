//! Main-thread job queue for cooperative poll-mode execution.

use super::sendable::{sendable_to_value_ref, SendableValue};
use crate::{call_neko_function, neko_call_hook_active, ValueRef};
use neko_ast::Span;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;

thread_local! {
    static POLL_JOBS: RefCell<VecDeque<PollJob>> = RefCell::new(VecDeque::new());
    static POLL_RESULTS: RefCell<std::collections::HashMap<u64, PollJobResult>> =
        RefCell::new(std::collections::HashMap::new());
}

static NEXT_POLL_JOB: AtomicU64 = AtomicU64::new(1);

pub struct PollJob {
    pub id: u64,
    pub callee: ValueRef,
    pub args: Vec<SendableValue>,
}

#[derive(Clone)]
pub enum PollJobResult {
    Pending,
    Done(Result<SendableValue, String>),
}

pub fn enqueue_poll_job(callee: ValueRef, args: Vec<SendableValue>) -> u64 {
    let id = NEXT_POLL_JOB.fetch_add(1, Ordering::Relaxed);
    POLL_RESULTS.with(|m| m.borrow_mut().insert(id, PollJobResult::Pending));
    POLL_JOBS.with(|q| {
        q.borrow_mut().push_back(PollJob { id, callee, args });
    });
    id
}

pub fn poll_one(span: Span) -> bool {
    let job = POLL_JOBS.with(|q| q.borrow_mut().pop_front());
    let Some(job) = job else {
        return false;
    };
    let args: Vec<ValueRef> = job.args.into_iter().map(sendable_to_value_ref).collect();
    let result = if neko_call_hook_active() {
        match call_neko_function(job.callee, &args, span) {
            Ok(v) => super::sendable::value_to_sendable(&v.borrow())
                .map_err(|e| format!("poll result not sendable: {e}")),
            Err(e) => Err(e.to_string()),
        }
    } else {
        Err("no Neko call hook registered for parallel_poll()".into())
    };
    POLL_RESULTS.with(|m| {
        m.borrow_mut()
            .insert(job.id, PollJobResult::Done(result));
    });
    true
}

pub fn poll_all(span: Span) {
    while poll_one(span) {}
}

pub fn wait_poll_job(id: u64) -> PollJobResult {
    loop {
        let done = POLL_RESULTS.with(|m| {
            m.borrow()
                .get(&id)
                .cloned()
                .filter(|r| !matches!(r, PollJobResult::Pending))
        });
        if let Some(res) = done {
            return res;
        }
        thread::yield_now();
    }
}

pub fn should_use_poll_mode() -> bool {
    !neko_call_hook_active()
}
