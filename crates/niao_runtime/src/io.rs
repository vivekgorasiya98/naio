//! Native I/O standard library: fast sync file operations, streaming handles,
//! path utilities, and background async tasks.
//!
//! Every function is exposed as an `io_*` builtin registered globally so
//! programs can `import "io"` (or `import "std/io"`) and call them at native
//! speed on both the bytecode VM and the tree-walking interpreter.

use crate::async_tasks::{
    async_io_error, cancel_task, spawn_async, task_done, task_result_value, task_wait_loop,
    with_task, AsyncValue,
};
use crate::{error_value, NativeFn, NiaoResult, RuntimeError, StringArray, Value, ValueRef};
use niao_ast::Span;
use niao_errors::codes;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::UNIX_EPOCH;

const IO_BUF_SIZE: usize = 256 * 1024;

// ---------------------------------------------------------------------------
// Argument helpers
// ---------------------------------------------------------------------------

fn type_err(span: Span, msg: impl Into<String>) -> RuntimeError {
    RuntimeError::TypeError {
        message: msg.into(),
        line: span.line,
        col: span.col,
    }
}

fn arity(args: &[ValueRef], n: usize, name: &str, span: Span) -> NiaoResult<()> {
    if args.len() != n {
        return Err(RuntimeError::at(
            span,
            codes::E1200_IO_ARITY,
            format!("{name}() expects {n} argument(s), got {}", args.len()),
        ));
    }
    Ok(())
}

fn arity_range(args: &[ValueRef], min: usize, max: usize, name: &str, span: Span) -> NiaoResult<()> {
    if args.len() < min || args.len() > max {
        return Err(RuntimeError::at(
            span,
            codes::E1200_IO_ARITY,
            format!("{name}() expects {min}..={max} argument(s), got {}", args.len()),
        ));
    }
    Ok(())
}

fn string_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> NiaoResult<String> {
    match &*args[idx].borrow() {
        Value::String(s) => Ok(s.clone()),
        other => Err(type_err(
            span,
            format!(
                "{name}() expects a string as argument {}, got {}",
                idx + 1,
                other.type_name()
            ),
        )),
    }
}

fn int_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> NiaoResult<i64> {
    match &*args[idx].borrow() {
        Value::Int(n) => Ok(*n),
        other => Err(type_err(
            span,
            format!(
                "{name}() expects an int as argument {}, got {}",
                idx + 1,
                other.type_name()
            ),
        )),
    }
}

fn size_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> NiaoResult<usize> {
    let n = int_arg(args, idx, name, span)?;
    if n < 0 {
        return Err(type_err(
            span,
            format!("{name}() expects a non-negative int as argument {}", idx + 1),
        ));
    }
    Ok(n as usize)
}

fn handle_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> NiaoResult<u64> {
    let id = int_arg(args, idx, name, span)?;
    if id <= 0 {
        return Err(type_err(
            span,
            format!("{name}() expects a positive file handle as argument {}", idx + 1),
        ));
    }
    Ok(id as u64)
}

fn io_error(span: Span, msg: impl Into<String>) -> ValueRef {
    error_value(
        codes::E1201_IO_ERROR,
        "io_error",
        msg.into(),
        span,
    )
}

fn ok_string(s: String) -> ValueRef {
    Value::String(s).ref_cell()
}

fn ok_bool(b: bool) -> ValueRef {
    Value::Bool(b).ref_cell()
}

fn ok_int(n: i64) -> ValueRef {
    Value::Int(n).ref_cell()
}

fn ok_nil() -> ValueRef {
    Value::Nil.ref_cell()
}

fn ok_string_array(items: Vec<String>) -> ValueRef {
    Value::StringArray(StringArray::dense(items)).ref_cell()
}

fn bytes_to_byte_array(data: Vec<u8>) -> ValueRef {
    Value::ByteArray(data).ref_cell()
}

fn int_array_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> NiaoResult<Vec<u8>> {
    match &*args[idx].borrow() {
        Value::ByteArray(data) => Ok(data.clone()),
        Value::IntArray(items) => {
            let mut out = Vec::with_capacity(items.len());
            for &n in items {
                if !(0..=255).contains(&n) {
                    return Err(type_err(
                        span,
                        format!("{name}() byte values must be 0..=255"),
                    ));
                }
                out.push(n as u8);
            }
            Ok(out)
        }
        Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                match &*item.borrow() {
                    Value::Int(n) if (0..=255).contains(n) => out.push(*n as u8),
                    other => {
                        return Err(type_err(
                            span,
                            format!(
                                "{name}() expects byte array, got {} in array",
                                other.type_name()
                            ),
                        ));
                    }
                }
            }
            Ok(out)
        }
        other => Err(type_err(
            span,
            format!(
                "{name}() expects a byte array as argument {}, got {}",
                idx + 1,
                other.type_name()
            ),
        )),
    }
}

fn metadata_time(path: &Path, modified: bool) -> Result<i64, String> {
    let meta = fs::metadata(path).map_err(|e| e.to_string())?;
    let time = if modified {
        meta.modified()
    } else {
        meta.created()
    }
    .map_err(|e| e.to_string())?;
    Ok(time
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis() as i64)
}

