# Changelog

## 0.2.2 — 2026-07-07

### Release
- Toolchain (`niao`, `nm`) and distribution bundles bumped to **0.2.2**.
- Standard library packages aligned to **0.2.2** (`core`, `dsa`, `json`, `io`, `re`, `net`, `parallel`, `time`, `nsqlite`, `npg`, `nmongo`, `nos`, `nenv`, `ncl`, `nml`, `nvis`).
- `ahiru` remains at **0.3.0**.

## 0.2.1 — 2026-07-06

### Performance
- VM, bytecode compiler, tensor runtime, and CLI startup optimizations across `niao_vm`, `niao_bytecode`, `niao_tensor`, `niao_runtime`, and `niao_cli`.

### Fixes
- Windows MSVC release builds link cleanly with `/NODEFAULTLIB:libcpmt.lib` (CRT mismatch with `libort_sys` / `libesaxx_rs`).
- Bytecode wire-format test checks magic header and roundtrip (wire container may be larger than pure JSON when OOP metadata is embedded).

### Notes
- `BYTECODE_CACHE_VERSION` remains **10** — bytecode format unchanged; existing `.niaobc` caches remain valid.
