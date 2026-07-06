use crate::cpu;
use crate::device::Device;
use crate::dtype::DType;
use crate::error::{TensorError, TensorResult};
use crate::shape::{numel, row_major_strides, validate_shape};
use rand::Rng;
use rand_distr::{Distribution, Normal};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub enum TensorStorage {
    Cpu(Vec<f32>),
    #[cfg(feature = "cuda")]
    Cuda(crate::cuda::CudaTensor),
}

#[derive(Debug, Clone)]
pub struct Tensor {
    pub storage: TensorStorage,
    pub shape: Vec<usize>,
    pub strides: Vec<usize>,
    pub dtype: DType,
    pub device: Device,
    pub requires_grad: bool,
    pub grad: Option<Arc<std::sync::Mutex<Vec<f32>>>>,
}

impl Tensor {
    pub fn zeros(shape: &[usize], device: Device) -> TensorResult<Self> {
        validate_shape(shape)?;
        let n = numel(shape);
        let data = vec![0.0f32; n];
        Self::from_cpu_data(shape, data, device)
    }

    pub fn ones(shape: &[usize], device: Device) -> TensorResult<Self> {
        validate_shape(shape)?;
        let n = numel(shape);
        let data = vec![1.0f32; n];
        Self::from_cpu_data(shape, data, device)
    }

    pub fn randn(shape: &[usize], device: Device) -> TensorResult<Self> {
        validate_shape(shape)?;
        let n = numel(shape);
        let mut rng = rand::thread_rng();
        let normal = Normal::new(0.0, 1.0).map_err(|e| TensorError::Op(e.to_string()))?;
        let data: Vec<f32> = (0..n).map(|_| normal.sample(&mut rng) as f32).collect();
        Self::from_cpu_data(shape, data, device)
    }

    pub fn from_cpu_data(shape: &[usize], data: Vec<f32>, device: Device) -> TensorResult<Self> {
        validate_shape(shape)?;
        let n = numel(shape);
        if data.len() != n {
            return Err(TensorError::Shape(format!(
                "data length {} != shape product {n}",
                data.len()
            )));
        }
        match device {
            Device::Cpu => Ok(Self {
                storage: TensorStorage::Cpu(data),
                shape: shape.to_vec(),
                strides: row_major_strides(shape),
                dtype: DType::F32,
                device,
                requires_grad: false,
                grad: None,
            }),
            Device::Cuda(idx) => {
                #[cfg(feature = "cuda")]
                {
                    let cuda = crate::cuda::CudaTensor::from_cpu(&data, shape, idx)?;
                    Ok(Self {
                        storage: TensorStorage::Cuda(cuda),
                        shape: shape.to_vec(),
                        strides: row_major_strides(shape),
                        dtype: DType::F32,
                        device,
                        requires_grad: false,
                        grad: None,
                    })
                }
                #[cfg(not(feature = "cuda"))]
                {
                    let _ = idx;
                    Err(TensorError::Device(
                        "CUDA not enabled; rebuild with --features cuda".into(),
                    ))
                }
            }
            Device::Wgpu => {
                #[cfg(feature = "wgpu")]
                {
                    crate::wgpu::to_wgpu_tensor(shape, data)
                }
                #[cfg(not(feature = "wgpu"))]
                {
                    Err(TensorError::Device(
                        "wgpu not enabled; rebuild with --features wgpu".into(),
                    ))
                }
            }
        }
    }

    pub fn from_float_slice(shape: &[usize], data: &[f32]) -> TensorResult<Self> {
        Self::from_cpu_data(shape, data.to_vec(), Device::Cpu)
    }

    pub fn to_cpu(&self) -> TensorResult<Vec<f32>> {
        match &self.storage {
            TensorStorage::Cpu(v) => Ok(v.clone()),
            #[cfg(feature = "cuda")]
            TensorStorage::Cuda(c) => c.to_cpu(),
        }
    }

    pub fn cpu_data(&self) -> TensorResult<&[f32]> {
        match &self.storage {
            TensorStorage::Cpu(v) => Ok(v),
            #[cfg(feature = "cuda")]
            TensorStorage::Cuda(_) => Err(TensorError::Device(
                "tensor on CUDA; call to_cpu() first".into(),
            )),
        }
    }

