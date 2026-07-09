//! Declarative preprocessing pipeline DAG.

use crate::normalize::{normalize_fit_transform, standardize_fit_transform, Normalizer};
use crate::split::train_test_split;
use niao_tensor::Tensor;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct PipelineSpec {
    pub steps: Vec<PipelineStepSpec>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "op")]
pub enum PipelineStepSpec {
    #[serde(rename = "split")]
    Split { ratio: f32, seed: u64 },
    #[serde(rename = "standardize")]
    Standardize,
    #[serde(rename = "normalize")]
    Normalize,
    #[serde(rename = "one_hot")]
    OneHot { classes: usize },
}

#[derive(Debug, Clone)]
pub enum PipelineStep {
    Split { ratio: f32, seed: u64 },
    Standardize,
    Normalize,
    OneHot { classes: usize },
}

#[derive(Debug, Clone)]
pub struct Pipeline {
    pub steps: Vec<PipelineStep>,
    pub normalizer: Option<Normalizer>,
}

#[derive(Debug, Clone)]
pub struct PipelineOutput {
    pub x_train: Tensor,
    pub y_train: Tensor,
    pub x_val: Tensor,
    pub y_val: Tensor,
}

impl Pipeline {
    pub fn from_spec(spec: &PipelineSpec) -> Self {
        let steps = spec
            .steps
            .iter()
            .map(|s| match s {
                PipelineStepSpec::Split { ratio, seed } => PipelineStep::Split {
                    ratio: *ratio,
                    seed: *seed,
                },
                PipelineStepSpec::Standardize => PipelineStep::Standardize,
                PipelineStepSpec::Normalize => PipelineStep::Normalize,
                PipelineStepSpec::OneHot { classes } => PipelineStep::OneHot {
                    classes: *classes,
                },
            })
            .collect();
        Self {
            steps,
            normalizer: None,
        }
    }

    pub fn run(&mut self, x: &Tensor, y: &Tensor) -> Result<PipelineOutput, niao_tensor::TensorError> {
        let mut x_cur = x.clone();
        let mut y_cur = y.clone();
        let mut split_out = None;
        for step in &self.steps {
            match step {
                PipelineStep::Split { ratio, seed } => {
                    let s = train_test_split(&x_cur, &y_cur, *ratio, *seed)?;
                    x_cur = s.x_train.clone();
                    y_cur = s.y_train.clone();
                    split_out = Some(s);
                }
                PipelineStep::Standardize => {
                    let (norm, xt) = standardize_fit_transform(&x_cur)?;
                    self.normalizer = Some(norm);
                    x_cur = xt;
                }
                PipelineStep::Normalize => {
                    let (norm, xt) = normalize_fit_transform(&x_cur)?;
                    self.normalizer = Some(norm);
                    x_cur = xt;
                }
                PipelineStep::OneHot { classes } => {
                    let labels: Vec<i64> = y_cur.to_cpu()?.iter().map(|&v| v as i64).collect();
                    y_cur = crate::one_hot_encode(&labels, *classes)?;
                }
            }
        }
        if let Some(s) = split_out {
            Ok(PipelineOutput {
                x_train: x_cur,
                y_train: y_cur,
                x_val: s.x_val,
                y_val: s.y_val,
            })
        } else {
            Ok(PipelineOutput {
                x_train: x_cur,
                y_train: y_cur,
                x_val: x.clone(),
                y_val: y.clone(),
            })
        }
    }
}
