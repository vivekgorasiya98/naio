//! Autograd builtins: enable_grad, zero_grad, backward, parameters.

use super::common::*;
use super::handles::{alloc_handle, with_handle, with_handle_mut, NmlHandle};
use crate::{NativeFn, NekoResult, RuntimeError, Value, ValueRef};
use neko_ast::Span;
use neko_errors::codes;
use neko_ml::loss;
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

thread_local! {
    static GRAD_TENSORS: RefCell<HashSet<u64>> = RefCell::new(HashSet::new());
}

pub fn nml_enable_grad(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "nml_enable_grad", span)?;
    let id = nml_handle_arg(args, 0, "nml_enable_grad", span)?;
    with_handle(id, "nml_enable_grad", span, |h| {
        match h {
            NmlHandle::Tensor(_) => Ok(()),
            _ => Err("expected tensor".into()),
        }
    })?;
    GRAD_TENSORS.with(|s| s.borrow_mut().insert(id));
    Ok(Value::NmlHandle(id).ref_cell())
}

pub fn is_grad_enabled(id: u64) -> bool {
    GRAD_TENSORS.with(|s| s.borrow().contains(&id))
}

pub fn nml_zero_grad(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "nml_zero_grad", span)?;
    let id = nml_handle_arg(args, 0, "nml_zero_grad", span)?;
    with_handle_mut(id, "nml_zero_grad", span, |h| match h {
        NmlHandle::Model(m) => {
            m.zero_grad();
            Ok(())
        }
        NmlHandle::Trainer(t) => {
            t.zero_grad();
            Ok(())
        }
        _ => Err("expected model or trainer".into()),
    })?;
    Ok(Value::Nil.ref_cell())
}

pub fn nml_backward(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 3, 4, "nml_backward", span)?;
    let model_id = nml_handle_arg(args, 0, "nml_backward", span)?;
    let pred_id = nml_handle_arg(args, 1, "nml_backward", span)?;
    let y_id = nml_handle_arg(args, 2, "nml_backward", span)?;
    let loss_name = if args.len() == 4 {
        string_arg(args, 3, "nml_backward", span)?
    } else {
        "mse".to_string()
    };
    let loss_fn = match loss_name.to_lowercase().as_str() {
        "cross_entropy" | "ce" => neko_ml::LossKind::CrossEntropy,
        "bce" => neko_ml::LossKind::BinaryCrossEntropy,
        _ => neko_ml::LossKind::Mse,
    };
    let pred = super::tensor_from_handle(pred_id, "nml_backward", span)?;
    let y = super::tensor_from_handle(y_id, "nml_backward", span)?;
    let grad_out = loss::loss_grad(loss_fn, &pred, &y)
        .map_err(|e| RuntimeError::at(span, codes::E1971_NML_ERROR, e.to_string()))?;
    with_handle_mut(model_id, "nml_backward", span, |h| {
        let NmlHandle::Model(m) = h else {
            return Err("expected model".into());
        };
        m.backward(grad_out).map_err(|e| e.to_string())?;
        Ok(())
    })?;
    Ok(Value::Nil.ref_cell())
}

pub fn nml_parameters(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "nml_parameters", span)?;
    let id = nml_handle_arg(args, 0, "nml_parameters", span)?;
    with_handle(id, "nml_parameters", span, |h| {
        let params = match h {
            NmlHandle::Model(m) => m.parameters(),
            NmlHandle::Trainer(t) => t.model.parameters(),
            _ => return Err("expected model or trainer".into()),
        };
        let handles: Vec<ValueRef> = params
            .into_iter()
            .map(|t| {
                let hid = alloc_handle(NmlHandle::Tensor(t.clone()));
                Value::NmlHandle(hid).ref_cell()
            })
            .collect();
        Ok(handles)
    })
    .map(|items| Value::Array(items).ref_cell())
}

pub fn autograd_builtins() -> Vec<(&'static str, NativeFn)> {
    vec![
        ("nml_enable_grad", Rc::new(nml_enable_grad)),
        ("nml_zero_grad", Rc::new(nml_zero_grad)),
        ("nml_backward", Rc::new(nml_backward)),
        ("nml_parameters", Rc::new(nml_parameters)),
    ]
}
