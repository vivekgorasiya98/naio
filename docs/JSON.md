# JSON standard library

Fast JSON parsing and manipulation for Neko programs, implemented in Rust via `serde_json`.

## Import

```neko
import "json"
```

Use the **`json`** namespace object for short names:

```neko
let data = json.parse("{\"ok\": true}")
print(json.stringify(data))
```

### Custom alias (`import as`)

```neko
import "json" as jsonobj

let data = jsonobj.parse("{\"name\": \"Neko\"}")
print(jsonobj.get(data, "name"))
```

Aliases also work on the bytecode VM — `import "json" as jsonobj` compiles to a copy of the `json` module binding.

Paths `import "std/json"` and `import "json"` are equivalent.

## Namespace API (recommended)

| Method | Description |
|--------|-------------|
| `json.parse(text)` | Parse JSON string → value |
| `json.stringify(value)` | Compact JSON text |
| `json.stringify_pretty(value, indent?)` | Pretty-print (default indent `2`) |
| `json.valid(text)` | `true` if text is valid JSON |
| `json.type(value)` | `"null"`, `"bool"`, `"number"`, `"string"`, `"array"`, `"object"` |
| `json.is_json(value)` | Whether value can be serialized to JSON |
| `json.keys(object)` | Sorted key list |
| `json.has(object, key)` | Key exists |
| `json.get(value, path)` | Nested read — e.g. `"user.items[0].id"` |
| `json.set(object, path, value)` | Set nested field in place |
| `json.merge(target, source)` | Deep-merge objects into `target` |
| `json.clone(value)` | Deep copy |
| `json.equal(a, b)` | Deep equality |
| `json.array_len(array)` | Array length |
| `json.object_len(object)` | Object field count |

## Legacy flat names

These global builtins remain available for backward compatibility:

`json_parse`, `json_stringify`, `json_stringify_pretty`, `json_valid`, `json_type`, `json_is_json`, `json_keys`, `json_has`, `json_get`, `json_set`, `json_merge`, `json_clone`, `json_equal`, `json_array_len`, `json_object_len`

## Examples

```neko
import "json" as j

fn main() {
    let doc = j.parse("{\"user\": {\"scores\": [98, 87]}}")
    print(j.get(doc, "user.scores[0]"))

    let text = j.stringify(doc)
    assert(j.equal(doc, j.parse(text)), "roundtrip")

    j.set(doc, "user.active", true)
    print(j.stringify_pretty(doc, 4))
}
```

Run the demo:

```bash
neko run examples/json_demo.neko
```

Run tests:

```bash
neko run tests/json.neko
```

## Errors

| Code | When |
|------|------|
| E1012 | Wrong argument count |
| E1013 | `json.parse` / `j.parse` invalid JSON |
| E1014 | Value cannot be serialized (functions, native DSA, NaN/Infinity, etc.) |

See [ERRORS.md](ERRORS.md) for the full error registry.

## Notes

- Numbers map to `int`, `float`, or `bigint` when possible.
- Object keys are sorted on `stringify` for stable output.
- Prefer `json.parse(...)` to build objects in tests on the bytecode VM when object literals are unavailable.
