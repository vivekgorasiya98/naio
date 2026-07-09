//! Unified error types, codes, diagnostics, and reporting for the Niao toolchain.
//!
//! # Quick start
//!
//! ```rust
//! use niao_errors::{RuntimeError, ErrorHandler, NiaoError};
//! use niao_ast::Span;
//!
//! let err = RuntimeError::division_by_zero(Span::dummy());
//! let niao_err = NiaoError::Runtime(err);
//! let handler = ErrorHandler::new();
//! let message = handler.format_error(&niao_err);
//! assert!(message.contains("E2001"));
//! ```

pub mod codes;
pub mod diagnostic;
pub mod handler;
pub mod niao_error;
pub mod runtime;
pub mod value;

pub use codes::*;
pub use diagnostic::{Diagnostic, ErrorCategory, Severity};
pub use handler::ErrorHandler;
pub use niao_error::{CompileError, LexError, NiaoError, ParseError, VmError};
pub use runtime::{NiaoResult, RuntimeError};
pub use value::NiaoErrorValue;
