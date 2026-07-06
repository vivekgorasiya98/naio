# `parallel` standard library

Threading, mutexes, channels, worker pools, and cooperative job polling for Neko programs.

The `parallel` module provides three layers:

| Layer | Use when | API style |
|-------|----------|-----------|
| **Namespace types** | Everyday concurrency | `p.Thread.spawn`, `p.Mutex.new`, `p.Pool.submit` |
| **Flat builtins** | Scripts, fine control | `parallel_thread_spawn`, `parallel_mutex_lock`, … |
| **Poll helpers** | VM / cooperative mode | `parallel_poll`, `parallel_poll_all` |

---

## Import

```neko
import "parallel"
```

`import "std/parallel"` is equivalent.

Importing `parallel` registers all `parallel_*` builtins globally and the `parallel` namespace object:

```neko
import "parallel" as p

let pool = p.Pool.new(4)
```

---

## Execution modes

### GIL mode (interpreter — default)

When running with the tree-walking interpreter (`neko run --mode interp` or default for programs with imports), OS threads are used and Neko callbacks execute under a **global interpreter lock** (Python-style). Only one Neko function runs at a time; threads still help overlap blocking I/O and organize worker pools.

### Poll mode (VM / no call hook)

When no interpreter call hook is active, `parallel_thread_spawn` and `parallel_pool_submit` enqueue jobs on a **main-thread queue**. Call `parallel_poll()` or `parallel_poll_all()` from the main loop to run queued Neko callbacks cooperatively.

---

## Sendable values

Only these types may cross threads (channels, mutex payloads, thread results):

| Allowed | Not allowed |
|---------|-------------|
| `nil`, `int`, `bool`, `float`, `string`, `int_array` | `function`, class instances |
| Arrays/objects of sendable values | Native DSA handles, I/O/net handles |

Violations return error `E1504` (`parallel_error`, kind `"parallel_error"`).

---

## Four namespace types

### Thread — raw OS threads

| Method | Description |
|--------|-------------|
| `spawn(fn, ...args)` | Start a thread running a Neko function |
| `join(handle)` | Wait for result |
| `detach(handle)` | Detach without joining |
| `is_alive(handle)` | Whether thread is still running |
| `yield()` | Hint scheduler |
| `sleep(ms)` | Sleep current thread |

### Mutex — shared sendable state

| Method | Description |
|--------|-------------|
| `new(initial?)` | Create mutex with optional initial value |
| `lock(handle)` | Acquire lock |
| `unlock(handle)` | Release lock |
| `try_lock(handle)` | Non-blocking lock attempt → `bool` |
| `get(handle)` | Read value (locked or auto-lock) |
| `set(handle, val)` | Write value |
| `run(handle, fn)` | Lock, call `fn()`, unlock |

### Channel — message passing

| Method | Description |
|--------|-------------|
| `new(capacity?)` | Unbounded if omitted; bounded if capacity > 0 |
| `send(ch, val)` | Send value |
| `recv(ch)` | Blocking receive |
| `try_recv(ch)` | Non-blocking receive (`nil` if empty) |
| `recv_timeout(ch, ms)` | Timed receive |
| `close(ch)` | Close channel |
| `is_closed(ch)` | Whether channel is closed |

### Pool — worker thread pool

| Method | Description |
|--------|-------------|
| `new(workers)` | Create pool with N worker threads |
| `submit(pool, fn, ...args)` | Queue work → task id |
| `wait(pool, task_id)` | Block for result |
| `shutdown(pool)` | Stop workers |
| `active(pool)` | Running task count |

---

## Flat builtins (raw API)

All namespace methods have `parallel_*` equivalents:

- `parallel_thread_spawn`, `parallel_thread_join`, `parallel_thread_detach`, `parallel_thread_is_alive`, `parallel_thread_yield`, `parallel_thread_sleep`, `parallel_cpu_count`
- `parallel_mutex_new`, `parallel_mutex_lock`, `parallel_mutex_unlock`, `parallel_mutex_try_lock`, `parallel_mutex_get`, `parallel_mutex_set`, `parallel_mutex_run`
- `parallel_channel_new`, `parallel_channel_send`, `parallel_channel_recv`, `parallel_channel_try_recv`, `parallel_channel_recv_timeout`, `parallel_channel_close`, `parallel_channel_is_closed`
- `parallel_pool_new`, `parallel_pool_submit`, `parallel_pool_wait`, `parallel_pool_shutdown`, `parallel_pool_active`
- `parallel_poll`, `parallel_poll_all`

---

## Error codes (E1500–E1505)

| Code | Use |
|------|-----|
| E1500 | Wrong arity / invalid argument |
| E1501 | Mutex lock error |
| E1502 | Channel closed |
| E1503 | Invalid handle |
| E1504 | Value not sendable |
| E1505 | Thread/pool/task not found |

See [ERRORS.md](ERRORS.md) for the full registry.

---

## Examples

```bash
neko run --mode interp examples/parallel_demo.neko
neko run tests/parallel_mutex.neko
neko run tests/parallel_channel.neko
neko run --mode interp tests/parallel_pool.neko
```

---

## Limitations

- Parallel Neko bytecode is serialized by the GIL; use pools/threads to split work and return sendable results.
- VM mode: sync primitives work; thread spawn uses poll queue — call `parallel_poll()` while waiting.
- Functions and native handles cannot be sent across threads.
