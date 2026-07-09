//! Model explainability hooks.

use crate::layer::LayerKind;
use crate::model::Sequential;
use niao_tensor::Tensor;
use niao_tensor::TensorResult;

pub fn feature_importance(model: &Sequential, x: &Tensor) -> TensorResult<Vec<f32>> {
    let mut importances = Vec::new();
    for layer in &model.layers {
        match &layer.kind {
            LayerKind::Linear { in_features, .. } => {
                if let Some(w) = &layer.weight {
                    let w_data = w.to_cpu()?;
                    let feat_imp: Vec<f32> = (0..*in_features)
                        .map(|f| {
                            w_data
                                .iter()
                                .skip(f)
                                .step_by(*in_features)
                                .map(|v| v.abs())
                                .sum::<f32>()
                        })
                        .collect();
                    importances.extend(feat_imp);
                }
            }
            LayerKind::Conv2d { out_channels, .. } => {
                if let Some(w) = &layer.weight {
                    let w_data = w.to_cpu()?;
                    let imp = w_data.iter().map(|v| v.abs()).sum::<f32>() / *out_channels as f32;
                    importances.push(imp);
                }
            }
            _ => {}
        }
    }
    if importances.is_empty() {
        let n = x.shape.get(1).copied().unwrap_or(x.len());
        importances = vec![1.0 / n as f32; n];
    }
    Ok(importances)
}
