# I/O standard library

Fast file and path operations for Niao programs, implemented in Rust (`std::fs`, buffered streams, and background worker threads).

The `io` module provides three layers:

| Layer | Use when | API style |
|-------|----------|-----------|
| **Sync whole-file** | Small/medium files, scripts, config | `io_read_file`, `io_write_file`, … |
| **Streaming handles** | Large files, incremental read/write | `io_open` → `io_read` / `io_write` → `io_close` |
| **Async background** | Overlap I/O with other work | `io_async_*` → `io_task_wait` / `io_task_poll` |

---

## Import

```niao
import "io"
```

`import "std/io"` is equivalent.

Importing `io` registers all `io_*` builtins globally. Unlike the JSON module, there is no `io.read_file` namespace object — call the flat names directly:

```niao
import "io"

fn main() {
    io_write_file("hello.txt", "Hello, Niao!\n")
    print(io_read_file("hello.txt"))
}
```

Programs that only `import "io"` (no other file imports) run on the bytecode VM as well as the interpreter.

---

## Error handling

Most I/O failures return a structured **`error` value** (code `E1201`, kind `"io_error"`) instead of aborting the program. Check with `is_error()` or inspect fields in `try/catch`:

```niao
import "io"

fn main() {
    let data = io_read_file("missing.txt")
    if is_error(data) {
        print("read failed: " + data.message)
        return
    }
    print(data)
}
```

**Type mistakes** (wrong argument type, wrong arity) still raise runtime errors immediately (`E1200`, `E2003`).

| Situation | Result |
|-----------|--------|
| File not found, permission denied, invalid path | `error` value (`E1201`) |
| Invalid / closed file handle | `error` value (`E1202`) or runtime error |
| Unknown async task id | Runtime error (`E1203`) |
| Wrong argument count | Runtime error (`E1200`) |
| Wrong argument type | Runtime error (`E2003`) |

See [ERRORS.md](ERRORS.md) for the full error registry.

---

## Path utilities

