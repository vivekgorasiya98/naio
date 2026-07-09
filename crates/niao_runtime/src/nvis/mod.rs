//! Lightweight chart generation (SVG + ASCII).

mod chart;

use crate::{NativeFn, NiaoResult, RuntimeError, Value, ValueRef};
use chart::{BarChart, Chart, HeatmapChart, HistogramChart, LineChart, ScatterChart};
use niao_ast::Span;
use niao_errors::codes;
use std::collections::HashMap;
use std::rc::Rc;

fn arity(args: &[ValueRef], n: usize, name: &str, span: Span) -> Result<(), RuntimeError> {
    if args.len() != n {
        return Err(RuntimeError::at(
            span,
            codes::E1970_NML_ARITY,
            format!("{name}() expects {n} argument(s), got {}", args.len()),
        ));
    }
    Ok(())
}

fn arity_range(args: &[ValueRef], min: usize, max: usize, name: &str, span: Span) -> Result<(), RuntimeError> {
    if args.len() < min || args.len() > max {
        return Err(RuntimeError::at(
            span,
            codes::E1970_NML_ARITY,
            format!("{name}() expects {min}..={max} arguments, got {}", args.len()),
        ));
    }
    Ok(())
}

fn float_array_from_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> Result<Vec<f32>, RuntimeError> {
    match &*args[idx].borrow() {
        Value::FloatArray(a) => Ok(a.iter().map(|&x| x as f32).collect()),
        Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                match &*item.borrow() {
                    Value::Float(f) => out.push(*f as f32),
                    Value::Int(n) => out.push(*n as f32),
                    other => {
                        return Err(RuntimeError::at(
                            span,
                            codes::E1974_NML_TYPE,
                            format!("{name}() expects numeric array, got {}", other.type_name()),
                        ));
                    }
                }
            }
            Ok(out)
        }
        other => Err(RuntimeError::at(
            span,
            codes::E1974_NML_TYPE,
            format!("{name}() expects array, got {}", other.type_name()),
        )),
    }
}

thread_local! {
    static CHARTS: RefCell<HashMap<u64, Chart>> = RefCell::new(HashMap::new());
    static NEXT_CHART_ID: RefCell<u64> = RefCell::new(1);
}

use std::cell::RefCell;

fn alloc_chart(c: Chart) -> u64 {
    let id = NEXT_CHART_ID.with(|n| {
        let mut next = n.borrow_mut();
        let id = *next;
        *next = id + 1;
        id
    });
    CHARTS.with(|m| m.borrow_mut().insert(id, c));
    id
}

fn nvis_line(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "nvis_line", span)?;
    let data = float_array_from_arg(args, 0, "nvis_line", span)?;
    let title = if args.len() > 1 {
        match &*args[1].borrow() {
            Value::String(s) => s.clone(),
            _ => "Line Chart".into(),
        }
    } else {
        "Line Chart".into()
    };
    let id = alloc_chart(Chart::Line(LineChart { data, title }));
    Ok(Value::Int(id as i64).ref_cell())
}

fn nvis_hist(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity_range(args, 1, 2, "nvis_hist", span)?;
    let data = float_array_from_arg(args, 0, "nvis_hist", span)?;
    let bins = if args.len() == 2 {
        match &*args[1].borrow() {
            Value::Int(n) => *n as usize,
            _ => 10,
        }
    } else {
        10
    };
    let id = alloc_chart(Chart::Hist(HistogramChart { data, bins }));
    Ok(Value::Int(id as i64).ref_cell())
}

fn nvis_scatter(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 2, "nvis_scatter", span)?;
    let x = float_array_from_arg(args, 0, "nvis_scatter", span)?;
    let y = float_array_from_arg(args, 1, "nvis_scatter", span)?;
    let id = alloc_chart(Chart::Scatter(ScatterChart { x, y }));
    Ok(Value::Int(id as i64).ref_cell())
}

fn nvis_heatmap(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 3, "nvis_heatmap", span)?;
    let data = float_array_from_arg(args, 0, "nvis_heatmap", span)?;
    let rows = match &*args[1].borrow() {
        Value::Int(n) => *n as usize,
        _ => return Err(RuntimeError::at(span, codes::E1974_NML_TYPE, "rows must be int")),
    };
    let cols = match &*args[2].borrow() {
        Value::Int(n) => *n as usize,
        _ => return Err(RuntimeError::at(span, codes::E1974_NML_TYPE, "cols must be int")),
    };
    let id = alloc_chart(Chart::Heatmap(HeatmapChart { data, rows, cols }));
    Ok(Value::Int(id as i64).ref_cell())
}

