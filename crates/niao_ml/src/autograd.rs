//! Reverse-mode autograd tape.

use niao_tensor::Tensor;
use std::collections::HashMap;

pub type VarId = u64;

#[derive(Clone)]
pub enum OpKind {
    Input,
    Matmul { a: VarId, b: VarId },
    Add { a: VarId, b: VarId },
    Relu { a: VarId },
    Linear { input: VarId, weight: VarId, bias: Option<VarId> },
}

#[derive(Clone)]
pub struct VarNode {
    pub id: VarId,
    pub value: Tensor,
    pub op: OpKind,
    pub parents: Vec<VarId>,
}

#[derive(Default)]
pub struct Graph {
    next_id: u64,
    nodes: HashMap<VarId, VarNode>,
    pub outputs: Vec<VarId>,
}

impl Graph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn input(&mut self, tensor: Tensor) -> VarId {
        let id = self.next_id;
        self.next_id += 1;
        self.nodes.insert(
            id,
            VarNode {
                id,
                value: tensor,
                op: OpKind::Input,
                parents: vec![],
            },
        );
        id
    }

    pub fn get(&self, id: VarId) -> Option<&VarNode> {
        self.nodes.get(&id)
    }

    pub fn get_mut(&mut self, id: VarId) -> Option<&mut VarNode> {
        self.nodes.get_mut(&id)
    }

    pub fn set_value(&mut self, id: VarId, tensor: Tensor) {
        if let Some(n) = self.nodes.get_mut(&id) {
            n.value = tensor;
        }
    }

    pub fn record_matmul(&mut self, a: VarId, b: VarId, result: Tensor) -> VarId {
        let id = self.next_id;
        self.next_id += 1;
        self.nodes.insert(
            id,
            VarNode {
                id,
                value: result,
                op: OpKind::Matmul { a, b },
                parents: vec![a, b],
            },
        );
        id
    }

    pub fn record_add(&mut self, a: VarId, b: VarId, result: Tensor) -> VarId {
        let id = self.next_id;
        self.next_id += 1;
        self.nodes.insert(
            id,
            VarNode {
                id,
                value: result,
                op: OpKind::Add { a, b },
                parents: vec![a, b],
            },
        );
        id
    }

    pub fn record_relu(&mut self, a: VarId, result: Tensor) -> VarId {
        let id = self.next_id;
        self.next_id += 1;
        self.nodes.insert(
            id,
            VarNode {
                id,
                value: result,
                op: OpKind::Relu { a },
                parents: vec![a],
            },
        );
        id
    }

    pub fn node_ids(&self) -> Vec<VarId> {
        self.nodes.keys().copied().collect()
    }
}

pub fn backward(graph: &Graph, loss_id: VarId) -> HashMap<VarId, Vec<f32>> {
    let mut grads: HashMap<VarId, Vec<f32>> = HashMap::new();
    let loss_node = match graph.get(loss_id) {
        Some(n) => n,
        None => return grads,
    };
    let n = loss_node.value.len();
    grads.insert(loss_id, vec![1.0; n]);

    let mut order: Vec<VarId> = graph.node_ids();
    order.reverse();

    for id in order {
        let Some(node) = graph.get(id) else { continue };
        let Some(grad_out) = grads.get(&id).cloned() else { continue };

        match &node.op {
            OpKind::Input => {}
            OpKind::Matmul { a, b } => {
                if let (Some(na), Some(nb)) = (graph.get(*a), graph.get(*b)) {
                    if na.value.shape.len() == 2 && nb.value.shape.len() == 2 {
                        let m = na.value.shape[0];
                        let n = na.value.shape[1];
                        let k = nb.value.shape[1];
                        if let (Ok(a_data), Ok(b_data)) = (na.value.to_cpu(), nb.value.to_cpu()) {
                            let grad_a = niao_tensor::cpu::matmul_f32(&grad_out, &b_data, m, k, n);
                            let bt = transpose2d(&b_data, n, k);
                            let grad_b = niao_tensor::cpu::matmul_f32(&at(&a_data, m, n), &grad_out, n, m, k);
                            acc_grad(&mut grads, *a, grad_a);
                            acc_grad(&mut grads, *b, grad_b);
                            let _ = bt;
                        }
                    }
                }
            }
            OpKind::Add { a, b } => {
                acc_grad(&mut grads, *a, grad_out.clone());
                acc_grad(&mut grads, *b, grad_out);
            }
            OpKind::Relu { a } => {
                if let Ok(fwd) = node.value.to_cpu() {
                    let mut g = vec![0.0; fwd.len()];
                    niao_tensor::cpu::relu_grad_f32(&fwd, &grad_out, &mut g);
                    acc_grad(&mut grads, *a, g);
                }
            }
            OpKind::Linear { input, weight, bias } => {
                if let (Some(ni), Some(nw)) = (graph.get(*input), graph.get(*weight)) {
                    if ni.value.shape.len() == 2 && nw.value.shape.len() == 2 {
                        let rows = ni.value.shape[0];
                        let in_f = ni.value.shape[1];
                        let out_f = nw.value.shape[0];
                        if let (Ok(x), Ok(w)) = (ni.value.to_cpu(), nw.value.to_cpu()) {
                            let grad_w = niao_tensor::cpu::matmul_f32(&grad_out, &x, out_f, rows, in_f);
                            let wt = transpose2d(&w, out_f, in_f);
                            let grad_x = niao_tensor::cpu::matmul_f32(&grad_out, &wt, rows, out_f, in_f);
                            acc_grad(&mut grads, *input, grad_x);
                            acc_grad(&mut grads, *weight, grad_w);
                            if let Some(bid) = bias {
                                let mut grad_b = vec![0.0f32; out_f];
                                for r in 0..rows {
                                    for c in 0..out_f {
                                        grad_b[c] += grad_out[r * out_f + c];
                                    }
                                }
                                acc_grad(&mut grads, *bid, grad_b);
                            }
                        }
                    }
                }
            }
        }
    }
    grads
}

fn acc_grad(grads: &mut HashMap<VarId, Vec<f32>>, id: VarId, g: Vec<f32>) {
    grads
        .entry(id)
        .and_modify(|v| {
            for (a, b) in v.iter_mut().zip(g.iter()) {
                *a += b;
            }
        })
        .or_insert(g);
}

fn transpose2d(data: &[f32], rows: usize, cols: usize) -> Vec<f32> {
    let mut out = vec![0.0; rows * cols];
    for r in 0..rows {
        for c in 0..cols {
            out[c * rows + r] = data[r * cols + c];
        }
    }
    out
}

fn at(data: &[f32], rows: usize, cols: usize) -> Vec<f32> {
    transpose2d(data, rows, cols)
}
