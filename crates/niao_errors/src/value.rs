use crate::codes;
use crate::diagnostic::Diagnostic;
use niao_ast::Span;
use std::fmt;

/// Language-level error value exposed to Niao programs via `try/catch` and `error()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NiaoErrorValue {
    pub code: u32,
    pub kind: String,
    pub message: String,
    pub line: usize,
    pub col: usize,
}

impl NiaoErrorValue {
    pub fn new(code: u32, kind: impl Into<String>, message: impl Into<String>, span: Span) -> Self {
        Self {
            code,
            kind: kind.into(),
            message: message.into(),
            line: span.line,
            col: span.col,
        }
    }

    pub fn from_message(message: impl Into<String>, span: Span) -> Self {
        Self::new(
            codes::E2007_THROWN,
            codes::runtime_kind_name(codes::E2007_THROWN),
            message,
            span,
        )
    }

    pub fn from_diagnostic(diag: &Diagnostic) -> Self {
        Self {
            code: diag.code,
            kind: codes::runtime_kind_name(diag.code).to_string(),
            message: diag.message.clone(),
            line: diag.line(),
            col: diag.col(),
        }
    }

    pub fn span(&self) -> Span {
        Span {
            start: 0,
            end: 0,
            line: self.line,
            col: self.col,
        }
    }
}

impl fmt::Display for NiaoErrorValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line > 0 {
            write!(
                f,
                "E{:04}: {} at line {}, col {}",
                self.code, self.message, self.line, self.col
            )
        } else {
            write!(f, "E{:04}: {}", self.code, self.message)
        }
    }
}
