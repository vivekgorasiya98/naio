//! K-means clustering with k-means++ initialization.

use rand::Rng;
use rayon::prelude::*;

#[derive(Clone)]
pub struct KMeans {
    pub k: usize,
    pub max_iters: usize,
    pub centroids: Vec<f32>,
    pub dims: usize,
}

impl KMeans {
    pub fn new(k: usize, max_iters: usize) -> Self {
        Self {
            k,
            max_iters,
            centroids: Vec::new(),
            dims: 0,
        }
    }

    pub fn fit(&mut self, data: &[f32], n: usize, dims: usize) -> Vec<usize> {
        self.dims = dims;
        self.centroids = kmeans_plus_plus_init(data, n, dims, self.k);
        let mut labels = vec![0usize; n];
        for _ in 0..self.max_iters {
            labels.par_iter_mut().enumerate().for_each(|(i, label)| {
                let point = &data[i * dims..(i + 1) * dims];
                *label = nearest_centroid(point, &self.centroids, self.k, dims);
            });
            let mut new_centroids = vec![0.0f32; self.k * dims];
            let mut counts = vec![0usize; self.k];
            for i in 0..n {
                let c = labels[i];
                counts[c] += 1;
                for d in 0..dims {
                    new_centroids[c * dims + d] += data[i * dims + d];
                }
            }
            for c in 0..self.k {
                if counts[c] > 0 {
                    for d in 0..dims {
                        new_centroids[c * dims + d] /= counts[c] as f32;
                    }
                }
            }
            if new_centroids == self.centroids {
                break;
            }
            self.centroids = new_centroids;
        }
        labels
    }

    pub fn predict(&self, data: &[f32], n: usize) -> Vec<usize> {
        (0..n)
            .map(|i| {
                let point = &data[i * self.dims..(i + 1) * self.dims];
                nearest_centroid(point, &self.centroids, self.k, self.dims)
            })
            .collect()
    }
}

fn nearest_centroid(point: &[f32], centroids: &[f32], k: usize, dims: usize) -> usize {
    let mut best = 0;
    let mut best_dist = f32::INFINITY;
    for c in 0..k {
        let cent = &centroids[c * dims..(c + 1) * dims];
        let mut dist = 0.0f32;
        for d in 0..dims {
            let diff = point[d] - cent[d];
            dist += diff * diff;
        }
        if dist < best_dist {
            best_dist = dist;
            best = c;
        }
    }
    best
}

fn kmeans_plus_plus_init(data: &[f32], n: usize, dims: usize, k: usize) -> Vec<f32> {
    let mut rng = rand::thread_rng();
    let mut centroids = Vec::with_capacity(k * dims);
    let first = rng.gen_range(0..n);
    centroids.extend_from_slice(&data[first * dims..(first + 1) * dims]);
    for _ in 1..k {
        let mut dists = vec![0.0f32; n];
        let mut total = 0.0f32;
        for i in 0..n {
            let point = &data[i * dims..(i + 1) * dims];
            let c = nearest_centroid(point, &centroids, centroids.len() / dims, dims);
            let cent = &centroids[c * dims..(c + 1) * dims];
            let mut d = 0.0f32;
            for j in 0..dims {
                let diff = point[j] - cent[j];
                d += diff * diff;
            }
            dists[i] = d;
            total += d;
        }
        let r = rng.gen::<f32>() * total;
        let mut acc = 0.0f32;
        let mut chosen = n - 1;
        for i in 0..n {
            acc += dists[i];
            if acc >= r {
                chosen = i;
                break;
            }
        }
        centroids.extend_from_slice(&data[chosen * dims..(chosen + 1) * dims]);
    }
    centroids
}
