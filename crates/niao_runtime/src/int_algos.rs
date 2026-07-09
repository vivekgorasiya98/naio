//! Hot-path integer array algorithms for DSA builtins and VM loop fusion.

use ahash::{HashSetExt, RandomState};
use std::collections::HashSet;

type IntHashSet = HashSet<i64, RandomState>;

/// Classic binary search on a sorted slice — returns whether `target` exists.
#[inline(always)]
pub fn sorted_contains(slice: &[i64], target: i64) -> bool {
    let mut lo = 0usize;
    let mut hi = slice.len();
    while lo < hi {
        let mid = lo + ((hi - lo) >> 1);
        // SAFETY: mid is always in bounds via binary search invariant.
        let v = unsafe { *slice.get_unchecked(mid) };
        if v < target {
            lo = mid + 1;
        } else if v > target {
            hi = mid;
        } else {
            return true;
        }
    }
    false
}

/// Index of `target` in sorted `slice`, or -1.
#[inline]
pub fn sorted_index(slice: &[i64], target: i64) -> i64 {
    let mut lo = 0usize;
    let mut hi = slice.len();
    while lo < hi {
        let mid = lo + ((hi - lo) >> 1);
        let v = unsafe { *slice.get_unchecked(mid) };
        if v < target {
            lo = mid + 1;
        } else if v > target {
            hi = mid;
        } else {
            return mid as i64;
        }
    }
    -1
}

/// Fused binary-search hit counting (`target = i * mul`).
pub fn binary_search_hits(slice: &[i64], start: i64, k: i64, mul: i64, hits: i64) -> (i64, i64) {
    let mut i = start;
    let mut h = hits;
    while i < k {
        if sorted_contains(slice, i.wrapping_mul(mul)) {
            h += 1;
        }
        i += 1;
    }
    (i, h)
}

/// Deduplicate int array preserving first-seen order.
pub fn unique_int(v: &[i64]) -> Vec<i64> {
    if v.is_empty() {
        return Vec::new();
    }
    if v.len() <= 24 {
        return unique_int_small(v);
    }

    let mut max = 0i64;
    let mut all_nonneg = true;
    for &n in v {
        if n < 0 {
            all_nonneg = false;
            break;
        }
        if n > max {
            max = n;
        }
    }

    if all_nonneg {
        let bound = max as usize + 1;
        // Marker array beats hashing when the value range is not much larger than output.
        if bound <= v.len().saturating_mul(3).max(512) && bound <= 4_000_000 {
            return unique_int_marker(v, bound);
        }
    }

    unique_int_hash(v)
}

#[inline]
fn unique_int_small(v: &[i64]) -> Vec<i64> {
    let mut out = Vec::with_capacity(v.len());
    for &n in v {
        if !out.iter().any(|&x| x == n) {
            out.push(n);
        }
    }
    out
}

fn unique_int_marker(v: &[i64], bound: usize) -> Vec<i64> {
    let mut seen = vec![false; bound];
    let mut out = Vec::with_capacity(v.len().min(bound));
    for &n in v {
        let i = n as usize;
        if i < bound && !seen[i] {
            seen[i] = true;
            out.push(n);
        }
    }
    out
}

fn unique_int_hash(v: &[i64]) -> Vec<i64> {
    let mut seen = IntHashSet::with_capacity(v.len());
    let mut out = Vec::with_capacity(v.len());
    for &n in v {
        if seen.insert(n) {
            out.push(n);
        }
    }
    out
}

/// Build map `i -> i * mul` for `i in start..limit` into a dense vec.
pub fn map_build_dense(start: i64, limit: i64, mul: i64) -> Vec<i64> {
    let count = (limit - start).max(0) as usize;
    let mut values = Vec::with_capacity(count);
    let mut i = start;
    while i < limit {
        values.push(i.wrapping_mul(mul));
        i += 1;
    }
    values
}

/// Sum `dense[i]` for `i in start..limit`.
#[inline]
pub fn map_lookup_dense_sum(dense: &[i64], start: i64, limit: i64) -> i64 {
    let mut sum = 0i64;
    let mut i = start;
    while i < limit {
        let idx = i as usize;
        if idx < dense.len() {
            sum += dense[idx];
        }
        i += 1;
    }
    sum
}
