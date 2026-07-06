#!/usr/bin/env python3
"""Cross-language MongoDB benchmark: Neko vs Python vs Node."""

from __future__ import annotations

import argparse
import os
import platform
import shutil
import subprocess
import sys
import time
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parent
NEKO_BENCH = ROOT / "nmongo_bench.neko"
MONGO_URI = os.environ.get("NEKO_MONGO_URL", os.environ.get("MONGO_URL", "mongodb://localhost:27017"))
DB = "neko_bench"

BENCHMARKS = [
    ("insert_one_loop", [1_000, 10_000]),
    ("insert_many_single", [1_000, 10_000]),
    ("insert_many_chunks", [1_000, 10_000]),
    ("find_all", [1_000, 10_000]),
    ("find_filtered", [1_000, 10_000]),
    ("count", [1_000, 10_000]),
    ("update_many", [1_000, 10_000]),
    ("delete_many", [1_000, 10_000]),
    ("bulk_write", [1_000, 10_000]),
    ("aggregate", [1_000, 10_000]),
    ("concurrent_reads", [1_000, 10_000]),
]


@dataclass
class BenchResult:
    name: str
    times_ms: list[float] = field(default_factory=list)

    @property
    def best_ms(self) -> float:
        return min(self.times_ms) if self.times_ms else float("inf")


def find_neko() -> str:
    for candidate in (
        shutil.which("neko"),
        Path(os.environ.get("USERPROFILE", "")) / ".cargo" / "bin" / "neko.exe",
        ROOT.parent / "target" / "release" / "neko.exe",
        ROOT.parent / "target" / "debug" / "neko.exe",
    ):
        if candidate and Path(candidate).is_file():
            return str(candidate)
    raise FileNotFoundError("neko not found")


def mongo_available() -> bool:
    try:
        from pymongo import MongoClient

        client = MongoClient(MONGO_URI, serverSelectionTimeoutMS=2000)
        client.admin.command("ping")
        client.close()
        return True
    except Exception:
        return False


def time_neko(neko: str, bench: str, n: int, runs: int) -> BenchResult:
    result = BenchResult(f"neko:{bench}:{n}")
    cmd = [neko, "run", "--mode", "vm", str(NEKO_BENCH), bench, str(n)]
    subprocess.run(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False)
    for _ in range(runs):
        start = time.perf_counter()
        proc = subprocess.run(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        elapsed = (time.perf_counter() - start) * 1000
        if proc.returncode == 0:
            result.times_ms.append(elapsed)
    return result


def time_python(bench: str, n: int, runs: int) -> BenchResult:
    import importlib.util

    spec = importlib.util.spec_from_file_location("nmongo_bench_py", ROOT / "nmongo_bench_py.py")
    mod = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(mod)

    result = BenchResult(f"python:{bench}:{n}")
    for _ in range(runs):
        start = time.perf_counter()
        mod.run_bench(MONGO_URI, DB, bench, n)
        result.times_ms.append((time.perf_counter() - start) * 1000)
    return result


def time_node(bench: str, n: int, runs: int) -> BenchResult:
    node = shutil.which("node")
    if not node:
        raise FileNotFoundError("node not found")
    script = ROOT / "nmongo_bench.js"
    result = BenchResult(f"node:{bench}:{n}")
    cmd = [node, str(script), MONGO_URI, DB, bench, str(n)]
    for _ in range(runs):
        start = time.perf_counter()
        proc = subprocess.run(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        elapsed = (time.perf_counter() - start) * 1000
        if proc.returncode == 0:
            result.times_ms.append(elapsed)
    return result


def neko_version(neko: str) -> str:
    try:
        out = subprocess.check_output([neko, "--version"], text=True).strip()
        return out
    except Exception:
        return "unknown"


def main() -> int:
    parser = argparse.ArgumentParser(description="MongoDB cross-language benchmark")
    parser.add_argument("--runs", type=int, default=1)
    parser.add_argument("--quick", action="store_true", help="smoke: one small cell per bench")
    args = parser.parse_args()

    if not mongo_available():
        print("MongoDB not available at", MONGO_URI)
        return 1

    neko = find_neko()
    runs = 1 if args.quick else args.runs
    benches = BENCHMARKS
    if args.quick:
        benches = [(name, [1_000]) for name, _ in BENCHMARKS]

    print(f"Neko: {neko_version(neko)} (mode=vm)")
    print(f"MongoDB URI: {MONGO_URI}")

    rows: list[tuple[str, int, float, float, float, str]] = []
    totals = {"Neko": 0.0, "Python": 0.0, "Node": 0.0}
    wins = {"Neko": 0, "Python": 0, "Node": 0}

    for bench, sizes in benches:
        for n in sizes:
            nk = time_neko(neko, bench, n, runs).best_ms
            py = time_python(bench, n, runs).best_ms
            nd = time_node(bench, n, runs).best_ms
            totals["Neko"] += nk
            totals["Python"] += py
            totals["Node"] += nd
            best = min(nk, py, nd)
            winner = "Neko" if best == nk else "Python" if best == py else "Node"
            wins[winner] += 1
            rows.append((bench, n, nk, py, nd, winner))

    overall = min(totals, key=totals.get)
    print()
    print(f"Overall winner: {overall} ({totals[overall]:,.1f} ms total)")
    print(f"Wins: Neko={wins['Neko']} Python={wins['Python']} Node={wins['Node']}")
    print()
    print(f"{'Benchmark':<24} {'n':>8} {'Neko':>10} {'Python':>10} {'Node':>10}  Winner")
    for bench, n, nk, py, nd, winner in rows:
        print(f"{bench:<24} {n:>8,} {nk:>10.2f} {py:>10.2f} {nd:>10.2f}  {winner}")

    return 0


if __name__ == "__main__":
    sys.exit(main())
