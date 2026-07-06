// JavaScript twin of math_bench_heavy.neko (same integer semantics).
// Run:  node benchmarks/math_bench_heavy.js

const MOD = 1000000007;

function heavy(n) {
    let acc = 12345;
    let b = 777;
    let c = 0;
    let i = 0;
    while (i < n) {
        acc = (acc + i) % MOD;
        acc = (acc * 7 + 13) % MOD;
        b = (b + acc) % 998244353;
        c = c + (i % 65536);
        acc = (acc - (b % 4096) + MOD) % MOD;
        acc = Math.floor(acc / 3);
        i = i + 1;
    }
    return acc + b + (c % MOD);
}

heavy(100000); // warmup
const runs = 3;
for (let r = 0; r < runs; r++) {
    const start = performance.now();
    const result = heavy(50000000);
    const ms = performance.now() - start;
    console.log(`result=${result} time=${ms.toFixed(1)}ms`);
}
