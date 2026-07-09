use crate::device::Device;
use crate::error::TensorResult;
use crate::tensor::Tensor;

pub trait Backend {
    fn device(&self) -> Device;
    fn matmul(&self, a: &Tensor, b: &Tensor) -> TensorResult<Tensor>;
    fn add(&self, a: &Tensor, b: &Tensor) -> TensorResult<Tensor>;
    fn relu(&self, a: &Tensor) -> TensorResult<Tensor>;
}

pub struct CpuBackend;

impl Backend for CpuBackend {
    fn device(&self) -> Device {
        Device::Cpu
    }

    fn matmul(&self, a: &Tensor, b: &Tensor) -> TensorResult<Tensor> {
        a.matmul(b)
    }

    fn add(&self, a: &Tensor, b: &Tensor) -> TensorResult<Tensor> {
        a.add(b)
    }

    fn relu(&self, a: &Tensor) -> TensorResult<Tensor> {
        a.relu()
    }
}

pub fn cpu_backend() -> CpuBackend {
    CpuBackend
}
