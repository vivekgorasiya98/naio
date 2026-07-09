# Network standard library

Python-style networking for Niao programs: HTTP/HTTPS client, programmatic HTTP server, TCP/UDP sockets, TLS, DNS, URL utilities, WebSocket, SMTP, and FTP. Implemented in Rust inside `niao_runtime` (`ureq`, `tiny_http`, `rustls`, `tungstenite`, `lettre`, `suppaftp`).

The `net` module provides three layers:

| Layer | Use when | API style |
|-------|----------|-----------|
| **HTTP client** | REST APIs, downloads, webhooks | `net_http_get`, `net_http_request`, … |
| **Sockets / TLS** | Custom protocols, low-level control | `net_tcp_*`, `net_udp_*`, `net_tls_*` |
| **HTTP server** | Embedded services, tests, APIs | `net_http_listen` → routes → `net_http_poll` / `net_http_serve` |

---

## Import

```niao
import "net"
```

`import "std/net"` is equivalent.

Importing `net` registers all `net_*` builtins globally. There is no `net.get` namespace object — call the flat names directly:

```niao
import "net"

fn main() {
    let r = net_http_get("https://httpbin.org/get")
    if r.ok {
        print("status: " + net_response_field(r, "status"))
    }
}
```

Programs that only `import "net"` (no other file imports) run on the **bytecode VM** as well as the interpreter — except HTTP server handlers (see [Runtime modes](#runtime-modes)).

---

## Error handling

Most recoverable network failures return a structured **`error` value** (kind `"net_error"`) instead of aborting the program. Check with `is_error()` or inspect fields in `try/catch`:

```niao
import "net"

fn main() {
    let r = net_http_get("https://invalid.example.test")
    if is_error(r) {
        print("request failed: " + r.message)
        return
    }
    print(r.body)
}
```

**Type mistakes** (wrong argument type, wrong arity, invalid handle type) raise runtime errors immediately (`E1400`, `E2003`).

| Situation | Result |
|-----------|--------|
| Connection refused, timeout, protocol failure | `error` value (`E1401`) |
| Invalid URL | `error` value (`E1403`) |
| HTTP protocol / handler error | `error` value (`E1404`) |
| TLS handshake failure | `error` value (`E1405`) |
| Invalid / closed socket or server handle | Runtime error (`E1402`) or `error` value |
| Unknown async task id | Runtime error (`E1406`) |
| Wrong argument count | Runtime error (`E1400`) |
| Wrong argument type | Runtime error (`E2003`) |

See [ERRORS.md](ERRORS.md) for the full error registry.

---

## Runtime modes

| Feature | Bytecode VM | Interpreter |
|---------|-------------|-------------|
| HTTP client (`net_http_*`) | Yes | Yes |
| URL, DNS | Yes | Yes |
| TCP / UDP / TLS / WebSocket | Yes | Yes |
| SMTP / FTP | Yes | Yes |
| Async tasks (`net_async_*`, `net_task_*`) | Yes | Yes |
| HTTP server **with Niao handler functions** | **No** | **Yes** |

HTTP server route handlers are invoked through an interpreter callback bridge (`call_niao_function`). Use:

```bash
niao run --mode interp my_server.niao
niao test tests          # always uses interpreter
```

**Handler registration:** pass a top-level function by name (e.g. `handler`), not a variable holding a callable. Niao does not yet support first-class function values through variables.

**Serving models:**

| Function | Thread | Niao handlers |
|----------|--------|---------------|
| `net_http_poll(server)` | Caller (cooperative) | Yes (interpreter) |
| `net_http_serve(server)` | Caller (blocking) | Yes (interpreter) |
| `net_http_serve_async(server)` | Background thread | **No** — use `net_http_poll` instead |

When using `net_http_poll` on the same thread as a blocking HTTP client, run the client in the background (`net_async_http_get`) while polling the server (see [HTTP server example](#http-server-with-handlers)).

---

## URL utilities

| Function | Arguments | Returns | Description |
|----------|-----------|---------|-------------|
| `net_url_parse(url)` | `string` | `object` or `error` | Parse URL into components |
| `net_url_encode(s)` | `string` | `string` | Query-string encode (`a b` → `a+b`) |
| `net_url_decode(s)` | `string` | `string` | Query-string decode |
| `net_url_join(base, ref)` | `string`, `string` | `string` or `error` | Resolve relative URL |
| `net_url_build(parts)` | `object` | `string` or `error` | Build URL from parts object |

### Parsed URL object (`net_url_parse`)

| Field | Type | Description |
|-------|------|-------------|
| `scheme` | `string` | e.g. `"https"` |
| `host` | `string` | Hostname |
| `port` | `int` | Port (default for scheme if omitted) |
| `path` | `string` | Path component |
| `query` | `string` | Query without `?` |
| `fragment` | `string` | Fragment without `#` |
| `user` | `string` | Username |
| `password` | `string` | Password |

```niao
let u = net_url_parse("https://user:pass@host:8080/path?q=1#frag")
print(u.host)    // "host"
print(u.port)    // 8080

let built = net_url_build({
    scheme: "http",
    host: "localhost",
    port: 3000,
    path: "/api",
    query: "x=1"
})
// "http://localhost:3000/api?x=1"
```

---

## DNS

| Function | Arguments | Returns | Description |
|----------|-----------|---------|-------------|
| `net_resolve(host, port)` | `string`, `int` | `array` or `error` | Resolve host to addresses |
| `net_hostname()` | — | `string` | Local hostname (`HOSTNAME` / `COMPUTERNAME`, else `"localhost"`) |

Each entry in `net_resolve` result:

| Field | Type | Description |
|-------|------|-------------|
| `ip` | `string` | IP address |
| `port` | `int` | Port number |
| `family` | `string` | `"ipv4"` or `"ipv6"` |

```niao
let addrs = net_resolve("localhost", 80)
print(addrs[0].ip)
```

---

## HTTP client

Blocking HTTP/HTTPS via `ureq` with `rustls` and platform root certificates (`rustls-native-certs`). Connection reuse is handled by the underlying agent.

| Function | Arguments | Returns | Description |
|----------|-----------|---------|-------------|
| `net_http_get(url, opts?)` | `string`, `object?` | `object` or `error` | GET request |
| `net_http_post(url, body, opts?)` | `string`, `string`, `object?` | `object` or `error` | POST with string body |
| `net_http_put(url, body, opts?)` | `string`, `string`, `object?` | `object` or `error` | PUT |
| `net_http_delete(url, opts?)` | `string`, `object?` | `object` or `error` | DELETE |
| `net_http_patch(url, body, opts?)` | `string`, `string`, `object?` | `object` or `error` | PATCH |
| `net_http_head(url, opts?)` | `string`, `object?` | `object` or `error` | HEAD |
| `net_http_request(method, url, opts)` | `string`, `string`, `object` | `object` or `error` | Arbitrary method |
| `net_http_download(url, path, opts?)` | `string`, `string`, `object?` | `object` or `error` | GET and write body to file |
| `net_response_field(resp, field)` | `object`, `string` | value | Read a response field by name |

### Request options (`opts` object)

| Key | Type | Description |
|-----|------|-------------|
| `headers` | `object` | Extra request headers (`string` → `string`) |
| `body` | `string` | Request body (POST/PUT/PATCH/DELETE) |
| `body_bytes` | `int_array` | Binary body (bytes 0–255) |
| `timeout_ms` | `int` | Per-request timeout in milliseconds |
| `user_agent` | `string` | `User-Agent` header |
| `auth` | `array` | Basic auth: `[username, password]` |
| `follow_redirects` | `bool` | Parsed (default `true`); redirect behavior follows `ureq` defaults |

GET with a `body` or `body_bytes` in opts returns an HTTP error (`E1404`).

### Response object

Access fields with dot notation or `net_response_field(resp, "field")`:

| Field | Type | Description |
|-------|------|-------------|
| `status` | `int` | HTTP status code |
| `ok` | `bool` | `true` when status is 200–299 |
| `body` | `string` | Response body as UTF-8 text |
| `body_bytes` | `int_array` | Raw response bytes |
| `headers` | `object` | Response headers (lowercase keys) |
| `url` | `string` | Final URL after redirects |

HTTP error status codes (4xx, 5xx) still return a **response object** with the error status — they are not `error` values unless the transport fails.

```niao
import "net"

fn main() {
    let r = net_http_request("POST", "https://httpbin.org/post", {
        headers: { "Content-Type": "application/json" },
        body: "{\"x\": 1}",
        timeout_ms: 5000,
        auth: ["user", "pass"]
    })
    if r.ok {
        print(net_response_field(r, "body"))
    }

    net_http_download("https://example.com/file.bin", "file.bin")
}
```

---

## TCP and UDP sockets

Socket and listener handles are positive `int` ids stored in a per-thread handle table. Always call `net_tcp_close` (or `net_ws_close` / `net_ftp_close`) when finished.

Binary send/receive uses **`int_array`** values (bytes 0–255), same as `io_read_bytes` / `io_write_bytes`.

### TCP client / server

| Function | Arguments | Returns | Description |
|----------|-----------|---------|-------------|
| `net_tcp_socket()` | — | `int` or `error` | Create unconnected TCP socket |
| `net_tcp_connect(host, port)` | `string`, `int` | `int` or `error` | Connect to remote host |
| `net_tcp_bind(host, port)` | `string`, `int` | `int` or `error` | Bind listener |
| `net_tcp_listen(listener, backlog)` | `int`, `int` | `nil` or `error` | Mark listener ready (backlog accepted) |
| `net_tcp_accept(listener)` | `int` | `int` or `error` | Accept connection; returns new socket handle |
| `net_tcp_send(sock, data)` | `int`, `string` or `int_array` | `int` or `error` | Bytes written |
| `net_tcp_recv(sock, n)` | `int`, `int` | `int_array` or `error` | Read up to `n` bytes |
| `net_tcp_close(handle)` | `int` | `nil` or `error` | Close socket or listener |

```niao
import "net"

fn main() {
    let port = 38421
    let listener = net_tcp_bind("127.0.0.1", port)
    net_tcp_listen(listener, 8)

    let client = net_tcp_connect("127.0.0.1", port)
    net_tcp_send(client, "ping")
    net_tcp_close(client)

    let accepted = net_tcp_accept(listener)
    let data = net_tcp_recv(accepted, 64)
    print(data[0])   // 112 ('p')
    net_tcp_close(accepted)
    net_tcp_close(listener)
}
```

> **Note:** `server` is a reserved keyword in Niao. Do not use it as a variable name.

### UDP

| Function | Arguments | Returns | Description |
|----------|-----------|---------|-------------|
| `net_udp_socket()` | — | `int` or `error` | Create UDP socket (ephemeral bind) |
| `net_udp_bind(host, port)` | `string`, `int` | `int` or `error` | Bind UDP socket |
| `net_udp_send(sock, host, port, data)` | `int`, `string`, `int`, `string`/`int_array` | `int` or `error` | Send datagram |
| `net_udp_recv(sock, n)` | `int`, `int` | `int_array` or `error` | Receive up to `n` bytes |

### Timeouts

| Function | Arguments | Returns | Description |
|----------|-----------|---------|-------------|
| `net_set_timeout(handle, ms)` | `int`, `int` | `nil` or `error` | Set read/write timeout; `ms < 0` clears timeout |

Applies to TCP sockets, TCP listeners (stored), UDP sockets, and TLS streams.

---

## TLS

HTTPS uses TLS automatically via `net_http_*`. For raw TLS TCP:

| Function | Arguments | Returns | Description |
|----------|-----------|---------|-------------|
| `net_tls_connect(host, port, sni?)` | `string`, `int`, `string?` | `int` or `error` | One-shot TLS TCP connection (handle) |
| `net_tls_wrap(tcp_handle, sni_host)` | `int`, `string` | `int` or `error` | Upgrade existing TCP handle to TLS |
| `net_tls_config(verify?, min_version?)` | `bool?`, … | `bool` | TLS config placeholder (verify flag) |

Wrapped TLS handles support `net_tcp_send`, `net_tcp_recv`, and `net_set_timeout`.

TLS uses **rustls** with the platform native root certificate store. No OpenSSL dependency.

```niao
let sock = net_tls_connect("example.com", 443)
net_tcp_send(sock, "GET / HTTP/1.1\r\nHost: example.com\r\n\r\n")
let bytes = net_tcp_recv(sock, 4096)
net_tcp_close(sock)
```

---

## HTTP server

Programmatic HTTP server via `tiny_http`. Register routes, then serve with `net_http_poll` (cooperative) or `net_http_serve` (blocking).

| Function | Arguments | Returns | Description |
|----------|-----------|---------|-------------|
| `net_http_listen(port, host?)` | `int`, `string` or `{host: ...}?` | `int` or `error` | Create server handle (default host `0.0.0.0`) |
| `net_http_route(server, method, path, handler)` | `int`, `string`, `string`, `fn` | `nil` or `error` | Per-route handler |
| `net_http_on_request(server, handler)` | `int`, `fn` | `nil` or `error` | Catch-all fallback handler |
| `net_http_poll(server)` | `int` | `nil` or `error` | Process one pending request (non-blocking) |
| `net_http_serve(server)` | `int` | `nil` or `error` | Blocking accept loop until `net_http_stop` |
| `net_http_serve_async(server)` | `int` | `int` (task id) | Background accept loop (**no Niao handlers**) |
| `net_http_stop(server)` | `int` | `nil` or `error` | Signal server to stop |
| `net_http_response(status, content_type, body)` | `int`, `string`, `string` | `object` | Build handler response object |
| `net_request_field(req, field)` | `object`, `string` | value | Read request field |

### Request object (passed to handler)

| Field | Type | Description |
|-------|------|-------------|
| `method` | `string` | HTTP method |
| `path` | `string` | Path without query string |
| `query` | `string` | Query string without `?` |
| `body` | `string` | Body as UTF-8 text |
| `body_bytes` | `int_array` | Raw body bytes |
| `headers` | `object` | Request headers (lowercase keys) |

### Response object (returned by handler)

| Field | Type | Description |
|-------|------|-------------|
| `status` | `int` | HTTP status code |
| `content_type` | `string` | `Content-Type` header |
| `body` | `string` | Response body |

### HTTP server with handlers

```niao
import "net"

fn handler(req) {
    if req.path == "/hello" {
        return net_http_response(200, "text/plain", "hello")
    }
    return net_http_response(404, "text/plain", "not found")
}

fn main() {
    let port = 8080
    let srv = net_http_listen(port)
    net_http_route(srv, "GET", "/hello", handler)

    // Background client + cooperative server poll (same-thread safe)
    let task = net_async_http_get("http://127.0.0.1:" + port + "/hello", {
        timeout_ms: 5000
    })
    while !net_task_done(task) {
        net_http_poll(srv)
    }
    let r = net_task_poll(task)
    print(net_response_field(r, "body"))

    net_http_stop(srv)
}
```

Blocking server (simple scripts):

```niao
import "net"

fn handle(req) {
    return net_http_response(200, "text/plain", "ok")
}

fn main() {
    let srv = net_http_listen(8080)
    net_http_on_request(srv, handle)
    net_http_serve(srv)   // blocks until net_http_stop
}
```

---

## WebSocket (client)

Blocking WebSocket client via `tungstenite`. Handles are separate from TCP handles.

| Function | Arguments | Returns | Description |
|----------|-----------|---------|-------------|
| `net_ws_connect(url, opts?)` | `string`, `object?` | `int` or `error` | Client handshake (`ws://` or `wss://`) |
| `net_ws_send(handle, message)` | `int`, `string` or `int_array` | `nil` or `error` | Send text or binary frame |
| `net_ws_recv(handle)` | `int` | `string`, `int_array`, or `nil` | Blocking receive; `nil` on close |
| `net_ws_close(handle)` | `int` | `nil` or `error` | Close connection |

```niao
let ws = net_ws_connect("wss://echo.websocket.events")
net_ws_send(ws, "hello")
let msg = net_ws_recv(ws)
net_ws_close(ws)
```

---

## SMTP

| Function | Arguments | Returns | Description |
|----------|-----------|---------|-------------|
| `net_smtp_send(host, port, from, to, subject, body, opts?)` | 6–7 args | `nil` or `error` | Send plain-text email |

### SMTP options (`opts` object, optional 7th argument)

| Key | Type | Description |
|-----|------|-------------|
| `user` | `string` | SMTP username |
| `password` | `string` | SMTP password |

Uses `lettre` with rustls. Relay mode (STARTTLS as configured by server).

```niao
net_smtp_send("smtp.example.com", 587, "from@example.com", "to@example.com",
    "Subject", "Body text", { user: "alice", password: "secret" })
```

---

## FTP

FTP connections use separate handle ids (not the TCP handle table).

| Function | Arguments | Returns | Description |
|----------|-----------|---------|-------------|
| `net_ftp_connect(host, port)` | `string`, `int` | `int` or `error` | Connect to FTP server |
| `net_ftp_login(handle, user, pass)` | `int`, `string`, `string` | `nil` or `error` | Authenticate |
| `net_ftp_get(handle, remote)` | `int`, `string` | `string` or `error` | Download file as string |
| `net_ftp_put(handle, remote, content)` | `int`, `string`, `string` | `nil` or `error` | Upload string content |
| `net_ftp_close(handle)` | `int` | `nil` or `error` | Quit and close |

```niao
let ftp = net_ftp_connect("ftp.example.com", 21)
net_ftp_login(ftp, "user", "pass")
let data = net_ftp_get(ftp, "/pub/readme.txt")
net_ftp_put(ftp, "/upload/out.txt", "hello")
net_ftp_close(ftp)
```

---

## Async background networking

Async functions spawn work on a background thread and return a **task id** (`int`). Poll or block for the result. The task pool is shared with `io_async_*` (`async_tasks` in `niao_runtime`).

| Function | Arguments | Returns | Description |
|----------|-----------|---------|-------------|
| `net_async_http_get(url, opts?)` | `string`, `object?` | `int` | Background HTTP GET |
| `net_async_tcp_connect(host, port)` | `string`, `int` | `int` | Background TCP connect (returns socket handle) |
| `net_task_done(task)` | `int` | `bool` | `true` when finished or cancelled |
| `net_task_poll(task)` | `int` | value | Result if done; `nil` if pending |
| `net_task_wait(task)` | `int` | value | Block until done, then return result |
| `net_task_cancel(task)` | `int` | `bool` | Cancel pending task |

### Task results

| Task state | `net_task_poll` / `net_task_wait` |
|------------|-----------------------------------|
| Pending | `nil` (poll only) |
| Success | HTTP response object, socket handle `int`, etc. |
| Failure | `error` value (`E1401`) |
| Cancelled | `error` value (cancelled message) |

```niao
import "net"

fn main() {
    let task = net_async_http_get("https://httpbin.org/get")
    let resp = net_task_wait(task)
    if resp.ok {
        print(net_response_field(resp, "status"))
    }
}
```

---

## Complete function index

All builtins registered by `import "net"`:

**URL:** `net_url_parse`, `net_url_encode`, `net_url_decode`, `net_url_join`, `net_url_build`

**DNS:** `net_resolve`, `net_hostname`

**HTTP client:** `net_http_get`, `net_http_post`, `net_http_put`, `net_http_delete`, `net_http_patch`, `net_http_head`, `net_http_request`, `net_http_download`, `net_response_field`

**TCP/UDP:** `net_tcp_socket`, `net_tcp_connect`, `net_tcp_bind`, `net_tcp_listen`, `net_tcp_accept`, `net_tcp_send`, `net_tcp_recv`, `net_tcp_close`, `net_udp_socket`, `net_udp_bind`, `net_udp_send`, `net_udp_recv`, `net_set_timeout`

**TLS:** `net_tls_connect`, `net_tls_wrap`, `net_tls_config`

**HTTP server:** `net_http_listen`, `net_http_route`, `net_http_on_request`, `net_http_poll`, `net_http_serve`, `net_http_serve_async`, `net_http_stop`, `net_http_response`, `net_request_field`

**WebSocket:** `net_ws_connect`, `net_ws_send`, `net_ws_recv`, `net_ws_close`

**SMTP:** `net_smtp_send`

**FTP:** `net_ftp_connect`, `net_ftp_login`, `net_ftp_get`, `net_ftp_put`, `net_ftp_close`

**Async:** `net_async_http_get`, `net_async_tcp_connect`, `net_task_done`, `net_task_poll`, `net_task_wait`, `net_task_cancel`

---

## Examples

### HTTP client

```niao
import "net"

fn main() {
    let u = net_url_parse("https://example.com/api")
    print(u.host)

    let r = net_http_get("https://httpbin.org/get")
    if r.ok {
        print("status: " + net_response_field(r, "status"))
    }
}
```

### TCP echo-style exchange

See [tests/net_tcp.niao](../tests/net_tcp.niao).

### Combined client + JSON

```niao
import "net"
import "json"

fn main() {
    let r = net_http_get("https://httpbin.org/get")
    if r.ok {
        let data = json.parse(net_response_field(r, "body"))
        print(json.stringify_pretty(data, 2))
    }
}
```

### Demo and tests

```bash
niao run examples/net_http_client.niao
niao run examples/net_demo.niao --mode interp   # includes network I/O
niao run --mode interp tests/net_server.niao
niao run tests/net_url.niao
niao run tests/net_tcp.niao
```

Network integration tests that use HTTP server handlers require the interpreter (`niao test` or `--mode interp`).

---

## Error codes

| Code | Kind | When |
|------|------|------|
| E1400 | `net_error` | Wrong argument count on a `net_*` builtin |
| E1401 | `net_error` | Recoverable connection / protocol failure (returned as `error` value) |
| E1402 | `net_error` | Invalid or closed socket / net handle |
| E1403 | `net_error` | Invalid URL |
| E1404 | `net_error` | HTTP protocol error |
| E1405 | `net_error` | TLS error |
| E1406 | `net_error` | Async net task id not found |

---

## Implementation notes

- **Runtime:** `crates/niao_runtime/src/net/` (submodules: `url`, `dns`, `http_client`, `http_server`, `socket`, `tls`, `websocket`, `smtp`, `ftp`, `handles`)
- **Async pool:** `crates/niao_runtime/src/async_tasks.rs` (shared with `io_*`)
- **Interpreter bridge:** `set_niao_call_hook` / `call_niao_function` in `niao_runtime`; registered by `niao_interpreter` for HTTP handler dispatch
- **Registration:** `net::builtins()` in `builtin_table()`; virtual module paths `net`, `std/net`
- **Binary data:** packed `IntArray` (bytes 0–255) for socket I/O and `body_bytes` fields
- **HTTPS:** `ureq` + `rustls` + native roots; no OpenSSL
- **HTTP server:** `tiny_http` on a background listener thread; `net_http_poll` uses `try_recv` for cooperative handling
- **No Tokio** inside native callbacks — blocking APIs only; async via OS thread pool

---

## Related documentation

- [ERRORS.md](ERRORS.md) — `try` / `catch`, `error` values, code registry
- [iodocs.md](iodocs.md) — file I/O (`net_http_download` writes via `std::fs`)
- [JSON.md](JSON.md) — parse API responses as JSON
- [DECISIONS.md](DECISIONS.md) — native Rust std modules (Tier 2 strategy)
