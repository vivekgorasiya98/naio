//! Schema migrations and introspection.
#![allow(dead_code)]

use super::handles::ConnHandle;
use super::query::exec_on_conn;
use super::types::{quote_ident, BoundValue};
use crate::{Value, ValueRef};
use neko_ast::Span;
use neko_errors::codes;
use std::collections::HashMap;

const MIGRATIONS_TABLE: &str = "_npg_schema_version";

pub fn ensure_migrations_table(conn: &mut ConnHandle) -> Result<(), String> {
    exec_on_conn(
        conn,
        &format!(
            "CREATE TABLE IF NOT EXISTS {MIGRATIONS_TABLE} (version BIGINT PRIMARY KEY NOT NULL)"
        ),
        &[],
    )?;
    Ok(())
}

pub fn current_version(conn: &mut ConnHandle) -> Result<i64, String> {
    ensure_migrations_table(conn)?;
    let rows = conn.client_mut()
        .query(
            &format!("SELECT version FROM {MIGRATIONS_TABLE} ORDER BY version DESC LIMIT 1"),
            &[],
        )
        .map_err(|e| e.to_string())?;
    if let Some(row) = rows.first() {
        Ok(row.get::<_, i64>(0))
    } else {
        Ok(0)
    }
}

pub fn set_version(conn: &mut ConnHandle, version: i64) -> Result<(), String> {
    exec_on_conn(
        conn,
        &format!("INSERT INTO {MIGRATIONS_TABLE} (version) VALUES ($1)"),
        &[BoundValue::Int(version)],
    )?;
    Ok(())
}

pub fn table_exists(conn: &mut ConnHandle, schema: &str, name: &str) -> Result<bool, String> {
    let rows = conn.client_mut()
        .query(
            "SELECT 1 FROM information_schema.tables WHERE table_schema = $1 AND table_name = $2 LIMIT 1",
            &[&schema, &name],
        )
        .map_err(|e| e.to_string())?;
    Ok(!rows.is_empty())
}

pub fn list_tables(conn: &mut ConnHandle, schema: &str) -> Result<Vec<String>, String> {
    let rows = conn.client_mut()
        .query(
            "SELECT table_name FROM information_schema.tables WHERE table_schema = $1 AND table_type = 'BASE TABLE' ORDER BY table_name",
            &[&schema],
        )
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.get::<_, String>(0));
    }
    Ok(out)
}

pub fn table_info(conn: &mut ConnHandle, schema: &str, table: &str) -> Result<Vec<ValueRef>, String> {
    let rows = conn.client_mut()
        .query(
            "SELECT column_name, data_type, is_nullable, column_default FROM information_schema.columns WHERE table_schema = $1 AND table_name = $2 ORDER BY ordinal_position",
            &[&schema, &table],
        )
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for row in rows {
        let mut map = HashMap::new();
        map.insert(
            "name".to_string(),
            Value::String(row.get::<_, String>(0)).ref_cell(),
        );
        map.insert(
            "type".to_string(),
            Value::String(row.get::<_, String>(1)).ref_cell(),
        );
        let nullable = row.get::<_, String>(2);
        map.insert(
            "nullable".to_string(),
            Value::Bool(nullable.eq_ignore_ascii_case("YES")).ref_cell(),
        );
        map.insert(
            "default".to_string(),
            match row.try_get::<_, Option<String>>(3) {
                Ok(Some(s)) => Value::String(s).ref_cell(),
                _ => Value::Nil.ref_cell(),
            },
        );
        out.push(Value::Object(map).ref_cell());
    }
    Ok(out)
}

pub fn list_indexes(
    conn: &mut ConnHandle,
    schema: &str,
    table: Option<&str>,
) -> Result<Vec<ValueRef>, String> {
    let rows = if let Some(t) = table {
        conn.client_mut()
            .query(
                "SELECT indexname, tablename, indexdef FROM pg_indexes WHERE schemaname = $1 AND tablename = $2 ORDER BY indexname",
                &[&schema, &t],
            )
            .map_err(|e| e.to_string())?
    } else {
        conn.client_mut()
            .query(
                "SELECT indexname, tablename, indexdef FROM pg_indexes WHERE schemaname = $1 ORDER BY indexname",
                &[&schema],
            )
            .map_err(|e| e.to_string())?
    };
    let mut out = Vec::new();
    for row in rows {
        let mut map = HashMap::new();
        map.insert(
            "name".to_string(),
            Value::String(row.get::<_, String>(0)).ref_cell(),
        );
        map.insert(
            "table".to_string(),
            Value::String(row.get::<_, String>(1)).ref_cell(),
        );
        map.insert(
            "definition".to_string(),
            Value::String(row.get::<_, String>(2)).ref_cell(),
        );
        out.push(Value::Object(map).ref_cell());
    }
    Ok(out)
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
                codes::E1904_NPG_MIGRATION,
                format!(
                    "npg_migrate() expects array of migration objects, got {}",
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
                    codes::E1904_NPG_MIGRATION,
                    format!("migration entry must be object, got {}", other.type_name()),
                ));
            }
        };
        let version = obj
            .get("version")
            .ok_or_else(|| {
                crate::RuntimeError::at(
                    span,
                    codes::E1904_NPG_MIGRATION,
                    "migration object missing field \"version\"",
                )
            })
            .and_then(|v| match &*v.borrow() {
                Value::Int(n) => Ok(*n),
                other => Err(crate::RuntimeError::at(
                    span,
                    codes::E1904_NPG_MIGRATION,
                    format!("migration.version must be int, got {}", other.type_name()),
                )),
            })?;
        let sql = obj
            .get("sql")
            .ok_or_else(|| {
                crate::RuntimeError::at(
                    span,
                    codes::E1904_NPG_MIGRATION,
                    "migration object missing field \"sql\"",
                )
            })
            .and_then(|v| match &*v.borrow() {
                Value::String(s) => Ok(s.clone()),
                other => Err(crate::RuntimeError::at(
                    span,
                    codes::E1904_NPG_MIGRATION,
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

pub fn default_schema(schema: Option<&str>) -> &str {
    schema.unwrap_or("public")
}

pub fn qualified_table(schema: &str, table: &str) -> String {
    format!("{}.{}", quote_ident(schema), quote_ident(table))
}
