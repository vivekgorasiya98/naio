# NVIS — Niao Visualization

Lightweight chart generation for training curves and data inspection. No browser required — export SVG or print ASCII to the terminal.

## Import

```niao
import "nvis"
```

Flat builtins (`nvis_line`, etc.) are also available after import, matching the `nml` pattern.

## Charts

| Builtin | Description |
|---------|-------------|
| `nvis_line(data)` | Line chart from `FloatArray` or numeric array |
| `nvis_hist(data, bins)` | Histogram |
| `nvis_scatter(x, y)` | Scatter plot |
| `nvis_heatmap(data, rows, cols)` | 2D heatmap |
| `nvis_bar(values)` | Bar chart |

## Output

```niao
let chart = nvis_line(loss_history)
nvis_print_ascii(chart)
nvis_save_svg(chart, "loss.svg")
let csv = nvis_to_csv(chart)
```

## Training integration

```niao
import "nml"
import "nvis"

let trainer = nml_trainer(model, "adam", "mse", 0.01)
// ... training loop ...
let chart = nml_plot_training(trainer)
nvis_print_ascii(chart)
```

`nml_plot_training` reads `trainer.loss_history` and builds a line chart automatically.
