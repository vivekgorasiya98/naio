use std::fmt;

#[derive(Debug)]
pub enum TensorError {
    Shape(String),
    Device(String),
    Dtype(String),
    Op(String),
    Io(String),
}

pub type TensorResult<T> = Result<T, TensorError>;

impl fmt::Display for TensorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TensorError::Shape(m) => write!(f, "shape error: {m}"),
            TensorError::Device(m) => write!(f, "device error: {m}"),
            TensorError::Dtype(m) => write!(f, "dtype error: {m}"),
            TensorError::Op(m) => write!(f, "op error: {m}"),
            TensorError::Io(m) => write!(f, "io error: {m}"),
        }
    }
}

impl std::error::Error for TensorError {}