// ---------------------------------------------------------------------------
// Open file handles (streaming I/O)
// ---------------------------------------------------------------------------

enum IoMode {
    TextRead,
    TextWrite,
    TextAppend,
    TextReadWrite,
    BinaryRead,
    BinaryWrite,
    BinaryAppend,
}

impl IoMode {
    fn parse(mode: &str) -> Option<Self> {
        match mode {
            "r" => Some(Self::TextRead),
            "w" => Some(Self::TextWrite),
            "a" => Some(Self::TextAppend),
            "r+" | "w+" | "a+" => Some(Self::TextReadWrite),
            "rb" => Some(Self::BinaryRead),
            "wb" => Some(Self::BinaryWrite),
            "ab" => Some(Self::BinaryAppend),
            _ => None,
        }
    }

    fn binary(&self) -> bool {
        matches!(
            self,
            Self::BinaryRead | Self::BinaryWrite | Self::BinaryAppend
        )
    }
}

enum IoHandle {
    Reader {
        reader: BufReader<File>,
        binary: bool,
        eof: bool,
    },
    Writer {
        writer: BufWriter<File>,
        binary: bool,
    },
    ReadWrite {
        file: File,
        binary: bool,
        eof: bool,
    },
}

impl IoHandle {
    fn binary(&self) -> bool {
        match self {
            Self::Reader { binary, .. } | Self::Writer { binary, .. } | Self::ReadWrite { binary, .. } => {
                *binary
            }
        }
    }

    fn is_eof(&self) -> bool {
        match self {
            Self::Reader { eof, .. } | Self::ReadWrite { eof, .. } => *eof,
            Self::Writer { .. } => false,
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Writer { writer, .. } => writer.flush(),
            Self::ReadWrite { file, .. } => file.flush(),
            Self::Reader { .. } => Ok(()),
        }
    }
}

thread_local! {
    static IO_HANDLES: RefCell<HashMap<u64, IoHandle>> = RefCell::new(HashMap::new());
    static NEXT_IO_HANDLE: Cell<u64> = const { Cell::new(1) };
}

fn alloc_handle(handle: IoHandle) -> u64 {
    let id = NEXT_IO_HANDLE.with(|n| {
        let id = n.get();
        n.set(id.saturating_add(1));
        id
    });
    IO_HANDLES.with(|map| map.borrow_mut().insert(id, handle));
    id
}

fn with_handle(
    id: u64,
    name: &str,
    span: Span,
    f: impl FnOnce(&mut IoHandle) -> Result<HandleResult, String>,
) -> NiaoResult<ValueRef> {
    IO_HANDLES.with(|map| {
        let mut guard = map.borrow_mut();
        let handle = guard.get_mut(&id).ok_or_else(|| {
            RuntimeError::at(
                span,
                codes::E1202_IO_INVALID_HANDLE,
                format!("{name}(): invalid or closed file handle {id}"),
            )
        })?;
        match f(handle) {
            Ok(v) => Ok(match v {
                HandleResult::String(s) => ok_string(s),
                HandleResult::Bytes(b) => bytes_to_byte_array(b),
                HandleResult::Int(n) => ok_int(n),
                HandleResult::Bool(b) => ok_bool(b),
                HandleResult::Nil => ok_nil(),
            }),
            Err(msg) => Ok(io_error(span, msg)),
        }
    })
}

enum HandleResult {
    String(String),
    Bytes(Vec<u8>),
    Int(i64),
    Bool(bool),
    Nil,
}

fn open_file(path: &Path, mode: IoMode) -> Result<IoHandle, String> {
    match mode {
        IoMode::TextRead | IoMode::BinaryRead => {
            let file = File::open(path).map_err(|e| e.to_string())?;
            Ok(IoHandle::Reader {
                reader: BufReader::with_capacity(IO_BUF_SIZE, file),
                binary: mode.binary(),
                eof: false,
            })
        }
        IoMode::TextWrite | IoMode::BinaryWrite => {
            let file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(path)
                .map_err(|e| e.to_string())?;
            Ok(IoHandle::Writer {
                writer: BufWriter::with_capacity(IO_BUF_SIZE, file),
                binary: mode.binary(),
            })
        }
        IoMode::TextAppend | IoMode::BinaryAppend => {
            let file = OpenOptions::new()
                .write(true)
                .create(true)
                .append(true)
                .open(path)
                .map_err(|e| e.to_string())?;
            Ok(IoHandle::Writer {
                writer: BufWriter::with_capacity(IO_BUF_SIZE, file),
                binary: mode.binary(),
            })
        }
        IoMode::TextReadWrite => {
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .open(path)
                .map_err(|e| e.to_string())?;
            Ok(IoHandle::ReadWrite {
                file,
                binary: false,
                eof: false,
            })
        }
    }
}

fn task_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> NiaoResult<u64> {
    let id = int_arg(args, idx, name, span)?;
    if id <= 0 {
        return Err(type_err(
            span,
            format!("{name}() expects a positive task id as argument {}", idx + 1),
        ));
    }
    Ok(id as u64)
}

// ---------------------------------------------------------------------------
// Path helpers
// ---------------------------------------------------------------------------

