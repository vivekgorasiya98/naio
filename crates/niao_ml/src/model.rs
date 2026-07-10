//! Sequential model container.

use crate::backward::ParamGrad;
use crate::layer::Layer;
use niao_tensor::{Device, Tensor, TensorResult};

#[derive(Debug, Clone)]
pub struct Sequential {
    pub layers: Vec<Layer>,
    pub device: Device,
}

impl Sequential {
    pub fn new(layers: Vec<Layer>, device: Device) -> Self {
        Self { layers, device }
    }

    pub fn forward(&mut self, input: &Tensor) -> TensorResult<Tensor> {
        let mut x = input.clone();
        for layer in &mut self.layers {
            x = layer.forward(&x)?;
        }
        Ok(x)
    }

    pub fn backward(&self, grad_output: Vec<f32>) -> TensorResult<Vec<ParamGrad>> {
        let mut grad = grad_output;
        let mut param_grads = Vec::new();
        for layer in self.layers.iter().rev() {
            let (grad_in, pg) = layer.backward(&grad)?;
            grad = grad_in;
            param_grads.push(pg);
        }
        param_grads.reverse();
        Ok(param_grads)
    }

    pub fn zero_grad(&mut self) {
        for layer in &mut self.layers {
            layer.cache = None;
        }
    }

    pub fn parameters(&self) -> Vec<&Tensor> {
        self.layers.iter().flat_map(|l| l.parameters()).collect()
    }

    pub fn train(&mut self) {
        for l in &mut self.layers {
            l.training = true;
        }
    }

    pub fn eval(&mut self) {
        for l in &mut self.layers {
            l.training = false;
        }
    }
}
