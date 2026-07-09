use crate::diagnostic::Diagnostic;
use crate::niao_error::NiaoError;
use std::fmt;
use std::io::{self, Write};

/// Formats and reports Niao errors consistently across CLI and tooling.
#[derive(Debug, Clone, Default)]
pub struct ErrorHandler {
    pub color: bool,
    pub show_help: bool,
}

impl ErrorHandler {
    pub fn new() -> Self {
        Self {
            color: false,
            show_help: true,
        }
    }

    pub fn with_color(mut self, color: bool) -> Self {
        self.color = color;
        self
    }

    /// Render a diagnostic to a string.
    pub fn format_diagnostic(&self, diag: &Diagnostic) -> String {
        let mut out = String::new();
        if self.color {
            out.push_str("\x1b[1;31m");
        }
        out.push_str(&format!("error[{}]", diag.prefix()));
        if self.color {
            out.push_str("\x1b[0m");
        }
        out.push_str(": ");
        out.push_str(&diag.message);
        if let Some(span) = diag.span {
            if span.line > 0 {
                out.push_str(&format!(" at line {}, col {}", span.line, span.col));
            }
        }
        if self.show_help {
            if let Some(help) = &diag.help {
                out.push('\n');
                out.push_str("  help: ");
                out.push_str(help);
            }
        }
        out
    }

    pub fn format_error(&self, err: &NiaoError) -> String {
        self.format_diagnostic(&err.diagnostic())
    }

    pub fn report(&self, err: &NiaoError) -> io::Result<()> {
        let msg = self.format_error(err);
        writeln!(io::stderr(), "{msg}")?;
        Ok(())
    }

    pub fn report_diagnostic(&self, diag: &Diagnostic) -> io::Result<()> {
        writeln!(io::stderr(), "{}", self.format_diagnostic(diag))?;
        Ok(())
    }
}

impl fmt::Display for ErrorHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ErrorHandler(color={})", self.color)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::{Diagnostic, ErrorCategory};
    use crate::runtime::RuntimeError;
    use niao_ast::Span;

    #[test]
    fn formats_runtime_error() {
        let handler = ErrorHandler::new();
        let err = NiaoError::Runtime(RuntimeError::division_by_zero(Span::dummy()));
        let msg = handler.format_error(&err);
        assert!(msg.contains("E2001"));
        assert!(msg.contains("division by zero"));
    }

    #[test]
    fn diagnostic_includes_code_prefix() {
        let handler = ErrorHandler::new();
        let diag = Diagnostic::error(ErrorCategory::Parse, 100, "bad token");
        assert!(handler.format_diagnostic(&diag).contains("error[E0100]"));
    }
}
