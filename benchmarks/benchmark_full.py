"""Niao vs Python vs Node: time + peak RSS memory + niao test suite."""
import json
import os
import re
import shutil
import subprocess
import sys
import threading
import time
from dataclasses import dataclass, field
from pathlib import Path

import psutil

ITERATIONS = 10_000_000
MOD = 1_000_000_007
EXPECTED = 31_101_423
RUNS = 5
ROOT = Path(__file__).resolve().parent
REPO = ROOT.parent
NIAO_FILE = ROOT / "math_bench.niao"
NIAO_TIME_RE = re.compile(
    r"(?:run: ([\d.]+) ms(?: \(compile: [\d.]+ ms\))?|finished in ([\d.]+) ms)"
)


@dataclass
class BenchResult:
    name: str
    times_ms: list[float] = field(default_factory=list)
    peak_rss_mb: list[float] = field(default_factory=list)
    idle_rss_mb: float = 0.0

    @property
    def best_ms(self) -> float:
        return min(self.times_ms)

    @property
    def avg_ms(self) -> float:
        return sum(self.times_ms) / len(self.times_ms)

    @property
    def peak_mb(self) -> float:
        return max(self.peak_rss_mb) if self.peak_rss_mb else 0.0

    @property
    def avg_peak_mb(self) -> float:
        return sum(self.peak_rss_mb) / len(self.peak_rss_mb) if self.peak_rss_mb else 0.0


def find_niao() -> str:
    for candidate in (
        REPO / "target" / "release" / "niao.exe",
        Path(os.environ.get("USERPROFILE", "")) / ".cargo" / "bin" / "niao.exe",
        shutil.which("niao"),
    ):
        if candidate and Path(candidate).is_file():
            return str(candidate)
    raise FileNotFoundError("niao not found")


def find_node() -> str:
    node = shutil.which("node")
    if not node:
        raise FileNotFoundError("node not found")
    return node


def parse_niao_time(stderr: str) -> float:
    m = NIAO_TIME_RE.search(stderr)
    if not m:
        raise RuntimeError(f"could not parse niao timing: {stderr!r}")
    return float(m.group(1) or m.group(2))


def monitor_peak(proc: subprocess.Popen, peak: list[float]) -> None:
    try:
        p = psutil.Process(proc.pid)
        while proc.poll() is None:
            try:
                peak[0] = max(peak[0], p.memory_info().rss)
            except (psutil.NoSuchProcess, psutil.AccessDenied):
                break
            time.sleep(0.003)
        try:
            peak[0] = max(peak[0], p.memory_info().rss)
        except (psutil.NoSuchProcess, psutil.AccessDenied):
            pass
    except (psutil.NoSuchProcess, psutil.AccessDenied):
        pass


def run_peak(cmd: list[str], **kw) -> tuple[subprocess.CompletedProcess, float]:
    peak = [0.0]
    proc = subprocess.Popen(cmd, **kw)
    t = threading.Thread(target=monitor_peak, args=(proc, peak), daemon=True)
    t.start()
    out, err = proc.communicate()
    t.join()
    cp = subprocess.CompletedProcess(cmd, proc.returncode, out, err)
    if proc.returncode:
        raise subprocess.CalledProcessError(proc.returncode, cmd, out, err)
    return cp, peak[0] / (1024 * 1024)


