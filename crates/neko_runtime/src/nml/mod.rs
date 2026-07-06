//! NML — Neko Machine Learning (native builtins).

mod autograd;
mod bridge;
mod classic;
mod common;
mod data;
mod graph;
mod handles;
mod train;

use crate::{NativeFn, NekoResult, RuntimeError, Value, ValueRef};
use common::*;
use handles::{alloc_handle, with_handle, with_handle_mut, NmlHandle};
use neko_ast::Span;
use neko_errors::codes;
use neko_ml::layer::Layer;
use neko_ml::model::Sequential;
use neko_tensor::{global_device, set_global_device, Device, Tensor};
use std::collections::HashMap;
use std::rc::Rc;

pub const MODULE_NAME: &str = "nml";
pub const MODULE_PATHS: &[&str] = &["nml", "std/nml"];

fn current_device() -> Device {
    global_device()
}

fn nml_set_device(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "nml_set_device", span)?;
    let s = string_arg(args, 0, "nml_set_device", span)?;
    let dev = Device::parse(&s).ok_or_else(|| {
        RuntimeError::at(
            span,
            codes::E1975_NML_DEVICE,
            format!("unknown device '{s}'"),
        )
    })?;
    set_global_device(dev);
    Ok(Value::Nil.ref_cell())
}

fn nml_device_count(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 0, "nml_device_count", span)?;
    let cuda = neko_tensor::cuda_device_count() as i64;
    Ok(ok_int(cuda))
}

fn nml_sync(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 0, "nml_sync", span)?;
    Ok(Value::Nil.ref_cell())
}

fn nml_zeros(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "nml_zeros", span)?;
    let shape = shape_from_arg(args, 0, "nml_zeros", span)?;
    let t = Tensor::zeros(&shape, current_device())
        .map_err(|e| RuntimeError::at(span, codes::E1973_NML_SHAPE, e.to_string()))?;
    Ok(ok_handle(alloc_handle(NmlHandle::Tensor(t))))
}

fn nml_ones(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "nml_ones", span)?;
    let shape = shape_from_arg(args, 0, "nml_ones", span)?;
    let t = Tensor::ones(&shape, current_device())
        .map_err(|e| RuntimeError::at(span, codes::E1973_NML_SHAPE, e.to_string()))?;
    Ok(ok_handle(alloc_handle(NmlHandle::Tensor(t))))
}

fn nml_randn(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "nml_randn", span)?;
    let shape = shape_from_arg(args, 0, "nml_randn", span)?;
    let t = Tensor::randn(&shape, current_device())
        .map_err(|e| RuntimeError::at(span, codes::E1973_NML_SHAPE, e.to_string()))?;
    Ok(ok_handle(alloc_handle(NmlHandle::Tensor(t))))
}

fn nml_tensor(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "nml_tensor", span)?;
    let t = bridge::from_packed_float_array(args, span)?;
    Ok(ok_handle(alloc_handle(NmlHandle::Tensor(t))))
}

fn nml_from_ncl(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "nml_from_ncl", span)?;
    let ncl_id = ncl_handle_from_arg(args, 0, "nml_from_ncl", span)?;
    let id = bridge::from_ncl_handle(ncl_id, span)?;
    Ok(ok_handle(id))
}

fn nml_to_float_array(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "nml_to_float_array", span)?;
    let id = nml_handle_arg(args, 0, "nml_to_float_array", span)?;
    with_handle(id, "nml_to_float_array", span, |h| {
        let NmlHandle::Tensor(t) = h else {
            return Err("expected tensor".into());
        };
        bridge::tensor_to_float_array(t)
    })
    .map(|data| Value::FloatArray(data.iter().map(|&x| x as f64).collect()).ref_cell())
}

fn nml_shape(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "nml_shape", span)?;
    let id = nml_handle_arg(args, 0, "nml_shape", span)?;
    with_handle(id, "nml_shape", span, |h| {
        let NmlHandle::Tensor(t) = h else {
            return Err("expected tensor".into());
        };
        Ok(t.shape.iter().map(|&d| d as i64).collect::<Vec<_>>())
    })
    .map(|s| Value::IntArray(s).ref_cell())
}

