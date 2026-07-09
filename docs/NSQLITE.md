# nsqlite standard library

Fast SQLite for Niao programs: cwd-relative database files, schema migrations, prepared statements, transactions, batch inserts, introspection, and async background queries. Implemented in Rust via `rusqlite` (bundled SQLite).

## Import

```niao
import "nsqlite"
```

Use the **`nsqlite`** namespace for short names:

```niao
let db = nsqlite.open("app.db")
nsqlite.migrate(db, [{version: 1, sql: "CREATE TABLE t (id INTEGER PRIMARY KEY)"}])
```

Paths `import "std/nsqlite"` and `import "nsqlite"` are equivalent.

Flat builtins (`nsqlite_open`, `nsqlite_query`, …) are also available globally after import.

## Connection

| Method / Builtin | Description |
|------------------|-------------|
| `nsqlite.open(path)` | Open or create a DB in **cwd** when `path` is relative; use `:memory:` for RAM |
| `nsqlite.open_abs(path)` | Open with no cwd join |
| `nsqlite.close(conn)` | Close connection and invalidate handles |
| `nsqlite.path(conn)` | Resolved path (`":memory:"` for in-memory) |
| `nsqlite.configure(conn, opts)` | Pragmas: `wal`, `synchronous`, `cache_size`, `mmap_size`, `foreign_keys` |
| `nsqlite.last_insert_rowid(conn)` | Rowid after last `INSERT` |
| `nsqlite.changes(conn)` | Rows changed by last statement |

File databases default to WAL journal, `NORMAL` synchronous, foreign keys on, and a 64 MB cache.

## Schema & migrations

| Method | Description |
|--------|-------------|
| `nsqlite.exec(conn, sql, params?)` | Run DDL/DML without a result set |
| `nsqlite.exec_many(conn, sql_list)` | Run multiple statements in one transaction |
| `nsqlite.migrate(conn, migrations)` | Apply `{version, sql}` objects in order; tracks `_nsqlite_schema_version` |
| `nsqlite.table_exists(conn, name)` | Returns bool |
| `nsqlite.list_tables(conn)` | Table names (excludes `sqlite_*`) |
| `nsqlite.table_info(conn, table)` | Column metadata: `name`, `type`, `notnull`, `pk`, `default` |
| `nsqlite.list_indexes(conn, table?)` | Index metadata |

## Queries

| Method | Description |
|--------|-------------|
| `nsqlite.query(conn, sql, params?, format?)` | All rows; `format` is `"object"` (default) or `"array"` |
| `nsqlite.query_row(conn, sql, params?)` | First row object or `nil` |
| `nsqlite.query_value(conn, sql, params?)` | First column of first row |
| `nsqlite.query_column(conn, sql, params?)` | First column of all rows |

**Object rows** (default):

```niao
[{id: 1, name: "neo"}]
```

**Compact rows** (`format: "array"`):

```niao
{columns: ["id", "name"], rows: [[1, "neo"]]}
```

Use `?` placeholders; pass params as an array.

## Prepared statements

| Method | Description |
|--------|-------------|
| `nsqlite.prepare(conn, sql)` | Statement handle |
| `nsqlite.bind(stmt, index, value)` | Positional bind (1-based) |
| `nsqlite.bind_named(stmt, name, value)` | Named bind (`:name`) |
| `nsqlite.stmt_exec(stmt)` | Execute without rows |
| `nsqlite.stmt_query(stmt, format?)` | Execute with rows |
| `nsqlite.stmt_reset(stmt)` | Clear bindings |
| `nsqlite.finalize(stmt)` | Free statement |

## Transactions

| Method | Description |
|--------|-------------|
| `nsqlite.begin(conn, mode?)` | `"deferred"` (default), `"immediate"`, `"exclusive"` |
| `nsqlite.commit(conn)` | Commit |
| `nsqlite.rollback(conn)` | Rollback |

## Batch & helpers

| Method | Description |
|--------|-------------|
| `nsqlite.batch(conn, sql, rows)` | `executemany` with array of param arrays |
| `nsqlite.insert(conn, table, data)` | Insert from object `{col: val, …}` |

## Backup & utilities

| Method | Description |
|--------|-------------|
| `nsqlite.backup(dest, src)` | Online backup between open connections |
| `nsqlite.vacuum(conn)` | `VACUUM` |
| `nsqlite.version()` | SQLite library version string |

## Async

Background tasks reopen the database by path in a worker thread (handles are thread-local).

| Builtin | Description |
|---------|-------------|
| `nsqlite_async_exec(conn, sql)` | Background DDL/DML |
| `nsqlite_async_query(conn, sql, params?, format?)` | Background read |
| `nsqlite_task_done(id)` | Poll completion |
| `nsqlite_task_wait(id)` | Block until done |
| `nsqlite_task_result(id)` | Result or error value |
| `nsqlite_task_cancel(id)` | Cancel pending task |

## Example

```niao
import "nsqlite"
import "io"

fn main() {
    print("db dir: " + io_cwd())

    let db = nsqlite.open("app.db")
    nsqlite.migrate(db, [
        {version: 1, sql: "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL)"},
        {version: 2, sql: "CREATE INDEX idx_users_name ON users(name)"}
    ])

    nsqlite.insert(db, "users", {name: "Niao"})
    let rows = nsqlite.query(db, "SELECT id, name FROM users WHERE id > ?", [0])
    print(rows[0].name)

    let task = nsqlite_async_query(db, "SELECT count(*) FROM users")
    nsqlite_task_wait(task)
    print(nsqlite_task_result(task))

    nsqlite.close(db)
}
```

## Error codes

| Code | Kind |
|------|------|
| E1700 | Arity / type error |
| E1701 | SQLite operation failed |
| E1702 | Invalid handle |
| E1703 | Schema / constraint error |
| E1704 | Migration error |
| E1705 | Async task not found |
| E1706 | Invalid bind value |

Errors surface as Niao error values with `kind: "nsqlite_error"`.
