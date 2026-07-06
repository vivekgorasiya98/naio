//! NCL dtype tags for packed columns and ndarrays.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dtype {
    Int,
    Float,
    Bool,
    String,
    Any,
}

impl Dtype {
    pub fn name(self) -> &'static str {
        match self {
            Dtype::Int => "int",
            Dtype::Float => "float",
            Dtype::Bool => "bool",
            Dtype::String => "string",
            Dtype::Any => "any",
        }
    }
}