fn nml_reshape(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "nml_reshape", span)?;
    let id = nml_handle_arg(args, 0, "nml_reshape", span)?;
    let new_shape = shape_from_arg(args, 1, "nml_reshape", span)?;
    let t = tensor_from_handle(id, "nml_reshape", span)?;
    let out = t.reshape(&new_shape)
        .map_err(|e| RuntimeError::at(span, codes::E1973_NML_SHAPE, e.to_string()))?;
    Ok(ok_handle(alloc_handle(NmlHandle::Tensor(out))))
}

fn nml_to_device(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "nml_to_device", span)?;
    let id = nml_handle_arg(args, 0, "nml_to_device", span)?;
    let s = string_arg(args, 1, "nml_to_device", span)?;
    let dev = Device::parse(&s).ok_or_else(|| {
        RuntimeError::at(span, codes::E1975_NML_DEVICE, format!("unknown device '{s}'"))
    })?;
    with_handle(id, "nml_to_device", span, |h| {
        let NmlHandle::Tensor(t) = h else {
            return Err("expected tensor".into());
        };
        let out = t.to_device(dev).map_err(|e| e.to_string())?;
        Ok(alloc_handle(NmlHandle::Tensor(out)))
    })
    .map(ok_handle)
}

fn nml_matmul(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "nml_matmul", span)?;
    let a_id = nml_handle_arg(args, 0, "nml_matmul", span)?;
    let b_id = nml_handle_arg(args, 1, "nml_matmul", span)?;
    matmul_handles(a_id, b_id, span).map(ok_handle)
}

pub fn matmul_handles(a_id: u64, b_id: u64, span: Span) -> Result<u64, RuntimeError> {
    let a = tensor_from_handle(a_id, "nml_matmul", span)?;
    let b = tensor_from_handle(b_id, "nml_matmul", span)?;
    let out = a.matmul(&b)
        .map_err(|e| RuntimeError::at(span, codes::E1971_NML_ERROR, e.to_string()))?;
    Ok(alloc_handle(NmlHandle::Tensor(out)))
}

fn nml_add(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    binary_op(args, span, "nml_add", |a, b| a.add(b))
}

fn nml_sub(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    binary_op(args, span, "nml_sub", |a, b| a.sub(b))
}

fn nml_mul(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    binary_op(args, span, "nml_mul", |a, b| a.mul(b))
}

fn nml_relu(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "nml_relu", span)?;
    let id = nml_handle_arg(args, 0, "nml_relu", span)?;
    unary_op(id, span, "nml_relu", |t| t.relu())
}

fn nml_softmax(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 1, 2, "nml_softmax", span)?;
    let id = nml_handle_arg(args, 0, "nml_softmax", span)?;
    let dim = if args.len() == 2 {
        int_arg(args, 1, "nml_softmax", span)? as usize
    } else {
        1
    };
    unary_op(id, span, "nml_softmax", |t| t.softmax(dim))
}

fn nml_linear(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "nml_linear", span)?;
    let in_f = int_arg(args, 0, "nml_linear", span)? as usize;
    let out_f = int_arg(args, 1, "nml_linear", span)? as usize;
    let layer = Layer::linear(in_f, out_f, current_device())
        .map_err(|e| RuntimeError::at(span, codes::E1971_NML_ERROR, e.to_string()))?;
    let model = Sequential::new(vec![layer], current_device());
    Ok(ok_handle(alloc_handle(NmlHandle::Model(model))))
}

fn nml_relu_layer(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 0, "nml_relu_layer", span)?;
    let model = Sequential::new(vec![Layer::relu()], current_device());
    Ok(ok_handle(alloc_handle(NmlHandle::Model(model))))
}

fn nml_conv2d_layer(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 3, "nml_conv2d_layer", span)?;
    let in_c = int_arg(args, 0, "nml_conv2d_layer", span)? as usize;
    let out_c = int_arg(args, 1, "nml_conv2d_layer", span)? as usize;
    let k = int_arg(args, 2, "nml_conv2d_layer", span)? as usize;
    let layer = Layer::conv2d(in_c, out_c, (k, k), current_device())
        .map_err(|e| RuntimeError::at(span, codes::E1971_NML_ERROR, e.to_string()))?;
    let model = Sequential::new(vec![layer], current_device());
    Ok(ok_handle(alloc_handle(NmlHandle::Model(model))))
}

