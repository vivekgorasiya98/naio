//! Unified error types, codes, diagnostics, and reporting for the Neko toolchain.
//!
//! # Quick start
//!
//! ```rust
//! use neko_errors::{RuntimeError, ErrorHandler, NekoError};
//! use neko_ast::Span;
//!
//! let err = RuntimeError::division_by_zero(Span::dummy());
//! let neko_err = NekoError::Runtime(err);
//! let handler = ErrorHandler::new();
//! let message = handler.format_error(&neko_err);
//! assert!(message.contains("E2001"));
//! ```

pub mod codes;
pub mod diagnostic;
pub mod handler;
pub mod neko_error;
pub mod runtime;
pub mod value;

pub use codes::*;
pub use diagnostic::{Diagnostic, ErrorCategory, Severity};
pub use handler::ErrorHandler;
pub use neko_error::{CompileError, LexError, NekoError, ParseError, VmError};
pub use runtime::{NekoResult, RuntimeError};
pub use value::NekoErrorValue;
