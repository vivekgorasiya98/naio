//! Multi-document transaction session management.

use super::common::*;
use super::handles::{alloc_session, remove_session, with_client, with_session_mut};
use super::runtime::block_on;
use crate::{error_from_runtime, NiaoResult, Value, ValueRef};
use mongodb::options::TransactionOptions;
use niao_ast::Span;

fn ok_int(n: i64) -> ValueRef {
    Value::Int(n).ref_cell()
}

fn ok_nil() -> ValueRef {
    Value::Nil.ref_cell()
}

pub fn nmongo_start_session(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "nmongo_start_session", span)?;
    let client_id = client_arg(args, 0, "nmongo_start_session", span)?;

    with_client(client_id, "nmongo_start_session", span, |client| {
        block_on(async move {
            let session = client.start_session().await.map_err(|e| e.to_string())?;
            Ok(session)
        })
    })
    .map(|session| {
        let sid = alloc_session(client_id, session);
        ok_int(sid as i64)
    })
    .or_else(|e| Ok(error_from_runtime(&e)))
}

pub fn nmongo_start_transaction(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity_range(args, 1, 2, "nmongo_start_transaction", span)?;
    let session_id = session_arg(args, 0, "nmongo_start_transaction", span)?;
    let tx_opts = TransactionOptions::builder().build();

    with_session_mut(session_id, "nmongo_start_transaction", span, |session| {
        block_on(async move {
            session
                .start_transaction()
                .with_options(tx_opts)
                .await
                .map_err(|e| e.to_string())
        })
    })
    .map(|_| ok_nil())
    .or_else(|e| Ok(error_from_runtime(&e)))
}

pub fn nmongo_commit_transaction(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "nmongo_commit_transaction", span)?;
    let session_id = session_arg(args, 0, "nmongo_commit_transaction", span)?;

    with_session_mut(session_id, "nmongo_commit_transaction", span, |session| {
        block_on(async move {
            session
                .commit_transaction()
                .await
                .map_err(|e| e.to_string())
        })
    })
    .map(|_| ok_nil())
    .or_else(|e| Ok(error_from_runtime(&e)))
}

pub fn nmongo_abort_transaction(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "nmongo_abort_transaction", span)?;
    let session_id = session_arg(args, 0, "nmongo_abort_transaction", span)?;

    with_session_mut(session_id, "nmongo_abort_transaction", span, |session| {
        block_on(async move {
            session
                .abort_transaction()
                .await
                .map_err(|e| e.to_string())
        })
    })
    .map(|_| ok_nil())
    .or_else(|e| Ok(error_from_runtime(&e)))
}

pub fn nmongo_end_session(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "nmongo_end_session", span)?;
    let session_id = session_arg(args, 0, "nmongo_end_session", span)?;
    remove_session(session_id);
    Ok(ok_nil())
}
