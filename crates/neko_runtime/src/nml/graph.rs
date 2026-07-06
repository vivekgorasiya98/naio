//! Graph ML builtins (neko_graph + GNN bridge).

use super::common::*;
use super::handles::{alloc_handle, with_handle, NmlHandle};
use crate::dsa::{GraphDs, NativeDs};
use crate::{NativeFn, NekoResult, RuntimeError, Value, ValueRef};
use neko_ast::Span;
use neko_errors::codes;
use neko_graph::{from_edge_list, normalize_adj, structural_embed};
use neko_ml::gnn::{GcnLayer, GraphSageLayer};
use std::cell::RefCell;
use std::rc::Rc;

fn graph_from_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> Result<Rc<RefCell<NativeDs>>, RuntimeError> {
    match &*args[idx].borrow() {
        Value::Native(rc) => Ok(rc.clone()),
        other => Err(RuntimeError::at(
            span,
            codes::E1974_NML_TYPE,
            format!("{name}() expects graph (Native DSA), got {}", other.type_name()),
        )),
    }
}

fn graph_to_sparse(g: &GraphDs) -> neko_graph::SparseAdj {
    from_edge_list(&g.edge_list_f32(), g.n)
}

pub fn nml_graph_from_dsa(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "nml_graph_from_dsa", span)?;
    let rc = graph_from_arg(args, 0, "nml_graph_from_dsa", span)?;
    let adj = {
        let mut b = rc.borrow_mut();
        match &mut *b {
            NativeDs::Graph(g) => graph_to_sparse(g),
            other => {
                return Err(RuntimeError::at(
                    span,
                    codes::E1974_NML_TYPE,
                    format!("nml_graph_from_dsa() expects graph, got {}", other.kind_name()),
                ));
            }
        }
    };
    Ok(ok_handle(alloc_handle(NmlHandle::SparseAdj(adj))))
}

pub fn nml_graph_normalize(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "nml_graph_normalize", span)?;
    let id = nml_handle_arg(args, 0, "nml_graph_normalize", span)?;
    with_handle(id, "nml_graph_normalize", span, |h| {
        let NmlHandle::SparseAdj(adj) = h else {
            return Err("expected sparse adjacency".into());
        };
        let norm = normalize_adj(adj);
        Ok(alloc_handle(NmlHandle::SparseAdj(norm)))
    })
    .map(ok_handle)
}

pub fn nml_gcn_layer(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "nml_gcn_layer", span)?;
    let in_f = int_arg(args, 0, "nml_gcn_layer", span)? as usize;
    let out_f = int_arg(args, 1, "nml_gcn_layer", span)? as usize;
    let layer = GcnLayer::new(in_f, out_f, neko_tensor::global_device())
        .map_err(|e| RuntimeError::at(span, codes::E1971_NML_ERROR, e.to_string()))?;
    Ok(ok_handle(alloc_handle(NmlHandle::GcnLayer(layer))))
}

pub fn nml_graph_forward(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 3, "nml_graph_forward", span)?;
    let model_id = nml_handle_arg(args, 0, "nml_graph_forward", span)?;
    let feat_id = nml_handle_arg(args, 1, "nml_graph_forward", span)?;
    let adj_id = nml_handle_arg(args, 2, "nml_graph_forward", span)?;
    let features = super::tensor_from_handle(feat_id, "nml_graph_forward", span)?;
    let adj = with_handle(adj_id, "nml_graph_forward", span, |h| {
        let NmlHandle::SparseAdj(a) = h else {
            return Err("expected adjacency".into());
        };
        Ok(a.clone())
    })?;
    with_handle(model_id, "nml_graph_forward", span, |model_h| match model_h {
        NmlHandle::GcnLayer(layer) => {
            let out = layer.forward(&features, &adj).map_err(|e| e.to_string())?;
            Ok(alloc_handle(NmlHandle::Tensor(out)))
        }
        NmlHandle::GnnModel(m) => {
            let out = m.forward(&features, &adj).map_err(|e| e.to_string())?;
            Ok(alloc_handle(NmlHandle::Tensor(out)))
        }
        _ => Err("expected GCN or GNN model".into()),
    })
    .map(ok_handle)
}

pub fn nml_graph_embed(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 2, 3, "nml_graph_embed", span)?;
    let adj_id = nml_handle_arg(args, 0, "nml_graph_embed", span)?;
    let dim = int_arg(args, 1, "nml_graph_embed", span)? as usize;
    let seed = if args.len() == 3 {
        int_arg(args, 2, "nml_graph_embed", span)? as u64
    } else {
        42
    };
    with_handle(adj_id, "nml_graph_embed", span, |h| {
        let NmlHandle::SparseAdj(adj) = h else {
            return Err("expected adjacency".into());
        };
        let emb = structural_embed(adj, dim, 100, 10, seed).map_err(|e| e.to_string())?;
        Ok(alloc_handle(NmlHandle::Tensor(emb)))
    })
    .map(ok_handle)
}

pub fn nml_graphsage_layer(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "nml_graphsage_layer", span)?;
    let in_f = int_arg(args, 0, "nml_graphsage_layer", span)? as usize;
    let out_f = int_arg(args, 1, "nml_graphsage_layer", span)? as usize;
    let layer = GraphSageLayer::new(in_f, out_f, neko_tensor::global_device())
        .map_err(|e| RuntimeError::at(span, codes::E1971_NML_ERROR, e.to_string()))?;
    Ok(ok_handle(alloc_handle(NmlHandle::GraphSageLayer(layer))))
}

pub fn graph_builtins() -> Vec<(&'static str, NativeFn)> {
    vec![
        ("nml_graph_from_dsa", Rc::new(nml_graph_from_dsa)),
        ("nml_graph_normalize", Rc::new(nml_graph_normalize)),
        ("nml_gcn_layer", Rc::new(nml_gcn_layer)),
        ("nml_graphsage_layer", Rc::new(nml_graphsage_layer)),
        ("nml_graph_forward", Rc::new(nml_graph_forward)),
        ("nml_graph_embed", Rc::new(nml_graph_embed)),
    ]
}
