//! Columnar data → tensor conversion.

use niao_tensor::{Device, Tensor, TensorResult};

pub fn one_hot_encode(labels: &[i64], num_classes: usize) -> TensorResult<Tensor> {
    let n = labels.len();
    let mut data = vec![0.0f32; n * num_classes];
    for (i, &l) in labels.iter().enumerate() {
        let c = l as usize;
        if c < num_classes {
            data[i * num_classes + c] = 1.0;
        }
    }
    Tensor::from_cpu_data(&[n, num_classes], data, Device::Cpu)
}

pub fn dataframe_columns_to_tensors(
    feature_cols: &[Vec<f32>],
    label_col: &[f32],
) -> TensorResult<(Tensor, Tensor)> {
    if feature_cols.is_empty() {
        return Err(niao_tensor::TensorError::Shape("no feature columns".into()));
    }
    let n = label_col.len();
    let feat_n = feature_cols.len();
    for col in feature_cols {
        if col.len() != n {
            return Err(niao_tensor::TensorError::Shape(
                "feature column length mismatch".into(),
            ));
        }
    }
    let mut x_data = vec![0.0f32; n * feat_n];
    for (f, col) in feature_cols.iter().enumerate() {
        for (r, &v) in col.iter().enumerate() {
            x_data[r * feat_n + f] = v;
        }
    }
    let x = Tensor::from_cpu_data(&[n, feat_n], x_data, Device::Cpu)?;
    let y = Tensor::from_cpu_data(&[n], label_col.to_vec(), Device::Cpu)?;
    Ok((x, y))
}
