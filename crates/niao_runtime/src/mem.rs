//! Process and VM memory introspection for benchmarks and diagnostics.

use crate::{NativeFn, NiaoResult, RuntimeError, Value, ValueRef};
use niao_ast::Span;
use niao_errors::codes;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

thread_local! {
    static VM_ARENA_READER: RefCell<Option<Rc<dyn Fn() -> (usize, usize)>>> = const {
        RefCell::new(None)
    };
}

/// Register a callback that returns `(heap_slots, native_slots)` while the VM runs.
pub fn set_vm_arena_reader(reader: Option<Rc<dyn Fn() -> (usize, usize)>>) {
    VM_ARENA_READER.with(|slot| *slot.borrow_mut() = reader);
}

fn vm_arena_stats() -> (usize, usize) {
    VM_ARENA_READER.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(|f| f())
            .unwrap_or((0, 0))
    })
}

#[cfg(windows)]
pub fn process_rss_bytes() -> usize {
    use std::ffi::c_void;

    #[repr(C)]
    struct ProcessMemoryCounters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set: usize,
        working_set: usize,
        quota_peak_paged_pool: usize,
        quota_paged_pool: usize,
        quota_peak_non_paged_pool: usize,
        quota_non_paged_pool: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
    }

    #[link(name = "psapi")]
    extern "system" {
        fn GetCurrentProcess() -> *mut c_void;
        fn GetProcessMemoryInfo(
            process: *mut c_void,
            counters: *mut ProcessMemoryCounters,
            cb: u32,
        ) -> i32;
    }

    unsafe {
        let mut counters = ProcessMemoryCounters {
            cb: std::mem::size_of::<ProcessMemoryCounters>() as u32,
            page_fault_count: 0,
            peak_working_set: 0,
            working_set: 0,
            quota_peak_paged_pool: 0,
            quota_paged_pool: 0,
            quota_peak_non_paged_pool: 0,
            quota_non_paged_pool: 0,
            pagefile_usage: 0,
            peak_pagefile_usage: 0,
        };
        let ok = GetProcessMemoryInfo(
            GetCurrentProcess(),
            &mut counters,
            counters.cb,
        );
        if ok != 0 {
            counters.working_set
        } else {
            0
        }
    }
}

#[cfg(target_os = "linux")]
pub fn process_rss_bytes() -> usize {
    if let Ok(s) = std::fs::read_to_string("/proc/self/statm") {
        if let Some(res) = s.split_whitespace().nth(1) {
            if let Ok(pages) = res.parse::<usize>() {
                return pages * 4096;
            }
        }
    }
    0
}

#[cfg(all(unix, not(target_os = "linux")))]
pub fn process_rss_bytes() -> usize {
    0
}

#[cfg(not(any(windows, unix)))]
pub fn process_rss_bytes() -> usize {
    0
}

#[derive(Clone, Copy, Debug, Default)]
struct MemSnapshot {
    rss_bytes: i64,
    vm_heap_slots: i64,
    vm_native_slots: i64,
    nml_handles: i64,
    ncl_handles: i64,
    tensor_budget_bytes: i64,
    tensor_tracked_bytes: i64,
}

impl MemSnapshot {
    fn current() -> Self {
        let (vm_heap, vm_native) = vm_arena_stats();
        let budget = niao_tensor::pool::memory_budget();
        let tracked = niao_tensor::pool::memory_used();
        Self {
            rss_bytes: process_rss_bytes() as i64,
            vm_heap_slots: vm_heap as i64,
            vm_native_slots: vm_native as i64,
            nml_handles: super::nml::handle_count() as i64,
            ncl_handles: super::ncl::handle_count() as i64,
            tensor_budget_bytes: budget as i64,
            tensor_tracked_bytes: tracked as i64,
        }
    }

    fn from_object(map: &HashMap<String, ValueRef>, span: Span) -> Result<Self, RuntimeError> {
        Ok(Self {
            rss_bytes: int_field(map, "rss_bytes", span)?,
            vm_heap_slots: int_field(map, "vm_heap_slots", span)?,
            vm_native_slots: int_field(map, "vm_native_slots", span)?,
            nml_handles: int_field(map, "nml_handles", span)?,
            ncl_handles: int_field(map, "ncl_handles", span)?,
            tensor_budget_bytes: int_field(map, "tensor_budget_bytes", span)?,
            tensor_tracked_bytes: int_field(map, "tensor_tracked_bytes", span)?,
        })
    }

    fn to_map(&self) -> HashMap<String, ValueRef> {
        let mut map = HashMap::new();
        map.insert("rss_bytes".into(), Value::Int(self.rss_bytes).ref_cell());
        map.insert(
            "vm_heap_slots".into(),
            Value::Int(self.vm_heap_slots).ref_cell(),
        );
        map.insert(
            "vm_native_slots".into(),
            Value::Int(self.vm_native_slots).ref_cell(),
        );
        map.insert("nml_handles".into(), Value::Int(self.nml_handles).ref_cell());
        map.insert("ncl_handles".into(), Value::Int(self.ncl_handles).ref_cell());
        map.insert(
            "tensor_budget_bytes".into(),
            Value::Int(self.tensor_budget_bytes).ref_cell(),
        );
        map.insert(
            "tensor_tracked_bytes".into(),
            Value::Int(self.tensor_tracked_bytes).ref_cell(),
        );
        map
    }
}

