//! Thread-local handle tables for SQLite connections and prepared statements.

use niao_ast::Span;
use niao_errors::codes;
use rusqlite::Connection;
use std::cell::RefCell;
use std::collections::HashMap as StdHashMap;
use std::path::PathBuf;

pub struct ConnHandle {
    pub conn: Connection,
    pub path: String,
}

pub struct StmtHandle {
    pub conn_id: u64,
    pub sql: String,
    pub params: Vec<(i32, crate::nsqlite::types::BoundValue)>,
    pub named_params: StdHashMap<String, crate::nsqlite::types::BoundValue>,
}

thread_local! {
    static NEXT_CONN: RefCell<u64> = RefCell::new(1);
    static NEXT_STMT: RefCell<u64> = RefCell::new(1);
    static CONNS: RefCell<StdHashMap<u64, ConnHandle>> = RefCell::new(StdHashMap::new());
    static STMTS: RefCell<StdHashMap<u64, StmtHandle>> = RefCell::new(StdHashMap::new());
}

pub fn alloc_conn(conn: Connection, path: String) -> u64 {
    let id = NEXT_CONN.with(|n| {
        let mut next = n.borrow_mut();
        let id = *next;
        *next = id + 1;
        id
    });
    CONNS.with(|m| {
        m.borrow_mut().insert(
            id,
            ConnHandle {
                conn,
                path,
            },
        );
    });
    id
}

pub fn remove_conn(id: u64) -> Option<ConnHandle> {
    STMTS.with(|m| {
        m.borrow_mut().retain(|_, stmt| stmt.conn_id != id);
    });
    CONNS.with(|m| m.borrow_mut().remove(&id))
}

pub fn conn_path(id: u64) -> Option<String> {
    CONNS.with(|m| m.borrow().get(&id).map(|c| c.path.clone()))
}

pub fn with_conn_mut<F, R>(id: u64, name: &str, span: Span, f: F) -> Result<R, crate::RuntimeError>
where
    F: FnOnce(&mut ConnHandle) -> Result<R, String>,
{
    CONNS.with(|m| {
        let mut guard = m.borrow_mut();
        let handle = guard.get_mut(&id).ok_or_else(|| {
            crate::RuntimeError::at(
                span,
                codes::E1702_NSQLITE_INVALID_HANDLE,
                format!("{name}(): invalid or closed connection handle {id}"),
            )
        })?;
        f(handle).map_err(|msg| {
            crate::RuntimeError::at(span, codes::E1701_NSQLITE_ERROR, format!("{name}(): {msg}"))
        })
    })
}

pub fn alloc_stmt(conn_id: u64, sql: String) -> u64 {
    let id = NEXT_STMT.with(|n| {
        let mut next = n.borrow_mut();
        let id = *next;
        *next = id + 1;
        id
    });
    STMTS.with(|m| {
        m.borrow_mut().insert(
            id,
            StmtHandle {
                conn_id,
                sql,
                params: Vec::new(),
                named_params: StdHashMap::new(),
            },
        );
    });
    id
}

pub fn remove_stmt(id: u64) -> Option<StmtHandle> {
    STMTS.with(|m| m.borrow_mut().remove(&id))
}

pub fn with_stmt_mut<F, R>(id: u64, name: &str, span: Span, f: F) -> Result<R, crate::RuntimeError>
where
    F: FnOnce(&mut StmtHandle) -> Result<R, String>,
{
    STMTS.with(|m| {
        let mut guard = m.borrow_mut();
        let handle = guard.get_mut(&id).ok_or_else(|| {
            crate::RuntimeError::at(
                span,
                codes::E1702_NSQLITE_INVALID_HANDLE,
                format!("{name}(): invalid or finalized statement handle {id}"),
            )
        })?;
        f(handle).map_err(|msg| {
            crate::RuntimeError::at(span, codes::E1701_NSQLITE_ERROR, format!("{name}(): {msg}"))
        })
    })
}

pub fn with_stmt_and_conn<F, R>(stmt_id: u64, name: &str, span: Span, f: F) -> Result<R, crate::RuntimeError>
where
    F: FnOnce(&mut StmtHandle, &mut ConnHandle) -> Result<R, String>,
{
    CONNS.with(|cm| {
        STMTS.with(|sm| {
            let mut cg = cm.borrow_mut();
            let mut sg = sm.borrow_mut();
            let stmt = sg.get_mut(&stmt_id).ok_or_else(|| {
                crate::RuntimeError::at(
                    span,
                    codes::E1702_NSQLITE_INVALID_HANDLE,
                    format!("{name}(): invalid statement handle {stmt_id}"),
                )
            })?;
            let conn_id = stmt.conn_id;
            let conn = cg.get_mut(&conn_id).ok_or_else(|| {
                crate::RuntimeError::at(
                    span,
                    codes::E1702_NSQLITE_INVALID_HANDLE,
                    format!("{name}(): invalid connection handle {conn_id}"),
                )
            })?;
            f(stmt, conn).map_err(|msg| {
                crate::RuntimeError::at(span, codes::E1701_NSQLITE_ERROR, format!("{name}(): {msg}"))
            })
        })
    })
}

pub fn resolve_db_path(path: &str, use_cwd: bool) -> Result<PathBuf, String> {
    if path == ":memory:" {
        return Ok(PathBuf::from(":memory:"));
    }
    let p = PathBuf::from(path);
    if use_cwd && !p.is_absolute() {
        Ok(std::env::current_dir()
            .map_err(|e| e.to_string())?
            .join(p))
    } else {
        Ok(p)
    }
}

pub fn open_connection(path: &str, use_cwd: bool) -> Result<(Connection, String), String> {
    let resolved = resolve_db_path(path, use_cwd)?;
    let display = if resolved == PathBuf::from(":memory:") {
        ":memory:".to_string()
    } else {
        resolved.to_string_lossy().into_owned()
    };
    let conn = Connection::open(&resolved).map_err(|e| e.to_string())?;
    Ok((conn, display))
}

pub fn apply_default_pragmas(conn: &Connection) -> Result<(), String> {
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(|e| e.to_string())?;
    conn.pragma_update(None, "synchronous", "NORMAL")
        .map_err(|e| e.to_string())?;
    conn.pragma_update(None, "foreign_keys", "ON")
        .map_err(|e| e.to_string())?;
    conn.pragma_update(None, "cache_size", -64000i64)
        .map_err(|e| e.to_string())?;
    Ok(())
}
