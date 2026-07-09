use crate::diagnostic::{Diagnostic, ErrorCategory, Severity};
use crate::runtime::RuntimeError;
use niao_ast::Span;
use std::fmt;
use std::io;

/// Top-level error type spanning the full Niao toolchain.
#[derive(Debug)]
pub enum NiaoError {
    Lex(LexError),
    Parse(ParseError),
    Compile(CompileError),
    Runtime(RuntimeError),
    Vm(VmError),
    Io(io::Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LexError {
    UnexpectedChar { ch: char, line: usize, col: usize },
    UnterminatedString { line: usize, col: usize },
}

impl LexError {
    pub fn span(&self) -> Span {
        let (line, col) = match self {
            LexError::UnexpectedChar { line, col, .. } | LexError::UnterminatedString { line, col } => {
                (*line, *col)
            }
        };
        Span {
            start: 0,
            end: 0,
            line,
            col,
        }
    }

    pub fn diagnostic(&self) -> Diagnostic {
        match self {
            LexError::UnexpectedChar { ch, .. } => Diagnostic::error(
                ErrorCategory::Lex,
                crate::codes::E0001_UNEXPECTED_CHAR,
                format!("unexpected character '{ch}'"),
            )
            .at(self.span()),
            LexError::UnterminatedString { .. } => Diagnostic::error(
                ErrorCategory::Lex,
                crate::codes::E0002_UNTERMINATED_STRING,
                "unterminated string",
            )
            .at(self.span()),
        }
    }
}

impl fmt::Display for LexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.diagnostic())
    }
}

impl std::error::Error for LexError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    Unexpected {
        found: String,
        expected: String,
        line: usize,
        col: usize,
    },
    Eof,
    Lex(LexError),
}

impl ParseError {
    pub fn span(&self) -> Option<Span> {
        match self {
            ParseError::Unexpected { line, col, .. } => Some(Span {
                start: 0,
                end: 0,
                line: *line,
                col: *col,
            }),
            ParseError::Eof => None,
            ParseError::Lex(e) => Some(e.span()),
        }
    }

    pub fn diagnostic(&self) -> Diagnostic {
        match self {
            ParseError::Unexpected {
                found,
                expected,
                line,
                col,
            } => Diagnostic::error(
                ErrorCategory::Parse,
                crate::codes::E0100_UNEXPECTED_TOKEN,
                format!("unexpected token {found:?}, expected {expected}"),
            )
            .at(Span {
                start: 0,
                end: 0,
                line: *line,
                col: *col,
            }),
            ParseError::Eof => Diagnostic::error(
                ErrorCategory::Parse,
                crate::codes::E0101_UNEXPECTED_EOF,
                "unexpected end of file",
            ),
            ParseError::Lex(e) => e.diagnostic(),
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.diagnostic())
    }
}

impl std::error::Error for ParseError {}

impl From<LexError> for ParseError {
    fn from(e: LexError) -> Self {
        ParseError::Lex(e)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompileError {
    Unsupported { message: String },
    UnknownFunction { name: String },
}

impl CompileError {
    pub fn diagnostic(&self) -> Diagnostic {
        match self {
            CompileError::Unsupported { message } => Diagnostic::error(
                ErrorCategory::Compile,
                crate::codes::E0200_UNSUPPORTED,
                message.clone(),
            ),
            CompileError::UnknownFunction { name } => Diagnostic::error(
                ErrorCategory::Compile,
                crate::codes::E0201_UNKNOWN_FUNCTION,
                format!("unknown function: {name}"),
            ),
        }
    }
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.diagnostic())
    }
}

impl std::error::Error for CompileError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VmError {
    Runtime(RuntimeError),
    StackUnderflow,
    UnknownFunction(String),
    NoMain,
}

impl VmError {
    pub fn diagnostic(&self) -> Diagnostic {
        match self {
            VmError::Runtime(e) => e.diagnostic(),
            VmError::StackUnderflow => Diagnostic::error(
                ErrorCategory::Vm,
                crate::codes::E2008_STACK_UNDERFLOW,
                "stack underflow",
            ),
            VmError::UnknownFunction(name) => Diagnostic::error(
                ErrorCategory::Vm,
                crate::codes::E0201_UNKNOWN_FUNCTION,
                format!("unknown function: {name}"),
            ),
            VmError::NoMain => Diagnostic::error(
                ErrorCategory::Vm,
                crate::codes::E2009_NO_MAIN,
                "no main function found",
            ),
        }
    }
}

impl fmt::Display for VmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.diagnostic())
    }
}

impl std::error::Error for VmError {}

impl From<RuntimeError> for VmError {
    fn from(e: RuntimeError) -> Self {
        VmError::Runtime(e)
    }
}

impl NiaoError {
    pub fn diagnostic(&self) -> Diagnostic {
        match self {
            NiaoError::Lex(e) => e.diagnostic(),
            NiaoError::Parse(e) => e.diagnostic(),
            NiaoError::Compile(e) => e.diagnostic(),
            NiaoError::Runtime(e) => e.diagnostic(),
            NiaoError::Vm(e) => e.diagnostic(),
            NiaoError::Io(e) => Diagnostic {
                code: 0,
                category: ErrorCategory::Io,
                severity: Severity::Error,
                message: e.to_string(),
                span: None,
                help: None,
            },
        }
    }
}

impl fmt::Display for NiaoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.diagnostic())
    }
}

impl std::error::Error for NiaoError {}

impl From<LexError> for NiaoError {
    fn from(e: LexError) -> Self {
        NiaoError::Lex(e)
    }
}

impl From<ParseError> for NiaoError {
    fn from(e: ParseError) -> Self {
        NiaoError::Parse(e)
    }
}

impl From<CompileError> for NiaoError {
    fn from(e: CompileError) -> Self {
        NiaoError::Compile(e)
    }
}

impl From<RuntimeError> for NiaoError {
    fn from(e: RuntimeError) -> Self {
        NiaoError::Runtime(e)
    }
}

impl From<VmError> for NiaoError {
    fn from(e: VmError) -> Self {
        NiaoError::Vm(e)
    }
}

impl From<io::Error> for NiaoError {
    fn from(e: io::Error) -> Self {
        NiaoError::Io(e)
    }
}
