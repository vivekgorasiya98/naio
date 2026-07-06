# Regular expression standard library

Pattern matching, capture groups, search/replace, and splitting for Neko programs. Implemented in Rust via the [`regex`](https://docs.rs/regex) crate.

Stateless functions compile the pattern on each call. For hot loops, use `re.compile` / `re_*_h` handle APIs to reuse a compiled regex.

## Import

```neko
import "re"
```

Use the **`re`** namespace object for short names:

```neko
if re.test("\\d+", "x42") {
    print(re.search("(\\d+)", "x42").groups[1])
}
```

### Custom alias

```neko
import "re" as rx

let ok = rx.valid("^\\w+@\\w+\\.\\w+$")
print(rx.escape("a.b"))
```

Paths `import "std/re"` and `import "re"` are equivalent.

## Pattern syntax & flags

Patterns follow **Rust regex** syntax (not JavaScript or PCRE). Common constructs:

| Construct | Meaning |
|-----------|---------|
| `.` | Any character (except newline by default) |
| `\d`, `\w`, `\s` | Digit, word, whitespace |
| `[abc]`, `[a-z]` | Character class |
| `*`, `+`, `?`, `{n,m}` | Quantifiers |
| `^`, `$` | Start / end of string |
| `(…)` | Capture group |
| `(?:…)` | Non-capturing group |

Optional **flags** string (last argument on stateless functions, baked in at compile time for handles):

| Flag | Effect |
|------|--------|
| `i` | Case-insensitive |
| `m` | Multi-line (`^` / `$` match line boundaries) |
| `s` | Dot matches newline |
| `u` / `U` | Unicode-aware matching |

Example: `re.test("hello", "HELLO", "i")` → `true`.

## Match object

`re.search` and `re.find_all` (with capture groups) return an object:

| Field | Type | Description |
|-------|------|-------------|
| `full` | string | Matched substring |
| `start` | int | Start index (byte offset, inclusive) |
| `end` | int | End index (byte offset, exclusive) |
| `groups` | array | Capture groups — index `0` is the full match, `1+` are parenthesized groups |

When no match is found, `re.search` returns `nil`.

## Namespace API

### Validation & escaping

| Method | Description |
|--------|-------------|
| `re.valid(pattern, flags?)` | `true` if pattern compiles |
| `re.escape(text)` | Escape literal metacharacters |

### Matching

| Method | Description |
|--------|-------------|
| `re.test(pattern, text, flags?)` | `true` if pattern matches anywhere in `text` |
| `re.match(pattern, text, flags?)` | `true` if the **entire** string matches |
| `re.search(pattern, text, flags?)` | First match → match object, or `nil` |
| `re.find_all(pattern, text, flags?)` | All matches → array of match objects |
| `re.find_all_strings(pattern, text, flags?)` | All matches → array of matched strings |
| `re.count(pattern, text, flags?)` | Number of non-overlapping matches |

`re.match` requires the pattern to cover the full input (`"42"` matches `\d+`, but `"x42"` does not). `re.test` only checks for a substring match.

### Replace & split

| Method | Description |
|--------|-------------|
| `re.replace(pattern, text, replacement, flags?)` | Replace all matches |
| `re.replace_n(pattern, text, replacement, count, flags?)` | Replace first `count` matches |
| `re.split(pattern, text, flags?)` | Split on pattern → string array |

Replacement strings support Rust regex expansion: `$0` (full match), `$1`, `$2`, … (capture groups).

### Compiled handles

| Method | Description |
|--------|-------------|
| `re.compile(pattern, flags?)` | Compile pattern → positive int handle |
| `re.close(handle)` | Release handle; returns `true` if it existed |
| `re.test_h(handle, text)` | Same as `re.test` |
| `re.match_h(handle, text)` | Same as `re.match` |
| `re.search_h(handle, text)` | Same as `re.search` |
| `re.find_all_h(handle, text)` | Same as `re.find_all` |
| `re.find_all_strings_h(handle, text)` | Same as `re.find_all_strings` |
| `re.replace_h(handle, text, replacement)` | Same as `re.replace` |
| `re.replace_n_h(handle, text, replacement, count)` | Same as `re.replace_n` |
| `re.split_h(handle, text)` | Same as `re.split` |
| `re.count_h(handle, text)` | Same as `re.count` |

Call `re.close(handle)` when done to free the compiled regex. Using a closed or invalid handle raises **E1302**.

## Legacy flat names

Global builtins with `re_` prefix mirror the namespace API:

`re_valid`, `re_escape`, `re_compile`, `re_close`, `re_test`, `re_match`, `re_search`, `re_find_all`, `re_find_all_strings`, `re_replace`, `re_replace_n`, `re_split`, `re_count`, `re_test_h`, `re_match_h`, `re_search_h`, `re_find_all_h`, `re_find_all_strings_h`, `re_replace_h`, `re_replace_n_h`, `re_split_h`, `re_count_h`

## Examples

```neko
import "re" as rx

fn main() {
    // Validate and escape
    assert(rx.valid("^\\w+@\\w+\\.\\w+$"))
    print(rx.escape("price: $9.99"))

    // Search with capture groups
    let m = rx.search("(\\w+)@(\\w+)\\.(\\w+)", "alice@example.com")
    print(m.groups[1])  // alice

    // Find all numeric runs
    let nums = rx.find_all_strings("-?\\d+", "temps: -3, 0, 12")
    for n in nums {
        print(n)
    }

    // Replace and split
    print(rx.replace("\\s+", "  too   many   spaces  ", " "))
    let fields = rx.split("[,;]\\s*", "name, age; city")

    // Compiled handle for repeated use
    let word = rx.compile("[A-Za-z]+", "i")
    let hits = rx.find_all_strings_h(word, "Neko runs FAST")
    rx.close(word)
}
```

Run the demo:

```bash
neko run examples/re_demo.neko
```

Run tests:

```bash
neko run tests/re.neko
```

## Errors

| Code | When |
|------|------|
| E1300 | Wrong argument count |
| E1301 | Invalid regex pattern (compile / match functions) |
| E1302 | Invalid or closed regex handle |

See [ERRORS.md](ERRORS.md) for the full error registry.

## Notes

- Patterns are compiled fresh on every stateless call; prefer `re.compile` + `re_*_h` in tight loops.
- `re.find_all` returns simple match objects (no capture detail) when the pattern has no groups; use a grouped pattern to get full capture arrays.
- Lookahead, lookbehind, and backreferences are **not** supported (Rust regex limitation).i