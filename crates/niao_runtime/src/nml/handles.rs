//! NML native handle arena.

use niao_ml::model::Sequential;
use niao_ml::trainer::Trainer;
use niao_ml::dataloader::DataLoader;
use niao_tensor::Tensor;
use niao_classic::{KMeans, LogisticRegression, DecisionTree, RandomForest};
use niao_graph::SparseAdj;
use niao_ml::gnn::{GcnLayer, GnnModel, GraphSageLayer};
use niao_ast::Span;
use niao_errors::codes;
use std::cell::RefCell;
use std::collections::HashMap;

#[derive(Clone)]
pub enum NmlHandle {
    Tensor(Tensor),
    Model(Sequential),
    Trainer(Trainer),
    DataLoader(DataLoader),
    KMeans(KMeans),
    Logistic(#[allow(dead_code)] LogisticRegression),
    DecisionTree(#[allow(dead_code)] DecisionTree),
    RandomForest(RandomForest),
    SparseAdj(SparseAdj),
    GcnLayer(GcnLayer),
    GraphSageLayer(GraphSageLayer),
    #[allow(dead_code)]
    GnnModel(GnnModel),
}

impl NmlHandle {
    pub fn kind_name(&self) -> &'static str {
        match self {
            NmlHandle::Tensor(_) => "nml_tensor",
            NmlHandle::Model(_) => "nml_model",
            NmlHandle::Trainer(_) => "nml_trainer",
            NmlHandle::DataLoader(_) => "nml_dataloader",
            NmlHandle::KMeans(_) => "nml_kmeans",
            NmlHandle::Logistic(_) => "nml_logistic",
            NmlHandle::DecisionTree(_) => "nml_decision_tree",
            NmlHandle::RandomForest(_) => "nml_random_forest",
            NmlHandle::SparseAdj(_) => "nml_sparse_adj",
            NmlHandle::GcnLayer(_) => "nml_gcn_layer",
            NmlHandle::GraphSageLayer(_) => "nml_graphsage_layer",
            NmlHandle::GnnModel(_) => "nml_gnn_model",
        }
    }

    pub fn display(&self) -> String {
        match self {
            NmlHandle::Tensor(t) => format!("Tensor{:?} device={}", t.shape, t.device),
            NmlHandle::Model(m) => format!("Model[{} layers]", m.layers.len()),
            NmlHandle::Trainer(_) => "Trainer".into(),
            NmlHandle::DataLoader(d) => format!("DataLoader[batch={}]", d.batch_size),
            NmlHandle::KMeans(k) => format!("KMeans[k={}]", k.k),
            NmlHandle::Logistic(_) => "LogisticRegression".into(),
            NmlHandle::DecisionTree(_) => "DecisionTree".into(),
            NmlHandle::RandomForest(r) => format!("RandomForest[{} trees]", r.n_trees),
            NmlHandle::SparseAdj(a) => format!("SparseAdj[n={} nnz={}]", a.n, a.nnz()),
            NmlHandle::GcnLayer(l) => format!("GcnLayer[{}->{}]", l.in_features, l.out_features),
            NmlHandle::GraphSageLayer(l) => format!("GraphSage[{}->{}]", l.in_features, l.out_features),
            NmlHandle::GnnModel(m) => format!("GnnModel[{} layers]", m.layers.len()),
        }
    }

    pub fn len(&self) -> usize {
        match self {
            NmlHandle::Tensor(t) => t.len(),
            NmlHandle::DataLoader(d) => d.features.shape[0],
            _ => 1,
        }
    }
}

thread_local! {
    static NEXT_ID: RefCell<u64> = RefCell::new(1);
    static HANDLES: RefCell<HashMap<u64, NmlHandle>> = RefCell::new(HashMap::new());
}

pub fn handle_count() -> usize {
    HANDLES.with(|m| m.borrow().len())
}

pub fn alloc_handle(h: NmlHandle) -> u64 {
    let id = NEXT_ID.with(|n| {
        let mut next = n.borrow_mut();
        let id = *next;
        *next = id + 1;
        id
    });
    HANDLES.with(|m| m.borrow_mut().insert(id, h));
    id
}

pub fn with_handle<F, R>(id: u64, name: &str, span: Span, f: F) -> Result<R, crate::RuntimeError>
where
    F: FnOnce(&NmlHandle) -> Result<R, String>,
{
    let h = HANDLES.with(|m| {
        m.borrow()
            .get(&id)
            .cloned()
            .ok_or_else(|| {
                crate::RuntimeError::at(
                    span,
                    codes::E1972_NML_INVALID_HANDLE,
                    format!("{name}(): invalid NML handle {id}"),
                )
            })
    })?;
    f(&h).map_err(|msg| {
        crate::RuntimeError::at(span, codes::E1971_NML_ERROR, format!("{name}(): {msg}"))
    })
}

pub fn with_handle_mut<F, R>(id: u64, name: &str, span: Span, f: F) -> Result<R, crate::RuntimeError>
where
    F: FnOnce(&mut NmlHandle) -> Result<R, String>,
{
    let mut h = HANDLES.with(|m| {
        m.borrow_mut().remove(&id).ok_or_else(|| {
            crate::RuntimeError::at(
                span,
                codes::E1972_NML_INVALID_HANDLE,
                format!("{name}(): invalid NML handle {id}"),
            )
        })
    })?;
    let result = f(&mut h).map_err(|msg| {
        crate::RuntimeError::at(span, codes::E1971_NML_ERROR, format!("{name}(): {msg}"))
    });
    HANDLES.with(|m| m.borrow_mut().insert(id, h));
    result
}

pub fn type_name_for(id: u64) -> String {
    HANDLES.with(|m| {
        m.borrow()
            .get(&id)
            .map(|h| h.kind_name().to_string())
            .unwrap_or_else(|| "nml_handle".into())
    })
}

pub fn display_for(id: u64) -> String {
    HANDLES.with(|m| {
        m.borrow()
            .get(&id)
            .map(|h| h.display())
            .unwrap_or_else(|| format!("nml_handle[{id}]"))
    })
}

pub fn len_for(id: u64) -> Option<usize> {
    HANDLES.with(|m| m.borrow().get(&id).map(|h| h.len()))
}

/// GNN forward without cloning layer/features/adj handles (hot path for graph ML).
pub fn graph_forward_handles(
    model_id: u64,
    feat_id: u64,
    adj_id: u64,
    name: &str,
    span: Span,
) -> Result<u64, crate::RuntimeError> {
    let out = HANDLES.with(|m| {
        let map = m.borrow();
        let model_h = map.get(&model_id).ok_or_else(|| {
            crate::RuntimeError::at(
                span,
                codes::E1972_NML_INVALID_HANDLE,
                format!("{name}(): invalid NML handle {model_id}"),
            )
        })?;
        let feat_h = map.get(&feat_id).ok_or_else(|| {
            crate::RuntimeError::at(
                span,
                codes::E1972_NML_INVALID_HANDLE,
                format!("{name}(): invalid NML handle {feat_id}"),
            )
        })?;
        let adj_h = map.get(&adj_id).ok_or_else(|| {
            crate::RuntimeError::at(
                span,
                codes::E1972_NML_INVALID_HANDLE,
                format!("{name}(): invalid NML handle {adj_id}"),
            )
        })?;

        let features = match feat_h {
            NmlHandle::Tensor(t) => t,
            _ => {
                return Err(crate::RuntimeError::at(
                    span,
                    codes::E1971_NML_ERROR,
                    format!("{name}(): expected tensor features"),
                ));
            }
        };
        let adj = match adj_h {
            NmlHandle::SparseAdj(a) => a,
            _ => {
                return Err(crate::RuntimeError::at(
                    span,
                    codes::E1971_NML_ERROR,
                    format!("{name}(): expected adjacency"),
                ));
            }
        };
        match model_h {
            NmlHandle::GcnLayer(layer) => layer.forward(features, adj).map_err(|e| {
                crate::RuntimeError::at(span, codes::E1971_NML_ERROR, format!("{name}(): {e}"))
            }),
            NmlHandle::GnnModel(m) => m.forward(features, adj).map_err(|e| {
                crate::RuntimeError::at(span, codes::E1971_NML_ERROR, format!("{name}(): {e}"))
            }),
            _ => Err(crate::RuntimeError::at(
                span,
                codes::E1971_NML_ERROR,
                format!("{name}(): expected GCN or GNN model"),
            )),
        }
    })?;
    Ok(alloc_handle(NmlHandle::Tensor(out)))
}
