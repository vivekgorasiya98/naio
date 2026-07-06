# ahiru-server

High-performance HTTP/WebSocket backend framework for Neko — Tokio/Axum native core with Neko route handlers, middleware, multi-database support, and project scaffolding.

**Library version:** `0.3.0` (`nm info ahiru`)

## Quick start

```bash
cargo build --release

# Create a project (interactive wizard)
./target/release/neko ahiru create myapi

# Or non-interactive defaults
./target/release/neko ahiru create myapi --yes

cd myapi
../target/release/neko ahiru serve          # VM mode (default, fastest)
../target/release/neko ahiru serve --mode interp   # interpreter fallback
```

## Hello example

```bash
./target/release/neko run --mode interp examples/ahiru_hello.neko
curl http://localhost:3000/health
```

## CLI

| Command | Description |
|---------|-------------|
| `neko ahiru create <name>` | Interactive wizard — DB, auth, WebSocket, security |
| `neko ahiru serve` | Run project entry with **VM bytecode** (default) |
| `neko ahiru serve --mode interp` | Interpreter mode (multi-file imports, dev side-effects) |
| `neko ahiru serve --dev` | Auto-reload on file changes |
| `neko ahiru serve --net` | Bind `0.0.0.0` and show LAN URL |
| `neko ahiru serve --port 3000` | Use port 3000 (prompts if busy) |
| `neko ahiru bench --routes health,ping` | Handler throughput micro-benchmark |
| `neko ahiru migrate` | Apply SQL migrations from `migrations/` |
| `neko ahiru routes` | Show `ahiru.config.toml` server/DB/auth settings |

## Performance (0.2.2)

| Layer | What changed |
|-------|----------------|
| **VM default** | `neko ahiru serve` compiles entry to bytecode and dispatches handlers via the VM |
| **Per-worker VMs** | Each worker thread owns a VM instance — no global interpreter GIL on the hot path |
| **Fast bridge** | Reusable ctx field buffer; handlers invoked by bytecode index when resolved |
| **Native probes** | `/health` and `/ping` can register zero-Neko Rust handlers (`native_routes = true`) |
| **Lazy body** | GET/HEAD/OPTIONS skip body buffering |

Tune worker count in `ahiru.config.toml`:

```toml
[server]
workers = 8   # defaults to CPU count
native_routes = true   # native /health and /ping (default)
```

## ahiru v3 helpers (`stdlib/ahiru/v3.neko`)

Import with `import "std/ahiru/v3"` (v2 helpers remain available).

See [ahiru0.3.md](ahiru0.3.md) for the full 0.3.0 reference.

## ahiru v2 helpers (`stdlib/ahiru/v2.neko`)

Import with `import "std/ahiru/v2"` (requires `neko ahiru serve` or stdlib on `NEKO_STDLIB` path):

| Helper | Description |
|--------|-------------|
| `ahiru_v2_create_app_from_config(path)` | Load `ahiru.config.toml` |
| `ahiru_v2_use_dev_middleware(app)` | request_id + logging + CORS |
| `ahiru_v2_use_standard_middleware(app)` | dev middleware + secure headers |
| `ahiru_v2_use_quiet_middleware(app)` | request_id only, no access log |
| `ahiru_v2_use_production_middleware(app)` | JSON logs + secure headers + skip health/ping |
| `ahiru_v2_set_quiet(app, bool)` | Runtime toggle for logs and handler print() |
| `ahiru_v2_mount_health(app, path)` | `GET /health` — native Rust handler when `native_routes` enabled |
| `ahiru_v2_mount_ping(app, path)` | `GET /ping` — native Rust handler when `native_routes` enabled |
| `ahiru_v2_ok_json(body)` / `ahiru_v2_error_json(...)` | Response builders |
| `ahiru_v2_get/post(app, path, handler)` | Public routes shorthand |
| `ahiru_v2_version()` | Returns `"2.2.0"` (matches ahiru lib **0.2.2**) |

### Port conflicts

- **Default port from config**: if busy, the next free port is used automatically (`3000` → `3001`, …).
- **`--port` or `ahiru_app_listen(app, host, port)`**: interactive prompt — use next free port, enter custom port, or quit.

Rebuild after pulling changes: `cargo build -p neko_cli` (or use `target/debug/neko.exe`).

## Neko API (builtins)

Import with `import "ahiru"` or use flat `ahiru_*` builtins:

