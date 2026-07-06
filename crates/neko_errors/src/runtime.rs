use crate::codes;
use crate::diagnostic::{Diagnostic, ErrorCategory};
use crate::value::NekoErrorValue;
use neko_ast::Span;
use std::fmt;

/// Runtime failure during program execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeError {
    Generic {
        code: u32,
        message: String,
        line: usize,
        col: usize,
    },
    Simple { code: u32, message: String },
    DivisionByZero { line: usize, col: usize },
    UndefinedVar {
        name: String,
        line: usize,
        col: usize,
    },
    TypeError {
        message: String,
        line: usize,
        col: usize,
    },
    AssertFailed {
        message: String,
        line: usize,
        col: usize,
    },
    ModuleNotFound { path: String },
    ImportCycle { path: String },
    /// User `throw` with a structured error value.
    Thrown(NekoErrorValue),
    Break,
    Continue,
}

impl std::error::Error for RuntimeError {}

impl RuntimeError {
    pub fn at(span: Span, code: u32, message: impl Into<String>) -> Self {
        RuntimeError::Generic {
            code,
            message: message.into(),
            line: span.line,
            col: span.col,
        }
    }

    pub fn span(&self) -> Option<Span> {
        let (line, col) = match self {
            RuntimeError::Generic { line, col, .. }
            | RuntimeError::DivisionByZero { line, col }
            | RuntimeError::UndefinedVar { line, col, .. }
            | RuntimeError::TypeError { line, col, .. }
            | RuntimeError::AssertFailed { line, col, .. } => (*line, *col),
            RuntimeError::Thrown(v) => (v.line, v.col),
            _ => return None,
        };
        Some(Span {
            start: 0,
            end: 0,
            line,
            col,
        })
    }

    pub fn code(&self) -> u32 {
        match self {
            RuntimeError::Generic { code, .. } | RuntimeError::Simple { code, .. } => *code,
            RuntimeError::DivisionByZero { .. } => codes::E2001_DIVISION_BY_ZERO,
            RuntimeError::UndefinedVar { .. } => codes::E2002_UNDEFINED_VAR,
            RuntimeError::TypeError { .. } => codes::E2003_TYPE_ERROR,
            RuntimeError::AssertFailed { .. } => codes::E2004_ASSERT_FAILED,
            RuntimeError::ModuleNotFound { .. } => codes::E2005_MODULE_NOT_FOUND,
            RuntimeError::ImportCycle { .. } => codes::E2006_IMPORT_CYCLE,
            RuntimeError::Thrown(v) => v.code,
            RuntimeError::Break | RuntimeError::Continue => codes::E1005_CONTROL_FLOW,
        }
    }

    pub fn kind_name(&self) -> &'static str {
        codes::runtime_kind_name(self.code())
    }

    pub fn message(&self) -> String {
        match self {
            RuntimeError::Generic { message, .. }
            | RuntimeError::Simple { message, .. }
            | RuntimeError::TypeError { message, .. }
            | RuntimeError::AssertFailed { message, .. } => message.clone(),
            RuntimeError::DivisionByZero { .. } => "division by zero".into(),
            RuntimeError::UndefinedVar { name, .. } => format!("undefined variable '{name}'"),
            RuntimeError::ModuleNotFound { path } => format!("module not found: {path}"),
            RuntimeError::ImportCycle { path } => format!("import cycle detected: {path}"),
            RuntimeError::Thrown(v) => v.message.clone(),
            RuntimeError::Break => "break outside loop".into(),
            RuntimeError::Continue => "continue outside loop".into(),
        }
    }

    pub fn diagnostic(&self) -> Diagnostic {
        let mut diag = Diagnostic::error(ErrorCategory::Runtime, self.code(), self.message());
        if let Some(span) = self.span() {
            diag = diag.at(span);
        }
        diag
    }

    pub fn to_neko_error_value(&self) -> NekoErrorValue {
        match self {
            RuntimeError::Thrown(v) => v.clone(),
            _ => NekoErrorValue::from_diagnostic(&self.diagnostic()),
        }
    }

    pub fn division_by_zero(span: Span) -> Self {
        RuntimeError::DivisionByZero {
            line: span.line,
            col: span.col,
        }
    }

    pub fn undefined_var(name: impl Into<String>, span: Span) -> Self {
        RuntimeError::UndefinedVar {
            name: name.into(),
            line: span.line,
            col: span.col,
        }
    }

    pub fn type_error(message: impl Into<String>, span: Span) -> Self {
        RuntimeError::TypeError {
            message: message.into(),
            line: span.line,
            col: span.col,
        }
    }

    pub fn assert_failed(message: impl Into<String>, span: Span) -> Self {
        RuntimeError::AssertFailed {
            message: message.into(),
            line: span.line,
            col: span.col,
        }
    }

    pub fn thrown(value: NekoErrorValue) -> Self {
        RuntimeError::Thrown(value)
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.diagnostic())
    }
}

pub type NekoResult<T> = Result<T, RuntimeError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn division_by_zero_has_code() {
        let err = RuntimeError::division_by_zero(Span::dummy());
        assert_eq!(err.code(), codes::E2001_DIVISION_BY_ZERO);
        assert!(err.to_string().starts_with("E2001:"));
    }

    #[test]
    fn generic_at_preserves_span() {
        let span = Span {
            start: 0,
            end: 5,
            line: 3,
            col: 7,
        };
        let err = RuntimeError::at(span, 1001, "bad call");
        assert_eq!(err.span().unwrap().line, 3);
    }
}
