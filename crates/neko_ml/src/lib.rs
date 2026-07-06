//! Deep learning for NML: autograd, layers, training, tuning.

pub mod autograd;
pub mod backward;
pub mod checkpoint;
pub mod dataloader;
pub mod columnar;
pub mod explain;
pub mod gnn;
pub mod layer;
pub mod loss;
pub mod model;
pub mod optimizer;
pub mod trainer;
pub mod tuning;

pub use autograd::{backward, Graph, VarId};
pub use backward::{clip_grads, ParamGrad};
pub use checkpoint::{load_model, save_model};
pub use dataloader::DataLoader;
pub use explain::feature_importance;
pub use gnn::{GcnLayer, GnnModel, GraphSageLayer};
pub use layer::{Layer, LayerCache, LayerKind};
pub use loss::{LossKind, cross_entropy_grad, bce_grad, loss_grad};
pub use model::Sequential;
pub use optimizer::OptimizerKind;
pub use trainer::{Trainer, TrainMetrics, ValMetrics};
pub use tuning::{EarlyStopping, GridSearch, RandomSearch, SearchResult};

#[cfg(test)]
mod tests {
    use super::*;
    use neko_tensor::Device;
    use crate::loss;
    use crate::optimizer::{self, OptimizerState};

    #[test]
    fn single_layer_forward() {
        let x = neko_tensor::Tensor::from_cpu_data(&[4, 2], vec![0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 1.0, 1.0], Device::Cpu).unwrap();
        let mut layer = Layer::linear(2, 1, Device::Cpu).unwrap();
        let y = layer.forward(&x).unwrap();
        assert_eq!(y.shape, vec![4, 1]);
    }

    #[test]
    fn optimizer_step_smoke() {
        let mut w = neko_tensor::Tensor::from_cpu_data(&[1, 1], vec![0.5], Device::Cpu).unwrap();
        let grad = vec![0.1];
        let mut state = OptimizerState::default();
        optimizer::step(&OptimizerKind::sgd(0.1), &mut state, 0, &mut w, &grad).unwrap();
        assert!(w.to_cpu().unwrap()[0] < 0.5);
    }

    #[test]
    fn single_layer_backward() {
        let x = neko_tensor::Tensor::from_cpu_data(&[2, 1], vec![0.0, 1.0], Device::Cpu).unwrap();
        let y = neko_tensor::Tensor::from_cpu_data(&[2, 1], vec![1.0, 3.0], Device::Cpu).unwrap();
        let mut model = Sequential::new(vec![Layer::linear(1, 1, Device::Cpu).unwrap()], Device::Cpu);
        let pred = model.forward(&x).unwrap();
        let grad = loss::loss_grad(LossKind::Mse, &pred, &y).unwrap();
        assert_eq!(grad.len(), 2);
        let _pgs = model.backward(grad).unwrap();
    }

    #[test]
    fn optimizer_step_two_params() {
        let mut w = neko_tensor::Tensor::from_cpu_data(&[1, 2], vec![0.5, 0.5], Device::Cpu).unwrap();
        let grad = vec![0.1, 0.2];
        let mut state = OptimizerState::default();
        optimizer::step(&OptimizerKind::sgd(0.1), &mut state, 0, &mut w, &grad).unwrap();
    }

    #[test]
    fn three_layer_train_epoch_smoke() {
        let device = Device::Cpu;
        let x = neko_tensor::Tensor::from_cpu_data(
            &[256, 4],
            (0..256 * 4).map(|i| (i as f32 * 0.01).sin()).collect(),
            device,
        )
        .unwrap();
        let y = neko_tensor::Tensor::from_cpu_data(&[256, 1], vec![0.0; 256], device).unwrap();
        let model = Sequential::new(
            vec![
                Layer::linear(4, 16, device).unwrap(),
                Layer::relu(),
                Layer::linear(16, 1, device).unwrap(),
            ],
            device,
        );
        let mut trainer = Trainer::new(model, OptimizerKind::adam(0.01), LossKind::Mse, device);
        let mut loader = DataLoader::new(x, y, 32).unwrap();
        let metrics = trainer.train_epoch(&mut loader).unwrap();
        assert!(metrics.loss.is_finite());
        assert_eq!(metrics.batches, 8);
    }

}
