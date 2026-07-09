//! N-dimensional homogeneous array.

use super::dtypes::Dtype;

#[derive(Clone)]
pub struct NDArray {
    pub dtype: Dtype,
    pub shape: Vec<usize>,
    pub strides: Vec<usize>,
    pub data_int: Option<Vec<i64>>,
    pub data_float: Option<Vec<f64>>,
}

impl NDArray {
    pub fn from_int(shape: Vec<usize>, data: Vec<i64>) -> Result<Self, String> {
        let n: usize = shape.iter().product();
        if data.len() != n {
            return Err(format!("data length {} does not match shape product {n}", data.len()));
        }
        let strides = row_major_strides(&shape);
        Ok(Self {
            dtype: Dtype::Int,
            shape,
            strides,
            data_int: Some(data),
            data_float: None,
        })
    }

    pub fn from_float(shape: Vec<usize>, data: Vec<f64>) -> Result<Self, String> {
        let n: usize = shape.iter().product();
        if data.len() != n {
            return Err(format!("data length {} does not match shape product {n}", data.len()));
        }
        let strides = row_major_strides(&shape);
        Ok(Self {
            dtype: Dtype::Float,
            shape,
            strides,
            data_int: None,
            data_float: Some(data),
        })
    }

    pub fn len(&self) -> usize {
        self.shape.iter().product()
    }

    pub fn reshape(&self, new_shape: Vec<usize>) -> Result<Self, String> {
        let n: usize = new_shape.iter().product();
        if n != self.len() {
            return Err("cannot reshape: element count mismatch".into());
        }
        match self.dtype {
            Dtype::Int => {
                let data = self.data_int.clone().unwrap();
                Self::from_int(new_shape, data)
            }
            Dtype::Float => {
                let data = self.data_float.clone().unwrap();
                Self::from_float(new_shape, data)
            }
            _ => Err("reshape only supported for int/float ndarrays".into()),
        }
    }

    pub fn flatten(&self) -> Self {
        let n = self.len();
        match self.dtype {
            Dtype::Int => Self::from_int(vec![n], self.data_int.clone().unwrap()).unwrap(),
            Dtype::Float => Self::from_float(vec![n], self.data_float.clone().unwrap()).unwrap(),
            _ => self.clone(),
        }
    }
}

fn row_major_strides(shape: &[usize]) -> Vec<usize> {
    let mut strides = vec![1usize; shape.len()];
    for i in (0..shape.len().saturating_sub(1)).rev() {
        strides[i] = strides[i + 1] * shape[i + 1];
    }
    strides
}

pub fn transpose_2d_f64(data: &[f64], rows: usize, cols: usize) -> Vec<f64> {
    let mut out = vec![0.0; rows * cols];
    for r in 0..rows {
        for c in 0..cols {
            out[c * rows + r] = data[r * cols + c];
        }
    }
    out
}
