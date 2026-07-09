# NML Graph ML

Graph neural network support: DSA graphs → sparse adjacency → GCN / GraphSAGE layers.

## Import

```niao
import "nml"
import "dsa"
```

## Workflow

```niao
let g = graph_new(6)
graph_add_edge(g, 0, 1)
graph_add_edge(g, 1, 2)

let adj = nml_graph_from_dsa(g)
let adj_norm = nml_graph_normalize(adj)
let features = nml_randn([6, 3])
let layer = nml_gcn_layer(3, 2)
let out = nml_graph_forward(layer, features, adj_norm)
```

## Builtins

| Builtin | Role |
|---------|------|
| `nml_graph_from_dsa(g)` | DSA graph → sparse adjacency handle |
| `nml_graph_normalize(adj)` | Symmetric normalized adjacency \(D^{-1/2} A D^{-1/2}\) |
| `nml_gcn_layer(in, out)` | GCN layer handle |
| `nml_graphsage_layer(in, out)` | GraphSAGE layer handle |
| `nml_graph_forward(layer, features, adj)` | One GNN layer forward pass |
| `nml_graph_embed(adj, dim)` | Structural embedding (random-walk lite) |
| `nml_node_features_from_ncl(df, id_col, feat_cols)` | Tabular features aligned to graph node ids |

## DSA export

- `graph_edge_list(g)` — packed edge list
- `graph_adjacency_coo(g)` — COO `(row, col, weight)` arrays

## Example

See `examples/ml/gnn_cora_style.niao` for a synthetic node-classification-style forward pass.
