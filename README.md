# Neko

A modern programming language and complete development ecosystem.

**One Language. One Engine. Everything Built-in.**

## Quick Start

```bash
cargo build --release
cargo run --release --bin neko -- run examples/hello.neko
# or shorthand:
cargo run --release --bin neko -- examples/hello.neko
cargo run --release --bin neko -- examples/hello.neko time   # print execution time
```

Programs run on the bytecode VM by default (much faster than the AST interpreter). Use `--mode interp` for file `import`s and web DSL features. Classes, traits, structs, objects, `for`-loops, and `try/catch` run on both the VM and interpreter. The first `neko run` compiles to a `.nekobc` cache under `.neko-build/` in the current working directory; later runs skip recompilation when the source file is unchanged. See [docs/VM_MEMORY_AND_CACHE.md](docs/VM_MEMORY_AND_CACHE.md) for VM garbage collection and cache behavior.

Entry points are flexible: statements can live at the top level and run in order, Python-style — no `main` required. If a `main` function is defined, it runs as the entry point (after any top-level statements).

**Benchmark correctly** — `cargo run` adds ~300ms startup. Build once, then use the binary directly:

```bash
cargo build --release
./target/release/neko bench examples/fibonacci.neko
./target/release/neko run examples/fibonacci.neko
```

## CLI Commands

| Command | Description |
|---------|-------------|
| `neko run <file>` | Run a .neko program |
| `neko version` | Print version |
| `neko new <name>` | Create a new project |
| `neko test` | Run tests in `tests/` |
| `neko format <file>` | Format source code |
| `neko lint <file>` | Lint source code |
| `neko docs <file>` | Generate HTML docs |
| `neko build <file>` | Compile to bytecode |
| `neko serve <file>` | Run web server DSL |
| `neko ahiru create <name>` | Create ahiru-server backend project (wizard) |
| `neko ahiru serve` | Run ahiru project |

See [docs/AHIRU.md](docs/AHIRU.md) for the ahiru-server framework.

## Project Structure

```
Neko/
  crates/          # Rust implementation (NFE engine + CLI)
  examples/        # Example .neko programs
  tests/           # Test programs
  docs/            # Language spec and decisions
```

## Architecture

```
.neko source → Lexer → Parser → AST → Interpreter / VM
```

See [docs/DECISIONS.md](docs/DECISIONS.md) for design decisions.

- [VM memory & bytecode cache](docs/VM_MEMORY_AND_CACHE.md) — mark-compact GC, `.nekobc` / `.neko-build/`
- [Error codes](docs/ERRORS.md)
- [Object-oriented programming](docs/OOP.md) — classes, traits, inheritance, `super`, VM opcodes
- [JSON module](docs/JSON.md)
