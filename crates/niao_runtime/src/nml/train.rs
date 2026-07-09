//! Training and tuning builtins.

use super::common::*;
use super::handles::{alloc_handle, with_handle, with_handle_mut, NmlHandle};
use crate::{NativeFn, NiaoResult, RuntimeError, Value, ValueRef};
use niao_ast::Span;
use niao_errors::codes;
use niao_ml::dataloader::DataLoader;
use niao_ml::loss::LossKind;
use niao_ml::optimizer::OptimizerKind;
use niao_ml::trainer::Trainer;
use niao_ml::tuning::{EarlyStopping, GridSearch, RandomSearch, SearchResult, trainer_from_params};
use std::collections::HashMap;
use std::rc::Rc;

fn parse_loss(s: &str) -> LossKind {
    match s.to_lowercase().as_str() {
        "mse" => LossKind::Mse,
        "bce" | "binary_cross_entropy" => LossKind::BinaryCrossEntropy,
        _ => LossKind::CrossEntropy,
    }
}

fn parse_batch_opts(args: &[ValueRef], _span: Span) -> Result<(usize, bool), RuntimeError> {
  let mut batch = 32usize;
  let mut shuffle = true;
  if args.len() >= 4 {
    match &*args[3].borrow() {
      Value::Object(map) => {
        if let Some(bs) = map.get("batch_size") {
          batch = match &*bs.borrow() {
            Value::Int(n) => *n as usize,
            _ => batch,
          };
        }
        if let Some(sh) = map.get("shuffle") {
          shuffle = match &*sh.borrow() {
            Value::Bool(b) => *b,
            _ => shuffle,
          };
        }
      }
      _ => {}
    }
  }
  let _ = shuffle;
  Ok((batch, shuffle))
}

pub fn nml_trainer(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity_range(args, 3, 4, "nml_trainer", span)?;
    let model_id = nml_handle_arg(args, 0, "nml_trainer", span)?;
    let opt_name = string_arg(args, 1, "nml_trainer", span)?;
    let loss_name = string_arg(args, 2, "nml_trainer", span)?;
    let lr = if args.len() == 4 {
        float_arg(args, 3, "nml_trainer", span)? as f32
    } else {
        0.001
    };
    with_handle(model_id, "nml_trainer", span, |h| {
        let NmlHandle::Model(model) = h else {
            return Err("expected model handle".into());
        };
        let optimizer = match opt_name.to_lowercase().as_str() {
            "sgd" => OptimizerKind::sgd(lr),
            "adamw" => OptimizerKind::adamw(lr),
            "rmsprop" => OptimizerKind::Rmsprop {
                lr,
                alpha: 0.99,
                eps: 1e-8,
            },
            _ => OptimizerKind::adam(lr),
        };
        let trainer = Trainer::new(
            model.clone(),
            optimizer,
            parse_loss(&loss_name),
            model.device,
        );
        Ok(alloc_handle(NmlHandle::Trainer(trainer)))
    })
    .map(ok_handle)
}

pub fn nml_train_epoch(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity_range(args, 3, 4, "nml_train_epoch", span)?;
    let trainer_id = nml_handle_arg(args, 0, "nml_train_epoch", span)?;
    let (batch, _shuffle) = parse_batch_opts(args, span)?;

    // If arg2 is dataloader handle, use it directly
    let use_loader = match &*args[2].borrow() {
        Value::NmlHandle(id) => with_handle(*id, "nml_train_epoch", span, |h| {
            Ok(matches!(h, NmlHandle::DataLoader(_)))
        })
        .unwrap_or(false),
        _ => false,
    };

    if use_loader {
        let loader_id = nml_handle_arg(args, 2, "nml_train_epoch", span)?;
        return with_handle_mut(trainer_id, "nml_train_epoch", span, |th| {
            let NmlHandle::Trainer(trainer) = th else {
                return Err("expected trainer handle".into());
            };
            let mut loader = with_handle(loader_id, "nml_train_epoch", span, |h| {
                let NmlHandle::DataLoader(l) = h else {
                    return Err("expected dataloader".into());
                };
                Ok(l.clone())
            })
            .map_err(|e| e.to_string())?;
            let metrics = trainer.train_epoch(&mut loader).map_err(|e| e.to_string())?;
            Ok(metrics.loss)
        })
        .map(|loss| ok_float(loss));
    }

    let x_id = nml_handle_arg(args, 1, "nml_train_epoch", span)?;
    let y_id = nml_handle_arg(args, 2, "nml_train_epoch", span)?;
    with_handle_mut(trainer_id, "nml_train_epoch", span, |th| {
        let NmlHandle::Trainer(trainer) = th else {
            return Err("expected trainer handle".into());
        };
        let (x, y) = extract_xy(x_id, y_id, span)?;
        let mut loader = DataLoader::new(x, y, batch).map_err(|e| e.to_string())?;
        let metrics = trainer.train_epoch(&mut loader).map_err(|e| e.to_string())?;
        Ok(metrics.loss)
    })
    .map(|loss| ok_float(loss))
}

