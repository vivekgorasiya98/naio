//! Shape and stride utilities.

use crate::error::{TensorError, TensorResult};

pub fn row_major_strides(shape: &[usize]) -> Vec<usize> {
    let mut strides = vec![1usize; shape.len()];
    for i in (0..shape.len().saturating_sub(1)).rev() {
        strides[i] = strides[i + 1] * shape[i + 1];
    }
    strides
}

pub fn numel(shape: &[usize]) -> usize {
    shape.iter().product()
}

pub fn validate_shape(shape: &[usize]) -> TensorResult<()> {
    if shape.is_empty() {
        return Err(TensorError::Shape("shape cannot be empty".into()));
    }
    if shape.iter().any(|&d| d == 0) {
        return Err(TensorError::Shape("shape dimensions must be > 0".into()));
    }
    Ok(())
}

pub fn reshape(shape: &[usize], new_shape: &[usize]) -> TensorResult<Vec<usize>> {
    let old_n = numel(shape);
    let new_n = numel(new_shape);
    if old_n != new_n {
        return Err(TensorError::Shape(format!(
            "cannot reshape {shape:?} to {new_shape:?}: element count mismatch"
        )));
    }
    validate_shape(new_shape)?;
    Ok(new_shape.to_vec())
}
