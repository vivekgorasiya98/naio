//! CSV read/write with typed column inference.

use super::column::Column;
use super::frame::DataFrame;
use super::series::Series;
use crate::StringArray;
use std::fs;
use std::path::Path;

pub fn read_csv(path: &str) -> Result<DataFrame, String> {
    let text = fs::read_to_string(Path::new(path)).map_err(|e| e.to_string())?;
    parse_csv(&text)
}

pub fn parse_csv(text: &str) -> Result<DataFrame, String> {
    let mut lines = text.lines().filter(|l| !l.is_empty());
    let header = lines
        .next()
        .ok_or_else(|| "empty csv".to_string())?;
    let names: Vec<String> = parse_csv_line(header);
    let ncols = names.len();
    let mut cols: Vec<Vec<String>> = vec![Vec::new(); ncols];

    for line in lines {
        let fields = parse_csv_line(line);
        for (i, f) in fields.iter().enumerate() {
            if i < ncols {
                cols[i].push(f.clone());
            }
        }
    }

    let mut series_vec = Vec::new();
    for (name, raw) in names.into_iter().zip(cols) {
        let col = infer_column(&raw);
        series_vec.push(Series::new(name, col));
    }
    DataFrame::new(series_vec)
}

pub fn to_csv(df: &DataFrame) -> String {
    let names = df.column_names();
    let mut out = names.join(",");
    out.push('\n');
    let n = df.len();
    for row in 0..n {
        let cells: Vec<String> = df
            .columns
            .iter()
            .map(|c| cell_str(&c.data, row))
            .collect();
        out.push_str(&cells.join(","));
        out.push('\n');
    }
    out
}

pub fn write_csv(path: &str, df: &DataFrame) -> Result<(), String> {
    fs::write(path, to_csv(df)).map_err(|e| e.to_string())
}

fn parse_csv_line(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if in_quotes => {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    cur.push('"');
                } else {
                    in_quotes = false;
                }
            }
            '"' => in_quotes = true,
            ',' if !in_quotes => {
                fields.push(cur.clone());
                cur.clear();
            }
            _ => cur.push(c),
        }
    }
    fields.push(cur);
    fields
}

fn infer_column(raw: &[String]) -> Column {
    let mut all_int = true;
    let mut all_float = true;
    let mut ints = Vec::with_capacity(raw.len());
    let mut floats = Vec::with_capacity(raw.len());

    for s in raw {
        if s.is_empty() {
            all_int = false;
            all_float = false;
            break;
        }
        if let Ok(n) = s.parse::<i64>() {
            ints.push(n);
            floats.push(n as f64);
        } else if let Ok(f) = s.parse::<f64>() {
            all_int = false;
            floats.push(f);
        } else {
            all_int = false;
            all_float = false;
            break;
        }
    }

    if all_int && !ints.is_empty() {
        Column::Int(ints)
    } else if all_float && !floats.is_empty() {
        Column::Float(floats)
    } else {
        Column::String(StringArray::dense(raw.to_vec()))
    }
}

fn cell_str(col: &Column, row: usize) -> String {
    match col {
        Column::Int(v) => v[row].to_string(),
        Column::Float(v) => v[row].to_string(),
        Column::Bool(v) => (v[row] != 0).to_string(),
        Column::String(s) => {
            let t = s.get(row).unwrap_or_default();
            if t.contains(',') || t.contains('"') {
                format!("\"{}\"", t.replace('"', "\"\""))
            } else {
                t
            }
        }
        Column::Any(v) => v[row].borrow().to_string(),
    }
}
