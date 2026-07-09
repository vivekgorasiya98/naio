# nenv standard library

Environment variables, `.env` file loading, typed accessors, schema validation, and isolated env stores. Process-env APIs that previously lived in `nos` are now in `nenv`.

## Import

```niao
import "nenv"
```

Paths `import "std/nenv"` and `import "nenv"` are equivalent.

Flat builtins (`nenv_get`, `nenv_load`, …) are also available globally after import.

## Quick start

```niao
import "nenv"

nenv.load_defaults()
let port = nenv.get_int("PORT", 3000)
let db = nenv.require("DATABASE_URL")
```

Call `load()` or `load_defaults()` before `get()` — variables are not read from `.env` automatically.

## Loading `.env` files

| Method / Builtin | Description |
|------------------|-------------|
| `nenv.load(path?, opts?)` | Load one file (default `.env` in cwd). `opts`: `{override: bool}` (default `false`). Returns `int` count applied or `error`. |
| `nenv.load_many(paths, opts?)` | Load files in order; later keys apply when `override: true` or key not yet set. |
| `nenv.load_defaults(opts?)` | Load `.env` then `.env.local` if they exist. |
| `nenv.parse(path)` | Parse file without mutating process env; returns `{KEY: "value", ...}`. |
| `nenv.parse_text(text)` | Parse string content without applying. |
| `nenv.find_up(filename?, start_dir?)` | Walk parent directories for a file (default `.env`); returns path or `nil`. |

**Override semantics:** by default, existing process variables are not overwritten. Pass `{override: true}` to replace them.

## Process environment

| Method | Description |
|--------|-------------|
| `nenv.get(key)` / `nenv.get(key, default)` | Read process env; `nil` or default if missing. |
| `nenv.set(key, value)` / `nenv.unset(key)` | Mutate process env. |
| `nenv.has(key)` | `bool` — key exists. |
| `nenv.all()` | Snapshot of all process variables as an object. |
| `nenv.require(key)` | Returns string or `error` if missing. |
| `nenv.get_int(key, default?)` | Parse integer. |
| `nenv.get_bool(key, default?)` | Accept `true/false/1/0/yes/no/on/off`. |
| `nenv.get_float(key, default?)` | Parse float. |
| `nenv.expand(text)` | Expand `$VAR` and `${VAR}` using process env. |
| `nenv.validate(schema)` | `schema`: `{required: [...], types: {KEY: "int"|"bool"|"float"|"string"}}`; returns `nil` or `error`. |

## Isolated stores

Use stores when you need env config without polluting the global process environment (e.g. tests).

| Method | Description |
|--------|-------------|
| `nenv.open(opts?)` | New store handle (`int`). `opts`: `{inherit: bool}` — seed from current process env. |
| `nenv.close(store)` | Release handle. |
| `nenv.from_object(map)` | Build store from `{KEY: value}` object. |
| `nenv.store_load(store, path, opts?)` | Parse file into store only. |
| `nenv.store_get(store, key, default?)` | Read from store (plus inherit layer if configured). |
| `nenv.store_set(store, key, value)` | Set in store. |
| `nenv.store_unset(store, key)` | Remove from store. |
| `nenv.store_all(store)` | Merged object view. |
| `nenv.store_apply(store, opts?)` | Push store vars into process env. |

## Error codes

| Code | Kind | Meaning |
|------|------|---------|
| E1950 | `nenv_error` | Wrong argument count |
| E1951 | `nenv_error` | Parse / load / I/O failure |
| E1952 | `nenv_error` | `require()` — variable missing |
| E1953 | `nenv_error` | Typed getter or validate type mismatch |
| E1954 | `nenv_error` | Invalid or closed store handle |

## Relationship to `nos`

`nos` covers process control, platform constants, and lightweight filesystem helpers. Use `nenv` for all environment-variable and `.env` configuration.

## Example

See [`examples/nenv_demo.niao`](../examples/nenv_demo.niao) and [`tests/nenv.niao`](../tests/nenv.niao).
