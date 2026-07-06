//! Schema migrations and introspection.

use super::handles::ConnHandle;
use super::query::exec_on_conn;
use super::types::BoundValue;
use crate::{Value, ValueRef};
use neko_ast::Span;
use neko_errors::codes;
use std::collections::HashMap;

const MIGRATIONS_TABLE: &str = "_nsqlite_schema_version";

pub fn ensure_migrations_table(conn: &mut ConnHandle) -> Result<(), String> {
    exec_on_conn(
        conn,
        &format!(
            "CREATE TABLE IF NOT EXISTS {MIGRATIONS_TABLE} (version INTEGER PRIMARY KEY NOT NULL)"
        ),
        &[],
    )
}

pub fn current_version(conn: &mut ConnHandle) -> Result<i64, String> {
    ensure_migrations_table(conn)?;
    let mut stmt = conn
        .conn
        .prepare(&format!("SELECT version FROM {MIGRATIONS_TABLE} ORDER BY version DESC LIMIT 1"))
        .map_err(|e| e.to_string())?;
    let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
    if let Some(row) = rows.next().map_err(|e| e.to_string())? {
        row.get(0).map_err(|e| e.to_string())
    } else {
        Ok(0)
    }
}

pub fn set_version(conn: &mut ConnHandle, version: i64) -> Result<(), String> {
    exec_on_conn(
        conn,
        &format!("INSERT INTO {MIGRATIONS_TABLE} (version) VALUES (?)"),
        &[BoundValue::Int(version)],
    )
}

pub fn table_exists(conn: &mut ConnHandle, name: &str) -> Result<bool, String> {
    let mut stmt = conn
        .conn
        .prepare("SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1 LIMIT 1")
        .map_err(|e| e.to_string())?;
    stmt.raw_bind_parameter(1, name).map_err(|e| e.to_string())?;
    let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
    Ok(rows.next().map_err(|e| e.to_string())?.is_some())
}

pub fn list_tables(conn: &mut ConnHandle) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .conn
        .prepare(
            "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )
        .map_err(|e| e.to_string())?;
    let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        out.push(row.get::<_, String>(0).map_err(|e| e.to_string())?);
    }
    Ok(out)
}

pub fn table_info(conn: &mut ConnHandle, table: &str) -> Result<Vec<ValueRef>, String> {
    let mut stmt = conn
        .conn
        .prepare(&format!("PRAGMA table_info({})", quote_ident(table)))
        .map_err(|e| e.to_string())?;
    let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let mut map = HashMap::new();
        map.insert("cid".to_string(), Value::Int(row.get(0).unwrap_or(0)).ref_cell());
        map.insert(
            "name".to_string(),
            Value::String(row.get::<_, String>(1).unwrap_or_default()).ref_cell(),
        );
        map.insert(
            "type".to_string(),
            Value::String(row.get::<_, String>(2).unwrap_or_default()).ref_cell(),
        );
        map.insert("notnull".to_string(), Value::Int(row.get(3).unwrap_or(0)).ref_cell());
        map.insert(
            "default".to_string(),
            match row.get::<_, Option<String>>(4) {
                Ok(Some(s)) => Value::String(s).ref_cell(),
                _ => Value::Nil.ref_cell(),
            },
        );
        map.insert("pk".to_string(), Value::Int(row.get(5).unwrap_or(0)).ref_cell());
        out.push(Value::Object(map).ref_cell());
    }
    Ok(out)
}

pub fn list_indexes(conn: &mut ConnHandle, table: Option<&str>) -> Result<Vec<ValueRef>, String> {
    let sql = if let Some(t) = table {
        format!(
            "SELECT name, tbl_name, sql FROM sqlite_master WHERE type='index' AND tbl_name={} ORDER BY name",
            quote_literal(t)
        )
    } else {
        "SELECT name, tbl_name, sql FROM sqlite_master WHERE type='index' ORDER BY name".to_string()
    };
    let mut stmt = conn.conn.prepare(&sql).map_err(|e| e.to_string())?;
    let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let mut map = HashMap::new();
        map.insert(
            "name".to_string(),
            Value::String(row.get::<_, String>(0).unwrap_or_default()).ref_cell(),
        );
        map.insert(
            "table".to_string(),
            Value::String(row.get::<_, String>(1).unwrap_or_default()).ref_cell(),
        );
        map.insert(
            "sql".to_string(),
            match row.get::<_, Option<String>>(2) {
                Ok(Some(s)) => Value::String(s).ref_cell(),
                _ => Value::Nil.ref_cell(),
            },
        );
        out.push(Value::Object(map).ref_cell());
    }
    Ok(out)
}

fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn quote_literal(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

pub struct Migration {
    pub version: i64,
    pub sql: String,
}

pub fn parse_migrations(migrations_ref: &ValueRef, span: Span) -> Result<Vec<Migration>, crate::RuntimeError> {
    let migrations_val = &*migrations_ref.borrow();
    let items = match migrations_val {
        Value::Array(items) => items,
        other => {
            return Err(crate::RuntimeError::at(
                span,
                codes::E1704_NSQLITE_MIGRATION,
                format!(
                    "nsqlite_migrate() expects array of migration objects, got {}",
                    other.type_name()
                ),
            ));
        }
    };
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let borrowed = item.borrow();
        let obj = match &*borrowed {
            Value::Object(map) => map,
            other => {
                return Err(crate::RuntimeError::at(
                    span,
                    codes::E1704_NSQLITE_MIGRATION,
                    format!("migration entry must be object, got {}", other.type_name()),
                ));
            }
        };
        let version = obj
            .get("version")
            .ok_or_else(|| {
                crate::RuntimeError::at(
                    span,
                    codes::E1704_NSQLITE_MIGRATION,
                    "migration object missing field \"version\"",
                )
            })
            .and_then(|v| match &*v.borrow() {
                Value::Int(n) => Ok(*n),
                other => Err(crate::RuntimeError::at(
                    span,
                    codes::E1704_NSQLITE_MIGRATION,
                    format!("migration.version must be int, got {}", other.type_name()),
                )),
            })?;
        let sql = obj
            .get("sql")
            .ok_or_else(|| {
                crate::RuntimeError::at(
                    span,
                    codes::E1704_NSQLITE_MIGRATION,
                    "migration object missing field \"sql\"",
                )
            })
            .and_then(|v| match &*v.borrow() {
                Value::String(s) => Ok(s.clone()),
                other => Err(crate::RuntimeError::at(
                    span,
                    codes::E1704_NSQLITE_MIGRATION,
                    format!("migration.sql must be string, got {}", other.type_name()),
                )),
            })?;
        out.push(Migration { version, sql });
    }
    out.sort_by_key(|m| m.version);
    Ok(out)
}

pub fn run_migrations(conn: &mut ConnHandle, migrations: &[Migration]) -> Result<i64, String> {
    ensure_migrations_table(conn)?;
    let mut current = current_version(conn)?;
    let mut applied = 0i64;
    for m in migrations {
        if m.version <= current {
            continue;
        }
        if m.version != current + 1 {
            return Err(format!(
                "expected migration version {}, got {}",
                current + 1,
                m.version
            ));
        }
        for stmt in m.sql.split(';') {
            let stmt = stmt.trim();
            if stmt.is_empty() {
                continue;
            }
            exec_on_conn(conn, stmt, &[])?;
        }
        set_version(conn, m.version)?;
        current = m.version;
        applied += 1;
    }
    Ok(applied)
}
