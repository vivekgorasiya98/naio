use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DType {
    F32,
    F16,
    Bf16,
    I64,
}

impl DType {
    pub fn name(self) -> &'static str {
        match self {
            DType::F32 => "f32",
            DType::F16 => "f16",
            DType::Bf16 => "bf16",
            DType::I64 => "i64",
        }
    }

    pub fn element_size(self) -> usize {
        match self {
            DType::F32 => 4,
            DType::F16 | DType::Bf16 => 2,
            DType::I64 => 8,
        }
    }
}

impl fmt::Display for DType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}