fn nml_batch_norm2d(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "nml_batch_norm2d", span)?;
    let ch = int_arg(args, 0, "nml_batch_norm2d", span)? as usize;
    let layer = Layer::batch_norm2d(ch, current_device())
        .map_err(|e| RuntimeError::at(span, codes::E1971_NML_ERROR, e.to_string()))?;
    let model = Sequential::new(vec![layer], current_device());
    Ok(ok_handle(alloc_handle(NmlHandle::Model(model))))
}

fn nml_sequential(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "nml_sequential", span)?;
    let layers = parse_layer_list(args, 0, span)?;
    let model = Sequential::new(layers, current_device());
    Ok(ok_handle(alloc_handle(NmlHandle::Model(model))))
}

fn nml_forward(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "nml_forward", span)?;
    let model_id = nml_handle_arg(args, 0, "nml_forward", span)?;
    let x_id = nml_handle_arg(args, 1, "nml_forward", span)?;
    forward_handles(model_id, x_id, span).map(ok_handle)
}

pub fn forward_handles(model_id: u64, x_id: u64, span: Span) -> Result<u64, RuntimeError> {
    let x = tensor_from_handle(x_id, "nml_forward", span)?;
    with_handle_mut(model_id, "nml_forward", span, |h| {
        let NmlHandle::Model(m) = h else {
            return Err("expected model".into());
        };
        let out = m.forward(&x).map_err(|e| e.to_string())?;
        Ok(alloc_handle(NmlHandle::Tensor(out)))
    })
}

pub fn backward_step_handles(
    trainer_id: u64,
    pred_id: u64,
    y_id: u64,
    span: Span,
) -> Result<(), RuntimeError> {
    let pred = tensor_from_handle(pred_id, "nml_backward_step", span)?;
    let y = tensor_from_handle(y_id, "nml_backward_step", span)?;
    with_handle_mut(trainer_id, "nml_backward_step", span, |h| {
        let NmlHandle::Trainer(t) = h else {
            return Err("expected trainer".into());
        };
        t.backward_step(&pred, &y).map_err(|e| e.to_string())?;
        Ok(())
    })
}

fn nml_conv2d(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "nml_conv2d", span)?;
    let model_id = nml_handle_arg(args, 0, "nml_conv2d", span)?;
    let x_id = nml_handle_arg(args, 1, "nml_conv2d", span)?;
    forward_handles(model_id, x_id, span).map(ok_handle)
}

fn nml_dataloader(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 3, "nml_dataloader", span)?;
    let x_id = nml_handle_arg(args, 0, "nml_dataloader", span)?;
    let y_id = nml_handle_arg(args, 1, "nml_dataloader", span)?;
    let batch = int_arg(args, 2, "nml_dataloader", span)? as usize;
    let x = tensor_from_handle(x_id, "nml_dataloader", span)?;
    let y = tensor_from_handle(y_id, "nml_dataloader", span)?;
    let loader = neko_ml::dataloader::DataLoader::new(x, y, batch)
        .map_err(|e| RuntimeError::at(span, codes::E1971_NML_ERROR, e.to_string()))?;
    Ok(ok_handle(alloc_handle(NmlHandle::DataLoader(loader))))
}

fn nml_kind(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "nml_kind", span)?;
    let id = nml_handle_arg(args, 0, "nml_kind", span)?;
    Ok(Value::String(handles::type_name_for(id)).ref_cell())
}

fn nml_len(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "nml_len", span)?;
    let id = nml_handle_arg(args, 0, "nml_len", span)?;
    let n = handles::len_for(id).unwrap_or(0) as i64;
    Ok(ok_int(n))
}

fn shape_from_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> Result<Vec<usize>, RuntimeError> {
    let ints = int_array_arg(args, idx, name, span)?;
    Ok(ints.iter().map(|&n| n as usize).collect())
}

pub fn tensor_from_handle(id: u64, name: &str, span: Span) -> Result<Tensor, RuntimeError> {
    with_handle(id, name, span, |h| {
        let NmlHandle::Tensor(t) = h else {
            return Err("expected tensor handle".into());
        };
        Ok(t.clone())
    })
}

fn binary_op<F>(args: &[ValueRef], span: Span, name: &str, op: F) -> NekoResult<ValueRef>
where
    F: Fn(&Tensor, &Tensor) -> Result<Tensor, neko_tensor::TensorError>,
{
    arity(args, 2, name, span)?;
    let a_id = nml_handle_arg(args, 0, name, span)?;
    let b_id = nml_handle_arg(args, 1, name, span)?;
    let a = tensor_from_handle(a_id, name, span)?;
    let b = tensor_from_handle(b_id, name, span)?;
    let out = op(&a, &b).map_err(|e| RuntimeError::at(span, codes::E1971_NML_ERROR, e.to_string()))?;
    Ok(ok_handle(alloc_handle(NmlHandle::Tensor(out))))
}