def node_idle_mb(node: str) -> float:
    p = subprocess.Popen(
        [node, "-e", "setInterval(()=>{},60000)"],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    time.sleep(0.12)
    rss = psutil.Process(p.pid).memory_info().rss / (1024 * 1024)
    p.kill()
    p.wait()
    return rss


def heavy_math(n: int) -> int:
    acc = 12_345
    i = 0
    while i < n:
        acc = (acc + i) % MOD
        acc = (acc - (i % 997)) % MOD
        acc = (acc * 3) % MOD
        acc = acc // 2
        i += 1
    return acc


def bench_python(iterations: int, runs: int) -> BenchResult:
    heavy_math(min(iterations, 100_000))
    r = BenchResult("Python 3")
    r.idle_rss_mb = psutil.Process().memory_info().rss / (1024 * 1024)
    for _ in range(runs):
        t0 = time.perf_counter()
        heavy_math(iterations)
        r.times_ms.append((time.perf_counter() - t0) * 1000)
    r.peak_rss_mb.append(psutil.Process().memory_info().rss / (1024 * 1024))
    return r


def bench_node(node: str, iterations: int, runs: int) -> BenchResult:
    script = str(ROOT / "math_bench_runner.js")
    subprocess.run(
        [node, script, str(iterations), "1"],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        check=True,
    )
    r = BenchResult("JavaScript (Node)")
    r.idle_rss_mb = node_idle_mb(node)
    for _ in range(runs):
        cp, rss = run_peak(
            [node, script, str(iterations), "1"],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
            text=True,
        )
        data = json.loads(cp.stderr.strip())
        r.times_ms.append(data["times_ms"][0])
        r.peak_rss_mb.append(rss)
    return r


def bench_niao(niao: str, iterations: int, runs: int) -> BenchResult:
    subprocess.run(
        [niao, "run", "--time", str(NIAO_FILE)],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        check=True,
    )
    r = BenchResult("Niao 0.2.2 (VM)")
    cp_idle, rss_idle = run_peak(
        [niao, "run", str(ROOT / "math_bench_verify.niao")],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE,
        text=True,
    )
    r.idle_rss_mb = rss_idle
    for _ in range(runs):
        cp, rss = run_peak(
            [niao, "run", "--time", str(NIAO_FILE)],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
            text=True,
        )
        r.times_ms.append(parse_niao_time(cp.stderr))
        r.peak_rss_mb.append(rss)
    return r


def run_niao_tests(niao: str) -> tuple[int, int, list[str]]:
    cp = subprocess.run(
        [niao, "test"],
        cwd=str(REPO),
        capture_output=True,
        text=True,
    )
    lines = (cp.stdout + cp.stderr).splitlines()
    passed = failed = 0
    details: list[str] = []
    for line in lines:
        if " passed, " in line and " failed" in line:
            parts = line.strip().split()
            passed = int(parts[0])
            failed = int(parts[2])
        if line.startswith("test "):
            details.append(line.strip())
    return passed, failed, details


def fibonacci_bench(niao: str) -> str:
    cp = subprocess.run(
        [niao, "bench", str(REPO / "examples" / "fibonacci.niao")],
        capture_output=True,
        text=True,
    )
    return (cp.stdout + cp.stderr).strip()


def print_report(results: list[BenchResult], runs: int, iterations: int) -> None:
    fastest = min(r.best_ms for r in results)
    total_ops = iterations * 4

    print("=" * 72)
    print("ARITHMETIC BENCHMARK (10M iterations, mod 1_000_000_007)")
    print("=" * 72)
    print(f"Expected result: {EXPECTED}")
    print(f"Runs per language: {runs}")
    print()

    hdr = f"{'Language':<22} {'Best':>9} {'Avg':>9} {'Peak RSS':>11} {'Idle RSS':>11} {'Ratio':>8}"
    print(hdr)
    print("-" * len(hdr))
    for r in sorted(results, key=lambda x: x.best_ms):
        ratio = r.best_ms / fastest
        print(
            f"{r.name:<22} {r.best_ms:8.1f}ms {r.avg_ms:8.1f}ms "
            f"{r.peak_mb:9.1f} MB {r.idle_rss_mb:9.1f} MB {ratio:7.2f}x"
        )

    print()
    print("Per-run times (ms):")
    for r in results:
        times = "  ".join(f"{t:8.1f}" for t in r.times_ms)
        peaks = "  ".join(f"{p:7.1f}MB" for p in r.peak_rss_mb)
        print(f"  {r.name}")
        print(f"    time: {times}")
        print(f"    peak: {peaks}")

    print()
    print("Throughput (best run):")
    for r in sorted(results, key=lambda x: x.best_ms):
        ops = total_ops / (r.best_ms / 1000)
        print(f"  {r.name:<22} {ops:,.0f} ops/s")


def main():
    iterations = int(sys.argv[1]) if len(sys.argv) > 1 else ITERATIONS
    runs = int(sys.argv[2]) if len(sys.argv) > 2 else RUNS
    niao = find_niao()
    node = find_node()

    ver = subprocess.run([niao, "--version"], capture_output=True, text=True, check=True)
    py_ver = sys.version.split()[0]
    node_ver = subprocess.run([node, "--version"], capture_output=True, text=True).stdout.strip()

    print("ENVIRONMENT")
    print(f"  Niao:   {ver.stdout.strip()} ({niao})")
    print(f"  Python: {py_ver}")
    print(f"  Node:   {node_ver}")
    print(f"  OS:     {sys.platform}")
    print()

    # Correctness
    print("CORRECTNESS CHECK")
    assert heavy_math(iterations) == EXPECTED
    subprocess.run([niao, "run", str(ROOT / "math_bench_verify.niao")], check=True, capture_output=True)
    print(f"  All implementations return {EXPECTED}")
    print()

    # Benchmark
    print(f"Running {runs} timed iterations each (warmup included)...")
    results = [
        bench_python(iterations, runs),
        bench_node(node, iterations, runs),
        bench_niao(niao, iterations, runs),
    ]
    print_report(results, runs, iterations)

    # Fibonacci
    print()
    print("=" * 72)
    print("NIAO FIBONACCI BENCH (fib(40), recursion)")
    print("=" * 72)
    print(f"  {fibonacci_bench(niao)}")
    print()

    # Tests
    print("=" * 72)
    print("NIAO TEST SUITE")
    print("=" * 72)
    passed, failed, details = run_niao_tests(niao)
    for line in details:
        print(f"  {line}")
    print()
    print(f"  TOTAL: {passed} passed, {failed} failed")
    if failed:
        sys.exit(1)


if __name__ == "__main__":
    main()
