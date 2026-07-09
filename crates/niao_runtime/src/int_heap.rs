//! Flat `Vec<i64>` binary heap — same layout as V8/JS benchmark heaps.

#[inline(always)]
pub fn push(data: &mut Vec<i64>, v: i64, min: bool) {
    data.push(v);
    let mut i = data.len() - 1;
    while i > 0 {
        let parent = (i - 1) / 2;
        let swap = if min {
            data[i] < data[parent]
        } else {
            data[i] > data[parent]
        };
        if !swap {
            break;
        }
        data.swap(i, parent);
        i = parent;
    }
}

#[inline(always)]
pub fn pop(data: &mut Vec<i64>, min: bool) -> Option<i64> {
    if data.is_empty() {
        return None;
    }
    if data.len() == 1 {
        return data.pop();
    }
    let top = data[0];
    let last = data.pop().unwrap();
    data[0] = last;
    let mut i = 0;
    loop {
        let left = 2 * i + 1;
        let right = left + 1;
        let mut best = i;
        if left < data.len() {
            let better = if min {
                data[left] < data[best]
            } else {
                data[left] > data[best]
            };
            if better {
                best = left;
            }
        }
        if right < data.len() {
            let better = if min {
                data[right] < data[best]
            } else {
                data[right] > data[best]
            };
            if better {
                best = right;
            }
        }
        if best == i {
            break;
        }
        data.swap(i, best);
        i = best;
    }
    Some(top)
}
