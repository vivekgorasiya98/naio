# Complete Neko User Guide

Everything you need to install, run, and write programs in the Neko programming language.

---

## Table of contents

1. [What is Neko?](#what-is-neko)
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

## What is Neko?

Neko is a modern programming language with a built-in runtime, bytecode VM, and standard libraries for JSON, I/O, networking, databases, regex, threading, and more.

```
.neko source  →  Lexer  →  Parser  →  AST  →  VM (fast) or Interpreter
```

- **File extension:** `.neko`
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

### Step 2 — Build Neko (one time, ~5–10 minutes)

```bash
cd mac
chmod +x setup.sh neko test.sh
./setup.sh
```

This compiles `neko` and `nm` into `neko_home/bin/`. All standard libraries are already registered — no extra install step.

### Step 3 — Verify

```bash
./neko version
./neko run examples/hello.neko
./test.sh
```

### Use `neko` from anywhere (optional)

Add to `~/.zshrc`:

```bash
export NEKO_HOME="/full/path/to/mac/neko_home"
export PATH="/full/path/to/mac/neko_home/bin:$PATH"
```

Reload: `source ~/.zshrc`

Then:

```bash
neko version
neko run myprogram.neko
```

### Without PATH setup

Always use the launcher from inside `mac/`:

```bash
./neko run examples/hello.neko
```

---

## Your first program

Create `hello.neko`:

```neko
fn greet(name: string) -> string {
    return "Hello, " + name
}

fn main() {
    print(greet("Neko"))
}
```

Run it:

```bash
./neko run hello.neko
# or shorthand:
./neko hello.neko
```

**Script style** — no `main` required:

```neko
let x = 10
let y = 32
print(x + y)
```

Run:

```bash
./neko run script.neko
```

---

## CLI commands

| Command | Description |
|---------|-------------|
| `neko run <file>` | Run a `.neko` program |
| `neko <file>` | Shorthand for `neko run <file>` |
| `neko <file> time` | Run and print execution time |
| `neko run <file> -t` | Same as above |
| `neko run <file> --mode interp` | Use interpreter (required for file imports) |
| `neko version` | Print version |
| `neko new <name>` | Create a new project |
| `neko test` | Run all `.neko` files in `tests/` |
| `neko format <file>` | Print formatted source |
| `neko format <file> --write` | Format file in place |
| `neko lint <file>` | Lint source |
| `neko build <file>` | Compile to bytecode (`.nekobc` cache) |
| `neko bench <file>` | Benchmark VM runs |
| `neko serve <file>` | Run web server DSL |
| `neko docs <file>` | Generate HTML documentation |
| `neko ahiru create <name>` | Create backend project |
| `neko ahiru serve` | Run ahiru backend |

### Pass arguments to your script

```bash
./neko run app.neko arg1 arg2
```

Inside the program, use the `nos` library:

```neko
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
./neko run program.neko
# same as:
./neko run program.neko --mode vm
```

- Fast bytecode execution
- Caches compiled bytecode in `.neko-build/` (in the current directory)
- Best for: math, loops, classes, most standalone programs

### Interpreter mode

```bash
./neko run program.neko --mode interp
```

Required when your program:

- `import`s another `.neko` file (e.g. `import "utils.neko"`)
- Uses the web server DSL or ahiru imports

The CLI auto-switches to interpreter when it detects file imports.

### Execution time

```bash
./neko run fibonacci.neko --time
./neko fibonacci.neko time
```

---

## Language basics

### Variables

```neko
let x = 42
let name = "Neko"
let ok = true
```

### Types

Neko uses gradual typing — types are optional but supported:

```neko
fn add(a: int, b: int) -> int {
    return a + b
}
```

Common types: `int`, `float`, `string`, `bool`, `array`, `object`, `nil`, `error`, `fn`

Check a value's type:

```neko
print(type(42))       // int
print(type("hello"))  // string
```

### Strings

```neko
let s = "Hello"
let combined = s + ", Neko"
let escaped = "line one\nline two"
```

### Arrays

```neko
let nums = [1, 2, 3]
print(nums[0])
print(len(nums))
```

### Objects

```neko
let user = { name: "Alice", age: 30 }
print(user.name)
user.age = 31
```

### Control flow

```neko
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

```neko
fn square(n: int) -> int {
    return n * n
}

print(square(5))
```

### Structs (data records)

```neko
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

### Standard library (built into `neko`)

```neko
import "json"
import "re"
import "io"
```

Also works with `std/` prefix:

```neko
import "std/json"
```

### Custom alias

```neko
import "re" as rx
print(rx.test("\\d+", "x42"))
```

### Import another `.neko` file

```neko
import "math.neko"

fn main() {
    print(add(2, 3))   // function exported from math.neko
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

```neko
import "json"

fn main() {
    let data = json.parse("{\"name\": \"Neko\"}")
    print(data.name)
    print(json.stringify(data))
}
```

### Example — Regex

```neko
import "re"

fn main() {
    if re.test("\\d+", "item42") {
        let m = re.search("(\\d+)", "item42")
        print(m.groups[1])   // 42
    }
}
```

### Example — File I/O

```neko
import "io"

fn main() {
    io.write_text("out.txt", "Hello from Neko")
    print(io.read_text("out.txt"))
}
```

### Test all libraries at once

```bash
./neko run examples/libs_smoke.neko
```

---

## Object-oriented programming

### Class

```neko
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

```neko
class Dog extends Animal {
    fn speak(self) -> string {
        return self.name + " barks"
    }
}
```

### Traits

```neko
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

```neko
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

```neko
let e = error("failed")
let e2 = error(4001, "invalid input")
```

---

## Projects and tests

### Create a project

```bash
./neko new myapp
```

Creates a project folder with starter files.

### Run tests

Put test files in `tests/` and run:

```bash
./neko test
./neko test path/to/tests
```

Tests use `assert()` — the program exits with an error if any assertion fails.

### Format code

```bash
./neko format myfile.neko
./neko format myfile.neko --write
```

### Compile to bytecode

```bash
./neko build myfile.neko
```

Bytecode is cached under `.neko-build/` in the current working directory.

---

## Package manager (`nm`)

`nm` manages library installs. In this Mac bundle, everything is already installed.

```bash
./neko_home/bin/nm list
./neko_home/bin/nm list --installed
```

If you use global PATH:

```bash
nm list --installed
```

---

## Tips and troubleshooting

### `neko: command not found`

You have not added Neko to PATH. Either:

```bash
cd mac
./neko run examples/hello.neko
```

Or add `mac/neko_home/bin` to PATH (see [Install on MacBook](#install-on-macbook)).

### `Neko not built yet`

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

- Standard libs: use `import "json"` not `import "json.neko"`
- File imports: file must be in the same folder or correct relative path
- File imports need interpreter mode (auto-selected by CLI)

### Program runs slow the first time

The first `neko run` compiles to bytecode and caches it. Later runs are faster.

### Use the binary directly for benchmarks

`cargo` adds startup overhead. After setup, benchmark with:

```bash
./neko_home/bin/neko bench examples/fibonacci.neko
```

### Refresh the Mac bundle (on Windows)

From the main Neko repo:

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
./neko run program.neko
./neko program.neko
./neko program.neko time

# Libraries
import "json"
import "re"
import "io"

# Check install
./neko version
./test.sh
```

---

## What's in this folder

```
mac/
  GUIDE.md          ← this file
  README.md         ← short install notes
  setup.sh          ← one-time Mac build
  neko              ← launcher script
  test.sh           ← smoke tests
  examples/         ← demo programs
  engine/           ← compiler source (for setup.sh)
  neko_home/
    bin/            ← neko + nm (after setup)
    neko_libs/      ← all libraries pre-registered
    install.json
```

Happy coding with Neko.