fn path_from_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> NiaoResult<PathBuf> {
    Ok(PathBuf::from(string_arg(args, idx, name, span)?))
}

fn io_join(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity_range(args, 2, 16, "io_join", span)?;
    let mut path = path_from_arg(args, 0, "io_join", span)?;
    for i in 1..args.len() {
        path.push(string_arg(args, i, "io_join", span)?);
    }
    Ok(ok_string(path.to_string_lossy().into_owned()))
}

fn io_join_many(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "io_join_many", span)?;
    let parts = match &*args[0].borrow() {
        Value::Array(items) => items
            .iter()
            .map(|v| match &*v.borrow() {
                Value::String(s) => Ok(s.clone()),
                other => Err(type_err(
                    span,
                    format!("io_join_many() expects array of strings, got {}", other.type_name()),
                )),
            })
            .collect::<Result<Vec<_>, _>>()?,
        other => {
            return Err(type_err(
                span,
                format!("io_join_many() expects an array, got {}", other.type_name()),
            ));
        }
    };
    if parts.is_empty() {
        return Ok(ok_string(String::new()));
    }
    let mut path = PathBuf::from(&parts[0]);
    for part in parts.iter().skip(1) {
        path.push(part);
    }
    Ok(ok_string(path.to_string_lossy().into_owned()))
}

fn io_dirname(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "io_dirname", span)?;
    let path = path_from_arg(args, 0, "io_dirname", span)?;
    Ok(ok_string(
        path.parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| ".".into()),
    ))
}

fn io_basename(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "io_basename", span)?;
    let path = path_from_arg(args, 0, "io_basename", span)?;
    Ok(ok_string(
        path.file_name()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default(),
    ))
}

fn io_stem(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "io_stem", span)?;
    let path = path_from_arg(args, 0, "io_stem", span)?;
    Ok(ok_string(
        path.file_stem()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default(),
    ))
}

fn io_extension(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "io_extension", span)?;
    let path = path_from_arg(args, 0, "io_extension", span)?;
    Ok(ok_string(
        path.extension()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default(),
    ))
}

fn io_is_absolute(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "io_is_absolute", span)?;
    let path = path_from_arg(args, 0, "io_is_absolute", span)?;
    Ok(ok_bool(path.is_absolute()))
}

fn io_canonical(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "io_canonical", span)?;
    let path = path_from_arg(args, 0, "io_canonical", span)?;
    match fs::canonicalize(&path) {
        Ok(p) => Ok(ok_string(p.to_string_lossy().into_owned())),
        Err(e) => Ok(io_error(span, e.to_string())),
    }
}

// ---------------------------------------------------------------------------
// Metadata
// ---------------------------------------------------------------------------

fn io_exists(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "io_exists", span)?;
    let path = path_from_arg(args, 0, "io_exists", span)?;
    Ok(ok_bool(path.exists()))
}

fn io_is_file(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "io_is_file", span)?;
    let path = path_from_arg(args, 0, "io_is_file", span)?;
    Ok(ok_bool(path.is_file()))
}

fn io_is_dir(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "io_is_dir", span)?;
    let path = path_from_arg(args, 0, "io_is_dir", span)?;
    Ok(ok_bool(path.is_dir()))
}

fn io_is_symlink(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "io_is_symlink", span)?;
    let path = path_from_arg(args, 0, "io_is_symlink", span)?;
    Ok(ok_bool(path.is_symlink()))
}

fn io_file_size(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "io_file_size", span)?;
    let path = path_from_arg(args, 0, "io_file_size", span)?;
    match fs::metadata(&path) {
        Ok(m) => Ok(ok_int(m.len() as i64)),
        Err(e) => Ok(io_error(span, e.to_string())),
    }
}

fn io_modified_ms(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "io_modified_ms", span)?;
    let path = path_from_arg(args, 0, "io_modified_ms", span)?;
    match metadata_time(&path, true) {
        Ok(ms) => Ok(ok_int(ms)),
        Err(e) => Ok(io_error(span, e)),
    }
}

fn io_created_ms(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "io_created_ms", span)?;
    let path = path_from_arg(args, 0, "io_created_ms", span)?;
    match metadata_time(&path, false) {
        Ok(ms) => Ok(ok_int(ms)),
        Err(e) => Ok(io_error(span, e)),
    }
}

// ---------------------------------------------------------------------------
// Whole-file sync I/O (fast path)
// ---------------------------------------------------------------------------

pub fn io_read_file(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "io_read_file", span)?;
    let path = path_from_arg(args, 0, "io_read_file", span)?;
    match fs::read_to_string(&path) {
        Ok(s) => Ok(ok_string(s)),
        Err(e) => Ok(io_error(span, e.to_string())),
    }
}

fn io_read_bytes(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "io_read_bytes", span)?;
    let path = path_from_arg(args, 0, "io_read_bytes", span)?;
    match fs::read(&path) {
        Ok(data) => Ok(bytes_to_byte_array(data)),
        Err(e) => Ok(io_error(span, e.to_string())),
    }
}

