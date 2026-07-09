//! wgpu backend stub — uses CPU fallback wrapped as Wgpu device label.

use crate::device::Device;
use crate::dtype::DType;
use crate::error::TensorResult;
use crate::shape::row_major_strides;
use crate::tensor::{Tensor, TensorStorage};

pub fn to_wgpu_tensor(shape: &[usize], data: Vec<f32>) -> TensorResult<Tensor> {
    // v1: wgpu path stores on CPU with Wgpu device tag; future: candle wgpu backend
    Ok(Tensor {
        storage: TensorStorage::Cpu(data),
        shape: shape.to_vec(),
        strides: row_major_strides(shape),
        dtype: DType::F32,
        device: Device::Wgpu,
        requires_grad: false,
        grad: None,
    })
}
