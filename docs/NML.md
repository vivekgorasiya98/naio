# NML — Neko Machine Learning

NML is Neko's native machine learning library. All hot paths run in Rust (SIMD CPU, optional CUDA GPU). Neko scripts orchestrate training; per-element loops in Neko are not used on the hot path.

## Install

```bash
nm install nml
```

Or use the built-in catalog entry shipped with Neko 0.2.0+.

## Import

```neko
import "nml"
import "ncl"   // optional: data loading / ndarray bridge
```

## Device

```neko
nml_set_device("cpu")       // default
nml_set_device("cuda:0")    // requires build with --features nml-cuda
print(nml_device_count())   // CUDA devices (0 if unavailable)
```

## Tensors

```neko
let a = nml_randn([4, 4])
let b = nml_zeros([4, 4])
let c = nml_matmul(a, b)
print(nml_shape(c))
let flat = nml_to_float_array(c)
```

Bridge from NCL:

```neko
import "ncl"
let nd = ncl_ndarray([2, 3], ncl_array([1.0, 2.0, 3.0, 4.0, 5.0, 6.0]))
let t = nml_from_ncl(nd)
```

## Deep learning

```neko
let l1 = nml_linear(4, 8)
let relu = nml_relu_layer()
let l2 = nml_linear(8, 2)
let model = nml_sequential([l1, relu, l2])

let x = nml_randn([32, 4])
let y = nml_zeros([32, 1])
let trainer = nml_trainer(model, "adam", "mse", 0.001)
let loss = nml_train_epoch(trainer, x, y)
print("loss", loss)
nml_save(model, "model.nml")
```

Layers: `nml_linear`, `nml_relu_layer`, `nml_conv2d_layer`, `nml_batch_norm2d`, `nml_sequential`, `nml_forward`.

Training: `nml_trainer`, `nml_train_epoch`, `nml_eval`, `nml_save`, `nml_load`.

Tuning: `nml_grid_search`, `nml_random_search`, `nml_early_stopping`.

Autograd: `nml_enable_grad`, `nml_zero_grad`, `nml_backward`, `nml_parameters`, `nml_backward_step`.

Data pipelines (`neko_data`): `nml_from_dataframe`, `nml_train_test_split`, `nml_normalize`, `nml_standardize`, `nml_one_hot`, `nml_batch`, `ncl_to_nml_matrix`, `npg_to_ncl`, `nmongo_to_ncl`, `nml_pipeline`, `nml_columnar_epoch`, `nml_node_features_from_ncl`.

Graph ML: see [NML_GRAPH.md](NML_GRAPH.md) — `nml_graph_from_dsa`, `nml_gcn_layer`, `nml_graph_forward`, etc.

Novel: `nml_memory_budget`, `nml_explain`, `nml_plot_training` (loss history → chart).

Visualization: import `nvis` — see [NVIS.md](NVIS.md).

## Classic ML

```neko
let km = nml_kmeans(data, n, dims, k)
let labels = nml_kmeans_predict(km, data, n, dims)
let lr = nml_logistic_fit(x, y, n, dims, epochs)
let tree = nml_decision_tree(x, y, n, dims, max_depth)
let forest = nml_random_forest(x, y, n, dims, n_trees, max_depth)
```

## Architecture

| Crate | Role |
|-------|------|
| `neko_tensor` | Contiguous f32 tensors, SIMD kernels, optional CUDA |
| `neko_ml` | Layers, losses, optimizers, trainer, `.nml` checkpoints |
| `neko_classic` | k-means, logistic regression, trees, random forest |
| `neko_data` | Preprocessing: split, normalize, pipeline DAG, columnar epochs |
| `neko_graph` | Sparse adjacency, GCN kernels, DSA bridge |
| `neko_runtime/nml` | Neko builtins (`nml_*`) |
| `neko_runtime/nvis` | Chart builtins (`nvis_*`) |

## Performance notes

- Use packed `FloatArray` / `nml_tensor` for inputs — not boxed `Array` of floats.
- Call `nml_train_epoch` once per epoch (full epoch runs in Rust).
- Native handles are not sendable across `parallel` workers (E1504).
- VM fast paths accelerate `nml_matmul`, `nml_forward`, and `nml_backward_step`.

## Build features

| Feature | Effect |
|---------|--------|
| `nml-cuda` (neko_runtime) | Enable CUDA via candle-core |
| `nml-wgpu` | wgpu backend stub |

```bash
cargo build -p neko_runtime --features nml-cuda
```

## Error codes

| Code | Meaning |
|------|---------|
| E1970 | Arity mismatch |
| E1971 | Operation failed |
| E1972 | Invalid handle |
| E1973 | Shape error |
| E1974 | Type mismatch |
| E1975 | Device error |
