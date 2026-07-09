//! Multi-column aligned table.

use super::column::Column;
use super::series::Series;
use std::collections::HashMap;

#[derive(Clone)]
pub struct DataFrame {
    pub columns: Vec<Series>,
    pub column_index: HashMap<String, usize>,
}

impl DataFrame {
    pub fn new(columns: Vec<Series>) -> Result<Self, String> {
        if columns.is_empty() {
            return Ok(Self {
                columns,
                column_index: HashMap::new(),
            });
        }
        let len = columns[0].len();
        for c in &columns[1..] {
            if c.len() != len {
                return Err(format!(
                    "column length mismatch: expected {len}, got {} for '{}'",
                    c.len(),
                    c.name
                ));
            }
        }
        let mut column_index = HashMap::new();
        for (i, c) in columns.iter().enumerate() {
            if column_index.insert(c.name.clone(), i).is_some() {
                return Err(format!("duplicate column name '{}'", c.name));
            }
        }
        Ok(Self {
            columns,
            column_index,
        })
    }

    pub fn from_map(map: HashMap<String, Column>) -> Result<Self, String> {
        let columns: Vec<Series> = map
            .into_iter()
            .map(|(name, data)| Series::new(name, data))
            .collect();
        Self::new(columns)
    }

    pub fn len(&self) -> usize {
        self.columns.first().map(|c| c.len()).unwrap_or(0)
    }

    pub fn column_count(&self) -> usize {
        self.columns.len()
    }

    pub fn column_names(&self) -> Vec<String> {
        self.columns.iter().map(|c| c.name.clone()).collect()
    }

    pub fn get_column(&self, name: &str) -> Option<&Series> {
        self.column_index.get(name).map(|&i| &self.columns[i])
    }

    pub fn get_column_mut(&mut self, name: &str) -> Option<&mut Series> {
        if let Some(&i) = self.column_index.get(name) {
            Some(&mut self.columns[i])
        } else {
            None
        }
    }

    pub fn set_column(&mut self, series: Series) -> Result<(), String> {
        if !self.columns.is_empty() && series.len() != self.len() {
            return Err(format!(
                "column '{}' length {} does not match frame length {}",
                series.name,
                series.len(),
                self.len()
            ));
        }
        if let Some(&i) = self.column_index.get(&series.name) {
            self.columns[i] = series;
        } else {
            let i = self.columns.len();
            self.column_index.insert(series.name.clone(), i);
            self.columns.push(series);
        }
        Ok(())
    }

    pub fn select_rows(&self, indices: &[usize]) -> Result<Self, String> {
        let cols: Vec<Series> = self
            .columns
            .iter()
            .map(|c| {
                let data = match &c.data {
                    Column::Int(v) => {
                        Column::Int(indices.iter().map(|&i| v[i]).collect())
                    }
                    Column::Float(v) => {
                        Column::Float(indices.iter().map(|&i| v[i]).collect())
                    }
                    Column::Bool(v) => Column::Bool(indices.iter().map(|&i| v[i]).collect()),
                    Column::String(s) => {
                        let dense: Vec<String> = indices
                            .iter()
                            .map(|&i| s.get(i).unwrap_or_default())
                            .collect();
                        Column::String(crate::StringArray::dense(dense))
                    }
                    Column::Any(v) => Column::Any(indices.iter().map(|&i| v[i].clone()).collect()),
                };
                Ok(Series {
                    name: c.name.clone(),
                    data,
                    validity: c.validity.clone(),
                    index: c.index.clone(),
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Self::new(cols)
    }
}
