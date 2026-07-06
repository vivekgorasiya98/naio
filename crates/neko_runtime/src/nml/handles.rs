//! NML native handle arena.

use neko_ml::model::Sequential;
use neko_ml::trainer::Trainer;
use neko_ml::dataloader::DataLoader;
use neko_tensor::Tensor;
use neko_classic::{KMeans, LogisticRegression, DecisionTree, RandomForest};
use neko_graph::SparseAdj;
use neko_ml::gnn::{GcnLayer, GnnModel, GraphSageLayer};
use neko_ast::Span;
use neko_errors::codes;
use std::cell::RefCell;
use std::collections::HashMap;

#[derive(Clone)]
pub enum NmlHandle {
    Tensor(Tensor),
    Model(Sequential),
    Trainer(Trainer),
    DataLoader(DataLoader),
    KMeans(KMeans),
    Logistic(LogisticRegression),
    DecisionTree(DecisionTree),
    RandomForest(RandomForest),
    SparseAdj(SparseAdj),
    GcnLayer(GcnLayer),
    GraphSageLayer(GraphSageLayer),
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

pub fn is_nml_handle(val: &crate::Value) -> Option<u64> {
    match val {
        crate::Value::NmlHandle(id) => Some(*id),
        _ => None,
    }
}
