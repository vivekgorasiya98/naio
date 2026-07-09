//! Niao tensor engine — contiguous f32 storage, CPU SIMD, optional CUDA.

pub mod backend;
pub mod cpu;
pub mod device;
pub mod dtype;
pub mod error;
pub mod pool;
pub mod shape;
pub mod tensor;

#[cfg(feature = "cuda")]
pub mod cuda;

#[cfg(feature = "wgpu")]
pub mod wgpu;

pub use backend::Backend;
pub use device::{cuda_device_count, global_device, set_global_device, Device};
pub use dtype::DType;
pub use error::{TensorError, TensorResult};
pub use pool::BufferPool;
pub use tensor::Tensor;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matmul_2x2() {
        let a = Tensor::from_float_slice(&[2, 2], &[1.0, 2.0, 3.0, 4.0]).unwrap();
        let b = Tensor::from_float_slice(&[2, 2], &[5.0, 6.0, 7.0, 8.0]).unwrap();
        let c = a.matmul(&b).unwrap();
        let data = c.to_cpu().unwrap();
        assert!((data[0] - 19.0).abs() < 1e-5);
        assert!((data[3] - 50.0).abs() < 1e-5);
    }

    #[test]
    fn zeros_and_shape() {
        let t = Tensor::zeros(&[2, 3], Device::Cpu).unwrap();
        assert_eq!(t.shape, vec![2, 3]);
        assert_eq!(t.len(), 6);
    }
}
