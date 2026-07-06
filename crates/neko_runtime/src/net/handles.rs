//! Thread-local handle table for sockets, TLS streams, HTTP servers, and WebSockets.

use super::socket::NetHandle;
use neko_errors::codes;
use neko_ast::Span;
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    static NEXT_HANDLE: RefCell<u64> = RefCell::new(1);
    static NET_HANDLES: RefCell<HashMap<u64, NetHandle>> = RefCell::new(HashMap::new());
}

pub fn alloc_handle(handle: NetHandle) -> u64 {
    let id = NEXT_HANDLE.with(|n| {
        let mut next = n.borrow_mut();
        let id = *next;
        *next = id + 1;
        id
    });
    NET_HANDLES.with(|m| m.borrow_mut().insert(id, handle));
    id
}

pub fn remove_handle(id: u64) -> Option<NetHandle> {
    NET_HANDLES.with(|m| m.borrow_mut().remove(&id))
}

pub fn with_handle_mut<F, R>(
    id: u64,
    name: &str,
    span: Span,
    f: F,
) -> Result<R, crate::RuntimeError>
where
    F: FnOnce(&mut NetHandle) -> Result<R, String>,
{
    NET_HANDLES.with(|m| {
        let mut guard = m.borrow_mut();
        let handle = guard.get_mut(&id).ok_or_else(|| {
            crate::RuntimeError::at(
                span,
                codes::E1402_NET_INVALID_HANDLE,
                format!("{name}(): invalid or closed handle {id}"),
            )
        })?;
        f(handle).map_err(|msg| {
            crate::RuntimeError::at(span, codes::E1401_NET_ERROR, format!("{name}(): {msg}"))
        })
    })
}