All path functions accept string paths. On Windows, separators in returned paths follow the OS (`\`).

| Function | Arguments | Returns | Description |
|----------|-----------|---------|-------------|
| `io_join(a, b, …)` | 2–16 strings | `string` | Join path segments (`PathBuf::push`) |
| `io_join_many(parts)` | `array` of strings | `string` | Join an array of segments; empty array → `""` |
| `io_dirname(path)` | `string` | `string` | Parent directory; root → `"."` |
| `io_basename(path)` | `string` | `string` | Final path component |
| `io_stem(path)` | `string` | `string` | File name without extension |
| `io_extension(path)` | `string` | `string` | Extension without dot, or `""` |
| `io_is_absolute(path)` | `string` | `bool` | Whether path is absolute |
| `io_canonical(path)` | `string` | `string` or `error` | Resolve to absolute canonical path |

```niao
let cfg = io_join(io_home_dir(), ".config", "niao", "settings.json")
let name = io_basename(cfg)   // "settings.json"
let dir  = io_dirname(cfg)    // parent path
```

---

## Metadata

| Function | Arguments | Returns | Description |
|----------|-----------|---------|-------------|
| `io_exists(path)` | `string` | `bool` | Path exists (any type) |
| `io_is_file(path)` | `string` | `bool` | Regular file |
| `io_is_dir(path)` | `string` | `bool` | Directory |
| `io_is_symlink(path)` | `string` | `bool` | Symbolic link |
| `io_file_size(path)` | `string` | `int` or `error` | Size in bytes |
| `io_modified_ms(path)` | `string` | `int` or `error` | Last modified time (Unix ms) |
| `io_created_ms(path)` | `string` | `int` or `error` | Creation time (Unix ms); may fail on some platforms |

---

## Sync whole-file I/O

Optimized for reading or writing an entire file in one call. Uses `std::fs::read` / `write` directly.

| Function | Arguments | Returns | Description |
|----------|-----------|---------|-------------|
| `io_read_file(path)` | `string` | `string` or `error` | Read entire file as UTF-8 text |
| `io_read_bytes(path)` | `string` | `array` or `error` | Read raw bytes as `int_array` (0–255 per element) |
| `io_write_file(path, text)` | `string`, `string` | `nil` or `error` | Create/truncate and write text |
| `io_write_bytes(path, bytes)` | `string`, `int_array` or `array` | `nil` or `error` | Write raw bytes |
| `io_append_file(path, text)` | `string`, `string` | `nil` or `error` | Append text (creates file if needed) |
| `io_read_lines(path)` | `string` | `array` or `error` | Split into lines; **newlines stripped** |
| `io_write_lines(path, lines)` | `string`, `array` of strings | `nil` or `error` | Write lines joined with `\n` (no trailing newline after last line) |

### Text vs binary

- **Text** functions use UTF-8 strings.
- **Binary** functions use packed `int_array` values (`make_int_array` / literal `[72, 105]`) with each element in `0..=255`.

```niao
io_write_file("log.txt", "started\n")
io_append_file("log.txt", "line 2\n")

let lines = io_read_lines("log.txt")
// lines[0] == "started"  (no trailing \n)

io_write_bytes("data.bin", [0x48, 0x69])   // "Hi"
let raw = io_read_bytes("data.bin")
```

---

## Directory operations

| Function | Arguments | Returns | Description |
|----------|-----------|---------|-------------|
| `io_list_dir(path)` | `string` | `array` or `error` | Immediate children (sorted names) |
| `io_list_dir_recursive(path)` | `string` | `array` or `error` | All files/dirs under `path` (relative paths; dirs end with `/`) |
| `io_create_dir(path)` | `string` | `nil` or `error` | Create single directory |
| `io_create_dir_all(path)` | `string` | `nil` or `error` | Create directory tree |
| `io_remove_file(path)` | `string` | `nil` or `error` | Delete a file |
| `io_remove_dir(path)` | `string` | `nil` or `error` | Remove **empty** directory |
| `io_remove_dir_all(path)` | `string` | `nil` or `error` | Remove directory tree |
| `io_copy(src, dst)` | `string`, `string` | `nil` or `error` | Copy file |
| `io_rename(src, dst)` | `string`, `string` | `nil` or `error` | Rename or move |

---

## Working directory and standard paths

| Function | Arguments | Returns | Description |
|----------|-----------|---------|-------------|
| `io_cwd()` | — | `string` or `error` | Current working directory |
| `io_chdir(path)` | `string` | `nil` or `error` | Change working directory |
| `io_temp_dir()` | — | `string` | OS temp directory |
| `io_home_dir()` | — | `string` or `error` | User home (`HOME` / `USERPROFILE`) |

```niao
let scratch = io_join(io_temp_dir(), "niao_scratch")
io_create_dir_all(scratch)
```

---

## Streaming file handles

For large files or incremental I/O, open a handle, read/write in chunks, then close.

### Open modes

| Mode | Meaning |
|------|---------|
| `"r"` | Read text |
| `"w"` | Write text (truncate or create) |
| `"a"` | Append text |
| `"r+"`, `"w+"`, `"a+"` | Read/write text |
| `"rb"` | Read binary |
| `"wb"` | Write binary (truncate or create) |
| `"ab"` | Append binary |

Binary handles return byte arrays from read functions; text handles return strings.

### Handle API

Handles are positive `int` ids. Always call `io_close` when finished.

| Function | Arguments | Returns | Description |
|----------|-----------|---------|-------------|
| `io_open(path, mode)` | `string`, `string` | `int` or `error` | Open file; returns handle id |
| `io_close(handle)` | `int` | `nil` or `error` | Flush and close |
| `io_read(handle, n)` | `int`, `int` | `string` / `array` or `error` | Read up to `n` bytes |
| `io_read_all(handle)` | `int` | `string` / `array` or `error` | Read remainder |
| `io_read_line(handle)` | `int` | `string`, `nil`, or `error` | One line (text only); `nil` at EOF |
| `io_write(handle, data)` | `int`, `string` or byte array | `int` or `error` | Bytes/chars written |
| `io_flush(handle)` | `int` | `nil` or `error` | Flush buffers |
| `io_seek(handle, offset, whence)` | `int`, `int`, `int` | `int` or `error` | Seek; returns new position |
| `io_tell(handle)` | `int` | `int` or `error` | Current position |
| `io_eof(handle)` | `int` | `bool` or `error` | Whether EOF was reached on last read |

### Seek `whence` values

| Value | Meaning |
|-------|---------|
| `0` | From start of file |
| `1` | From current position |
| `2` | From end of file |

Writer-only handles (`"w"`, `"a"`, `"wb"`, `"ab"`) do not support `io_seek` / `io_tell`.

```niao
let h = io_open("big.log", "r")
let chunk = io_read(h, 4096)
while chunk != "" {
    print(chunk)
    chunk = io_read(h, 4096)
}
io_close(h)
```

Streaming uses 64 KiB `BufReader` / `BufWriter` internally for throughput.

---

## Async background I/O

Async functions spawn work on a background thread and return a **task id** (`int`). Poll or block for the result.

| Function | Arguments | Returns | Description |
|----------|-----------|---------|-------------|
| `io_async_read(path)` | `string` | `int` | Background read text |
| `io_async_read_bytes(path)` | `string` | `int` | Background read bytes |
| `io_async_write(path, text)` | `string`, `string` | `int` | Background write text |
| `io_async_write_bytes(path, bytes)` | `string`, byte array | `int` | Background write bytes |
| `io_async_copy(src, dst)` | `string`, `string` | `int` | Background file copy |
| `io_task_done(task)` | `int` | `bool` | `true` when finished or cancelled |
| `io_task_poll(task)` | `int` | value | Result if done; `nil` if pending |
| `io_task_wait(task)` | `int` | value | Block until done, then return result |
| `io_task_cancel(task)` | `int` | `bool` | Cancel pending task; `true` if cancelled |

### Task results

| Task state | `io_task_poll` / `io_task_wait` |
|------------|----------------------------------|
| Pending | `nil` (poll only) |
| Success | Written value (`string`, `nil`, byte array, …) |
| I/O failure | `error` value (`E1201`) |
| Cancelled | `error` value (cancelled message) |

```niao
// Fire-and-wait (simplest)
let task = io_async_read("large.txt")
let content = io_task_wait(task)

// Overlap work
let task = io_async_write("out.txt", payload)
while !io_task_done(task) {
    // do other work
}
let result = io_task_poll(task)
```

Async tasks use a process-wide task pool (`async_tasks` in `niao_runtime`). Rebuild Niao from source after upgrading the runtime if async calls hang on an older binary.

---

## Complete function index

All builtins registered by `import "io"`:

**Paths:** `io_join`, `io_join_many`, `io_dirname`, `io_basename`, `io_stem`, `io_extension`, `io_is_absolute`, `io_canonical`

**Metadata:** `io_exists`, `io_is_file`, `io_is_dir`, `io_is_symlink`, `io_file_size`, `io_modified_ms`, `io_created_ms`

**Sync files:** `io_read_file`, `io_read_bytes`, `io_write_file`, `io_write_bytes`, `io_append_file`, `io_read_lines`, `io_write_lines`

**Directories:** `io_list_dir`, `io_list_dir_recursive`, `io_create_dir`, `io_create_dir_all`, `io_remove_file`, `io_remove_dir`, `io_remove_dir_all`, `io_copy`, `io_rename`

**Environment:** `io_cwd`, `io_chdir`, `io_temp_dir`, `io_home_dir`

**Handles:** `io_open`, `io_close`, `io_read`, `io_read_all`, `io_read_line`, `io_write`, `io_flush`, `io_seek`, `io_tell`, `io_eof`

**Async:** `io_async_read`, `io_async_read_bytes`, `io_async_write`, `io_async_write_bytes`, `io_async_copy`, `io_task_done`, `io_task_poll`, `io_task_wait`, `io_task_cancel`

---

## Examples

### Quick read/write

```niao
import "io"

fn main() {
    io_write_file("greeting.txt", "Hello from Niao I/O!\n")
    print(io_read_file("greeting.txt"))
}
```

### Line-based config

```niao
import "io"

fn main() {
    io_write_lines("hosts.txt", ["127.0.0.1 localhost", "10.0.0.1 gateway"])
    let lines = io_read_lines("hosts.txt")
    for i in 0..len(lines) {
        print(lines[i])
    }
}
```

### Safe error handling

```niao
import "io"

fn read_config(path: string) -> string {
    let data = io_read_file(path)
    if is_error(data) {
        throw data
    }
    return data
}

fn main() {
    try {
        print(read_config("niao.config"))
    } catch (e) {
        print("config error: " + e.message)
    }
}
```

### Demo and tests

```bash
niao run examples/io_demo.niao
niao run tests/io.niao
```

---

## Error codes

| Code | Kind | When |
|------|------|------|
| E1200 | `io_error` | Wrong argument count on an `io_*` builtin |
| E1201 | `io_error` | Recoverable I/O failure (returned as `error` value) |
| E1202 | `io_error` | Invalid or already-closed file handle |
| E1203 | `io_error` | Async task id not found |

---

## Implementation notes

- **Runtime:** `crates/niao_runtime/src/io.rs`
- **Async pool:** `crates/niao_runtime/src/async_tasks.rs` (shared with other async natives)
- **Registration:** `io::builtins()` in `builtin_table()`; virtual module paths `io`, `std/io`
- **Performance:** whole-file ops avoid extra copies; streaming uses 64 KiB buffers; binary data uses packed `IntArray`
- **Platform:** paths and metadata follow the host OS; `io_created_ms` may be unavailable on some filesystems

---

## Related documentation

- [ERRORS.md](ERRORS.md) — `try` / `catch`, `error` values, code registry
- [DECISIONS.md](DECISIONS.md) — native Rust std modules (Tier 2 strategy)
- [JSON.md](JSON.md) — JSON standard library (complementary to file I/O)
