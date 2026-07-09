# Complete Niao User Guide

Everything you need to install, run, and write programs in the Niao programming language.

---

## Table of contents

1. [What is Niao?](#what-is-niao)
2. [Install on MacBook](#install-on-macbook)
3. [Your first program](#your-first-program)
4. [CLI commands](#cli-commands)
5. [How programs run](#how-programs-run)
6. [Language basics](#language-basics)
7. [Imports and modules](#imports-and-modules)
8. [Standard libraries](#standard-libraries)
9. [Object-oriented programming](#object-oriented-programming)
10. [Error handling](#error-handling)
11. [Projects and tests](#projects-and-tests)
12. [Package manager (`nm`)](#package-manager-nm)
13. [Tips and troubleshooting](#tips-and-troubleshooting)

---

## What is Niao?

Niao is a modern programming language with a built-in runtime, bytecode VM, and standard libraries for JSON, I/O, networking, databases, regex, threading, and more.

```
.niao source  →  Lexer  →  Parser  →  AST  →  VM (fast) or Interpreter
```

- **File extension:** `.niao`
- **Default execution:** bytecode VM (fast)
- **Entry point:** top-level statements run in order (Python-style), or a `main()` function if you define one

---

## Install on MacBook

Copy **only the `mac/` folder** to your MacBook. You do not need the rest of the repo.

### Step 1 — System tools (one time)

Open **Terminal** and run:

```bash
xcode-select --install
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

### Step 2 — Build Niao (one time, ~5–10 minutes)

```bash
cd mac
chmod +x setup.sh niao test.sh
./setup.sh
```

This compiles `niao` and `nm` into `niao_home/bin/`. All standard libraries are already registered — no extra install step.

### Step 3 — Verify

```bash
./niao version
./niao run examples/hello.niao
./test.sh
```

### Use `niao` from anywhere (optional)

Add to `~/.zshrc`:

```bash
export NIAO_HOME="/full/path/to/mac/niao_home"
export PATH="/full/path/to/mac/niao_home/bin:$PATH"
```

Reload: `source ~/.zshrc`

Then:

```bash
niao version
niao run myprogram.niao
```

### Without PATH setup

Always use the launcher from inside `mac/`:

```bash
./niao run examples/hello.niao
```

---

## Your first program

Create `hello.niao`:

```niao
fn greet(name: string) -> string {
    return "Hello, " + name
}

fn main() {
    print(greet("Niao"))
}
```

Run it:

```bash
./niao run hello.niao
# or shorthand:
./niao hello.niao
```

**Script style** — no `main` required:

```niao
let x = 10
let y = 32
print(x + y)
```

Run:

```bash
./niao run script.niao
```

---

## CLI commands

| Command | Description |
|---------|-------------|
| `niao run <file>` | Run a `.niao` program |
| `niao <file>` | Shorthand for `niao run <file>` |
| `niao <file> time` | Run and print execution time |
| `niao run <file> -t` | Same as above |
| `niao run <file> --mode interp` | Use interpreter (required for file imports) |
| `niao version` | Print version |
| `niao new <name>` | Create a new project |
| `niao test` | Run all `.niao` files in `tests/` |
| `niao format <file>` | Print formatted source |
| `niao format <file> --write` | Format file in place |
| `niao lint <file>` | Lint source |
| `niao build <file>` | Compile to bytecode (`.niaobc` cache) |
| `niao bench <file>` | Benchmark VM runs |
| `niao serve <file>` | Run web server DSL |
| `niao docs <file>` | Generate HTML documentation |
| `niao ahiru create <name>` | Create backend project |
| `niao ahiru serve` | Run ahiru backend |

### Pass arguments to your script

```bash
./niao run app.niao arg1 arg2
```

Inside the program, use the `nos` library:

```niao
import "nos"

fn main() {
    for a in nos.argv() {
        print(a)
    }
}
```

---

## How programs run

### VM mode (default)

```bash
./niao run program.niao
# same as:
./niao run program.niao --mode vm
```

- Fast bytecode execution
- Caches compiled bytecode in `.niao-build/` (in the current directory)
- Best for: math, loops, classes, most standalone programs

### Interpreter mode

```bash
./niao run program.niao --mode interp
```

Required when your program:

- `import`s another `.niao` file (e.g. `import "utils.niao"`)
- Uses the web server DSL or ahiru imports

The CLI auto-switches to interpreter when it detects file imports.

### Execution time

```bash
./niao run fibonacci.niao --time
./niao fibonacci.niao time
```

---

## Language basics

### Variables

```niao
let x = 42
let name = "Niao"
let ok = true
```

### Types

Niao uses gradual typing — types are optional but supported:

```niao
fn add(a: int, b: int) -> int {
    return a + b
}
```

Common types: `int`, `float`, `string`, `bool`, `array`, `object`, `nil`, `error`, `fn`

Check a value's type:

```niao
print(type(42))       // int
print(type("hello"))  // string
```

### Strings

```niao
let s = "Hello"
let combined = s + ", Niao"
let escaped = "line one\nline two"
```

### Arrays

```niao
let nums = [1, 2, 3]
print(nums[0])
print(len(nums))
```

### Objects

```niao
let user = { name: "Alice", age: 30 }
print(user.name)
user.age = 31
```

### Control flow

```niao
if x > 0 {
    print("positive")
} else {
    print("zero or negative")
}

while x > 0 {
    x = x - 1
}

for item in [1, 2, 3] {
    print(item)
}
```

### Functions

```niao
fn square(n: int) -> int {
    return n * n
}

print(square(5))
```

### Structs (data records)

```niao
struct Point {
    x: int
    y: int
}

fn main() {
    let p = Point { x: 1, y: 2 }
    print(p.x)
}
```

### Builtins

| Function | Description |
|----------|-------------|
| `print(...)` | Print values to stdout |
| `len(x)` | Length of string or array |
| `type(x)` | Type name as string |
| `assert(cond, msg?)` | Stop if condition is false |
| `input(prompt?)` | Read line from stdin |

---

## Imports and modules

### Standard library (built into `niao`)

```niao
import "json"
import "re"
import "io"
```

Also works with `std/` prefix:

```niao
import "std/json"
```

### Custom alias

```niao
import "re" as rx
print(rx.test("\\d+", "x42"))
```

### Import another `.niao` file

```niao
import "math.niao"

fn main() {
    print(add(2, 3))   // function exported from math.niao
}
```

File imports resolve relative to the importing file's directory. Use interpreter mode (automatic when file imports are detected).

---

## Standard libraries

All libraries below are **pre-installed** in this Mac bundle. Import and use directly.

| Library | Import | What it does |
|---------|--------|--------------|
| **core** | (builtins) | `print`, `len`, `type`, `assert`, arrays |
| **json** | `import "json"` | Parse, stringify, object utilities |
| **io** | `import "io"` | Files, paths, streaming I/O |
| **re** | `import "re"` | Regex match, search, replace, split |
| **nos** | `import "nos"` | OS, process, paths, `argv()` |
| **nenv** | `import "nenv"` | Environment variables, `.env` files |
| **time** | `import "time"` | Clock, formatting, time zones |
| **dsa** | `import "dsa"` | Lists, stacks, queues, maps, sorting |
| **net** | `import "net"` | HTTP, TCP, DNS, TLS, WebSocket |
| **parallel** | `import "parallel"` | Threads, mutexes, channels |
| **nsqlite** | `import "nsqlite"` | SQLite database |
| **npg** | `import "npg"` | PostgreSQL |
| **nmongo** | `import "nmongo"` | MongoDB |
| **ncl** | `import "ncl"` | DataFrames, CSV, column math |
| **ahiru** | `import "ahiru"` | HTTP/WebSocket backend framework |

### Example — JSON

```niao
import "json"

fn main() {
    let data = json.parse("{\"name\": \"Niao\"}")
    print(data.name)
    print(json.stringify(data))
}
```

### Example — Regex

```niao
import "re"

fn main() {
    if re.test("\\d+", "item42") {
        let m = re.search("(\\d+)", "item42")
        print(m.groups[1])   // 42
    }
}
```

### Example — File I/O

```niao
import "io"

fn main() {
    io.write_text("out.txt", "Hello from Niao")
    print(io.read_text("out.txt"))
}
```

### Test all libraries at once

```bash
./niao run examples/libs_smoke.niao
```

---

## Object-oriented programming

### Class

```niao
class Animal {
    name: string;

    fn speak(self) -> string {
        return self.name + " makes a sound"
    }
}

fn main() {
    let a = Animal { name: "Cat" }
    print(a.speak())
}
```

### Inheritance

```niao
class Dog extends Animal {
    fn speak(self) -> string {
        return self.name + " barks"
    }
}
```

### Traits

```niao
trait Drawable {
    fn draw(self) -> string
}

class Circle implements Drawable {
    radius: int;

    fn draw(self) -> string {
        return "circle r=" + self.radius
    }
}
```

Keywords: `class`, `trait`, `extends`, `implements`, `self`, `super`, `static`, `public`, `private`

---

## Error handling

```niao
fn risky() -> int {
    throw error("something went wrong")
}

fn main() {
    try {
        risky()
    } catch (e) {
        print(e.message)
        print(e.code)
    }
}
```

Create errors:

```niao
let e = error("failed")
let e2 = error(4001, "invalid input")
```

---

## Projects and tests

### Create a project

```bash
./niao new myapp
```

Creates a project folder with starter files.

### Run tests

Put test files in `tests/` and run:

```bash
./niao test
./niao test path/to/tests
```

Tests use `assert()` — the program exits with an error if any assertion fails.

### Format code

```bash
./niao format myfile.niao
./niao format myfile.niao --write
```

### Compile to bytecode

```bash
./niao build myfile.niao
```

Bytecode is cached under `.niao-build/` in the current working directory.

---

## Package manager (`nm`)

`nm` manages library installs. In this Mac bundle, everything is already installed.

```bash
./niao_home/bin/nm list
./niao_home/bin/nm list --installed
```

If you use global PATH:

```bash
nm list --installed
```

---

## Tips and troubleshooting

### `niao: command not found`

You have not added Niao to PATH. Either:

```bash
cd mac
./niao run examples/hello.niao
```

Or add `mac/niao_home/bin` to PATH (see [Install on MacBook](#install-on-macbook)).

### `Niao not built yet`

Run setup first:

```bash
chmod +x setup.sh
./setup.sh
```

### `Rust not found`

Install Rust:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
./setup.sh
```

### Import errors / module not found

- Standard libs: use `import "json"` not `import "json.niao"`
- File imports: file must be in the same folder or correct relative path
- File imports need interpreter mode (auto-selected by CLI)

### Program runs slow the first time

The first `niao run` compiles to bytecode and caches it. Later runs are faster.

### Use the binary directly for benchmarks

`cargo` adds startup overhead. After setup, benchmark with:

```bash
./niao_home/bin/niao bench examples/fibonacci.niao
```

### Refresh the Mac bundle (on Windows)

From the main Niao repo:

```powershell
powershell -File mac/prepare-bundle.ps1
```

Then copy the updated `mac/` folder to your MacBook again.

---

## Quick reference card

```bash
# Setup (once)
./setup.sh

# Run
./niao run program.niao
./niao program.niao
./niao program.niao time

# Libraries
import "json"
import "re"
import "io"

# Check install
./niao version
./test.sh
```

---

## What's in this folder

```
mac/
  GUIDE.md          ← this file
  README.md         ← short install notes
  setup.sh          ← one-time Mac build
  niao              ← launcher script
  test.sh           ← smoke tests
  examples/         ← demo programs
  engine/           ← compiler source (for setup.sh)
  niao_home/
    bin/            ← niao + nm (after setup)
    niao_libs/      ← all libraries pre-registered
    install.json
```

Happy coding with Niao.
