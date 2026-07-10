//! Bridge between NCL and NML tensors.

use super::handles::{alloc_handle, NmlHandle};
use crate::ncl::handles::{with_handle, NclHandle};
use crate::ValueRef;
use niao_ast::Span;
use niao_errors::codes;
use niao_tensor::{Device, Tensor};

pub fn tensor_to_float_array(t: &Tensor) -> Result<Vec<f32>, String> {
    t.to_cpu().map_err(|e| e.to_string())
}

pub fn from_ncl_handle(ncl_id: u64, span: Span) -> Result<u64, crate::RuntimeError> {
    with_handle(ncl_id, "nml_from_ncl", span, |h| {
        match h {
            NclHandle::NDArray(arr) => {
                let data: Vec<f32> = arr
                    .data_float
                    .clone()
                    .ok_or_else(|| "nml_from_ncl requires float ndarray".to_string())?
                    .into_iter()
                    .map(|v| v as f32)
                    .collect();
                let t = Tensor::from_cpu_data(&arr.shape, data, Device::Cpu)
                    .map_err(|e| e.to_string())?;
                Ok(alloc_handle(NmlHandle::Tensor(t)))
            }
            _ => Err("nml_from_ncl expects NCL NDArray handle".into()),
        }
    })
}

pub fn from_packed_float_array(args: &[ValueRef], span: Span) -> Result<Tensor, crate::RuntimeError> {
    let arr = super::common::float_array_arg(args, 0, "nml_tensor", span)?;
    let shape = super::common::int_array_arg(args, 1, "nml_tensor", span)?;
    let shape: Vec<usize> = shape.iter().map(|&n| n as usize).collect();
    let data: Vec<f32> = arr.iter().map(|&x| x as f32).collect();
    Tensor::from_cpu_data(&shape, data, niao_tensor::global_device()).map_err(|e| {
        crate::RuntimeError::at(span, codes::E1973_NML_SHAPE, e.to_string())
    })
}
