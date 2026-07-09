//! Logistic regression via SGD.

#[derive(Clone)]
pub struct LogisticRegression {
    pub weights: Vec<f32>,
    pub bias: f32,
    pub lr: f32,
    pub epochs: usize,
}

impl LogisticRegression {
    pub fn new(dims: usize, lr: f32, epochs: usize) -> Self {
        Self {
            weights: vec![0.0; dims],
            bias: 0.0,
            lr,
            epochs,
        }
    }

    pub fn fit(&mut self, x: &[f32], y: &[f32], n: usize, dims: usize) {
        for _ in 0..self.epochs {
            for i in 0..n {
                let row = &x[i * dims..(i + 1) * dims];
                let pred = sigmoid(dot(row, &self.weights) + self.bias);
                let err = pred - y[i];
                for d in 0..dims {
                    self.weights[d] -= self.lr * err * row[d];
                }
                self.bias -= self.lr * err;
            }
        }
    }

    pub fn predict_proba(&self, x: &[f32], n: usize, dims: usize) -> Vec<f32> {
        (0..n)
            .map(|i| {
                let row = &x[i * dims..(i + 1) * dims];
                sigmoid(dot(row, &self.weights) + self.bias)
            })
            .collect()
    }

    pub fn predict(&self, x: &[f32], n: usize, dims: usize) -> Vec<u8> {
        self.predict_proba(x, n, dims)
            .into_iter()
            .map(|p| if p >= 0.5 { 1 } else { 0 })
            .collect()
    }
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(&x, &y)| x * y).sum()
}

fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}
