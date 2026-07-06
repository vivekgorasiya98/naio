# DSA standard library

Native data structures and algorithms for Neko programs: linked list, stack, queue, deque, heap, hash set, hash map, graphs, and array utilities. Implemented in Rust for speed on both the bytecode VM and the tree-walking interpreter.

Unlike `json`, `time`, or `parallel`, the DSA module exposes **flat global builtins only** — there is no `dsa.list_new` namespace object. Call `list_new()`, `stack_push()`, and the rest directly after importing.

---

## Import

```neko
import "dsa"
```

`import "std/dsa"` is equivalent.

```neko
import "dsa"

fn main() {
    let s = stack_new()
    stack_push(s, 42)
    print(stack_pop(s))
}
```

DSA builtins are registered at startup. The import statement documents the dependency and is required for programs that use only native modules (so they can run on the bytecode VM without file-based imports).

---

## Native handles

Constructors such as `list_new()` and `map_new()` return opaque **native** values (not ordinary arrays or objects).

| Property | Behavior |
|----------|----------|
| **Mutability** | Structures are mutated in place. Passing a handle to a function updates the same structure. |
| **`len(x)`** | Works on all DSA types. For graphs, `len(g)` is the **node count** (same as `graph_node_count(g)`). |
| **`print(x)`** | Human-readable summary, e.g. `list[1, 2, 3]`, `set{1, 2}`, `graph(nodes=6, edges=5)`. Large values truncate at 32 elements. |
| **JSON** | Native DSA values **cannot** be serialized with `json.stringify` (error `E1014`). Convert with `list_to_array`, `set_to_array`, etc. when needed. |
| **Equality** | Use structure-specific helpers (`list_contains`, `set_contains`, `map_has`) — native handles are not compared with `==`. |

Empty pop/peek/get operations return `nil` (not an error).

---

## Linked list

Double-ended sequence backed by a `VecDeque`. O(1) push/pop at both ends; O(1) indexed get/set; O(n) insert/remove in the middle.

| Function | Args | Returns | Description |
|----------|------|---------|-------------|
| `list_new()` | — | list | Empty list |
| `list_from_array(arr)` | array | list | Copy from array |
| `list_push_front(l, v)` | list, value | nil | Prepend |
| `list_push_back(l, v)` | list, value | nil | Append |
| `list_pop_front(l)` | list | value \| nil | Remove first |
| `list_pop_back(l)` | list | value \| nil | Remove last |
| `list_front(l)` | list | value \| nil | First without removing |
| `list_back(l)` | list | value \| nil | Last without removing |
| `list_get(l, i)` | list, int | value | Index (bounds-checked) |
| `list_set(l, i, v)` | list, int, value | nil | Replace at index |
| `list_insert(l, i, v)` | list, int, value | nil | Insert before index (`i` may equal `len`) |
| `list_remove(l, i)` | list, int | value | Remove and return at index |
| `list_contains(l, v)` | list, value | bool | Membership by value equality |
| `list_index_of(l, v)` | list, value | int | Index or `-1` |
| `list_reverse(l)` | list | nil | Reverse in place |
| `list_to_array(l)` | list | array | Snapshot as array |
| `list_clear(l)` | list | nil | Remove all elements |
| `list_is_empty(l)` | list | bool | Whether empty |

---

## Stack (LIFO)

| Function | Args | Returns | Description |
|----------|------|---------|-------------|
| `stack_new()` | — | stack | Empty stack |
| `stack_push(s, v)` | stack, value | nil | Push on top |
| `stack_pop(s)` | stack | value \| nil | Pop top |
| `stack_peek(s)` | stack | value \| nil | Top without removing |
| `stack_is_empty(s)` | stack | bool | Whether empty |
| `stack_clear(s)` | stack | nil | Remove all |
| `stack_to_array(s)` | stack | array | Bottom-to-top snapshot |

---

## Queue (FIFO)

| Function | Args | Returns | Description |
|----------|------|---------|-------------|
| `queue_new()` | — | queue | Empty queue |
| `queue_push(q, v)` | queue, value | nil | Enqueue at back |
| `queue_pop(q)` | queue | value \| nil | Dequeue from front |
| `queue_front(q)` | queue | value \| nil | Front without removing |
| `queue_back(q)` | queue | value \| nil | Back without removing |
| `queue_is_empty(q)` | queue | bool | Whether empty |
| `queue_clear(q)` | queue | nil | Remove all |
| `queue_to_array(q)` | queue | array | Front-to-back snapshot |

