//! Graph tensor formats and structural embeddings.

pub mod sparse;

pub use sparse::{
    add_self_loops, from_edge_list, normalize_adj, sparse_matmul, structural_embed, SparseAdj,
};
