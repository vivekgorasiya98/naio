# Niao

**One language. One engine. Everything built-in.**

Niao is a modern programming language and full development ecosystem — compiler, bytecode VM, package manager, standard libraries, web framework, and machine-learning stack — implemented in Rust.

- **Website:** [niao.risu.in](https://niao.risu.in)
- **Downloads & registry:** [nms.taurus-tech.in](https://nms.taurus-tech.in)
- **License:** MIT

---

## Why Niao?

| | |
|---|---|
| **Fast by default** | Programs compile to bytecode and run on a stack VM with mark-compact GC and `.niaobc` caching |
| **Familiar syntax** | C/Rust-style functions, types, classes, and traits — plus a block DSL for HTTP servers |
| **Batteries included** | JSON, I/O, networking, databases, parallelism, ML, and a production web framework ship with the toolchain |
| **Script or project** | Top-level statements run in order (Python-style); optional `main()` entry point |
| **Single toolchain** | `niao` runs and builds programs; `nm` installs and manages libraries |

---

## Quick start

### Install (recommended)

Download pre-built installers for Windows, Linux, or macOS from the [nm registry](https://nms.taurus-tech.in).

**Windows** — run `NiaoSetup.exe`, open a new terminal, then:

```cmd
niao version
niao run examples\hello.niao
```

**Linux / macOS** — after installing, run:

```bash
nm install --global    # install standard libraries to ~/.niao
niao version
niao run examples/hello.niao
```

See [windows/README.md](windows/README.md) and [mac/README.md](mac/README.md) for platform-specific guides.

### Build from source

Requires [Rust](https://rustup.rs/) 1.70+.

```bash
git clone https://github.com/vivekgorasiya98/naio.git
cd naio
cargo build --release --no-default-features -p niao_cli -p niao_nm

# Run a program
cargo run --release --bin niao -- examples/hello.niao

# Or use the built binary directly (faster — no cargo startup overhead)
./target/release/niao run examples/hello.niao
./target/release/niao bench examples/fibonacci.niao
```

### Hello, Niao

```niao
fn greet(name: string) -> string {
    return "Hello, " + name
}

fn main() {
    print(greet("Niao"))
}
```

```bash
niao run hello.niao
niao hello.niao          # shorthand
niao hello.niao time     # print execution time
```

---

## Language overview

Niao uses gradual static typing — annotate types when you want them, skip them when you don't.

```niao
// Variables and functions
let x = 42
fn add(a: int, b: int) -> int { return a + b }

// Control flow
for n in [1, 2, 3] { print(n) }

// Structs (data records)
struct Point { x: float, y: float }
let p = Point { x: 1.0, y: 2.0 }

// Classes, inheritance, and traits
class Counter {
    value: int
    fn inc(self) { self.value = self.value + 1 }
    fn get(self) -> int { return self.value }
}

// Error handling
try {
    throw error("something went wrong")
} catch (e) {
    print(e.message)
}
```

**Execution modes**

| Mode | Flag | Best for |
|------|------|----------|
| **VM** (default) | `niao run file.niao` | Speed — compiles to `.niaobc` cache under `.niao-build/` |
| **Interpreter** | `niao run file.niao --mode interp` | File `import`s, web DSL, multi-file dev workflows |

Entry points are flexible: top-level statements run in order, or define `fn main()` as the entry point.

More: [docs/OOP.md](docs/OOP.md) · [docs/ERRORS.md](docs/ERRORS.md) · [docs/grammar.ebnf](docs/grammar.ebnf)

---

## Web servers

### Block DSL

```niao
server {
    port = 3000
}

GET "/" {
    return "Hello Niao"
}

GET "/health" {
    return "ok"
}
```

```bash
niao serve web_server.niao
```

### ahiru-server (production framework)

Full HTTP/WebSocket backend with middleware, auth, databases, and project scaffolding:

```bash
niao ahiru create myapi
cd myapi
niao ahiru serve
```

```niao
import "ahiru"

fn main() {
    let app = ahiru_app_new()
    ahiru_app_get(app, "/health", health, {is_public: true})
    ahiru_app_listen(app, "0.0.0.0", 3000)
}

fn health(ctx) {
    return ahiru_json_response(200, "{\"status\":\"ok\"}")
}
```

More: [docs/AHIRU.md](docs/AHIRU.md)

---

## Standard libraries

Install with `nm install <name>` or `nm install --global` for the full set.

| Library | Import | Description |
|---------|--------|-------------|
| **core** | *(built-in)* | `print`, `len`, `type`, `assert`, arrays, errors |
| **json** | `import "json"` | Parse, stringify, object utilities |
| **io** | `import "io"` | Files, paths, streaming, async tasks |
| **re** | `import "re"` | Regular expressions |
| **net** | `import "net"` | HTTP, TCP/UDP, DNS, TLS, WebSocket |
| **parallel** | `import "parallel"` | Threads, mutexes, channels, worker pools |
| **time** | `import "time"` | Clocks, formatting, time zones |
| **dsa** | `import "dsa"` | Lists, stacks, heaps, maps, graphs, sort |
| **nos** | `import "nos"` | OS interface, process, filesystem |
| **nenv** | `import "nenv"` | Environment variables, `.env` loading |
| **nsqlite** | `import "nsqlite"` | SQLite — schema, migrations, async |
| **npg** | `import "npg"` | PostgreSQL — pools, migrations, async |
| **nmongo** | `import "nmongo"` | MongoDB — CRUD, aggregation, GridFS |
| **ncl** | `import "ncl"` | DataFrames, Series, CSV, ndarray bridge |
| **nml** | `import "nml"` | Machine learning — tensors, training, classic ML |
| **nvis** | `import "nvis"` | Charts — line, scatter, histogram (SVG + ASCII) |
| **ahiru** | `import "ahiru"` | ahiru-server web framework |

**Registry-only libraries** (larger downloads):

| Library | Description |
|---------|-------------|
| **nllm** | GGUF LLM inference (llama.cpp / Candle) |
| **nrag** | Vector RAG index and embeddings |

```bash
nm search              # browse catalog
nm install json io     # install specific libraries
nm list --installed    # show what's installed
nm info nml            # library details
```

More: [docs/JSON.md](docs/JSON.md) · [docs/NET.md](docs/NET.md) · [docs/NML.md](docs/NML.md) · [docs/NPG.md](docs/NPG.md) · [docs/NMONGO.md](docs/NMONGO.md)

---

## Machine learning (NML)

Native ML with Rust SIMD kernels (optional CUDA). Training loops run in native code — Niao scripts orchestrate, not inner loops.

```niao
import "nml"

fn main() {
    let l1 = nml_linear(4, 8)
    let relu = nml_relu_layer()
    let l2 = nml_linear(8, 2)
    let model = nml_sequential([l1, relu, l2])

    let x = nml_randn([32, 4])
    let y = nml_zeros([32, 1])
    let trainer = nml_trainer(model, "adam", "mse", 0.001)
    let loss = nml_train_epoch(trainer, x, y)
    print("loss", loss)
}
```

Includes deep learning (autograd, layers, optimizers), classic ML (k-means, trees, random forest), data pipelines, and graph neural networks.

More: [docs/NML.md](docs/NML.md) · [docs/NML_GRAPH.md](docs/NML_GRAPH.md) · [docs/NVIS.md](docs/NVIS.md)

---

## CLI reference

| Command | Description |
|---------|-------------|
| `niao run <file>` | Run a `.niao` program (VM by default) |
| `niao <file>` | Shorthand for `run` |
| `niao version` | Print toolchain version |
| `niao new <name>` | Scaffold a new project |
| `niao test` | Run test programs in `tests/` |
| `niao format <file>` | Format source (`--write` to save in place) |
| `niao lint <file>` | Lint source |
| `niao docs <file>` | Generate HTML documentation |
| `niao build <file>` | Compile to bytecode (`.niaobc`) |
| `niao serve <file>` | Start web server DSL |
| `niao bench <file>` | Benchmark VM execution |
| `niao clean` | Remove stale bytecode caches |
| `niao ahiru create <name>` | Create ahiru-server project |
| `niao ahiru serve` | Run ahiru project |
| `niao ahiru migrate` | Apply SQL migrations |
| `nm install --global` | Install full toolchain + libraries |
| `nm install <lib>` | Install a library |
| `nm search [query]` | Search the catalog |
| `nm update` | Update installed libraries |

---

## Project layout

```
naio/
├── crates/              # Rust implementation
│   ├── niao_lexer       # Tokenizer
│   ├── niao_parser      # Parser → AST
│   ├── niao_vm          # Bytecode stack VM
│   ├── niao_runtime     # Standard library builtins
│   ├── niao_cli         # `niao` command-line tool
│   ├── niao_nm          # `nm` package manager
│   ├── ahiru_core       # ahiru-server HTTP core
│   ├── niao_ml          # Deep learning engine
│   └── ...
├── examples/            # Example .niao programs
├── tests/               # Test programs (`niao test`)
├── docs/                # Language spec and library reference
├── windows/             # Windows installer build
├── mac/                 # macOS portable bundle
├── vscode-niao/         # VS Code syntax highlighting
└── package-manager/     # nm online registry (Node.js)
```

### Architecture

```
.niao source → Lexer → Parser → AST → Interpreter
                                    ↘ IR → Bytecode → VM
```

Bytecode caches live in `.niao-build/` and invalidate on source changes. See [docs/VM_MEMORY_AND_CACHE.md](docs/VM_MEMORY_AND_CACHE.md).

Design decisions: [docs/DECISIONS.md](docs/DECISIONS.md)

---

## Examples

```bash
niao run examples/hello.niao           # Hello world
niao run examples/loops.niao           # for-loops and arrays
niao run examples/oop_basics.niao      # classes and methods
niao run examples/json_demo.niao       # JSON library
niao run examples/net_demo.niao        # HTTP client
niao run examples/npg_demo.niao        # PostgreSQL
niao run examples/parallel_demo.niao   # threading
niao run examples/ahiru_hello.niao     # ahiru-server API
niao run examples/fibonacci.niao       # performance demo
```

---

## Editor support

A VS Code extension provides syntax highlighting for `.niao` files.

```bash
cd vscode-niao
npm install
npm run package
# Install niao-language-0.1.0.vsix via Extensions: Install from VSIX
```

See [vscode-niao/README.md](vscode-niao/README.md).

---

## Documentation

| Topic | Doc |
|-------|-----|
| Design decisions | [docs/DECISIONS.md](docs/DECISIONS.md) |
| Object-oriented programming | [docs/OOP.md](docs/OOP.md) |
| Error codes | [docs/ERRORS.md](docs/ERRORS.md) |
| VM memory & cache | [docs/VM_MEMORY_AND_CACHE.md](docs/VM_MEMORY_AND_CACHE.md) |
| ahiru-server | [docs/AHIRU.md](docs/AHIRU.md) |
| Machine learning | [docs/NML.md](docs/NML.md) |
| JSON | [docs/JSON.md](docs/JSON.md) |
| Networking | [docs/NET.md](docs/NET.md) |
| PostgreSQL | [docs/NPG.md](docs/NPG.md) |
| MongoDB | [docs/NMONGO.md](docs/NMONGO.md) |
| SQLite | [docs/NSQLITE.md](docs/NSQLITE.md) |
| Windows guide | [windows/GUIDE.md](windows/GUIDE.md) |

---

## Contributing

Contributions are welcome. To get started:

1. Fork the repository
2. Create a feature branch
3. Make your changes and run `cargo test` / `niao test`
4. Open a pull request

For release builds and publishing packages, see [package-manager/README.md](package-manager/README.md).

---

## License

MIT — see [LICENSE](LICENSE) or the workspace `Cargo.toml` for details.
