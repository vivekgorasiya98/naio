# Niao

A modern programming language and complete development ecosystem.

**One Language. One Engine. Everything Built-in.**

## Quick Start

```bash
cargo build --release --no-default-features -p niao_cli -p niao_nm
cargo run --release --no-default-features --bin niao -- run examples/hello.niao
# or shorthand:
cargo run --release --bin niao -- examples/hello.niao
cargo run --release --bin niao -- examples/hello.niao time   # print execution time
```

Programs run on the bytecode VM by default (much faster than the AST interpreter). Use `--mode interp` for file `import`s and web DSL features. Classes, traits, structs, objects, `for`-loops, and `try/catch` run on both the VM and interpreter. The first `niao run` compiles to a `.niaobc` cache under `.niao-build/` in the current working directory; later runs skip recompilation when the source file is unchanged. See [docs/VM_MEMORY_AND_CACHE.md](docs/VM_MEMORY_AND_CACHE.md) for VM garbage collection and cache behavior.

Entry points are flexible: statements can live at the top level and run in order, Python-style — no `main` required. If a `main` function is defined, it runs as the entry point (after any top-level statements).

**Benchmark correctly** — `cargo run` adds ~300ms startup. Build once, then use the binary directly:

```bash
cargo build --release --no-default-features -p niao_cli
./target/release/niao bench examples/fibonacci.niao
./target/release/niao run examples/fibonacci.niao
```

## CLI Commands

| Command | Description |
|---------|-------------|
| `niao run <file>` | Run a .niao program |
| `niao version` | Print version |
| `niao new <name>` | Create a new project |
| `niao test` | Run tests in `tests/` |
| `niao format <file>` | Format source code |
| `niao lint <file>` | Lint source code |
| `niao docs <file>` | Generate HTML docs |
| `niao build <file>` | Compile to bytecode |
| `niao serve <file>` | Run web server DSL |
| `niao ahiru create <name>` | Create ahiru-server backend project (wizard) |
| `niao ahiru serve` | Run ahiru project |

See [docs/AHIRU.md](docs/AHIRU.md) for the ahiru-server framework.

## Project Structure

```
Niao/
  crates/          # Rust implementation (NFE engine + CLI)
  examples/        # Example .niao programs
  tests/           # Test programs
  docs/            # Language spec and decisions
```

## Architecture

```
.niao source → Lexer → Parser → AST → Interpreter / VM
```

See [docs/DECISIONS.md](docs/DECISIONS.md) for design decisions.

- [VM memory & bytecode cache](docs/VM_MEMORY_AND_CACHE.md) — mark-compact GC, `.niaobc` / `.niao-build/`
- [Error codes](docs/ERRORS.md)
- [Object-oriented programming](docs/OOP.md) — classes, traits, inheritance, `super`, VM opcodes
- [JSON module](docs/JSON.md)
