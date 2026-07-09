# npg standard library

Fast PostgreSQL for Niao programs: connection pools, schema migrations, prepared statements, transactions, introspection, LISTEN/NOTIFY, COPY bulk load, and async background queries. Implemented in Rust via the `postgres` crate (pure Rust wire protocol) with `postgres-native-tls` and `r2d2` pooling.

## Import

```niao
import "npg"
```

Use the **`npg`** namespace for short names:

```niao
let db = npg.connect("postgresql://user:pass@localhost:5432/mydb")
npg.migrate(db, [{version: 1, sql: "CREATE TABLE t (id SERIAL PRIMARY KEY)"}])
```

Paths `import "std/npg"` and `import "npg"` are equivalent.

Flat builtins (`npg_connect`, `npg_query`, …) are also available globally after import.

## Connection

| Method / Builtin | Description |
|------------------|-------------|
| `npg.connect(url)` | Connect via PostgreSQL URL |
| `npg.connect_opts(opts)` | Object: `host`, `port`, `user`, `password`, `database`, `sslmode`, `connect_timeout`, `application_name`, or `url` |
| `npg.close(conn)` | Close connection and invalidate handles |
| `npg.ping(conn)` | `SELECT 1` health check |
| `npg.conninfo(conn)` | Redacted connection info (password hidden) |
| `npg.configure(conn, opts)` | Session GUCs: `statement_timeout`, `lock_timeout`, `search_path`, `timezone` |
| `npg.server_version(conn)` | Server version string |
| `npg.is_in_transaction(conn)` | Bool (client-tracked transaction state) |

**SSL modes:** `disable`, `prefer` (default), `require`, `verify-ca`, `verify-full` (mapped to driver capabilities).

## Connection pool

| Method | Description |
|--------|-------------|
| `npg.pool(opts)` | Create pool; opts include URL or discrete fields + `max_size`, `min_idle`, `max_lifetime_secs`, `connection_timeout_secs` |
| `npg.pool_close(pool)` | Drain and close pool |
| `npg.pool_get(pool)` | Checkout connection handle |
| `npg.pool_status(pool)` | `{size, idle, in_use}` |

Pool checkouts are ordinary connection handles.

## Schema & migrations

| Method | Description |
|--------|-------------|
| `npg.exec(conn, sql, params?)` | DDL/DML without result set; returns affected row count |
| `npg.exec_many(conn, sql_list)` | Multiple statements in one transaction |
| `npg.migrate(conn, migrations)` | Apply `{version, sql}` objects in order; tracks `_npg_schema_version` |
| `npg.table_exists(conn, schema?, name)` | `information_schema` lookup (default schema `public`) |
| `npg.list_tables(conn, schema?)` | Table names |
| `npg.table_info(conn, table, schema?)` | Column metadata: `name`, `type`, `nullable`, `default` |
| `npg.list_indexes(conn, schema?, table?)` | Index metadata |

## Queries

| Method | Description |
|--------|-------------|
| `npg.query(conn, sql, params?, format?)` | All rows; `format` is `"object"` (default) or `"array"` |
| `npg.query_row(conn, sql, params?)` | First row object or `nil` |
| `npg.query_value(conn, sql, params?)` | Scalar |
| `npg.query_column(conn, sql, params?)` | First column of all rows |

**Placeholders:** PostgreSQL-native `$1`, `$2`, … You may also use `?` in SQL; it is rewritten to `$N` before execution.

## Prepared statements

| Method | Description |
|--------|-------------|
| `npg.prepare(conn, sql)` | Statement handle |
| `npg.bind(stmt, index, value)` | Positional bind (1-based) |
| `npg.stmt_exec(stmt)` | Execute without rows |
| `npg.stmt_query(stmt, format?)` | Execute with rows |
| `npg.stmt_reset(stmt)` | Clear bindings |
| `npg.finalize(stmt)` | Free statement |

## Transactions

| Method | Description |
|--------|-------------|
| `npg.begin(conn, opts?)` | `isolation`, `read_only`, `deferrable` |
| `npg.commit(conn)` | Commit |
| `npg.rollback(conn)` | Rollback |
| `npg.savepoint(conn, name)` | `SAVEPOINT` |
| `npg.rollback_to(conn, name)` | `ROLLBACK TO SAVEPOINT` |

## Batch & helpers

| Method | Description |
|--------|-------------|
| `npg.batch(conn, sql, rows)` | Repeated exec in one transaction |
| `npg.insert(conn, table, data, schema?)` | Insert from object; returns inserted row (`RETURNING *`) |
| `npg.copy_from(conn, table, columns, rows)` | Bulk load via `COPY … FROM STDIN` (CSV) |

## PostgreSQL extras

| Method | Description |
|--------|-------------|
| `npg.listen(conn, channel)` | `LISTEN` |
| `npg.unlisten(conn, channel?)` | `UNLISTEN` / `UNLISTEN *` |
| `npg.notify(conn, channel, payload?)` | `pg_notify` |
| `npg.poll_notify(conn, timeout_ms?)` | Drain pending notifications |
| `npg.advisory_lock(conn, key)` | Session advisory lock |
| `npg.advisory_unlock(conn, key)` | Release lock |

## Async

Background tasks reopen the database by connection string in a worker thread.

| Builtin | Description |
|---------|-------------|
| `npg_async_exec(conn, sql, params?)` | Background write |
| `npg_async_query(conn, sql, params?, format?)` | Background read |
| `npg_task_done(id)` | Poll completion |
| `npg_task_wait(id)` | Block until done |
| `npg_task_result(id)` | Result or error value |
| `npg_task_cancel(id)` | Cancel pending task |

## Utilities

| Method | Description |
|--------|-------------|
| `npg.version()` | Runtime library version |
| `npg.escape_literal(s)` | Safe literal quoting |
| `npg.quote_ident(name)` | Identifier quoting |

## Example

```niao
import "npg"
import "nenv"

fn main() {
    let db = npg.connect(nenv.require("NIAO_PG_URL"))
    npg.migrate(db, [
        {version: 1, sql: "CREATE TABLE users (id SERIAL PRIMARY KEY, name TEXT NOT NULL)"}
    ])
    npg.insert(db, "users", {name: "Niao"})
    let rows = npg.query(db, "SELECT id, name FROM users WHERE id > $1", [0])
    print(rows[0].name)
    npg.close(db)
}
```

Set `NIAO_PG_URL` for integration tests (`tests/npg.niao` skips when unset).

## Error codes

| Code | Kind |
|------|------|
| E1900 | Arity / type error |
| E1901 | PostgreSQL operation failed |
| E1902 | Invalid handle |
| E1903 | Schema / constraint error |
| E1904 | Migration error |
| E1905 | Async task not found |
| E1906 | Invalid bind value |
| E1907 | TLS / connection error |

Errors surface as Niao error values with `kind: "npg_error"`.
