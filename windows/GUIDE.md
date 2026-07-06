# Complete Neko User Guide (Windows)

Everything you need to install, run, and write Neko programs on Windows.

---

## Install (like Python)

### Option A — Installer (recommended)

1. Run **`NekoSetup.exe`** (double-click)
2. Open a **new** Command Prompt or PowerShell
3. Run:

```cmd
neko version
neko run examples\hello.neko
```

Installs to `%USERPROFILE%\.neko`:

```
C:\Users\You\.neko\
  bin\neko.exe
  bin\nm.exe
  install.json
  neko_libs\     ← all 15 libraries pre-installed
  examples\
```

PATH is updated automatically. No Rust, no build step, no `nm install`.

### Option B — Portable (no install)

From the `windows\` folder after `build.ps1`:

```cmd
neko.cmd run examples\hello.neko
test.cmd
```

### Build the installer yourself

From the Neko repo:

```powershell
powershell -File windows\build.ps1
```

Creates `windows\NekoSetup.exe`.

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

Run:

```cmd
neko run hello.neko
neko hello.neko
```

---

## CLI commands

| Command | Description |
|---------|-------------|
| `neko run <file>` | Run a program |
| `neko <file>` | Shorthand for run |
| `neko <file> time` | Run and show execution time |
| `neko version` | Print version |
| `neko new <name>` | Create project |
| `neko test` | Run tests in `tests\` |
| `neko format <file>` | Format source |
| `neko format <file> --write` | Format in place |
| `neko lint <file>` | Lint source |
| `neko build <file>` | Compile to bytecode |
| `neko bench <file>` | Benchmark |
| `neko serve <file>` | Web server DSL |
| `nm list --installed` | List installed libraries |

---

## Standard libraries (pre-installed)

| Library | Import |
|---------|--------|
| json | `import "json"` |
| io | `import "io"` |
| re | `import "re"` |
| nos | `import "nos"` |
| nenv | `import "nenv"` |
| time | `import "time"` |
| dsa | `import "dsa"` |
| net | `import "net"` |
| parallel | `import "parallel"` |
| nsqlite | `import "nsqlite"` |
| npg | `import "npg"` |
| nmongo | `import "nmongo"` |
| ncl | `import "ncl"` |
| ahiru | `import "ahiru"` |

Example:

```neko
import "json"
import "re"

fn main() {
    print(json.stringify({ "ok": true }))
    print(re.test("\\d+", "x42"))
}
```

---

## Language basics

```neko
let x = 42
let name = "Neko"

if x > 0 {
    print(name)
}

for i in [1, 2, 3] {
    print(i)
}

fn add(a: int, b: int) -> int {
    return a + b
}
```

Top-level statements run without `main()` (script style).

---

## Imports

```neko
import "json"           // standard library
import "re" as rx       // alias
import "utils.neko"     // local file (uses interpreter)
```

---

## Error handling

```neko
try {
    throw error("failed")
} catch (e) {
    print(e.message)
}
```

---

## Troubleshooting

### `neko` is not recognized

- Run `NekoSetup.exe` again, or
- Open a **new** terminal after install (PATH refresh), or
- Use portable: `neko.cmd run examples\hello.neko`

### Libraries not found

All libs are built into `neko.exe`. The `neko_libs` folder is for `nm list` — already populated by the installer.

### Reinstall

Run `NekoSetup.exe` again. It overwrites `%USERPROFILE%\.neko`.

---

## Folder layout

```
windows/
  NekoSetup.exe     ← double-click to install
  neko.cmd          ← portable launcher
  test.cmd          ← smoke tests
  neko_home/        ← portable runtime
  examples/
  GUIDE.md          ← this file
  build.ps1         ← rebuild installer
```

---

For the full language reference (OOP, ahiru, VM modes), see the main repo `docs/` folder or `mac/GUIDE.md`.