| Builtin | Description |
|---------|-------------|
| `ahiru_app_new()` / `ahiru_app_from_config(path)` | Create app handle |
| `ahiru_app_get/post/put/delete/patch(app, path, handler, opts?)` | Register routes |
| `ahiru_app_ws(app, path, handler, opts?)` | WebSocket route |
| `ahiru_app_use(app, middleware, opts?)` | Middleware: `cors`, `rate_limit`, `request_id`, `logging`, `secure_headers` |
| `ahiru_app_set_logging(app, opts?)` | Runtime log control: `access_log`, `json_logs`, `quiet_handlers`, `skip_paths` |
| `ahiru_app_init_db(app)` | Connect pools from config |
| `ahiru_app_listen(app, host?, port?)` | Start server (blocking) |
| `ahiru_app_routes(app)` | List registered routes |
| `ahiru_native_routes()` | Whether native health/ping routes are enabled |
| `ahiru_native_mount_health(app, path)` | Register native `GET /health` |
| `ahiru_native_mount_ping(app, path)` | Register native `GET /ping` |
| `ahiru_response(status, content_type, body)` | Build response object |
| `ahiru_json_response(status, json)` | JSON response helper |

### Handler `ctx` object

| Field | Type | Description |
|-------|------|-------------|
| `method` | string | HTTP method |
| `path` | string | Request path |
| `body` | string | Request body |
| `query` | object | Query parameters |
| `params` | object | Path parameters (`:id` routes) |
| `headers` | object | Lowercase header names |
| `user` | object | Authenticated user (when auth enabled) |
| `request_id` | string | Request correlation ID |

### Route options

```neko
ahiru_app_get(app, "/api/users", list_users, {
    permission: "users.read",
    is_public: false,
    ws: false
})
```

## Configuration (`ahiru.config.toml`)

Generated by `neko ahiru create`. Key sections:

- **`[server]`** — host, port, workers, TLS paths, body limit
- **`[[databases]]`** — SQLite, PostgreSQL, MySQL, or multiple named pools
- **`[auth]`** — `none`, `jwt`, `session`, `api_key`, `rbac`
- **`[websocket]`** — `disabled`, `global`, `per_route`
- **`[security]`** — CORS, rate limit, secure headers, CSRF
- **`[logging]`** — level, request ID header, JSON logs, access log, startup banner, quiet handlers, slow-request threshold, skip paths

```toml
[logging]
level = "info"
access_log = true
startup_banner = true
json_logs = false
request_id = true
quiet_handlers = false
slow_request_ms = 0
skip_paths = ["/health", "/ping"]
```

Set `AHIRU_QUIET=1` to suppress startup banner and access logs.

## Databases

Wizard supports:

| Driver | URL example |
|--------|-------------|
| sqlite | `sqlite://data/app.db` |
| postgres | `postgres://user:pass@localhost:5432/app` |
| mysql | `mysql://user:pass@localhost:3306/app` |

Run migrations:

```bash
neko ahiru migrate
```

Migrations are SQL files in `migrations/` tracked in `_ahiru_migrations` (SQLite) or applied per driver.

## Auth modes

| Mode | Behavior |
|------|----------|
| JWT | `Authorization: Bearer <token>` |
| session | `ahiru_session` cookie |
| api_key | `X-API-Key` header |
| rbac | JWT/session + `permission:` on routes |

## WebSocket

Register with `ahiru_app_ws`. Wizard sets `[websocket].mode` to `global` or `per_route`.

## Runtime modes

| Mode | Command | Handlers |
|------|---------|----------|
| Interpreter (default for servers) | `neko run --mode interp` or `neko ahiru serve` | Shared interpreter + `call_neko_function` |
| VM (experimental) | `neko run --mode vm` with VM call hook | `neko_vm::call_bridge` |

HTTP route handlers require the interpreter or VM call hook — use `--mode interp` for `import` and ahiru apps.

## Architecture

```
Client → Tokio/Axum (ahiru_core) → Rust middleware → Neko handler bridge → your .neko code
```

- **ahiru_core** — Rust server engine (routing, auth, DB pools, WebSocket)
- **neko_runtime/ahiru** — Neko builtins
- **stdlib/ahiru** — optional helper functions

## Performance notes

- Middleware runs entirely in Rust (no interpreter overhead).
- Handlers run via `tokio::task::spawn_blocking` on a multi-thread Tokio runtime (`server.workers`).
- When `server.workers > 1`, a handler worker pool dispatches Neko calls in parallel (GIL still serializes interpreter access per worker thread).
- Request body UTF-8 decoding is lazy — only when the handler reads `ctx.body`.
- Disable access logging with `[logging].access_log = false` or `ahiru_v2_use_quiet_middleware` to reduce per-request overhead.

### Benchmarks

Run throughput benchmarks (requires release build):

```bash
cargo test -p ahiru_core --test request_throughput -- --nocapture
```

Targets vs baseline: simple/bridge routes ≥5x RPS with worker pool; full-stack (auth+DB) ≥3x RPS.

## Legacy

- `neko serve` (web DSL) — use ahiru for new projects
- `net_http_*` — low-level programmatic server; still available

## Error codes

| Code | Meaning |
|------|---------|
| E2100 | ahiru builtin arity |
| E2101 | ahiru operation error |
| E2102 | invalid app handle |

See [ERRORS.md](ERRORS.md).