fn int_field(
    map: &HashMap<String, ValueRef>,
    key: &str,
    span: Span,
) -> Result<i64, RuntimeError> {
    let Some(val) = map.get(key) else {
        return Ok(-1);
    };
    match &*val.borrow() {
        Value::Int(n) => Ok(*n),
        other => Err(RuntimeError::at(
            span,
            codes::E1974_NML_TYPE,
            format!("niao_mem_* expected int field '{key}', got {}", other.type_name()),
        )),
    }
}

fn snapshot_from_arg(val: &ValueRef, name: &str, span: Span) -> Result<MemSnapshot, RuntimeError> {
    match &*val.borrow() {
        Value::Object(map) => MemSnapshot::from_object(map, span),
        other => Err(RuntimeError::at(
            span,
            codes::E1974_NML_TYPE,
            format!("{name} expects object, got {}", other.type_name()),
        )),
    }
}

pub fn format_bytes(n: i64) -> String {
    if n < 0 {
        return "n/a".into();
    }
    let n = n as u64;
    if n < 1024 {
        return format!("{n} B");
    }
    let kb = n / 1024;
    if kb < 1024 {
        return format!("{kb} KB");
    }
    let mb = kb / 1024;
    let kb_rem = kb % 1024;
    if kb_rem == 0 {
        format!("{mb} MB")
    } else {
        format!("{mb} MB {kb_rem} KB")
    }
}

fn format_signed_bytes(n: i64) -> String {
    if n == 0 {
        "0 B".into()
    } else if n > 0 {
        format!("+{}", format_bytes(n))
    } else {
        format!("-{}", format_bytes(-n))
    }
}

fn format_snapshot_line(s: &MemSnapshot) -> String {
    let mut line = format!("rss {}", format_bytes(s.rss_bytes));
    line.push_str(&format!(
        ", heap {}, native {}, nml {}, ncl {}",
        s.vm_heap_slots, s.vm_native_slots, s.nml_handles, s.ncl_handles
    ));
    if s.tensor_budget_bytes > 0 {
        line.push_str(&format!(
            ", tensor {}/{}",
            format_bytes(s.tensor_tracked_bytes),
            format_bytes(s.tensor_budget_bytes)
        ));
    }
    line
}

pub fn collect_stats() -> HashMap<String, ValueRef> {
    MemSnapshot::current().to_map()
}

fn niao_mem_stats(_args: &[ValueRef], _span: Span) -> NiaoResult<ValueRef> {
    Ok(Value::Object(collect_stats()).ref_cell())
}

fn niao_mem_format(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    let snap = if args.is_empty() {
        MemSnapshot::current()
    } else if args.len() == 1 {
        snapshot_from_arg(&args[0], "niao_mem_format", span)?
    } else {
        return Err(RuntimeError::at(
            span,
            codes::E1974_NML_TYPE,
            format!(
                "niao_mem_format expects 0 or 1 argument(s), got {}",
                args.len()
            ),
        )
        .into());
    };
    Ok(Value::String(format_snapshot_line(&snap)).ref_cell())
}

fn niao_mem_diff(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    if args.len() != 2 {
        return Err(RuntimeError::at(
            span,
            codes::E1974_NML_TYPE,
            format!("niao_mem_diff expects 2 argument(s), got {}", args.len()),
        )
        .into());
    }
    let before = snapshot_from_arg(&args[0], "niao_mem_diff", span)?;
    let after = snapshot_from_arg(&args[1], "niao_mem_diff", span)?;
    let rss_delta = after.rss_bytes - before.rss_bytes;
    let heap_delta = after.vm_heap_slots - before.vm_heap_slots;
    let native_delta = after.vm_native_slots - before.vm_native_slots;
    let nml_delta = after.nml_handles - before.nml_handles;
    let ncl_delta = after.ncl_handles - before.ncl_handles;
    let mut map = HashMap::new();
    map.insert("rss_delta".into(), Value::Int(rss_delta).ref_cell());
    map.insert("heap_delta".into(), Value::Int(heap_delta).ref_cell());
    map.insert("native_delta".into(), Value::Int(native_delta).ref_cell());
    map.insert("nml_delta".into(), Value::Int(nml_delta).ref_cell());
    map.insert("ncl_delta".into(), Value::Int(ncl_delta).ref_cell());
    map.insert(
        "rss_delta_fmt".into(),
        Value::String(format_signed_bytes(rss_delta)).ref_cell(),
    );
    map.insert(
        "before_line".into(),
        Value::String(format_snapshot_line(&before)).ref_cell(),
    );
    map.insert(
        "after_line".into(),
        Value::String(format_snapshot_line(&after)).ref_cell(),
    );
    Ok(Value::Object(map).ref_cell())
}

pub fn builtins() -> Vec<(&'static str, NativeFn)> {
    vec![
        ("niao_mem_stats", Rc::new(niao_mem_stats)),
        ("niao_mem_format", Rc::new(niao_mem_format)),
        ("niao_mem_diff", Rc::new(niao_mem_diff)),
    ]
}

pub fn stats_object(_span: Span) -> Result<ValueRef, RuntimeError> {
    Ok(Value::Object(collect_stats()).ref_cell())
}
