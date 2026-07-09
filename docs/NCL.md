# NCL — Niao Column Library

Fast **pandas + numpy**-style columnar data for Niao. Implemented in native Rust with packed `IntArray` / `FloatArray` / `BoolArray` columns, vectorized kernels, and optional Rayon parallel reductions.

## Import

```niao
import "ncl"
```

Paths `import "std/ncl"` and `import "ncl"` are equivalent.

## numpy-like arrays

| API | Description |
|-----|-------------|
| `ncl.zeros(n, as_float?)` | Zero-filled packed array |
| `ncl.ones(n, as_float?)` | Ones-filled packed array |
| `ncl.arange(start, stop, step?)` | Integer range → `IntArray` |
| `ncl.linspace(start, stop, n)` | Evenly spaced floats → `FloatArray` |
| `ncl.array(data)` | Homogeneous array from Niao array literal |
| `ncl.slice(arr, start, end)` | Zero-copy-style slice (new buffer) |
| `ncl.add(a, b)` / `sub` / `mul` / `div` | Element-wise ops on packed arrays |
| `ncl.abs` / `sqrt` / `exp` / `log` / `sin` / `cos` | Unary math ufuncs |
| `ncl.sum` / `mean` / `min` / `max` / `std` / `var` / `median` | Reductions |
| `ncl.corr(a, b)` | Pearson correlation |
| `ncl.dot(a, b)` | Dot product |
| `ncl.parallel_sum(arr)` | Rayon parallel sum (large arrays) |

## Series & DataFrame

Handles are opaque `ncl_*` objects (use `ncl.kind(h)` → `ncl_series`, `ncl_dataframe`, …).

| API | Description |
|-----|-------------|
| `ncl.series(data, name?)` | 1D column from packed array |
| `ncl.dataframe({col: array, …})` | Aligned multi-column table |
| `ncl.df_get(df, name)` | Column as Series |
| `ncl.df_set(df, name, array)` | Assign/replace column |
| `ncl.df_columns(df)` | Column name list |
| `ncl.df_shape(df)` | `[rows, cols]` |
| `ncl.series_values(s)` | Packed array backing |
| `ncl.head(df, n?)` / `tail` | Row slice |
| `ncl.filter(df, mask)` | Boolean/int mask select |
| `ncl.sort_values(df, col, desc?)` | Sort by column |

## Analytics

| API | Description |
|-----|-------------|
| `ncl.groupby(df, key)` | Split by int/string key column |
| `ncl.agg(group, {col: "sum"\|"mean"\|"count"})` | Per-group aggregates |
| `ncl.merge(left, right, on)` | Inner join on int key |
| `ncl.concat(a, b)` | Vertical stack |
| `ncl.pivot(df, index, columns, values)` | Pivot table |
| `ncl.melt(df, id_vars)` | Wide → long |

## Missing values & rolling

| API | Description |
|-----|-------------|
| `ncl.isna(series)` | Null mask |
| `ncl.fillna(series, value)` | Fill nulls |
| `ncl.dropna(series)` | Remove null rows |
| `ncl.rolling_mean(s, window)` | Sliding mean |
| `ncl.rolling_sum` / `rolling_std` | Sliding sum / std |
| `ncl.describe(series)` | count, mean, std, min, max |

## I/O

| API | Description |
|-----|-------------|
| `ncl.read_csv(path)` | Typed CSV → DataFrame |
| `ncl.to_csv(df, path?)` | Write file or return string |
| `ncl.from_sqlite(conn, sql, params?)` | Packed columns from nsqlite |
| `ncl.to_datetime(series, format?)` | Parse string column to epoch ms |

## NDArray

| API | Description |
|-----|-------------|
| `ncl.ndarray(shape, data)` | N-dimensional homogeneous array |
| `ncl.shape(arr)` / `dtype(arr)` / `reshape` / `flatten` | Shape ops |

## Performance notes

- Prefer **packed arrays** (`IntArray` / `FloatArray`) over generic `Array` of boxed values.
- Use **batch builtins** (`ncl.mul(a, 2)`) instead of per-element loops.
- `ncl_sum` / `ncl_mean` have **VM fast paths** on the bytecode VM.
- `parallel_sum` uses Rayon when length ≥ 65,536.
- Benchmark: `./target/debug/niao bench benchmarks/ncl_bench.niao`

## Error codes

| Code | Kind |
|------|------|
| E1960 | Arity error |
| E1961 | Operation failed |
| E1962 | Invalid handle |
| E1963 | Index bounds |
| E1964 | Type mismatch |
| E1965 | Shape error |

Errors surface as Niao error values with `kind: "ncl_error"`.

## Example

See [examples/ncl_demo.niao](../examples/ncl_demo.niao).