pub fn nml_plot_training(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "nml_plot_training", span)?;
    let trainer_id = nml_handle_arg(args, 0, "nml_plot_training", span)?;
    let history = with_handle(trainer_id, "nml_plot_training", span, |h| {
        let NmlHandle::Trainer(t) = h else {
            return Err("expected trainer".into());
        };
        Ok(t.loss_history.clone())
    })?;
    let chart_args = vec![Value::FloatArray(history.iter().map(|&x| x as f64).collect()).ref_cell()];
    crate::nvis::builtins()
        .into_iter()
        .find(|(n, _)| *n == "nvis_line")
        .map(|(_, f)| f(&chart_args, span))
        .ok_or_else(|| RuntimeError::at(span, codes::E1971_NML_ERROR, "nvis_line unavailable"))?
}

pub fn nml_eval(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 3, "nml_eval", span)?;
    let trainer_id = nml_handle_arg(args, 0, "nml_eval", span)?;
    let x_id = nml_handle_arg(args, 1, "nml_eval", span)?;
    let y_id = nml_handle_arg(args, 2, "nml_eval", span)?;
    with_handle(trainer_id, "nml_eval", span, |h| {
        let NmlHandle::Trainer(trainer) = h else {
            return Err("expected trainer".into());
        };
        let (x, y) = extract_xy(x_id, y_id, span)?;
        let mut loader = DataLoader::new(x, y, 64).map_err(|e| e.to_string())?;
        let m = trainer.validate(&mut loader).map_err(|e| e.to_string())?;
        Ok((m.loss, m.accuracy))
    })
    .map(|(loss, acc)| {
        let mut map = HashMap::new();
        map.insert("loss".to_string(), Value::Float(loss as f64).ref_cell());
        map.insert("accuracy".to_string(), Value::Float(acc as f64).ref_cell());
        Value::Object(map).ref_cell()
    })
}

pub fn nml_save(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 2, "nml_save", span)?;
    let id = nml_handle_arg(args, 0, "nml_save", span)?;
    let path = string_arg(args, 1, "nml_save", span)?;
    with_handle(id, "nml_save", span, |h| match h {
        NmlHandle::Model(m) => {
            niao_ml::checkpoint::save_model(std::path::Path::new(&path), m)
                .map_err(|e| e.to_string())?;
            Ok(())
        }
        NmlHandle::Trainer(t) => {
            niao_ml::checkpoint::save_model(std::path::Path::new(&path), &t.model)
                .map_err(|e| e.to_string())?;
            Ok(())
        }
        _ => Err("expected model or trainer".into()),
    })?;
    Ok(Value::Nil.ref_cell())
}

pub fn nml_load(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "nml_load", span)?;
    let path = string_arg(args, 0, "nml_load", span)?;
    let model = niao_ml::checkpoint::load_model(
        std::path::Path::new(&path),
        niao_tensor::global_device(),
    )
    .map_err(|e| RuntimeError::at(span, codes::E1971_NML_ERROR, e.to_string()))?;
    Ok(ok_handle(alloc_handle(NmlHandle::Model(model))))
}