---

## Deque (double-ended queue)

| Function | Args | Returns | Description |
|----------|------|---------|-------------|
| `deque_new()` | — | deque | Empty deque |
| `deque_push_front(d, v)` | deque, value | nil | Prepend |
| `deque_push_back(d, v)` | deque, value | nil | Append |
| `deque_pop_front(d)` | deque | value \| nil | Remove first |
| `deque_pop_back(d)` | deque | value \| nil | Remove last |
| `deque_front(d)` | deque | value \| nil | First without removing |
| `deque_back(d)` | deque | value \| nil | Last without removing |
| `deque_is_empty(d)` | deque | bool | Whether empty |
| `deque_clear(d)` | deque | nil | Remove all |
| `deque_to_array(d)` | deque | array | Front-to-back snapshot |

---

## Heap (priority queue)

Binary heap. Use `heap_new_min()` for a min-heap or `heap_new_max()` for a max-heap. Elements must be **numbers** (`int` or `float`). Integer-only heaps use a fast path; mixing in a float promotes to a generic heap.

| Function | Args | Returns | Description |
|----------|------|---------|-------------|
| `heap_new_min()` | — | heap | Min-heap |
| `heap_new_max()` | — | heap | Max-heap |
| `heap_push(h, n)` | heap, number | nil | Insert |
| `heap_pop(h)` | heap | number \| nil | Remove extremum |
| `heap_peek(h)` | heap | number \| nil | View extremum |
| `heap_is_empty(h)` | heap | bool | Whether empty |

---

## Hash set

Keys must be **int**, **float**, **string**, or **bool**. Insertion order is preserved for deterministic iteration (`set_to_array`).

| Function | Args | Returns | Description |
|----------|------|---------|-------------|
| `set_new()` | — | set | Empty set |
| `set_from_array(arr)` | array | set | Build from array (deduplicates) |
| `set_add(s, key)` | set, key | bool | `true` if newly inserted |
| `set_remove(s, key)` | set, key | bool | `true` if removed |
| `set_contains(s, key)` | set, key | bool | Membership |
| `set_is_empty(s)` | set | bool | Whether empty |
| `set_clear(s)` | set | nil | Remove all |
| `set_to_array(s)` | set | array | Values in insertion order |
| `set_union(a, b)` | set, set | set | New set: elements in either |
| `set_intersect(a, b)` | set, set | set | New set: elements in both |
| `set_diff(a, b)` | set, set | set | New set: in `a` but not `b` |

---

## Hash map

Keys follow the same rules as sets. Values can be any Neko value. Insertion order is preserved for `map_keys` / `map_values`.

| Function | Args | Returns | Description |
|----------|------|---------|-------------|
| `map_new()` | — | map | Empty map |
| `map_set(m, key, value)` | map, key, value | nil | Insert or update |
| `map_get(m, key)` | map, key | value \| nil | Lookup |
| `map_has(m, key)` | map, key | bool | Key exists |
| `map_remove(m, key)` | map, key | bool | `true` if removed |
| `map_keys(m)` | map | array | Keys in insertion order |
| `map_values(m)` | map | array | Values in insertion order |
| `map_is_empty(m)` | map | bool | Whether empty |
| `map_clear(m)` | map | nil | Remove all |

---

## Graph

Adjacency-list graphs with integer node ids `0 .. n-1`. Use `graph_new(n)` for undirected graphs and `graph_new_directed(n)` for directed graphs.

### Construction

| Function | Args | Returns | Description |
|----------|------|---------|-------------|
| `graph_new(n)` | int ≥ 0 | graph | Undirected graph with `n` nodes |
| `graph_new_directed(n)` | int ≥ 0 | graph | Directed graph with `n` nodes |
| `graph_add_edge(g, u, v)` | graph, int, int | nil | Add edge with weight `1` |
| `graph_add_edge_w(g, u, v, w)` | graph, int, int, int | nil | Add edge with weight `w` |
| `graph_neighbors(g, u)` | graph, int | int_array | Adjacent node ids |
| `graph_node_count(g)` | graph | int | Number of nodes |
| `graph_edge_count(g)` | graph | int | Number of edges added |

Undirected edges are stored in both directions. Self-loops add one directed edge entry.

### Algorithms