fn unary_op<F>(id: u64, span: Span, name: &str, op: F) -> NekoResult<ValueRef>
where
    F: Fn(&Tensor) -> Result<Tensor, neko_tensor::TensorError>,
{
    let t = tensor_from_handle(id, name, span)?;
    let out = op(&t).map_err(|e| RuntimeError::at(span, codes::E1971_NML_ERROR, e.to_string()))?;
    Ok(ok_handle(alloc_handle(NmlHandle::Tensor(out))))
}

fn parse_layer_list(args: &[ValueRef], idx: usize, span: Span) -> Result<Vec<Layer>, RuntimeError> {
    match &*args[idx].borrow() {
        Value::Array(items) => {
            let mut layers = Vec::new();
            for item in items {
                let id = match &*item.borrow() {
                    Value::NmlHandle(id) => *id,
                    other => {
                        return Err(RuntimeError::at(
                            span,
                            codes::E1974_NML_TYPE,
                            format!("nml_sequential expects model handles, got {}", other.type_name()),
                        ));
                    }
                };
                with_handle(id, "nml_sequential", span, |h| {
                    let NmlHandle::Model(m) = h else {
                        return Err("expected layer model".into());
                    };
                    layers.extend(m.layers.clone());
                    Ok(())
                })?;
            }
            Ok(layers)
        }
        other => Err(RuntimeError::at(
            span,
            codes::E1974_NML_TYPE,
            format!("nml_sequential expects array, got {}", other.type_name()),
        )),
    }
}

fn core_builtins() -> Vec<(&'static str, NativeFn)> {
    vec![
        ("nml_set_device", Rc::new(nml_set_device)),
        ("nml_device_count", Rc::new(nml_device_count)),
        ("nml_sync", Rc::new(nml_sync)),
        ("nml_zeros", Rc::new(nml_zeros)),
        ("nml_ones", Rc::new(nml_ones)),
        ("nml_randn", Rc::new(nml_randn)),
        ("nml_tensor", Rc::new(nml_tensor)),
        ("nml_from_ncl", Rc::new(nml_from_ncl)),
        ("nml_to_float_array", Rc::new(nml_to_float_array)),
        ("nml_shape", Rc::new(nml_shape)),
        ("nml_reshape", Rc::new(nml_reshape)),
        ("nml_to_device", Rc::new(nml_to_device)),
        ("nml_matmul", Rc::new(nml_matmul)),
        ("nml_add", Rc::new(nml_add)),
        ("nml_sub", Rc::new(nml_sub)),
        ("nml_mul", Rc::new(nml_mul)),
        ("nml_relu", Rc::new(nml_relu)),
        ("nml_softmax", Rc::new(nml_softmax)),
        ("nml_linear", Rc::new(nml_linear)),
        ("nml_relu_layer", Rc::new(nml_relu_layer)),
        ("nml_conv2d_layer", Rc::new(nml_conv2d_layer)),
        ("nml_batch_norm2d", Rc::new(nml_batch_norm2d)),
        ("nml_sequential", Rc::new(nml_sequential)),
        ("nml_forward", Rc::new(nml_forward)),
        ("nml_conv2d", Rc::new(nml_conv2d)),
        ("nml_dataloader", Rc::new(nml_dataloader)),
        ("nml_kind", Rc::new(nml_kind)),
        ("nml_len", Rc::new(nml_len)),
    ]
}

pub fn builtins() -> Vec<(&'static str, NativeFn)> {
    let mut b = core_builtins();
    b.extend(train::train_builtins());
    b.extend(autograd::autograd_builtins());
    b.extend(classic::classic_builtins());
    b.extend(data::data_builtins());
    b.extend(graph::graph_builtins());
    b
}

pub fn namespace() -> Value {
    let mut map = HashMap::new();
    for (name, f) in builtins() {
        let short = name.strip_prefix("nml_").unwrap_or(name);
        map.insert(short.to_string(), Value::NativeFunction(f).ref_cell());
    }
    Value::Object(map)
}

pub use handles::{display_for, handle_count, len_for, type_name_for};