pub fn io_write_file(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 2, "io_write_file", span)?;
    let path = path_from_arg(args, 0, "io_write_file", span)?;
    let content = string_arg(args, 1, "io_write_file", span)?;
    match fs::write(&path, content) {
        Ok(()) => Ok(ok_nil()),
        Err(e) => Ok(io_error(span, e.to_string())),
    }
}

fn io_write_bytes(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 2, "io_write_bytes", span)?;
    let path = path_from_arg(args, 0, "io_write_bytes", span)?;
    match &*args[1].borrow() {
        Value::ByteArray(data) => match fs::write(&path, data.as_slice()) {
            Ok(()) => Ok(ok_nil()),
            Err(e) => Ok(io_error(span, e.to_string())),
        },
        _ => {
            let data = int_array_arg(args, 1, "io_write_bytes", span)?;
            match fs::write(&path, data) {
                Ok(()) => Ok(ok_nil()),
                Err(e) => Ok(io_error(span, e.to_string())),
            }
        }
    }
}

fn io_append_file(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 2, "io_append_file", span)?;
    let path = path_from_arg(args, 0, "io_append_file", span)?;
    let content = string_arg(args, 1, "io_append_file", span)?;
    match OpenOptions::new().create(true).append(true).open(&path) {
        Ok(file) => {
            let mut writer = BufWriter::with_capacity(IO_BUF_SIZE, file);
            match writer.write_all(content.as_bytes()) {
                Ok(()) => match writer.flush() {
                    Ok(()) => Ok(ok_nil()),
                    Err(e) => Ok(io_error(span, e.to_string())),
                },
                Err(e) => Ok(io_error(span, e.to_string())),
            }
        }
        Err(e) => Ok(io_error(span, e.to_string())),
    }
}

fn io_read_lines(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "io_read_lines", span)?;
    let path = path_from_arg(args, 0, "io_read_lines", span)?;
    let data = match fs::read(&path) {
        Ok(d) => d,
        Err(e) => return Ok(io_error(span, e.to_string())),
    };
    Ok(Value::StringArray(StringArray::from_line_bytes(data)).ref_cell())
}

fn io_write_lines(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 2, "io_write_lines", span)?;
    let path = path_from_arg(args, 0, "io_write_lines", span)?;
    let lines = match &*args[1].borrow() {
        Value::StringArray(items) => items.dense_vec(),
        Value::Array(items) => items
            .iter()
            .map(|v| match &*v.borrow() {
                Value::String(s) => Ok(s.clone()),
                other => Err(type_err(
                    span,
                    format!("io_write_lines() expects array of strings, got {}", other.type_name()),
                )),
            })
            .collect::<Result<Vec<_>, _>>()?,
        other => {
            return Err(type_err(
                span,
                format!("io_write_lines() expects an array, got {}", other.type_name()),
            ));
        }
    };
    let mut file = match File::create(&path) {
        Ok(f) => BufWriter::with_capacity(IO_BUF_SIZE, f),
        Err(e) => return Ok(io_error(span, e.to_string())),
    };
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            if let Err(e) = file.write_all(b"\n") {
                return Ok(io_error(span, e.to_string()));
            }
        }
        if let Err(e) = file.write_all(line.as_bytes()) {
            return Ok(io_error(span, e.to_string()));
        }
    }
    if let Err(e) = file.flush() {
        return Ok(io_error(span, e.to_string()));
    }
    Ok(ok_nil())
}

// ---------------------------------------------------------------------------
// Directory operations
// ---------------------------------------------------------------------------

fn io_list_dir(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "io_list_dir", span)?;
    let path = path_from_arg(args, 0, "io_list_dir", span)?;
    match fs::read_dir(&path) {
        Ok(entries) => {
            let mut names = Vec::new();
            for entry in entries {
                match entry {
                    Ok(e) => names.push(e.file_name().to_string_lossy().into_owned()),
                    Err(e) => return Ok(io_error(span, e.to_string())),
                }
            }
            names.sort_unstable();
            Ok(ok_string_array(names))
        }
        Err(e) => Ok(io_error(span, e.to_string())),
    }
}

fn collect_dir_recursive(root: &Path, out: &mut Vec<String>) -> Result<(), String> {
    for entry in fs::read_dir(root).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let rel = path
            .strip_prefix(root)
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        if path.is_dir() {
            if !rel.is_empty() {
                out.push(format!("{rel}/"));
            }
            collect_dir_recursive(&path, out)?;
        } else {
            out.push(rel);
        }
    }
    Ok(())
}

fn io_list_dir_recursive(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "io_list_dir_recursive", span)?;
    let path = path_from_arg(args, 0, "io_list_dir_recursive", span)?;
    let mut names = Vec::new();
    match collect_dir_recursive(&path, &mut names) {
        Ok(()) => {
            names.sort_unstable();
            Ok(ok_string_array(names))
        }
        Err(e) => Ok(io_error(span, e)),
    }
}

fn io_create_dir(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "io_create_dir", span)?;
    let path = path_from_arg(args, 0, "io_create_dir", span)?;
    match fs::create_dir(&path) {
        Ok(()) => Ok(ok_nil()),
        Err(e) => Ok(io_error(span, e.to_string())),
    }
}

