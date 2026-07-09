# Complete Niao User Guide (Windows)

Everything you need to install, run, and write Niao programs on Windows.

---

## Install (like Python)

### Option A — Installer (recommended)

1. Run **`NiaoSetup.exe`** (double-click)
2. Open a **new** Command Prompt or PowerShell
3. Run:

```cmd
niao version
niao run examples\hello.niao
```

Installs to `%USERPROFILE%\.niao`:

```
C:\Users\You\.niao\
  bin\niao.exe
  bin\nm.exe
  install.json
  niao_libs\     ← all 15 libraries pre-installed
  examples\
```

PATH is updated automatically. No Rust, no build step, no `nm install`.

### Option B — Portable (no install)

From the `windows\` folder after `build.ps1`:

```cmd
niao.cmd run examples\hello.niao
test.cmd
```

### Build the installer yourself

From the Niao repo:

```powershell
powershell -File windows\build.ps1
```

Creates `windows\NiaoSetup.exe`.

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

Run:

```cmd
niao run hello.niao
niao hello.niao
```

---

## CLI commands

| Command | Description |
|---------|-------------|
| `niao run <file>` | Run a program |
| `niao <file>` | Shorthand for run |
| `niao <file> time` | Run and show execution time |
| `niao version` | Print version |
| `niao new <name>` | Create project |
| `niao test` | Run tests in `tests\` |
| `niao format <file>` | Format source |
| `niao format <file> --write` | Format in place |
| `niao lint <file>` | Lint source |
| `niao build <file>` | Compile to bytecode |
| `niao bench <file>` | Benchmark |
| `niao serve <file>` | Web server DSL |
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

```niao
import "json"
import "re"

fn main() {
    print(json.stringify({ "ok": true }))
    print(re.test("\\d+", "x42"))
}
```

---

## Language basics

```niao
let x = 42
let name = "Niao"

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

```niao
import "json"           // standard library
import "re" as rx       // alias
import "utils.niao"     // local file (uses interpreter)
```

---

## Error handling

```niao
try {
    throw error("failed")
} catch (e) {
    print(e.message)
}
```

---

## Troubleshooting

### `niao` is not recognized

- Run `NiaoSetup.exe` again, or
- Open a **new** terminal after install (PATH refresh), or
- Use portable: `niao.cmd run examples\hello.niao`

### Libraries not found

All libs are built into `niao.exe`. The `niao_libs` folder is for `nm list` — already populated by the installer.

### Reinstall

Run `NiaoSetup.exe` again. It overwrites `%USERPROFILE%\.niao`.

---

## Folder layout

```
windows/
  NiaoSetup.exe     ← double-click to install
  niao.cmd          ← portable launcher
  test.cmd          ← smoke tests
  niao_home/        ← portable runtime
  examples/
  GUIDE.md          ← this file
  build.ps1         ← rebuild installer
```

---

For the full language reference (OOP, ahiru, VM modes), see the main repo `docs/` folder or `mac/GUIDE.md`.