pub fn nml_grid_search(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 4, "nml_grid_search", span)?;
    let model_id = nml_handle_arg(args, 0, "nml_grid_search", span)?;
    let x_id = nml_handle_arg(args, 1, "nml_grid_search", span)?;
    let y_id = nml_handle_arg(args, 2, "nml_grid_search", span)?;
    let epochs = int_arg(args, 3, "nml_grid_search", span)? as usize;
    let (x, y) = extract_xy(x_id, y_id, span).map_err(|e| {
        RuntimeError::at(span, codes::E1971_NML_ERROR, e)
    })?;
    let base_model = with_handle(model_id, "nml_grid_search", span, |h| {
        let NmlHandle::Model(m) = h else {
            return Err("expected model".into());
        };
        Ok(m.clone())
    })?;
    let mut grid = HashMap::new();
    grid.insert("lr".to_string(), vec![0.01, 0.001, 0.0001]);
    let search = GridSearch::new(grid);
    let device = base_model.device;
    let loader = DataLoader::new(x.clone(), y.clone(), 32).map_err(|e| {
        RuntimeError::at(span, codes::E1971_NML_ERROR, e.to_string())
    })?;
    let val_loader = DataLoader::new(x, y, 64).map_err(|e| {
        RuntimeError::at(span, codes::E1971_NML_ERROR, e.to_string())
    })?;
    let results = search.run(
        |params| trainer_from_params(base_model.clone(), params, LossKind::Mse, device),
        loader,
        val_loader,
        epochs,
    );
    let best = results.first().cloned().unwrap_or(SearchResult {
        params: HashMap::new(),
        train_loss: 0.0,
        val_loss: 0.0,
    });
    let mut map = HashMap::new();
    map.insert(
        "val_loss".to_string(),
        Value::Float(best.val_loss as f64).ref_cell(),
    );
    map.insert(
        "lr".to_string(),
        Value::Float(best.params.get("lr").copied().unwrap_or(0.001) as f64).ref_cell(),
    );
    Ok(Value::Object(map).ref_cell())
}

pub fn nml_random_search(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 4, "nml_random_search", span)?;
    let model_id = nml_handle_arg(args, 0, "nml_random_search", span)?;
    let x_id = nml_handle_arg(args, 1, "nml_random_search", span)?;
    let y_id = nml_handle_arg(args, 2, "nml_random_search", span)?;
    let epochs = int_arg(args, 3, "nml_random_search", span)? as usize;
    let (x, y) = extract_xy(x_id, y_id, span).map_err(|e| {
        RuntimeError::at(span, codes::E1971_NML_ERROR, e)
    })?;
    let base_model = with_handle(model_id, "nml_random_search", span, |h| {
        let NmlHandle::Model(m) = h else {
            return Err("expected model".into());
        };
        Ok(m.clone())
    })?;
    let mut space = HashMap::new();
    space.insert("lr".to_string(), (0.0001, 0.01));
    let search = RandomSearch::new(space, 5);
    let device = base_model.device;
    let loader = DataLoader::new(x.clone(), y.clone(), 32).map_err(|e| {
        RuntimeError::at(span, codes::E1971_NML_ERROR, e.to_string())
    })?;
    let val_loader = DataLoader::new(x, y, 64).map_err(|e| {
        RuntimeError::at(span, codes::E1971_NML_ERROR, e.to_string())
    })?;
    let results = search.run(
        |params| trainer_from_params(base_model.clone(), params, LossKind::Mse, device),
        loader,
        val_loader,
        epochs,
    );
    let best = results.first().cloned().unwrap_or(SearchResult {
        params: HashMap::new(),
        train_loss: 0.0,
        val_loss: 0.0,
    });
    Ok(ok_float(best.val_loss))
}

