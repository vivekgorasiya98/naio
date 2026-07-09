# nmongo standard library

Fast MongoDB for Niao programs: connection pooling, CRUD, aggregation, indexes, transactions, GridFS, change streams, and async background tasks. Implemented in Rust via the official `mongodb` driver.

## Import

```niao
import "nmongo"
```

Use the **`nmongo`** namespace for short names:

```niao
let client = nmongo.connect({host: "localhost", port: 27017, database: "app"})
let users = nmongo.find(client, "app", "users", {active: true})
```

Paths `import "std/nmongo"` and `import "nmongo"` are equivalent.

Flat builtins (`nmongo_connect`, `nmongo_find`, …) are also available globally after import.

## Connection

| Method | Description |
|--------|-------------|
| `nmongo.connect(opts)` | Structured connect (preferred); credentials never logged |
| `nmongo.connect_uri(uri)` | Connect via MongoDB URI |
| `nmongo.close(client)` | Close client and invalidate handle |
| `nmongo.ping(client)` | Server health check |
| `nmongo.list_databases(client)` | Database names |

**`connect` opts** (secure defaults):

```niao
{
    host: "localhost",
    port: 27017,
    hosts: ["h1:27017", "h2:27017"],  // optional replica set hosts
    database: "app",
    user: "...",
    password: "...",                  // use nenv.get / nos in app code
    auth_source: "admin",
    tls: {
        enabled: true,
        ca_file: nil,
        allow_invalid_certs: false
    },
    max_pool_size: 100,
    min_pool_size: 0,
    server_selection_timeout_ms: 30000,
    connect_timeout_ms: 10000,
    app_name: "niao"
}
```

## CRUD

| Method | Description |
|--------|-------------|
| `find(client, db, coll, filter?, opts?)` | All matching documents |
| `find_one(...)` | First match or `nil` |
| `insert_one(client, db, coll, doc)` | `{inserted_id}` |
| `insert_many(client, db, coll, docs, opts?)` | `{inserted_ids}` |
| `update_one/many(...)` | `{matched, modified, upserted_id?}` |
| `replace_one(...)` | Same shape as update |
| `delete_one/many(...)` | `{deleted_count}` |
| `count_documents(...)` | Document count |
| `distinct(client, db, coll, field, filter?)` | Distinct values array |

**`find` opts**: `limit`, `skip`, `sort`, `projection`, `batch_size`, `session`.

Filters and updates are BSON documents — not string-built queries.

MongoDB operators (`$match`, `$set`, `$gte`, …) cannot be used as bare object keys in Niao source (`{$match: ...}` is a parse error). Use `json.parse` for those documents:

```niao
import "json"

let pipeline = json.parse("[{\"$match\":{\"active\":true}},{\"$count\":\"n\"}]")
nmongo.aggregate(client, "app", "users", pipeline)
```

## Aggregation & indexes

| Method | Description |
|--------|-------------|
| `aggregate(client, db, coll, pipeline, opts?)` | Aggregation pipeline |
| `create_index(client, db, coll, keys, opts?)` | Returns index name |
| `list_indexes(client, db, coll)` | Index metadata |
| `drop_index(client, db, coll, name)` | Drop index |
| `list_collections(client, db)` | Collection names |
| `drop_collection(client, db, coll)` | Drop collection |

## Bulk writes

`bulk_write(client, db, coll, ops, opts?)` — sequential execution for broad compatibility.

Ops array entries:

```niao
[
    {insert_one: {document: {name: "a"}}},
    {update_one: {filter: {name: "a"}, update: {$set: {active: true}}, upsert: false}},
    {delete_one: {filter: {name: "b"}}}
]
```

Opts: `ordered: true` (default) or `false`.

## Transactions

| Method | Description |
|--------|-------------|
| `start_session(client)` | Session handle |
| `start_transaction(session, opts?)` | Begin transaction |
| `commit_transaction(session)` | Commit |
| `abort_transaction(session)` | Abort |
| `end_session(session)` | End session |

Pass `{session: sid}` in opts on CRUD/aggregate calls for transaction-scoped operations.

## GridFS

| Method | Description |
|--------|-------------|
| `gridfs_upload(client, db, filename, data, opts?)` | Upload string or `byte_array` |
| `gridfs_download(client, db, filename, opts?)` | Returns `byte_array` |
| `gridfs_delete(client, db, filename, opts?)` | Delete by filename |
| `gridfs_list(client, db, opts?)` | List files + metadata |

Opts: `metadata`, `chunk_size`.

## Change streams

| Method | Description |
|--------|-------------|
| `watch(client, db, coll, pipeline?, opts?)` | Returns watch handle (background thread) |
| `watch_next(watch_id)` | Next event or `nil` |
| `watch_close(watch_id)` | Stop stream |

## BSON helpers

| Method | Description |
|--------|-------------|
| `object_id(hex)` | ObjectId for filters/inserts (`{$oid: "..."}`) |
| `is_object_id(s)` | Validate hex string |
| `to_extended_json(value)` | Niao value → extended JSON string |
| `from_extended_json(s)` | Parse extended JSON → Niao value |

## Async

| Method | Description |
|--------|-------------|
| `async_find(...)` | Background find |
| `async_bulk_write(...)` | Background bulk insert loop |
| `task_done(task)` | Poll completion |
| `task_wait(task)` | Block until done |
| `task_result(task)` | Result value or error |
| `task_cancel(task)` | Cancel task |

## Errors

Recoverable failures return `error` values (`kind: "nmongo_error"`, codes E1920–E1928). Use `is_error()` / `try/catch`.

## Requirements

- MongoDB server reachable from the host running `niao`
- For integration tests: set `NIAO_MONGO_URL` or `MONGO_URL` (e.g. `mongodb://localhost:27017`)
