# Niao Design Decisions

## Core Architecture

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Host language | Rust | Safety, performance, excellent ecosystem for compilers and VMs |
| Typing | Gradual static typing | Types optional in v0.1; type checker added in v0.2 |
| Execution v0.1 | Tree-walking interpreter | Fastest path to `niao run` |
| Execution v0.2+ | Bytecode stack VM | Balance of speed and implementation complexity |
| Syntax style | C/Rust-like core + block DSL for web | Familiar syntax; DSL blocks for `server {}`, `GET "/"` |
| Error handling | `try` / `catch`, `throw`, typed `error` values, `niao_errors` crate | Structured errors with E#### codes; see [ERRORS.md](ERRORS.md) |
| Memory management | Reference counting + VM mark-compact GC | `Rc` for nested values; VM arena collected under load — see [VM_MEMORY_AND_CACHE.md](VM_MEMORY_AND_CACHE.md) |

## Pipeline

```
Source (.niao) → Lexer → Parser → AST → [Interpreter | IR → Bytecode → VM]
```

## Module System

- `import "path"` resolves relative to the importing file's directory
- Each `.niao` file is a module exporting top-level `fn` definitions

## Project Layout

- `niao.config` — project manifest (name, version, entry)
- `src/` — source files
- `tests/` — test programs run by `niao test`
- `examples/` — example programs and acceptance specs

## Tier 2 Strategy

Built-in features (web, DB, auth, AI, JSON, networking) are implemented as native Rust modules
exposing a clean Niao API, wrapping battle-tested libraries (axum, sqlx, serde_json, ureq, etc.).
See [JSON.md](JSON.md) for the JSON standard library and [NET.md](NET.md) for the network standard library.

## Versioning

- Language spec: v0.1 (with OOP: `class`, `trait`, `extends`, `implements`, `self`, `super`, `static`)
- CLI/engine: 0.2.2

## Object-Oriented Programming

- **`struct`** — data-only records (unchanged)
- **`class`** — fields, instance methods, static methods, single inheritance (`extends`), `super`
- **`trait`** — method contracts; classes declare `implements Trait`
- **`self`** — first parameter of instance methods
- **`has_trait(x, "Name")`** — runtime trait check builtin
- Instances are `Value::Instance` with vtable dispatch; VM supports `CallMethod`, `CallSuper`, `MakeInstance`, `GetField`, `SetField`
- Full OOP guide: [OOP.md](OOP.md)

## Bytecode Cache

See [VM_MEMORY_AND_CACHE.md](VM_MEMORY_AND_CACHE.md) for full details. Summary:

- `niao run` and `niao build` write `.niaobc` files under `.niao-build/` (cwd), keyed by source path
- Caches invalidate on source mtime, `BYTECODE_CACHE_VERSION`, and builtin fingerprint changes
- Atomic write: `*.niaobc.tmp` then rename
