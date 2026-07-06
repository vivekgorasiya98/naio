import json
import os
import re
import shutil
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path

ITERATIONS = 10_000_000
MOD = 1_000_000_007
EXPECTED = 31_101_423
RUNS = 5
ROOT = Path(__file__).resolve().parent
NEKO_FILE = ROOT / "math_bench.neko"
NEKO_TIME_RE = re.compile(
    r"(?:run: ([\d.]+) ms(?: \(compile: [\d.]+ ms\))?|finished in ([\d.]+) ms)"
)


def parse_neko_time(stderr: str) -> float:
    match = NEKO_TIME_RE.search(stderr)
    if not match:
        raise RuntimeError(f"could not parse neko timing: {stderr!r}")
    return float(match.group(1) or match.group(2))


@dataclass
class BenchResult:
    name: str
    times_ms: list[float]

    @property
    def best_ms(self) -> float:
        return min(self.times_ms)

    @property
    def avg_ms(self) -> float:
        return sum(self.times_ms) / len(self.times_ms)

    @property
    def worst_ms(self) -> float:
        return max(self.times_ms)


def find_neko() -> str:
    for candidate in (
        shutil.which("neko"),
        Path(os.environ.get("USERPROFILE", "")) / ".cargo" / "bin" / "neko.exe",
        Path(os.environ.get("USERPROFILE", "")) / ".cargo" / "bin" / "neko",
        ROOT.parent / "target" / "release" / "neko.exe",
    ):
        if candidate and Path(candidate).is_file():
            return str(candidate)
    raise FileNotFoundError("neko not found (install or add ~/.cargo/bin to PATH)")


def find_node() -> str:
    node = shutil.which("node")
    if not node:
        raise FileNotFoundError("node not found on PATH")
    return node


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


def verify_python(n: int = ITERATIONS) -> None:
    if heavy_math(n) != EXPECTED:
        raise ValueError(f"Python math checksum mismatch")


def verify_neko(neko: str) -> None:
    proc = subprocess.run(
        [neko, str(ROOT / "math_bench_verify.neko")],
        capture_output=True,
        text=True,
        check=True,
    )
    if int(proc.stdout.strip()) != EXPECTED:
        raise ValueError(f"Neko math checksum mismatch: {proc.stdout.strip()!r}")


def verify_javascript(node: str) -> None:
    script = f"""
const MOD = {MOD};
function heavyMath(n) {{
  let acc = 12345;
  let i = 0;
  while (i < n) {{
    acc = (acc + i) % MOD;
    acc = (acc - (i % 997) + MOD) % MOD;
    acc = (acc * 3) % MOD;
    acc = Math.floor(acc / 2);
    i = i + 1;
  }}
  return acc;
}}
console.log(heavyMath({ITERATIONS}));
"""
    proc = subprocess.run([node, "-e", script], capture_output=True, text=True, check=True)
    if int(proc.stdout.strip()) != EXPECTED:
        raise ValueError(f"JavaScript math checksum mismatch: {proc.stdout.strip()!r}")


def time_python(iterations: int, runs: int) -> BenchResult:
    heavy_math(min(iterations, 100_000))  # warmup
    times: list[float] = []
    for _ in range(runs):
        start = time.perf_counter()
        heavy_math(iterations)
        times.append((time.perf_counter() - start) * 1000)
    return BenchResult("Python", times)


def time_javascript(node: str, iterations: int, runs: int) -> BenchResult:
    script = ROOT / "math_bench_runner.js"
    proc = subprocess.run(
        [node, str(script), str(iterations), str(runs)],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE,
        text=True,
        check=True,
    )
    data = json.loads(proc.stderr.strip())
    if data["count"] != iterations:
        raise ValueError(f"JavaScript iteration mismatch: {data['count']}")
    if data["result"] != EXPECTED:
        raise ValueError(f"JavaScript result mismatch: {data['result']}")
    return BenchResult("JavaScript (Node)", data["times_ms"])


def time_neko(neko: str, iterations: int, runs: int) -> BenchResult:
    if not NEKO_FILE.is_file():
        raise FileNotFoundError(f"missing {NEKO_FILE}")

    subprocess.run(
        [neko, str(NEKO_FILE), "time"],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        check=True,
    )  # warmup + bytecode cache

    times: list[float] = []
    for _ in range(runs):
        proc = subprocess.run(
            [neko, str(NEKO_FILE), "time"],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
            text=True,
            check=True,
        )
        times.append(parse_neko_time(proc.stderr))

    return BenchResult("Neko", times)


def print_report(results: list[BenchResult], runs: int, iterations: int) -> None:
    fastest = min(r.best_ms for r in results)
    ops_per_iter = 4
    total_ops = iterations * ops_per_iter

    print(f"Arithmetic stress test: {iterations:,} iterations")
    print(f"Per iteration: +, -, *, / (integer mod {MOD:,})")
    print(f"Total ops per run: {total_ops:,} ({ops_per_iter} per iteration)")
    print(f"Expected result: {EXPECTED}")
    print(f"Runs per language: {runs} timed (plus 1 warmup each)")
    print()

    header = f"{'Language':<22}"
    for i in range(1, runs + 1):
        header += f" {'Run ' + str(i):>10}"
    header += f" {'Best':>10} {'Avg':>10} {'Worst':>10} {'Ratio':>9}"
    print(header)
    print("-" * len(header))

    for r in results:
        row = f"{r.name:<22}"
        for t in r.times_ms:
            row += f" {t:9.1f}ms"
        ratio = r.best_ms / fastest if fastest else 1.0
        ratio_label = "1.00x" if ratio <= 1.001 else f"{ratio:.1f}x"
        row += f" {r.best_ms:9.1f}ms {r.avg_ms:9.1f}ms {r.worst_ms:9.1f}ms {ratio_label:>9}"
        print(row)

    print()
    ranked = sorted(results, key=lambda r: r.best_ms)
    print("Ranking (best of 5):")
    for i, r in enumerate(ranked, 1):
        ops_per_sec = total_ops / (r.best_ms / 1000)
        slower = ""
        if i > 1:
            slower = f"  ({r.best_ms / fastest:.1f}x slower)"
        print(f"  {i}. {r.name:<22} {r.best_ms:,.1f} ms  ({ops_per_sec:,.0f} ops/s){slower}")


def main():
    iterations = int(sys.argv[1]) if len(sys.argv) > 1 else ITERATIONS
    runs = int(sys.argv[2]) if len(sys.argv) > 2 else RUNS

    neko = find_neko()
    node = find_node()

    print("Verifying arithmetic loop correctness...")
    verify_python(iterations)
    verify_neko(neko)
    verify_javascript(node)
    print(f"All implementations return {EXPECTED}")
    print()
    print(f"Warming up and benchmarking {iterations:,} iterations...")
    print("(this may take a while)")
    print()

    results = [
        time_python(iterations, runs),
        time_javascript(node, iterations, runs),
        time_neko(neko, iterations, runs),
    ]
    print_report(results, runs, iterations)


if __name__ == "__main__":
    main()