fn io_create_dir_all(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "io_create_dir_all", span)?;
    let path = path_from_arg(args, 0, "io_create_dir_all", span)?;
    match fs::create_dir_all(&path) {
        Ok(()) => Ok(ok_nil()),
        Err(e) => Ok(io_error(span, e.to_string())),
    }
}

fn io_remove_file(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "io_remove_file", span)?;
    let path = path_from_arg(args, 0, "io_remove_file", span)?;
    match fs::remove_file(&path) {
        Ok(()) => Ok(ok_nil()),
        Err(e) => Ok(io_error(span, e.to_string())),
    }
}

fn io_remove_dir(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "io_remove_dir", span)?;
    let path = path_from_arg(args, 0, "io_remove_dir", span)?;
    match fs::remove_dir(&path) {
        Ok(()) => Ok(ok_nil()),
        Err(e) => Ok(io_error(span, e.to_string())),
    }
}

fn io_remove_dir_all(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "io_remove_dir_all", span)?;
    let path = path_from_arg(args, 0, "io_remove_dir_all", span)?;
    match fs::remove_dir_all(&path) {
        Ok(()) => Ok(ok_nil()),
        Err(e) => Ok(io_error(span, e.to_string())),
    }
}

fn io_copy(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 2, "io_copy", span)?;
    let src = path_from_arg(args, 0, "io_copy", span)?;
    let dst = path_from_arg(args, 1, "io_copy", span)?;
    match fs::copy(&src, &dst) {
        Ok(_) => Ok(ok_nil()),
        Err(e) => Ok(io_error(span, e.to_string())),
    }
}

fn io_rename(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 2, "io_rename", span)?;
    let src = path_from_arg(args, 0, "io_rename", span)?;
    let dst = path_from_arg(args, 1, "io_rename", span)?;
    match fs::rename(&src, &dst) {
        Ok(()) => Ok(ok_nil()),
        Err(e) => Ok(io_error(span, e.to_string())),
    }
}

// ---------------------------------------------------------------------------
// Working directory & standard paths
// ---------------------------------------------------------------------------

fn io_cwd(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 0, "io_cwd", span)?;
    match env::current_dir() {
        Ok(p) => Ok(ok_string(p.to_string_lossy().into_owned())),
        Err(e) => Ok(io_error(span, e.to_string())),
    }
}

fn io_chdir(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "io_chdir", span)?;
    let path = path_from_arg(args, 0, "io_chdir", span)?;
    match env::set_current_dir(&path) {
        Ok(()) => Ok(ok_nil()),
        Err(e) => Ok(io_error(span, e.to_string())),
    }
}

fn io_temp_dir(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 0, "io_temp_dir", span)?;
    Ok(ok_string(
        env::temp_dir().to_string_lossy().into_owned(),
    ))
}

fn io_home_dir(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 0, "io_home_dir", span)?;
    match home_dir() {
        Some(p) => Ok(ok_string(p.to_string_lossy().into_owned())),
        None => Ok(io_error(span, "home directory not available")),
    }
}

mod env {
    pub use std::env::{current_dir, set_current_dir, temp_dir, var};
}

fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        if let Ok(home) = env::var("USERPROFILE") {
            return Some(PathBuf::from(home));
        }
    }
    if let Ok(home) = env::var("HOME") {
        return Some(PathBuf::from(home));
    }
    None
}

// ---------------------------------------------------------------------------
// Streaming file handles
// ---------------------------------------------------------------------------

fn io_open(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 2, "io_open", span)?;
    let path = path_from_arg(args, 0, "io_open", span)?;
    let mode_str = string_arg(args, 1, "io_open", span)?;
    let mode = match IoMode::parse(&mode_str) {
        Some(m) => m,
        None => {
            return Ok(io_error(
                span,
                format!(
                    "io_open(): invalid mode \"{mode_str}\" (use r, w, a, r+, rb, wb, ab)"
                ),
            ));
        }
    };
    match open_file(&path, mode) {
        Ok(handle) => Ok(ok_int(alloc_handle(handle) as i64)),
        Err(e) => Ok(io_error(span, e)),
    }
}

fn io_close(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "io_close", span)?;
    let id = handle_arg(args, 0, "io_close", span)?;
    IO_HANDLES.with(|map| {
        let mut guard = map.borrow_mut();
        let mut handle = guard.remove(&id).ok_or_else(|| {
            RuntimeError::at(
                span,
                codes::E1202_IO_INVALID_HANDLE,
                format!("io_close(): invalid or closed file handle {id}"),
            )
        })?;
        if let Err(e) = handle.flush() {
            return Ok(io_error(span, e.to_string()));
        }
        Ok(ok_nil())
    })
}