fn nvis_bar(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "nvis_bar", span)?;
    let data = float_array_from_arg(args, 0, "nvis_bar", span)?;
    let id = alloc_chart(Chart::Bar(BarChart { data }));
    Ok(Value::Int(id as i64).ref_cell())
}

fn nvis_save_svg(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 2, "nvis_save_svg", span)?;
    let chart_id = match &*args[0].borrow() {
        Value::Int(n) => *n as u64,
        other => {
            return Err(RuntimeError::at(
                span,
                codes::E1974_NML_TYPE,
                format!("expected chart id, got {}", other.type_name()),
            ));
        }
    };
    let path = match &*args[1].borrow() {
        Value::String(s) => s.clone(),
        other => {
            return Err(RuntimeError::at(
                span,
                codes::E1974_NML_TYPE,
                format!("expected path string, got {}", other.type_name()),
            ));
        }
    };
    let svg = CHARTS.with(|m| {
        m.borrow()
            .get(&chart_id)
            .map(|c| c.to_svg())
            .ok_or_else(|| "invalid chart id".to_string())
    })
    .map_err(|e| RuntimeError::at(span, codes::E1971_NML_ERROR, e))?;
    std::fs::write(&path, svg)
        .map_err(|e| RuntimeError::at(span, codes::E1971_NML_ERROR, e.to_string()))?;
    Ok(Value::Nil.ref_cell())
}

fn nvis_print_ascii(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "nvis_print_ascii", span)?;
    let chart_id = match &*args[0].borrow() {
        Value::Int(n) => *n as u64,
        other => {
            return Err(RuntimeError::at(
                span,
                codes::E1974_NML_TYPE,
                format!("expected chart id, got {}", other.type_name()),
            ));
        }
    };
    let ascii = CHARTS.with(|m| {
        m.borrow()
            .get(&chart_id)
            .map(|c| c.to_ascii())
            .ok_or_else(|| "invalid chart id".to_string())
    })
    .map_err(|e| RuntimeError::at(span, codes::E1971_NML_ERROR, e))?;
    println!("{ascii}");
    Ok(Value::String(ascii).ref_cell())
}

fn nvis_to_csv(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "nvis_to_csv", span)?;
    let chart_id = match &*args[0].borrow() {
        Value::Int(n) => *n as u64,
        other => {
            return Err(RuntimeError::at(
                span,
                codes::E1974_NML_TYPE,
                format!("expected chart id, got {}", other.type_name()),
            ));
        }
    };
    let csv = CHARTS.with(|m| {
        m.borrow()
            .get(&chart_id)
            .map(|c| c.to_csv())
            .ok_or_else(|| "invalid chart id".to_string())
    })
    .map_err(|e| RuntimeError::at(span, codes::E1971_NML_ERROR, e))?;
    Ok(Value::String(csv).ref_cell())
}

pub const MODULE_NAME: &str = "nvis";
pub const MODULE_PATHS: &[&str] = &["nvis", "std/nvis"];

pub fn builtins() -> Vec<(&'static str, NativeFn)> {
    vec![
        ("nvis_line", Rc::new(nvis_line)),
        ("nvis_hist", Rc::new(nvis_hist)),
        ("nvis_scatter", Rc::new(nvis_scatter)),
        ("nvis_heatmap", Rc::new(nvis_heatmap)),
        ("nvis_bar", Rc::new(nvis_bar)),
        ("nvis_save_svg", Rc::new(nvis_save_svg)),
        ("nvis_print_ascii", Rc::new(nvis_print_ascii)),
        ("nvis_to_csv", Rc::new(nvis_to_csv)),
    ]
}

pub fn namespace() -> Value {
    let mut map = HashMap::new();
    for (name, f) in builtins() {
        let short = name.strip_prefix("nvis_").unwrap_or(name);
        map.insert(short.to_string(), Value::NativeFunction(f).ref_cell());
    }
    Value::Object(map)
}
