use criterion::{black_box, criterion_group, criterion_main, Criterion};
use neko_tensor::{Device, Tensor};

fn bench_matmul(c: &mut Criterion) {
    let a = Tensor::randn(&[512, 512], Device::Cpu).unwrap();
    let b = Tensor::randn(&[512, 512], Device::Cpu).unwrap();
    c.bench_function("matmul_512", |bencher| {
        bencher.iter(|| black_box(a.matmul(&b).unwrap()));
    });
}

criterion_group!(benches, bench_matmul);
criterion_main!(benches);