fn io_read(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 2, "io_read", span)?;
    let id = handle_arg(args, 0, "io_read", span)?;
    let n = size_arg(args, 1, "io_read", span)?;
    with_handle(id, "io_read", span, |handle| {
        if handle.binary() {
            let mut buf = vec![0u8; n];
            match handle {
                IoHandle::Reader { reader, eof, .. } => {
                    let read = reader.read(&mut buf).map_err(|e| e.to_string())?;
                    if read == 0 {
                        *eof = true;
                    }
                    buf.truncate(read);
                    Ok(HandleResult::Bytes(buf))
                }
                IoHandle::ReadWrite { file, eof, .. } => {
                    let read = file.read(&mut buf).map_err(|e| e.to_string())?;
                    if read == 0 {
                        *eof = true;
                    }
                    buf.truncate(read);
                    Ok(HandleResult::Bytes(buf))
                }
                IoHandle::Writer { .. } => Err("io_read(): handle is not open for reading".into()),
            }
        } else {
            let mut buf = vec![0u8; n];
            let read = match handle {
                IoHandle::Reader { reader, eof, .. } => {
                    let read = reader.read(&mut buf).map_err(|e| e.to_string())?;
                    if read == 0 {
                        *eof = true;
                    }
                    read
                }
                IoHandle::ReadWrite { file, eof, .. } => {
                    let read = file.read(&mut buf).map_err(|e| e.to_string())?;
                    if read == 0 {
                        *eof = true;
                    }
                    read
                }
                IoHandle::Writer { .. } => {
                    return Err("io_read(): handle is not open for reading".into());
                }
            };
            buf.truncate(read);
            Ok(HandleResult::String(
                String::from_utf8(buf)
                    .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned()),
            ))
        }
    })
}

fn io_read_all(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "io_read_all", span)?;
    let id = handle_arg(args, 0, "io_read_all", span)?;
    with_handle(id, "io_read_all", span, |handle| {
        match handle {
            IoHandle::Reader { reader, binary, eof, .. } => {
                if *binary {
                    let mut data = Vec::new();
                    reader.read_to_end(&mut data).map_err(|e| e.to_string())?;
                    *eof = true;
                    Ok(HandleResult::Bytes(data))
                } else {
                    let mut data = String::new();
                    reader.read_to_string(&mut data).map_err(|e| e.to_string())?;
                    *eof = true;
                    Ok(HandleResult::String(data))
                }
            }
            IoHandle::ReadWrite { file, binary, eof, .. } => {
                if *binary {
                    let mut data = Vec::new();
                    file.read_to_end(&mut data).map_err(|e| e.to_string())?;
                    *eof = true;
                    Ok(HandleResult::Bytes(data))
                } else {
                    let mut data = String::new();
                    file.read_to_string(&mut data).map_err(|e| e.to_string())?;
                    *eof = true;
                    Ok(HandleResult::String(data))
                }
            }
            IoHandle::Writer { .. } => Err("io_read_all(): handle is not open for reading".into()),
        }
    })
}

fn io_read_line(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "io_read_line", span)?;
    let id = handle_arg(args, 0, "io_read_line", span)?;
    with_handle(id, "io_read_line", span, |handle| {
        if handle.binary() {
            return Err("io_read_line(): binary handles do not support line reads".into());
        }
        match handle {
            IoHandle::Reader { reader, eof, .. } => {
                let mut line = String::new();
                let n = reader.read_line(&mut line).map_err(|e| e.to_string())?;
                if n == 0 {
                    *eof = true;
                    return Ok(HandleResult::Nil);
                }
                Ok(HandleResult::String(line))
            }
            IoHandle::ReadWrite { file, eof, .. } => {
                let mut line = String::new();
                let mut ch = [0u8; 1];
                loop {
                    let n = file.read(&mut ch).map_err(|e| e.to_string())?;
                    if n == 0 {
                        *eof = true;
                        if line.is_empty() {
                            return Ok(HandleResult::Nil);
                        }
                        break;
                    }
                    let c = ch[0] as char;
                    line.push(c);
                    if c == '\n' {
                        break;
                    }
                }
                Ok(HandleResult::String(line))
            }
            IoHandle::Writer { .. } => Err("io_read_line(): handle is not open for reading".into()),
        }
    })
}

fn io_write(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 2, "io_write", span)?;
    let id = handle_arg(args, 0, "io_write", span)?;
    with_handle(id, "io_write", span, |handle| {
        if handle.binary() {
            let data = int_array_arg(args, 1, "io_write", span).map_err(|e| e.to_string())?;
            match handle {
                IoHandle::Writer { writer, .. } => {
                    writer.write_all(&data).map_err(|e| e.to_string())?;
                    Ok(HandleResult::Int(data.len() as i64))
                }
                IoHandle::ReadWrite { file, .. } => {
                    file.write_all(&data).map_err(|e| e.to_string())?;
                    Ok(HandleResult::Int(data.len() as i64))
                }
                IoHandle::Reader { .. } => Err("io_write(): handle is not open for writing".into()),
            }
        } else {
            let text = string_arg(args, 1, "io_write", span).map_err(|e| e.to_string())?;
            match handle {
                IoHandle::Writer { writer, .. } => {
                    writer.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
                    Ok(HandleResult::Int(text.len() as i64))
                }
                IoHandle::ReadWrite { file, .. } => {
                    file.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
                    Ok(HandleResult::Int(text.len() as i64))
                }
                IoHandle::Reader { .. } => Err("io_write(): handle is not open for writing".into()),
            }
        }
    })
}

