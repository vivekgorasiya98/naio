# Changelog

## 0.2.1 — 2026-07-06

### Performance
- VM, bytecode compiler, tensor runtime, and CLI startup optimizations across `neko_vm`, `neko_bytecode`, `neko_tensor`, `neko_runtime`, and `neko_cli`.

### Fixes
- Windows MSVC release builds link cleanly with `/NODEFAULTLIB:libcpmt.lib` (CRT mismatch with `libort_sys` / `libesaxx_rs`).
- Bytecode wire-format test checks magic header and roundtrip (wire container may be larger than pure JSON when OOP metadata is embedded).

### Notes
- `BYTECODE_CACHE_VERSION` remains **10** — bytecode format unchanged; existing `.nekobc` caches remain valid.
