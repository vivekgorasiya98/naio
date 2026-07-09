# ahiru-server 0.3.0

Framework expansion release — shared state, custom middleware, route groups, cache, jobs, metrics, and full CLI toolkit.

| | |
|---|---|
| **Library version** | `0.3.0` |
| **v3 helpers** | `stdlib/ahiru/v3.niao` → `"3.0.0"` |
| **Builtins** | 36 |
| **Verify** | `nm info ahiru` |

## Highlights

- **Shared state** — `ahiru_app_set_state(app, key, value)` → `ctx.state`
- **Custom middleware** — `ahiru_app_use(app, my_fn, { only: ["/admin/*"], order: 10 })`
- **Route groups** — `ahiru_app_group(app, "/api/v1", opts)` → scoped handle
- **REST resources** — `ahiru_app_resource(app, "/posts", { index, show, ... })`
- **Static files** — `ahiru_app_static(app, "/public", "./public")`
- **Cache** — `[[caches]]` + `ahiru_cache_get/set/incr` (memory default; redis optional feature)
- **Jobs & cron** — `ahiru_job_enqueue`, `ahiru_app_cron`
- **WebSocket rooms** — `ahiru_ws_broadcast(app, room, msg)`
- **Metrics** — `ahiru_v3_mount_metrics(app, "/metrics")`
- **Health split** — `ahiru_v3_mount_health` → `/health/live` + `/health/ready`
- **Error hooks** — `ahiru_app_on_error`, `ahiru_app_not_found`
- **Validation** — route opt `{ schema: fn }` → 422 on failure

## CLI

| Command | Description |
|---------|-------------|
| `niao ahiru db migrate` | Apply migrations |
| `niao ahiru db status` | Applied vs pending |
| `niao ahiru db seed` | Run `seeds/*.sql` |
| `niao ahiru db rollback` | Roll back last migration (`.down.sql`) |
| `niao ahiru db reset --force` | Drop SQLite + re-migrate |
| `niao ahiru doctor` | Config + DB + port checks |
| `niao ahiru add <feature>` | auth, db, websocket, cache |
| `niao ahiru generate resource <name>` | Handler + migration scaffold |
| `niao ahiru openapi` | Emit `public/openapi.json` |
| `niao ahiru test` | Run `tests/**/*.niao` |
| `niao ahiru console` | Project REPL stub |
| `niao ahiru worker` | Job worker entry |

## Config additions

```toml
[[caches]]
name = "default"
driver = "memory"  # or "redis" (feature flag)

[[databases]]
role = "write"     # or "read"

[websocket]
heartbeat_secs = 30

[security]
compression = true
etag = true
csp_policy = "default-src 'self'"
```

`.env` and `ahiru.config.{AHIRU_ENV}.toml` overlays supported via `AhiruConfig::load_with_env`.

## Error codes

| Range | Layer |
|-------|-------|
| E2110–E2119 | State / routing |
| E2120–E2129 | Validation |
| E2130–E2139 | Stream / multipart |
| E2200–E2209 | Jobs / cron |
| E2300–E2309 | Cache |
| E2400–E2409 | Auth extensions |
| E2500–E2509 | WebSocket rooms |

See [ERRORS.md](ERRORS.md).

## Migration from 0.2.2

All 0.2.2 builtins remain. `import "std/ahiru/v2"` still works; new projects use `import "std/ahiru/v3"`.
