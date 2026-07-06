//! NCL native handles — Series, DataFrame, NDArray, GroupBy.

use super::frame::DataFrame;
use super::groupby::GroupBy;
use super::ndarray::NDArray;
use super::series::Series;
use neko_ast::Span;
use neko_errors::codes;
use std::cell::RefCell;
use std::collections::HashMap;

#[derive(Clone)]
pub enum NclHandle {
    Series(Series),
    DataFrame(DataFrame),
    NDArray(NDArray),
    GroupBy(GroupBy),
}

impl NclHandle {
    pub fn kind_name(&self) -> &'static str {
        match self {
            NclHandle::Series(_) => "ncl_series",
            NclHandle::DataFrame(_) => "ncl_dataframe",
            NclHandle::NDArray(_) => "ncl_ndarray",
            NclHandle::GroupBy(_) => "ncl_groupby",
        }
    }

    pub fn len(&self) -> usize {
        match self {
            NclHandle::Series(s) => s.len(),
            NclHandle::DataFrame(df) => df.len(),
            NclHandle::NDArray(a) => a.len(),
            NclHandle::GroupBy(g) => g.len(),
        }
    }

    pub fn display(&self) -> String {
        match self {
            NclHandle::Series(s) => format!("Series[{}] name={}", s.len(), s.name),
            NclHandle::DataFrame(df) => {
                format!("DataFrame[{} x {}]", df.len(), df.column_count())
            }
            NclHandle::NDArray(a) => format!("NDArray{:?} dtype={}", a.shape, a.dtype.name()),
            NclHandle::GroupBy(g) => format!("GroupBy[{} groups]", g.group_count()),
        }
    }
}

thread_local! {
    static NEXT_ID: RefCell<u64> = RefCell::new(1);
    static HANDLES: RefCell<HashMap<u64, NclHandle>> = RefCell::new(HashMap::new());
}

pub fn handle_count() -> usize {
    HANDLES.with(|m| m.borrow().len())
}

pub fn alloc_handle(h: NclHandle) -> u64 {
    let id = NEXT_ID.with(|n| {
        let mut next = n.borrow_mut();
        let id = *next;
        *next = id + 1;
        id
    });
    HANDLES.with(|m| {
        m.borrow_mut().insert(id, h);
    });
    id
}

pub fn remove_handle(id: u64) -> Option<NclHandle> {
    HANDLES.with(|m| m.borrow_mut().remove(&id))
}

/// Export tabular NCL handles as plain `Value` trees for JSON and printing.
pub fn handle_to_json_value(id: u64) -> Option<crate::Value> {
    let h = HANDLES.with(|m| m.borrow().get(&id).cloned())?;
    match h {
        NclHandle::Series(s) => Some(s.data.to_value_array()),
        NclHandle::DataFrame(df) => {
            let mut map = std::collections::HashMap::new();
            for col in &df.columns {
                map.insert(col.name.clone(), col.data.to_value_array().ref_cell());
            }
            Some(crate::Value::Object(map))
        }
        _ => None,
    }
}

pub fn with_handle<F, R>(id: u64, name: &str, span: Span, f: F) -> Result<R, crate::RuntimeError>
where
    F: FnOnce(&NclHandle) -> Result<R, String>,
{
    let h = HANDLES.with(|m| {
        m.borrow()
            .get(&id)
            .cloned()
            .ok_or_else(|| {
                crate::RuntimeError::at(
                    span,
                    codes::E1962_NCL_INVALID_HANDLE,
                    format!("{name}(): invalid NCL handle {id}"),
                )
            })
    })?;
    f(&h).map_err(|msg| {
        crate::RuntimeError::at(span, codes::E1961_NCL_ERROR, format!("{name}(): {msg}"))
    })
}

pub fn with_handle_mut<F, R>(id: u64, name: &str, span: Span, f: F) -> Result<R, crate::RuntimeError>
where
    F: FnOnce(&mut NclHandle) -> Result<R, String>,
{
    let mut h = HANDLES.with(|m| {
        m.borrow_mut().remove(&id).ok_or_else(|| {
            crate::RuntimeError::at(
                span,
                codes::E1962_NCL_INVALID_HANDLE,
                format!("{name}(): invalid NCL handle {id}"),
            )
        })
    })?;
    let result = f(&mut h).map_err(|msg| {
        crate::RuntimeError::at(span, codes::E1961_NCL_ERROR, format!("{name}(): {msg}"))
    });
    HANDLES.with(|m| {
        m.borrow_mut().insert(id, h);
    });
    result
}

pub fn is_ncl_handle(val: &crate::Value) -> Option<u64> {
    match val {
        crate::Value::NclHandle(id) => Some(*id),
        _ => None,
    }
}

pub fn type_name_for(id: u64) -> String {
    HANDLES.with(|m| {
        m.borrow()
            .get(&id)
            .map(|h| h.kind_name().to_string())
            .unwrap_or_else(|| "ncl_handle".into())
    })
}

pub fn display_for(id: u64) -> String {
    HANDLES.with(|m| {
        m.borrow()
            .get(&id)
            .map(|h| h.display())
            .unwrap_or_else(|| format!("ncl_handle[{id}]"))
    })
}

pub fn len_for(id: u64) -> Option<usize> {
    HANDLES.with(|m| m.borrow().get(&id).map(|h| h.len()))
}
