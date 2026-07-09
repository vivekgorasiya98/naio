//! Classic ML builtins.

use super::common::*;
use super::handles::{alloc_handle, with_handle_mut, NmlHandle};
use crate::{NativeFn, NiaoResult, Value, ValueRef};
use niao_ast::Span;
use niao_classic::{DecisionTree, KMeans, LogisticRegression, RandomForest};
use std::rc::Rc;

pub fn nml_kmeans(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 4, "nml_kmeans", span)?;
    let data = float_array_arg(args, 0, "nml_kmeans", span)?;
    let n = int_arg(args, 1, "nml_kmeans", span)? as usize;
    let dims = int_arg(args, 2, "nml_kmeans", span)? as usize;
    let k = int_arg(args, 3, "nml_kmeans", span)? as usize;
    let data_f32: Vec<f32> = data.iter().map(|&x| x as f32).collect();
    let mut km = KMeans::new(k, 100);
    km.fit(&data_f32, n, dims);
    Ok(ok_handle(alloc_handle(NmlHandle::KMeans(km))))
}

pub fn nml_kmeans_predict(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 4, "nml_kmeans_predict", span)?;
    let id = nml_handle_arg(args, 0, "nml_kmeans_predict", span)?;
    let data = float_array_arg(args, 1, "nml_kmeans_predict", span)?;
    let n = int_arg(args, 2, "nml_kmeans_predict", span)? as usize;
    let _dims = int_arg(args, 3, "nml_kmeans_predict", span)? as usize;
    let data_f32: Vec<f32> = data.iter().map(|&x| x as f32).collect();
    with_handle_mut(id, "nml_kmeans_predict", span, |h| {
        let NmlHandle::KMeans(km) = h else {
            return Err("expected kmeans handle".into());
        };
        let labels = km.predict(&data_f32, n);
        Ok(labels.iter().map(|&l| l as i64).collect::<Vec<_>>())
    })
    .map(|labels| Value::IntArray(labels).ref_cell())
}

pub fn nml_logistic_fit(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 5, "nml_logistic_fit", span)?;
    let x = float_array_arg(args, 0, "nml_logistic_fit", span)?;
    let y = float_array_arg(args, 1, "nml_logistic_fit", span)?;
    let n = int_arg(args, 2, "nml_logistic_fit", span)? as usize;
    let dims = int_arg(args, 3, "nml_logistic_fit", span)? as usize;
    let epochs = int_arg(args, 4, "nml_logistic_fit", span)? as usize;
    let mut lr = LogisticRegression::new(dims, 0.01, epochs);
    let xf: Vec<f32> = x.iter().map(|&v| v as f32).collect();
    let yf: Vec<f32> = y.iter().map(|&v| v as f32).collect();
    lr.fit(&xf, &yf, n, dims);
    Ok(ok_handle(alloc_handle(NmlHandle::Logistic(lr))))
}

pub fn nml_decision_tree(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 5, "nml_decision_tree", span)?;
    let x = float_array_arg(args, 0, "nml_decision_tree", span)?;
    let y = float_array_arg(args, 1, "nml_decision_tree", span)?;
    let n = int_arg(args, 2, "nml_decision_tree", span)? as usize;
    let dims = int_arg(args, 3, "nml_decision_tree", span)? as usize;
    let max_depth = int_arg(args, 4, "nml_decision_tree", span)? as usize;
    let mut tree = DecisionTree::new(max_depth, 2);
    let xf: Vec<f32> = x.iter().map(|&v| v as f32).collect();
    let yf: Vec<f32> = y.iter().map(|&v| v as f32).collect();
    tree.fit(&xf, &yf, n, dims);
    Ok(ok_handle(alloc_handle(NmlHandle::DecisionTree(tree))))
}

pub fn nml_random_forest(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 6, "nml_random_forest", span)?;
    let x = float_array_arg(args, 0, "nml_random_forest", span)?;
    let y = float_array_arg(args, 1, "nml_random_forest", span)?;
    let n = int_arg(args, 2, "nml_random_forest", span)? as usize;
    let dims = int_arg(args, 3, "nml_random_forest", span)? as usize;
    let n_trees = int_arg(args, 4, "nml_random_forest", span)? as usize;
    let max_depth = int_arg(args, 5, "nml_random_forest", span)? as usize;
    let mut rf = RandomForest::new(n_trees, max_depth, 2);
    let xf: Vec<f32> = x.iter().map(|&v| v as f32).collect();
    let yf: Vec<f32> = y.iter().map(|&v| v as f32).collect();
    rf.fit(&xf, &yf, n, dims);
    Ok(ok_handle(alloc_handle(NmlHandle::RandomForest(rf))))
}

pub fn classic_builtins() -> Vec<(&'static str, NativeFn)> {
    vec![
        ("nml_kmeans", Rc::new(nml_kmeans)),
        ("nml_kmeans_predict", Rc::new(nml_kmeans_predict)),
        ("nml_logistic_fit", Rc::new(nml_logistic_fit)),
        ("nml_decision_tree", Rc::new(nml_decision_tree)),
        ("nml_random_forest", Rc::new(nml_random_forest)),
    ]
}
