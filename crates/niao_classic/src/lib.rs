//! Classic ML algorithms on CPU tensors.

pub mod kmeans;
pub mod logistic;
pub mod tree;

pub use kmeans::KMeans;
pub use logistic::LogisticRegression;
pub use tree::{DecisionTree, RandomForest};
