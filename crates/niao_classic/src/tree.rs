//! CART decision tree and random forest.

use rand::seq::SliceRandom;
use rand::Rng;
use rayon::prelude::*;

#[derive(Clone, Debug)]
enum Node {
    Leaf { value: f32, class: usize },
    Split {
        feature: usize,
        threshold: f32,
        left: Box<Node>,
        right: Box<Node>,
    },
}

#[derive(Clone)]
pub struct DecisionTree {
    root: Option<Node>,
    pub max_depth: usize,
    pub min_samples: usize,
}

impl DecisionTree {
    pub fn new(max_depth: usize, min_samples: usize) -> Self {
        Self {
            root: None,
            max_depth,
            min_samples,
        }
    }

    pub fn fit(&mut self, x: &[f32], y: &[f32], n: usize, dims: usize) {
        let indices: Vec<usize> = (0..n).collect();
        self.root = Some(build_tree(
            x,
            y,
            &indices,
            dims,
            0,
            self.max_depth,
            self.min_samples,
        ));
    }

    pub fn predict(&self, x: &[f32], n: usize, dims: usize) -> Vec<usize> {
        let root = self.root.as_ref().unwrap();
        (0..n)
            .map(|i| {
                let row = &x[i * dims..(i + 1) * dims];
                predict_node(root, row)
            })
            .collect()
    }
}

#[derive(Clone)]
pub struct RandomForest {
    pub trees: Vec<DecisionTree>,
    pub n_trees: usize,
}

impl RandomForest {
    pub fn new(n_trees: usize, max_depth: usize, min_samples: usize) -> Self {
        Self {
            trees: (0..n_trees)
                .map(|_| DecisionTree::new(max_depth, min_samples))
                .collect(),
            n_trees,
        }
    }

    pub fn fit(&mut self, x: &[f32], y: &[f32], n: usize, dims: usize) {
        let mut rng = rand::thread_rng();
        self.trees.par_iter_mut().for_each(|tree| {
            let mut local_rng = rand::thread_rng();
            let sample: Vec<usize> = (0..n).map(|_| local_rng.gen_range(0..n)).collect();
            let n_feat = (dims as f32).sqrt() as usize;
            let mut feats: Vec<usize> = (0..dims).collect();
            feats.shuffle(&mut local_rng);
            feats.truncate(n_feat.max(1));
            let mut sub_x = vec![0.0f32; n * n_feat];
            for (i, &si) in sample.iter().enumerate() {
                for (fi, &f) in feats.iter().enumerate() {
                    sub_x[i * n_feat + fi] = x[si * dims + f];
                }
            }
            let sub_y: Vec<f32> = sample.iter().map(|&i| y[i]).collect();
            tree.fit(&sub_x, &sub_y, n, n_feat);
        });
    }

    pub fn predict(&self, x: &[f32], n: usize, dims: usize) -> Vec<usize> {
        (0..n)
            .map(|i| {
                let row = &x[i * dims..(i + 1) * dims];
                let mut votes = vec![0usize; 16];
                for tree in &self.trees {
                    if let Some(root) = &tree.root {
                        let c = predict_node(root, row);
                        if c < votes.len() {
                            votes[c] += 1;
                        }
                    }
                }
                votes
                    .iter()
                    .enumerate()
                    .max_by_key(|(_, v)| *v)
                    .map(|(i, _)| i)
                    .unwrap_or(0)
            })
            .collect()
    }
}

fn build_tree(
    x: &[f32],
    y: &[f32],
    indices: &[usize],
    dims: usize,
    depth: usize,
    max_depth: usize,
    min_samples: usize,
) -> Node {
    let n = indices.len();
    if n < min_samples || depth >= max_depth {
        return leaf(y, indices);
    }
    let mut best_feat = 0;
    let mut best_thr = 0.0f32;
    let mut best_gain = -1.0f32;
    let parent_imp = gini(y, indices);
    for f in 0..dims {
        let mut values: Vec<f32> = indices.iter().map(|&i| x[i * dims + f]).collect();
        values.sort_by(|a, b| a.partial_cmp(b).unwrap());
        for thr in values.windows(2) {
            let t = (thr[0] + thr[1]) / 2.0;
            let (left, right): (Vec<_>, Vec<_>) = indices
                .iter()
                .copied()
                .partition(|&i| x[i * dims + f] <= t);
            if left.is_empty() || right.is_empty() {
                continue;
            }
            let gain = parent_imp
                - left.len() as f32 / n as f32 * gini(y, &left)
                - right.len() as f32 / n as f32 * gini(y, &right);
            if gain > best_gain {
                best_gain = gain;
                best_feat = f;
                best_thr = t;
            }
        }
    }
    if best_gain <= 0.0 {
        return leaf(y, indices);
    }
    let (left_idx, right_idx): (Vec<_>, Vec<_>) = indices
        .iter()
        .copied()
        .partition(|&i| x[i * dims + best_feat] <= best_thr);
    Node::Split {
        feature: best_feat,
        threshold: best_thr,
        left: Box::new(build_tree(
            x,
            y,
            &left_idx,
            dims,
            depth + 1,
            max_depth,
            min_samples,
        )),
        right: Box::new(build_tree(
            x,
            y,
            &right_idx,
            dims,
            depth + 1,
            max_depth,
            min_samples,
        )),
    }
}

fn leaf(y: &[f32], indices: &[usize]) -> Node {
    let sum: f32 = indices.iter().map(|&i| y[i]).sum();
    let mean = sum / indices.len().max(1) as f32;
    let class = mean.round() as usize;
    Node::Leaf {
        value: mean,
        class,
    }
}

fn gini(y: &[f32], indices: &[usize]) -> f32 {
    if indices.is_empty() {
        return 0.0;
    }
    let mut counts = [0usize; 16];
    for &i in indices {
        let c = y[i].round() as usize;
        if c < counts.len() {
            counts[c] += 1;
        }
    }
    let n = indices.len() as f32;
    let mut imp = 1.0f32;
    for &c in &counts {
        if c > 0 {
            let p = c as f32 / n;
            imp -= p * p;
        }
    }
    imp
}

fn predict_node(node: &Node, row: &[f32]) -> usize {
    match node {
        Node::Leaf { class, .. } => *class,
        Node::Split {
            feature,
            threshold,
            left,
            right,
        } => {
            if row[*feature] <= *threshold {
                predict_node(left, row)
            } else {
                predict_node(right, row)
            }
        }
    }
}
