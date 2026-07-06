use crate::codes;
use neko_ast::Span;
use std::fmt;

/// Severity of a diagnostic message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Note,
}

/// High-level category for grouping errors in tooling output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCategory {
    Lex,
    Parse,
    Compile,
    Runtime,
    Io,
    Lint,
    Vm,
}

impl fmt::Display for ErrorCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErrorCategory::Lex => write!(f, "lex"),
            ErrorCategory::Parse => write!(f, "parse"),
            ErrorCategory::Compile => write!(f, "compile"),
            ErrorCategory::Runtime => write!(f, "runtime"),
            ErrorCategory::Io => write!(f, "io"),
            ErrorCategory::Lint => write!(f, "lint"),
            ErrorCategory::Vm => write!(f, "vm"),
        }
    }
}

/// A structured diagnostic with code, message, and optional source location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: u32,
    pub category: ErrorCategory,
    pub severity: Severity,
    pub message: String,
    pub span: Option<Span>,
    pub help: Option<String>,
}

impl Diagnostic {
    pub fn error(category: ErrorCategory, code: u32, message: impl Into<String>) -> Self {
        Self {
            code,
            category,
            severity: Severity::Error,
            message: message.into(),
            span: None,
            help: None,
        }
    }

    pub fn at(mut self, span: Span) -> Self {
        self.span = Some(span);
        self
    }

    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    pub fn line(&self) -> usize {
        self.span.map(|s| s.line).unwrap_or(0)
    }

    pub fn col(&self) -> usize {
        self.span.map(|s| s.col).unwrap_or(0)
    }

    pub fn prefix(&self) -> String {
        match self.severity {
            Severity::Error => format!("E{:04}", self.code),
            Severity::Warning => format!("W{:04}", self.code),
            Severity::Note => format!("N{:04}", self.code),
        }
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.prefix(), self.message)?;
        if let Some(span) = self.span {
            if span.line > 0 {
                write!(f, " at line {}, col {}", span.line, span.col)?;
            }
        }
        if let Some(help) = &self.help {
            write!(f, "\n  help: {help}")?;
        }
        Ok(())
    }
}

impl Diagnostic {
    /// Infer category from numeric code when not explicitly set.
    pub fn category_for_code(code: u32) -> ErrorCategory {
        match code {
            1..=99 => ErrorCategory::Lex,
            100..=199 => ErrorCategory::Parse,
            200..=299 => ErrorCategory::Compile,
            1000..=2099 => ErrorCategory::Runtime,
            _ => ErrorCategory::Runtime,
        }
    }

    pub fn from_runtime(code: u32, message: String, span: Span) -> Self {
        Self {
            code,
            category: ErrorCategory::Runtime,
            severity: Severity::Error,
            message,
            span: Some(span),
            help: None,
        }
    }

    pub fn division_by_zero(span: Span) -> Self {
        Self::from_runtime(
            codes::E2001_DIVISION_BY_ZERO,
            "division by zero".into(),
            span,
        )
    }

    pub fn undefined_var(name: &str, span: Span) -> Self {
        Self::from_runtime(
            codes::E2002_UNDEFINED_VAR,
            format!("undefined variable '{name}'"),
            span,
        )
    }

    pub fn type_error(message: impl Into<String>, span: Span) -> Self {
        Self::from_runtime(codes::E2003_TYPE_ERROR, message.into(), span)
    }
}
