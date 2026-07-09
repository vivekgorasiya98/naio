//! CUDA backend via candle-core (optional feature).

use crate::device::Device;
use crate::error::{TensorError, TensorResult};
use crate::shape::row_major_strides;
use crate::tensor::{Tensor, TensorStorage};
use crate::dtype::DType;
use candle_core::{Device as CandleDevice, Tensor as CandleTensor};

#[derive(Debug, Clone)]
pub struct CudaTensor {
    pub inner: CandleTensor,
    pub device_idx: usize,
}

impl CudaTensor {
    pub fn from_cpu(data: &[f32], shape: &[usize], device_idx: usize) -> TensorResult<Self> {
        let dev = candle_device(device_idx)?;
        let t = CandleTensor::from_slice(data, shape, &dev)
            .map_err(|e| TensorError::Device(e.to_string()))?;
        Ok(Self {
            inner: t,
            device_idx,
        })
    }

    pub fn to_cpu(&self) -> TensorResult<Vec<f32>> {
        let flat = self
            .inner
            .flatten_all()
            .map_err(|e| TensorError::Device(e.to_string()))?;
        let v: Vec<f32> = flat
            .to_vec1()
            .map_err(|e| TensorError::Device(e.to_string()))?;
        Ok(v)
    }
}

pub fn device_count() -> usize {
    // candle does not expose device count directly; assume 1 if cuda builds
    1
}

fn candle_device(idx: usize) -> TensorResult<CandleDevice> {
    CandleDevice::new_cuda(idx).map_err(|e| TensorError::Device(e.to_string()))
}

pub fn matmul(
    a: &CudaTensor,
    b: &CudaTensor,
    m: usize,
    n: usize,
    k: usize,
) -> TensorResult<Tensor> {
    let out = a
        .inner
        .matmul(&b.inner)
        .map_err(|e| TensorError::Op(e.to_string()))?;
    Ok(Tensor {
        storage: TensorStorage::Cuda(CudaTensor {
            inner: out,
            device_idx: a.device_idx,
        }),
        shape: vec![m, k],
        strides: row_major_strides(&[m, k]),
        dtype: DType::F32,
        device: Device::Cuda(a.device_idx),
        requires_grad: false,
        grad: None,
    })
}

pub fn relu(t: &CudaTensor) -> TensorResult<CudaTensor> {
    let out = t
        .inner
        .relu()
        .map_err(|e| TensorError::Op(e.to_string()))?;
    Ok(CudaTensor {
        inner: out,
        device_idx: t.device_idx,
    })
}