    pub fn len(&self) -> usize {
        numel(&self.shape)
    }

    pub fn enable_grad(&mut self) {
        self.requires_grad = true;
        if self.grad.is_none() {
            self.grad = Some(Arc::new(std::sync::Mutex::new(vec![0.0; self.len()])));
        }
    }

    pub fn zero_grad(&mut self) {
        if let Some(g) = &self.grad {
            g.lock().unwrap().fill(0.0);
        }
    }

    pub fn reshape(&self, new_shape: &[usize]) -> TensorResult<Self> {
        let new_shape = crate::shape::reshape(&self.shape, new_shape)?;
        let data = self.to_cpu()?;
        Self::from_cpu_data(&new_shape, data, self.device)
    }

    pub fn to_device(&self, device: Device) -> TensorResult<Self> {
        if self.device == device {
            return Ok(self.clone());
        }
        let data = self.to_cpu()?;
        Self::from_cpu_data(&self.shape, data, device)
    }

    pub fn add(&self, other: &Self) -> TensorResult<Self> {
        if self.shape != other.shape {
            return Err(TensorError::Shape("add: shape mismatch".into()));
        }
        let a = self.to_cpu()?;
        let b = other.to_cpu()?;
        let mut out = vec![0.0; a.len()];
        cpu::add_f32(&a, &b, &mut out);
        Self::from_cpu_data(&self.shape, out, self.device)
    }

    pub fn sub(&self, other: &Self) -> TensorResult<Self> {
        if self.shape != other.shape {
            return Err(TensorError::Shape("sub: shape mismatch".into()));
        }
        let a = self.to_cpu()?;
        let b = other.to_cpu()?;
        let mut out = vec![0.0; a.len()];
        cpu::sub_f32(&a, &b, &mut out);
        Self::from_cpu_data(&self.shape, out, self.device)
    }

    pub fn mul(&self, other: &Self) -> TensorResult<Self> {
        if self.shape != other.shape {
            return Err(TensorError::Shape("mul: shape mismatch".into()));
        }
        let a = self.to_cpu()?;
        let b = other.to_cpu()?;
        let mut out = vec![0.0; a.len()];
        cpu::mul_f32(&a, &b, &mut out);
        Self::from_cpu_data(&self.shape, out, self.device)
    }

    pub fn relu(&self) -> TensorResult<Self> {
        let a = self.to_cpu()?;
        let mut out = vec![0.0; a.len()];
        cpu::relu_f32(&a, &mut out);
        Self::from_cpu_data(&self.shape, out, self.device)
    }

    pub fn matmul(&self, other: &Self) -> TensorResult<Self> {
        if self.shape.len() != 2 || other.shape.len() != 2 {
            return Err(TensorError::Shape("matmul requires 2D tensors".into()));
        }
        let (m, n) = (self.shape[0], self.shape[1]);
        let (n2, k) = (other.shape[0], other.shape[1]);
        if n != n2 {
            return Err(TensorError::Shape("matmul inner dim mismatch".into()));
        }
        #[cfg(feature = "cuda")]
        if let (TensorStorage::Cuda(a), TensorStorage::Cuda(b)) = (&self.storage, &other.storage) {
            return crate::cuda::matmul(a, b, m, n, k);
        }
        let a = self.to_cpu()?;
        let b = other.to_cpu()?;
        let out = cpu::matmul_f32(&a, &b, m, n, k);
        Self::from_cpu_data(&[m, k], out, self.device)
    }

    pub fn softmax(&self, dim: usize) -> TensorResult<Self> {
        if self.shape.len() != 2 || dim != 1 {
            return Err(TensorError::Op("softmax supports 2D tensors along dim 1".into()));
        }
        let rows = self.shape[0];
        let cols = self.shape[1];
        let a = self.to_cpu()?;
        let mut out = vec![0.0; a.len()];
        cpu::softmax_2d(&a, rows, cols, &mut out);
        Self::from_cpu_data(&self.shape, out, self.device)
    }
}
