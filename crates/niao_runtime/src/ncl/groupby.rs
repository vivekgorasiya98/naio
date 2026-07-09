//! Split-apply-combine grouping.

use super::column::Column;
use super::frame::DataFrame;
use super::series::Series;
use ahash::RandomState;
use std::collections::HashMap;

type KeyMap = HashMap<i64, Vec<usize>, RandomState>;

#[derive(Clone)]
pub struct GroupBy {
    pub key_name: String,
    pub groups: KeyMap,
    pub frame: DataFrame,
}

impl GroupBy {
    pub fn new(frame: &DataFrame, key: &str) -> Result<Self, String> {
        let col = frame
            .get_column(key)
            .ok_or_else(|| format!("column '{key}' not found"))?;
        let mut groups: KeyMap = HashMap::with_hasher(RandomState::new());
        match &col.data {
            Column::Int(v) => {
                for (i, &k) in v.iter().enumerate() {
                    groups.entry(k).or_default().push(i);
                }
            }
            Column::String(s) => {
                let mut str_map: HashMap<String, Vec<usize>> = HashMap::new();
                for i in 0..s.len() {
                    let k = s.get(i).unwrap_or_default();
                    str_map.entry(k).or_default().push(i);
                }
                let mut id = 0i64;
                let mut key_ids: HashMap<String, i64> = HashMap::new();
                for (k, indices) in str_map {
                    let kid = *key_ids.entry(k).or_insert_with(|| {
                        let v = id;
                        id += 1;
                        v
                    });
                    groups.insert(kid, indices);
                }
            }
            _ => return Err("groupby key must be int or string column".into()),
        }
        Ok(Self {
            key_name: key.to_string(),
            groups,
            frame: frame.clone(),
        })
    }

    pub fn len(&self) -> usize {
        self.frame.len()
    }

    pub fn group_count(&self) -> usize {
        self.groups.len()
    }

    pub fn agg_sum(&self, col_name: &str) -> Result<Series, String> {
        let col = self
            .frame
            .get_column(col_name)
            .ok_or_else(|| format!("column '{col_name}' not found"))?;
        let mut keys = Vec::new();
        let mut vals = Vec::new();
        let mut group_keys: Vec<i64> = self.groups.keys().copied().collect();
        group_keys.sort();
        for k in group_keys {
            let indices = &self.groups[&k];
            let sum = match &col.data {
                Column::Int(v) => {
                    let s: i64 = indices.iter().map(|&i| v[i]).sum();
                    keys.push(k);
                    vals.push(s as f64);
                    continue;
                }
                Column::Float(v) => indices.iter().map(|&i| v[i]).sum::<f64>(),
                _ => return Err("agg sum requires numeric column".into()),
            };
            keys.push(k);
            vals.push(sum);
        }
        Ok(Series::from_float_array(format!("{}_sum", col_name), vals))
    }

    pub fn agg_mean(&self, col_name: &str) -> Result<Series, String> {
        let col = self
            .frame
            .get_column(col_name)
            .ok_or_else(|| format!("column '{col_name}' not found"))?;
        let mut vals = Vec::new();
        let mut group_keys: Vec<i64> = self.groups.keys().copied().collect();
        group_keys.sort();
        for k in group_keys {
            let indices = &self.groups[&k];
            let mean = match &col.data {
                Column::Int(v) => {
                    let s: i64 = indices.iter().map(|&i| v[i]).sum();
                    s as f64 / indices.len() as f64
                }
                Column::Float(v) => {
                    indices.iter().map(|&i| v[i]).sum::<f64>() / indices.len() as f64
                }
                _ => return Err("agg mean requires numeric column".into()),
            };
            vals.push(mean);
        }
        Ok(Series::from_float_array(format!("{}_mean", col_name), vals))
    }

    pub fn agg_count(&self) -> Series {
        let mut group_keys: Vec<i64> = self.groups.keys().copied().collect();
        group_keys.sort();
        let counts: Vec<i64> = group_keys
            .iter()
            .map(|k| self.groups[k].len() as i64)
            .collect();
        Series::from_int_array("count", counts)
    }
}
