# Neko Error Handling

Complete reference for Neko’s typed error system — from language-level `try/catch` to the Rust `neko_errors` crate used by the toolchain.

---

## Overview

Neko provides **structured errors** at two levels:

| Layer | Type | Purpose |
|-------|------|---------|
| **Language** | `error` value type | User programs catch, inspect, and throw errors |
| **Rust toolchain** | `neko_errors` crate | Lexer, parser, compiler, VM, and CLI diagnostics |

All runtime failures use a consistent **`E####`** code format with optional source location (`line`, `col`).

---

## Language-level errors

### The `error` type

Errors are first-class values with type `error`. Each error exposes these fields:

| Field | Type | Description |
|-------|------|-------------|
| `code` | `int` | Numeric error code (e.g. `2001`) |
| `kind` | `string` | Machine-readable category (e.g. `"division_by_zero"`) |
| `message` | `string` | Human-readable description |
| `line` | `int` | Source line where the error occurred |
| `col` | `int` | Source column where the error occurred |

Access fields with member syntax:

```neko
catch (e) {
    print(e.message)
    print(e.code)
}
```

Use `type(value)` to confirm a value is an error:

```neko
if is_error(e) {
    print("got an error: " + e.message)
}
```

### Creating errors — `error()`

```neko
// Message only (code E2007, kind "thrown")
let e1 = error("something failed")

// Custom code + message
let e2 = error(4001, "invalid input")
```

### Throwing errors — `throw`

```neko
fn require_positive(n: int) {
    if n <= 0 {
        throw error("expected positive number")
    }
}
```

You may `throw` any value. Non-error values are automatically wrapped as a thrown error with the value’s string representation.

### Catching errors — `try/catch`

```neko
try {
    let result = risky_operation()
    print(result)
} catch (e) {
    print("Failed: " + e.message)
    print("Code: " + type(e.code))
}
```

The catch variable always receives a typed **`error`** value (never a plain string).

Runtime failures (division by zero, undefined variables, type errors, failed assertions) are automatically converted to structured errors in the catch block.

### Assertions — `assert()`

```neko
assert(condition)
assert(condition, "custom message")
```

Failed assertions produce error code **E2004** (`assert_failed`).

---

## Built-in functions

| Function | Signature | Description |
|----------|-----------|-------------|
| `error` | `error(message)` or `error(code, message)` | Create a structured error value |
| `is_error` | `is_error(value) -> bool` | Returns `true` if value has type `error` |
| `type` | `type(value) -> string` | Returns `"error"` for error values |
| `assert` | `assert(cond)` or `assert(cond, msg)` | Fails with E2004 if condition is falsy |

---

## Error code catalog

### Lexer (E0001–E0099)

| Code | Kind | Message |
|------|------|---------|
| E0001 | lex | Unexpected character |
| E0002 | lex | Unterminated string |

### Parser (E0100–E0199)

| Code | Kind | Message |
|------|------|---------|
| E0100 | parse | Unexpected token |
| E0101 | parse | Unexpected end of file |

### Compiler (E0200–E0299)

| Code | Kind | Message |
|------|------|---------|
| E0200 | compile | Unsupported construct |
| E0201 | compile | Unknown function |

### Builtins (E1000–E1099)

| Code | Description |
|------|-------------|
| E1001 | Builtin wrong arity |
| E1002 | `type()` wrong arity |
| E1003 | `assert()` wrong arity |
| E1004 | Wrong function argument count |
| E1005 | Break/continue outside loop |
| E1006 | Index or field access error |
| E1007 | Array allocation arity |
| E1008 | Index out of bounds |
| E1009 | Unknown struct / sort arity |
| E1010 | Unknown struct field |
| E1020 | Unknown class |
| E1021 | Unknown method on class/instance |
| E1022 | Trait not implemented |
| E1023 | Invalid `super` call |
| E1024 | Private member access |
| E1025 | Static/instance call mismatch |
| E1011 | Super-boom builtin arity |

### DSA builtins (E1100–E1199)

| Code | Description |
|------|-------------|
| E1100 | DSA builtin arity |
| E1101 | DSA index out of bounds |
| E1102 | DSA graph node out of range |

### I/O builtins (E1200–E1299)

| Code | Kind | Description |
|------|------|-------------|
| E1200 | `io_error` | I/O builtin wrong arity |
| E1201 | `io_error` | I/O operation failed |
| E1202 | `io_error` | Invalid or closed file handle |
| E1203 | `io_error` | Async I/O task not found |

### Regex builtins (E1300–E1399)

| Code | Kind | Description |
|------|------|-------------|
| E1300 | `re_error` | Regex builtin wrong arity |
| E1301 | `re_error` | Invalid regex pattern |
| E1302 | `re_error` | Invalid or closed regex handle |

### Net builtins (E1400–E1499)

| Code | Kind | Description |
|------|------|-------------|
| E1400 | `net_error` | Net builtin wrong arity |
| E1401 | `net_error` | Connection or protocol failure |
| E1402 | `net_error` | Invalid socket or net handle |
| E1403 | `net_error` | Invalid URL |
| E1404 | `net_error` | HTTP protocol error |
| E1405 | `net_error` | TLS error |
| E1406 | `net_error` | Async net task not found |

### Parallel (E1500–E1599)

| Code | Kind | Description |
|------|------|-------------|
| E1500 | `parallel_error` | Parallel builtin arity error |
| E1501 | `parallel_error` | Mutex lock contention |
| E1502 | `parallel_error` | Channel closed |
| E1503 | `parallel_error` | Invalid parallel handle |
| E1504 | `parallel_error` | Value not sendable across threads |
| E1505 | `parallel_error` | Thread, pool, or task not found |

