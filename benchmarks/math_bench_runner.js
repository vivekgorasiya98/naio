const MOD = 1_000_000_007;

function heavyMath(n) {
  let acc = 12345;
  let i = 0;
  while (i < n) {
    acc = (acc + i) % MOD;
    acc = (acc - (i % 997) + MOD) % MOD;
    acc = (acc * 3) % MOD;
    acc = Math.floor(acc / 2);
    i += 1;
  }
  return acc;
}

const iterations = Number(process.argv[2] ?? 10_000_000);
const runs = Number(process.argv[3] ?? 5);
const times = [];

heavyMath(Math.min(iterations, 100_000));

let result = 0;
for (let r = 0; r < runs; r++) {
  const start = performance.now();
  result = heavyMath(iterations);
  times.push(performance.now() - start);
}

process.stderr.write(
  JSON.stringify({ count: iterations, result, times_ms: times }) + "\n"
);