| Function | Args | Returns | Description |
|----------|------|---------|-------------|
| `graph_bfs(g, src)` | graph, int | int_array | BFS visit order from `src` |
| `graph_dfs(g, src)` | graph, int | int_array | DFS visit order from `src` |
| `graph_dijkstra(g, src)` | graph, int | int_array | Shortest distances from `src`; unreachable nodes are `-1` |
| `graph_topo_sort(g)` | directed graph | int_array \| nil | Topological order (Kahn); `nil` if cycle |

**Notes:**

- BFS/DFS only visit nodes reachable from `src`.
- `graph_dijkstra` requires **non-negative** edge weights.
- `graph_topo_sort` requires a **directed** graph (`graph_new_directed`).

---

## Array algorithms

These operate on ordinary Neko **arrays** (`[1, 2, 3]`) and **int arrays**, mutating the array in place where noted. They are registered by the DSA module alongside native structures.

| Function | Args | Returns | Description |
|----------|------|---------|-------------|
| `push(arr, v)` | array, value | nil | Append (promotes int array to generic array if needed) |
| `pop(arr)` | array | value \| nil | Remove last |
| `sort(arr)` | array | nil | Ascending sort in place |
| `sort_desc(arr)` | array | nil | Descending sort in place |
| `binary_search(arr, target)` | array, value | int | Index in sorted array, or `-1` |
| `reverse(arr)` | array | nil | Reverse in place |
| `sum(arr)` | array | number | Sum of numeric elements |
| `min(arr)` | array | value \| nil | Minimum (nil if empty) |
| `max(arr)` | array | value \| nil | Maximum (nil if empty) |
| `index_of(arr, v)` | array, value | int | First index or `-1` |
| `contains(arr, v)` | array, value | bool | Whether `index_of >= 0` |
| `unique(arr)` | array | array | New array with duplicates removed (hashable keys first; linear scan fallback) |

`sort` / `sort_desc` / `binary_search` require comparable elements: all numbers or all strings for generic arrays; int arrays use integer fast paths.

---

## Examples

Full walkthrough:

```bash
neko run examples/dsa_demo.neko
```

Minimal graph + heap snippet:

```neko
import "dsa"

fn main() {
    let h = heap_new_min()
    heap_push(h, 30)
    heap_push(h, 10)
    heap_push(h, 20)
    print(heap_pop(h))   // 10

    let g = graph_new_directed(3)
    graph_add_edge(g, 0, 1)
    graph_add_edge(g, 1, 2)
    print(graph_topo_sort(g))   // [0, 1, 2]
}
```

---

## Tests

```bash
neko run tests/dsa_list.neko
neko run tests/dsa_stack_queue.neko
neko run tests/dsa_heap.neko
neko run tests/dsa_set_map.neko
neko run tests/dsa_graph.neko
neko run tests/dsa_arrays.neko
```

Benchmark:

```bash
neko run benchmarks/dsa_bench.neko
```

---

## Errors

| Code | When |
|------|------|
| E1100 | Wrong argument count for a DSA builtin |
| E1101 | List index out of bounds (`list_get`, `list_set`, `list_insert`, `list_remove`) |
| E1102 | Graph node id out of range |
| E1014 | `json.stringify` on a native DSA value |

Type errors (wrong handle kind, invalid key type, negative graph weight in Dijkstra, topo sort on undirected graph) surface as `type_error` with a descriptive message.

See [ERRORS.md](ERRORS.md) for the full error registry.

---

## Performance

- All DSA builtins run as **native Rust** code — no per-op interpreter overhead for the structure itself.
- On the bytecode VM, hot integer paths (`stack_push`, `queue_pop`, `heap_push`, `map_set` with int keys, etc.) use **unboxed fast paths** and **fused loop** optimizations when the compiler detects tight loops over DSA operations.
- Native handles live in the VM **native arena** and participate in mark-and-compact GC. See [VM_MEMORY_AND_CACHE.md](VM_MEMORY_AND_CACHE.md).

For heavy numeric workloads, prefer **int arrays** and integer-only heaps/maps where possible; the runtime promotes to generic representations when mixed types appear.

---

## When to use what

| Need | Use |
|------|-----|
| Fixed-size numeric data, sorting, search | Built-in array + `sort` / `binary_search` |
| Frequent push/pop at both ends | `list_*` or `deque_*` |
| LIFO (undo, DFS stack) | `stack_*` |
| FIFO (BFS queue, task queue) | `queue_*` |
| Priority scheduling | `heap_*` |
| Unique keys, set algebra | `set_*` |
| Key–value lookup | `map_*` |
| Graph traversal / shortest path | `graph_*` |