### Runtime semantics (E2000–E2099)

| Code | Kind | Description |
|------|------|-------------|
| E2001 | `division_by_zero` | Integer or float division by zero |
| E2002 | `undefined_variable` | Use of undefined name |
| E2003 | `type_error` | Invalid operation for operand types |
| E2004 | `assert_failed` | `assert()` condition was false |
| E2005 | `module_not_found` | Import target file missing |
| E2006 | `import_cycle` | Circular import detected |
| E2007 | `thrown` | User `throw` or `error()` |
| E2008 | `stack_underflow` | VM stack underflow |
| E2009 | `no_main` | No `main` function (VM) |

Custom user codes (e.g. `error(4001, "msg")`) are allowed for application-level errors.

---

## Diagnostic format

All errors render in a unified format:

```
E2001: division by zero at line 4, col 15
```

The CLI and `ErrorHandler` use a slightly richer prefix:

```
error[E2001]: division by zero at line 4, col 15
```

---

## Execution modes

| Feature | Interpreter (`--mode interp`) | VM (default) |
|---------|------------------------------|--------------|
| `try/catch` | ✅ Supported | ❌ Not compiled to bytecode |
| `throw` | ✅ Supported | ❌ Not compiled to bytecode |
| Typed `error` values | ✅ Supported | ❌ Not compiled to bytecode |
| Runtime errors (E2001+) | ✅ Both modes | ✅ Both modes |

Use interpreter mode for full error-handling features:

```bash
neko run examples/errors.neko --mode interp
```

---

## Rust API — `neko_errors` crate

The `neko_errors` crate is the single source of truth for error types across the Neko toolchain.

### Module layout

```
neko_errors/
  codes.rs       — Error code constants (E0001, E2001, …)
  diagnostic.rs  — Diagnostic, Severity, ErrorCategory
  runtime.rs     — RuntimeError, NekoResult<T>
  value.rs       — NekoErrorValue (language-level error struct)
  neko_error.rs  — NekoError, LexError, ParseError, CompileError, VmError
  handler.rs     — ErrorHandler (formatting and reporting)
```

### Quick example

```rust
use neko_errors::{RuntimeError, ErrorHandler, NekoError};
use neko_ast::Span;

// Create a runtime error
let err = RuntimeError::division_by_zero(Span { line: 4, col: 15, ..Span::dummy() });

// Convert to top-level error
let neko_err = NekoError::Runtime(err);

// Format for stderr
let handler = ErrorHandler::new();
eprintln!("{}", handler.format_error(&neko_err));
// → error[E2001]: division by zero at line 4, col 15
```

### Key types

#### `RuntimeError`

Primary execution error. Variants include `DivisionByZero`, `UndefinedVar`, `TypeError`, `AssertFailed`, `Thrown(NekoErrorValue)`, and more.

Methods:

- `RuntimeError::at(span, code, message)` — Generic located error
- `RuntimeError::division_by_zero(span)`
- `RuntimeError::undefined_var(name, span)`
- `RuntimeError::type_error(message, span)`
- `RuntimeError::thrown(NekoErrorValue)`
- `.code() -> u32`
- `.span() -> Option<Span>`
- `.kind_name() -> &str`
- `.diagnostic() -> Diagnostic`
- `.to_neko_error_value() -> NekoErrorValue`

#### `NekoErrorValue`

Language-level error struct stored in `Value::Error`:

```rust
pub struct NekoErrorValue {
    pub code: u32,
    pub kind: String,
    pub message: String,
    pub line: usize,
    pub col: usize,
}
```

#### `NekoError`

Top-level enum composing all toolchain errors:

```rust
pub enum NekoError {
    Lex(LexError),
    Parse(ParseError),
    Compile(CompileError),
    Runtime(RuntimeError),
    Vm(VmError),
    Io(io::Error),
}
```

#### `ErrorHandler`

Formats and reports errors consistently:

```rust
let handler = ErrorHandler::new().with_color(true);
handler.report(&neko_err)?;
```

#### `Diagnostic`

Structured diagnostic with code, category, severity, message, span, and optional help text.

---

## Integration points

| Crate | Uses |
|-------|------|
| `neko_lexer` | Re-exports `LexError` from `neko_errors` |
| `neko_parser` | Re-exports `ParseError`; lex errors preserve line/col |
| `neko_runtime` | Re-exports `RuntimeError`; adds `Value::Error` |
| `neko_interpreter` | `try/catch`, `throw`, error field access |
| `neko_vm` | Propagates `RuntimeError` with E#### codes |
| `neko_cli` | Displays formatted errors to stderr |

---

## Examples

- [`examples/errors.neko`](../examples/errors.neko) — try/catch, throw, error fields
- [`tests/errors_typed.neko`](../tests/errors_typed.neko) — automated typed error tests

Run the example:

```bash
neko run examples/errors.neko --mode interp
```

Run typed error tests:

```bash
neko run tests/errors_typed.neko --mode interp
```

---

## Grammar additions

```ebnf
try_stmt   = "try" block "catch" "(" ident ")" block ;
throw_stmt = "throw" expr ";" ;
type_name  = ... | "error" ;
```

See [`grammar.ebnf`](grammar.ebnf) for the full grammar.

---

## Design principles

1. **Consistent codes** — Every error has an `E####` code; no silent format differences.
2. **Structured catch** — Catch blocks receive typed `error` values, not strings.
3. **Single crate** — `neko_errors` centralizes codes, diagnostics, and formatting.
4. **Backward compatible** — Existing `RuntimeError` call sites continue to work; Display output is now uniform.
5. **Progressive enhancement** — VM path gets runtime errors; interpreter path gets full try/catch/throw.