pub fn nml_early_stopping(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity_range(args, 2, 3, "nml_early_stopping", span)?;
    let patience = int_arg(args, 0, "nml_early_stopping", span)? as usize;
    let min_delta = float_arg(args, 1, "nml_early_stopping", span)? as f32;
    let val_loss = if args.len() == 3 {
        float_arg(args, 2, "nml_early_stopping", span)? as f32
    } else {
        0.0
    };
    thread_local! {
        static ES: std::cell::RefCell<Option<EarlyStopping>> = std::cell::RefCell::new(None);
    }
    let stop = ES.with(|es| {
        let mut guard = es.borrow_mut();
        if guard.is_none() {
            *guard = Some(EarlyStopping::new(patience, min_delta));
        }
        guard.as_mut().unwrap().step(0, val_loss)
    });
    Ok(ok_bool(stop))
}

pub fn nml_memory_budget(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    if args.is_empty() {
        return crate::mem::stats_object(span);
    }
    arity(args, 1, "nml_memory_budget", span)?;
    let bytes = int_arg(args, 0, "nml_memory_budget", span)? as usize;
    niao_tensor::pool::set_memory_budget(bytes);
    Ok(Value::Nil.ref_cell())
}

pub fn nml_explain(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 2, "nml_explain", span)?;
    let model_id = nml_handle_arg(args, 0, "nml_explain", span)?;
    let x_id = nml_handle_arg(args, 1, "nml_explain", span)?;
    let x = super::tensor_from_handle(x_id, "nml_explain", span)?;
    let importances = with_handle(model_id, "nml_explain", span, |h| match h {
        NmlHandle::Model(m) => niao_ml::feature_importance(m, &x).map_err(|e| e.to_string()),
        NmlHandle::Trainer(t) => {
            niao_ml::feature_importance(&t.model, &x).map_err(|e| e.to_string())
        }
        _ => Err("expected model or trainer".into()),
    })?;
    let names: Vec<ValueRef> = importances
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            let mut row = HashMap::new();
            row.insert("feature".to_string(), Value::Int(i as i64).ref_cell());
            row.insert("importance".to_string(), Value::Float(v as f64).ref_cell());
            Value::Object(row).ref_cell()
        })
        .collect();
    Ok(Value::Array(names).ref_cell())
}

pub fn nml_backward_step(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 3, "nml_backward_step", span)?;
    let trainer_id = nml_handle_arg(args, 0, "nml_backward_step", span)?;
    let pred_id = nml_handle_arg(args, 1, "nml_backward_step", span)?;
    let y_id = nml_handle_arg(args, 2, "nml_backward_step", span)?;
    super::backward_step_handles(trainer_id, pred_id, y_id, span)?;
    Ok(Value::Nil.ref_cell())
}

fn extract_xy(
    x_id: u64,
    y_id: u64,
    span: Span,
) -> Result<(niao_tensor::Tensor, niao_tensor::Tensor), String> {
    let x = with_handle(x_id, "nml", span, |h| {
        let NmlHandle::Tensor(t) = h else {
            return Err("expected tensor for features".into());
        };
        Ok(t.clone())
    })
    .map_err(|e| e.to_string())?;
    let y = with_handle(y_id, "nml", span, |h| {
        let NmlHandle::Tensor(t) = h else {
            return Err("expected tensor for labels".into());
        };
        Ok(t.clone())
    })
    .map_err(|e| e.to_string())?;
    Ok((x, y))
}

pub fn train_builtins() -> Vec<(&'static str, NativeFn)> {
    vec![
        ("nml_trainer", Rc::new(nml_trainer)),
        ("nml_train_epoch", Rc::new(nml_train_epoch)),
        ("nml_eval", Rc::new(nml_eval)),
        ("nml_save", Rc::new(nml_save)),
        ("nml_load", Rc::new(nml_load)),
        ("nml_grid_search", Rc::new(nml_grid_search)),
        ("nml_random_search", Rc::new(nml_random_search)),
        ("nml_early_stopping", Rc::new(nml_early_stopping)),
        ("nml_plot_training", Rc::new(nml_plot_training)),
        ("nml_memory_budget", Rc::new(nml_memory_budget)),
        ("nml_explain", Rc::new(nml_explain)),
        ("nml_backward_step", Rc::new(nml_backward_step)),
    ]
}