fn io_flush(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "io_flush", span)?;
    let id = handle_arg(args, 0, "io_flush", span)?;
    with_handle(id, "io_flush", span, |handle| {
        handle.flush().map_err(|e| e.to_string())?;
        Ok(HandleResult::Nil)
    })
}

fn io_seek(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 3, "io_seek", span)?;
    let id = handle_arg(args, 0, "io_seek", span)?;
    let offset = int_arg(args, 1, "io_seek", span)?;
    let whence = int_arg(args, 2, "io_seek", span)?;
    let pos = match whence {
        0 => SeekFrom::Start(offset as u64),
        1 => SeekFrom::Current(offset),
        2 => SeekFrom::End(offset),
        _ => return Ok(io_error(span, "io_seek(): whence must be 0 (start), 1 (current), or 2 (end)")),
    };
    with_handle(id, "io_seek", span, |handle| {
        let pos = match handle {
            IoHandle::Reader { reader, eof, .. } => {
                let p = reader.seek(pos).map_err(|e| e.to_string())?;
                *eof = false;
                p
            }
            IoHandle::ReadWrite { file, eof, .. } => {
                let p = file.seek(pos).map_err(|e| e.to_string())?;
                *eof = false;
                p
            }
            IoHandle::Writer { .. } => {
                return Err("io_seek(): writer-only handle does not support seek".into())
            }
        };
        Ok(HandleResult::Int(pos as i64))
    })
}

fn io_tell(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "io_tell", span)?;
    let id = handle_arg(args, 0, "io_tell", span)?;
    with_handle(id, "io_tell", span, |handle| {
        let pos = match handle {
            IoHandle::Reader { reader, .. } => reader.stream_position().map_err(|e| e.to_string())?,
            IoHandle::ReadWrite { file, .. } => file.stream_position().map_err(|e| e.to_string())?,
            IoHandle::Writer { .. } => {
                return Err("io_tell(): writer-only handle does not support tell".into())
            }
        };
        Ok(HandleResult::Int(pos as i64))
    })
}

fn io_eof(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "io_eof", span)?;
    let id = handle_arg(args, 0, "io_eof", span)?;
    with_handle(id, "io_eof", span, |handle| Ok(HandleResult::Bool(handle.is_eof())))
}

// ---------------------------------------------------------------------------
// Async background I/O
// ---------------------------------------------------------------------------

fn io_async_read(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "io_async_read", span)?;
    let path = path_from_arg(args, 0, "io_async_read", span)?;
    let id = spawn_async(move || {
        fs::read_to_string(&path)
            .map(AsyncValue::String)
            .map_err(|e| e.to_string())
    });
    Ok(ok_int(id as i64))
}

fn io_async_read_bytes(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "io_async_read_bytes", span)?;
    let path = path_from_arg(args, 0, "io_async_read_bytes", span)?;
    let id = spawn_async(move || {
        fs::read(&path)
            .map(|data| AsyncValue::ByteArray(data))
            .map_err(|e| e.to_string())
    });
    Ok(ok_int(id as i64))
}

fn io_async_write(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 2, "io_async_write", span)?;
    let path = path_from_arg(args, 0, "io_async_write", span)?;
    let content = string_arg(args, 1, "io_async_write", span)?;
    let id = spawn_async(move || {
        fs::write(&path, content)
            .map(|_| AsyncValue::nil())
            .map_err(|e| e.to_string())
    });
    Ok(ok_int(id as i64))
}

fn io_async_write_bytes(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 2, "io_async_write_bytes", span)?;
    let path = path_from_arg(args, 0, "io_async_write_bytes", span)?;
    let data = int_array_arg(args, 1, "io_async_write_bytes", span)?;
    let id = spawn_async(move || {
        fs::write(&path, data)
            .map(|_| AsyncValue::nil())
            .map_err(|e| e.to_string())
    });
    Ok(ok_int(id as i64))
}

fn io_async_copy(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 2, "io_async_copy", span)?;
    let src = path_from_arg(args, 0, "io_async_copy", span)?;
    let dst = path_from_arg(args, 1, "io_async_copy", span)?;
    let id = spawn_async(move || {
        fs::copy(&src, &dst)
            .map(|_| AsyncValue::nil())
            .map_err(|e| e.to_string())
    });
    Ok(ok_int(id as i64))
}

fn io_task_done(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "io_task_done", span)?;
    let id = task_arg(args, 0, "io_task_done", span)?;
    with_task(
        id,
        "io_task_done",
        span,
        codes::E1203_IO_TASK_NOT_FOUND,
        "async task cancelled",
        async_io_error,
        |state| Ok(ok_bool(task_done(state))),
    )
}

fn io_task_poll(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "io_task_poll", span)?;
    let id = task_arg(args, 0, "io_task_poll", span)?;
    with_task(
        id,
        "io_task_poll",
        span,
        codes::E1203_IO_TASK_NOT_FOUND,
        "async task cancelled",
        async_io_error,
        |state| Ok(task_result_value(state, span, "async task cancelled", async_io_error)),
    )
}

fn io_task_wait(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "io_task_wait", span)?;
    let id = task_arg(args, 0, "io_task_wait", span)?;
    task_wait_loop(id);
    with_task(
        id,
        "io_task_wait",
        span,
        codes::E1203_IO_TASK_NOT_FOUND,
        "async task cancelled",
        async_io_error,
        |state| Ok(task_result_value(state, span, "async task cancelled", async_io_error)),
    )
}

