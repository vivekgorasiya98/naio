//! Change stream watch handles.

use super::common::*;
use super::runtime::runtime;
use super::types::bson_doc_to_niao_ref;
use crate::{error_value, NiaoResult, RuntimeError, Value, ValueRef};
use futures::StreamExt;
use mongodb::bson::Document;
use niao_ast::Span;
use niao_errors::codes;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

struct WatchHandle {
    sender: mpsc::Sender<()>,
    events: Arc<Mutex<Vec<Document>>>,
    error: Arc<Mutex<Option<String>>>,
    closed: Arc<Mutex<bool>>,
}

thread_local! {
    static NEXT_WATCH: RefCell<u64> = RefCell::new(1);
    static WATCHES: RefCell<HashMap<u64, Arc<WatchHandle>>> = RefCell::new(HashMap::new());
}

fn alloc_watch(handle: WatchHandle) -> u64 {
    let id = NEXT_WATCH.with(|n| {
        let mut next = n.borrow_mut();
        let wid = *next;
        *next = wid + 1;
        wid
    });
    WATCHES.with(|m| m.borrow_mut().insert(id, Arc::new(handle)));
    id
}

fn remove_watch(id: u64) -> Option<Arc<WatchHandle>> {
    WATCHES.with(|m| m.borrow_mut().remove(&id))
}

fn get_watch(id: u64) -> Option<Arc<WatchHandle>> {
    WATCHES.with(|m| m.borrow().get(&id).cloned())
}

pub fn nmongo_watch(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity_range(args, 3, 5, "nmongo_watch", span)?;
    let (client_id, db, coll) = db_coll_args_range(args, 3, 5, "nmongo_watch", span)?;
    let pipeline = if args.len() >= 4 {
        match &*args[3].borrow() {
            Value::Array(_) => {
                let arr = array_arg(args, 3, "nmongo_watch", span)?;
                let mut stages = Vec::with_capacity(arr.len());
                for s in &arr {
                    stages.push(super::types::niao_to_bson(s, span)?);
                }
                stages
            }
            _ => Vec::new(),
        }
    } else {
        Vec::new()
    };

    let client = super::handles::client_clone(client_id).ok_or_else(|| {
        RuntimeError::at(
            span,
            codes::E1922_NMONGO_INVALID_HANDLE,
            "invalid client handle",
        )
    })?;

    let events: Arc<Mutex<Vec<Document>>> = Arc::new(Mutex::new(Vec::new()));
    let error: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let closed: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
    let (stop_tx, stop_rx) = mpsc::channel::<()>();

    let events_bg = Arc::clone(&events);
    let error_bg = Arc::clone(&error);
    let closed_bg = Arc::clone(&closed);
    let db_bg = db.clone();
    let coll_bg = coll.clone();

    thread::spawn(move || {
        let rt = runtime();
        let result = rt.block_on(async {
            let collection = client.database(&db_bg).collection::<Document>(&coll_bg);
            let mut stream = if pipeline.is_empty() {
                collection.watch().await?
            } else {
                collection.watch().pipeline(pipeline).await?
            };

            loop {
                if stop_rx.try_recv().is_ok() {
                    break;
                }
                match tokio::time::timeout(std::time::Duration::from_millis(200), stream.next())
                    .await
                {
                    Ok(Some(Ok(change))) => {
                        if let Ok(doc) = bson::to_document(&change) {
                            events_bg.lock().unwrap().push(doc);
                        }
                    }
                    Ok(Some(Err(e))) => {
                        *error_bg.lock().unwrap() = Some(e.to_string());
                        break;
                    }
                    Ok(None) => break,
                    Err(_) => continue,
                }
            }
            Ok::<(), mongodb::error::Error>(())
        });
        if let Err(e) = result {
            *error_bg.lock().unwrap() = Some(e.to_string());
        }
        *closed_bg.lock().unwrap() = true;
    });

    let id = alloc_watch(WatchHandle {
        sender: stop_tx,
        events,
        error,
        closed,
    });
    Ok(Value::Int(id as i64).ref_cell())
}

pub fn nmongo_watch_next(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "nmongo_watch_next", span)?;
    let watch_id = match &*args[0].borrow() {
        Value::Int(id) if *id > 0 => *id as u64,
        other => {
            return Err(RuntimeError::at(
                span,
                codes::E1922_NMONGO_INVALID_HANDLE,
                format!("nmongo_watch_next() expects watch id, got {}", other.type_name()),
            ));
        }
    };

    let Some(handle) = get_watch(watch_id) else {
        return Ok(error_value(
            codes::E1928_NMONGO_CHANGE_STREAM,
            "nmongo_error",
            format!("invalid watch handle {watch_id}"),
            span,
        ));
    };

    if let Some(err) = handle.error.lock().unwrap().clone() {
        return Ok(error_value(
            codes::E1928_NMONGO_CHANGE_STREAM,
            "nmongo_error",
            err,
            span,
        ));
    }

    let mut queue = handle.events.lock().unwrap();
    if let Some(doc) = queue.first() {
        let event = bson_doc_to_niao_ref(doc);
        queue.remove(0);
        return Ok(event);
    }

    if *handle.closed.lock().unwrap() {
        return Ok(Value::Nil.ref_cell());
    }

    Ok(Value::Nil.ref_cell())
}

pub fn nmongo_watch_close(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "nmongo_watch_close", span)?;
    let watch_id = match &*args[0].borrow() {
        Value::Int(id) if *id > 0 => *id as u64,
        other => {
            return Err(RuntimeError::at(
                span,
                codes::E1922_NMONGO_INVALID_HANDLE,
                format!("nmongo_watch_close() expects watch id, got {}", other.type_name()),
            ));
        }
    };
    if let Some(handle) = remove_watch(watch_id) {
        let _ = handle.sender.send(());
    }
    Ok(Value::Nil.ref_cell())
}