fn io_task_cancel(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "io_task_cancel", span)?;
    let id = task_arg(args, 0, "io_task_cancel", span)?;
    let cancelled = cancel_task(id, span, codes::E1203_IO_TASK_NOT_FOUND)?;
    Ok(ok_bool(cancelled))
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

/// All io builtins in registration order.
pub fn builtins() -> Vec<(&'static str, NativeFn)> {
    vec![
        // path utilities
        ("io_join", Rc::new(io_join)),
        ("io_join_many", Rc::new(io_join_many)),
        ("io_dirname", Rc::new(io_dirname)),
        ("io_basename", Rc::new(io_basename)),
        ("io_stem", Rc::new(io_stem)),
        ("io_extension", Rc::new(io_extension)),
        ("io_is_absolute", Rc::new(io_is_absolute)),
        ("io_canonical", Rc::new(io_canonical)),
        // metadata
        ("io_exists", Rc::new(io_exists)),
        ("io_is_file", Rc::new(io_is_file)),
        ("io_is_dir", Rc::new(io_is_dir)),
        ("io_is_symlink", Rc::new(io_is_symlink)),
        ("io_file_size", Rc::new(io_file_size)),
        ("io_modified_ms", Rc::new(io_modified_ms)),
        ("io_created_ms", Rc::new(io_created_ms)),
        // whole-file sync
        ("io_read_file", Rc::new(io_read_file)),
        ("io_read_bytes", Rc::new(io_read_bytes)),
        ("io_write_file", Rc::new(io_write_file)),
        ("io_write_bytes", Rc::new(io_write_bytes)),
        ("io_append_file", Rc::new(io_append_file)),
        ("io_read_lines", Rc::new(io_read_lines)),
        ("io_write_lines", Rc::new(io_write_lines)),
        // directories
        ("io_list_dir", Rc::new(io_list_dir)),
        ("io_list_dir_recursive", Rc::new(io_list_dir_recursive)),
        ("io_create_dir", Rc::new(io_create_dir)),
        ("io_create_dir_all", Rc::new(io_create_dir_all)),
        ("io_remove_file", Rc::new(io_remove_file)),
        ("io_remove_dir", Rc::new(io_remove_dir)),
        ("io_remove_dir_all", Rc::new(io_remove_dir_all)),
        ("io_copy", Rc::new(io_copy)),
        ("io_rename", Rc::new(io_rename)),
        // cwd / standard paths
        ("io_cwd", Rc::new(io_cwd)),
        ("io_chdir", Rc::new(io_chdir)),
        ("io_temp_dir", Rc::new(io_temp_dir)),
        ("io_home_dir", Rc::new(io_home_dir)),
        // streaming handles
        ("io_open", Rc::new(io_open)),
        ("io_close", Rc::new(io_close)),
        ("io_read", Rc::new(io_read)),
        ("io_read_all", Rc::new(io_read_all)),
        ("io_read_line", Rc::new(io_read_line)),
        ("io_write", Rc::new(io_write)),
        ("io_flush", Rc::new(io_flush)),
        ("io_seek", Rc::new(io_seek)),
        ("io_tell", Rc::new(io_tell)),
        ("io_eof", Rc::new(io_eof)),
        // async background I/O
        ("io_async_read", Rc::new(io_async_read)),
        ("io_async_read_bytes", Rc::new(io_async_read_bytes)),
        ("io_async_write", Rc::new(io_async_write)),
        ("io_async_write_bytes", Rc::new(io_async_write_bytes)),
        ("io_async_copy", Rc::new(io_async_copy)),
        ("io_task_done", Rc::new(io_task_done)),
        ("io_task_poll", Rc::new(io_task_poll)),
        ("io_task_wait", Rc::new(io_task_wait)),
        ("io_task_cancel", Rc::new(io_task_cancel)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use niao_ast::Span;
    use std::fs;

    fn s(v: &str) -> ValueRef {
        Value::String(v.to_string()).ref_cell()
    }

    #[test]
    fn io_roundtrip_text_file() {
        let dir = env::temp_dir().join("niao_io_test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("hello.txt");
        let span = Span::dummy();

        io_write_file(&[s(file.to_str().unwrap()), s("hello world")], span).unwrap();
        let out = io_read_file(&[s(file.to_str().unwrap())], span).unwrap();
        assert_eq!(out.borrow().to_string(), "hello world");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn io_async_read_completes() {
        let dir = env::temp_dir().join("niao_io_async_test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("async.txt");
        fs::write(&file, "async data").unwrap();
        let span = Span::dummy();

        let task = io_async_read(&[s(file.to_str().unwrap())], span).unwrap();
        let task_id = match &*task.borrow() {
            Value::Int(n) => *n,
            _ => panic!("expected task id"),
        };
        let result = io_task_wait(&[Value::Int(task_id).ref_cell()], span).unwrap();
        assert_eq!(result.borrow().to_string(), "async data");

        let _ = fs::remove_dir_all(&dir);
    }
}
