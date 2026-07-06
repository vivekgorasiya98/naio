//! Compact backing storage for native DSA containers.
//!
//! Int-only workloads use packed `Vec`/`VecDeque<i64>` without per-element
//! `Rc<RefCell<Value>>`. Mixed-type values promote once to the generic path.

use crate::{Value, ValueRef};
use ahash::{HashMapExt, HashSetExt, RandomState};
use indexmap::{IndexMap, IndexSet};
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;

/// Hashable key for sets and maps. Whole floats normalize to ints so that
/// `1` and `1.0` are the same key.
#[derive(Clone, PartialEq, Eq, Hash)]
pub enum DsKey {
    Int(i64),
    Str(String),
    Bool(bool),
    Float(u64),
}

pub fn value_to_key(v: &Value) -> Option<DsKey> {
    match v {
        Value::Int(n) => Some(DsKey::Int(*n)),
        Value::String(s) => Some(DsKey::Str(s.clone())),
        Value::Bool(b) => Some(DsKey::Bool(*b)),
        Value::Float(f) => {
            if f.fract() == 0.0 && *f >= i64::MIN as f64 && *f <= i64::MAX as f64 {
                Some(DsKey::Int(*f as i64))
            } else {
                Some(DsKey::Float(f.to_bits()))
            }
        }
        _ => None,
    }
}

pub fn key_to_value(k: &DsKey) -> Value {
    match k {
        DsKey::Int(n) => Value::Int(*n),
        DsKey::Str(s) => Value::String(s.clone()),
        DsKey::Bool(b) => Value::Bool(*b),
        DsKey::Float(bits) => Value::Float(f64::from_bits(*bits)),
    }
}

pub type IntHashSet = HashSet<i64, RandomState>;
pub type IntHashMap = HashMap<i64, i64, RandomState>;
pub type AnySet = IndexSet<DsKey, RandomState>;
pub type AnyMap = IndexMap<DsKey, ValueRef, RandomState>;

#[derive(Clone)]
pub enum SeqData {
    Int(VecDeque<i64>),
    Any(VecDeque<ValueRef>),
}

impl Default for SeqData {
    fn default() -> Self {
        SeqData::Int(VecDeque::new())
    }
}

#[derive(Clone)]
pub enum StackData {
    Int(Vec<i64>),
    Any(Vec<ValueRef>),
}

impl Default for StackData {
    fn default() -> Self {
        StackData::Int(Vec::new())
    }
}

#[derive(Clone)]
pub enum SetData {
    Int {
        set: IntHashSet,
        order: Vec<i64>,
    },
    Any(AnySet),
}

impl Default for SetData {
    fn default() -> Self {
        SetData::Int {
            set: IntHashSet::new(),
            order: Vec::new(),
        }
    }
}

#[derive(Clone)]
pub enum MapData {
    /// Keys `0..len-1` map to `values[i]` — O(1) index lookup.
    Dense(Vec<i64>),
    IntInt {
        map: IntHashMap,
        keys: Vec<i64>,
    },
    Any(AnyMap),
}

impl Default for MapData {
    fn default() -> Self {
        MapData::IntInt {
            map: IntHashMap::new(),
            keys: Vec::new(),
        }
    }
}

#[inline]
fn int_from_val(v: &ValueRef) -> Option<i64> {
    match &*v.borrow() {
        Value::Int(n) => Some(*n),
        _ => None,
    }
}

impl SeqData {
    pub fn len(&self) -> usize {
        match self {
            SeqData::Int(v) => v.len(),
            SeqData::Any(v) => v.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn clear(&mut self) {
        match self {
            SeqData::Int(v) => v.clear(),
            SeqData::Any(v) => v.clear(),
        }
    }

    pub fn push_back(&mut self, v: ValueRef) {
        if let SeqData::Int(vec) = self {
            if let Some(n) = int_from_val(&v) {
                vec.push_back(n);
                return;
            }
            let mut any = VecDeque::with_capacity(vec.len() + 1);
            for n in vec.drain(..) {
                any.push_back(Value::Int(n).ref_cell());
            }
            any.push_back(v);
            *self = SeqData::Any(any);
        } else if let SeqData::Any(vec) = self {
            vec.push_back(v);
        }
    }

    pub fn push_back_int(&mut self, n: i64) {
        match self {
            SeqData::Int(vec) => vec.push_back(n),
            SeqData::Any(vec) => vec.push_back(Value::Int(n).ref_cell()),
        }
    }

    /// Push `i * mul + add` for `i in start..limit` on the int fast path.
    pub fn push_back_range_int(&mut self, start: i64, limit: i64, mul: i64, add: i64) -> bool {
        let SeqData::Int(vec) = self else {
            return false;
        };
        let count = (limit - start).max(0) as usize;
        vec.reserve(count);
        let mut i = start;
        while i < limit {
            vec.push_back(i.wrapping_mul(mul).wrapping_add(add));
            i += 1;
        }
        true
    }

    /// Pop front until empty, returning the sum of all values.
    pub fn drain_sum_front(&mut self) -> Option<i64> {
        let SeqData::Int(vec) = self else {
            return None;
        };
        let mut sum = 0i64;
        while let Some(n) = vec.pop_front() {
            sum += n;
        }
        Some(sum)
    }

    pub fn push_front(&mut self, v: ValueRef) {
        if let SeqData::Int(vec) = self {
            if let Some(n) = int_from_val(&v) {
                vec.push_front(n);
                return;
            }
            let mut any = VecDeque::with_capacity(vec.len() + 1);
            for n in vec.drain(..) {
                any.push_back(Value::Int(n).ref_cell());
            }
            any.push_front(v);
            *self = SeqData::Any(any);
        } else if let SeqData::Any(vec) = self {
            vec.push_front(v);
        }
    }

    pub fn push_front_int(&mut self, n: i64) {
        match self {
            SeqData::Int(vec) => vec.push_front(n),
            SeqData::Any(vec) => vec.push_front(Value::Int(n).ref_cell()),
        }
    }

    pub fn pop_front(&mut self) -> Option<ValueRef> {
        match self {
            SeqData::Int(vec) => vec.pop_front().map(|n| Value::Int(n).ref_cell()),
            SeqData::Any(vec) => vec.pop_front(),
        }
    }

    pub fn pop_front_int(&mut self) -> Option<i64> {
        match self {
            SeqData::Int(vec) => vec.pop_front(),
            SeqData::Any(vec) => {
                let n = int_from_val(vec.front()?)?;
                vec.pop_front();
                Some(n)
            }
        }
    }

    pub fn pop_back(&mut self) -> Option<ValueRef> {
        match self {
            SeqData::Int(vec) => vec.pop_back().map(|n| Value::Int(n).ref_cell()),
            SeqData::Any(vec) => vec.pop_back(),
        }
    }

    pub fn pop_back_int(&mut self) -> Option<i64> {
        match self {
            SeqData::Int(vec) => vec.pop_back(),
            SeqData::Any(vec) => {
                let n = int_from_val(vec.back()?)?;
                vec.pop_back();
                Some(n)
            }
        }
    }

    pub fn front(&self) -> Option<ValueRef> {
        match self {
            SeqData::Int(vec) => vec.front().copied().map(|n| Value::Int(n).ref_cell()),
            SeqData::Any(vec) => vec.front().map(Rc::clone),
        }
    }

    pub fn back(&self) -> Option<ValueRef> {
        match self {
            SeqData::Int(vec) => vec.back().copied().map(|n| Value::Int(n).ref_cell()),
            SeqData::Any(vec) => vec.back().map(Rc::clone),
        }
    }

    pub fn get(&self, i: usize) -> Option<ValueRef> {
        match self {
            SeqData::Int(vec) => vec.get(i).copied().map(|n| Value::Int(n).ref_cell()),
            SeqData::Any(vec) => vec.get(i).map(Rc::clone),
        }
    }

    pub fn set(&mut self, i: usize, v: ValueRef) {
        match self {
            SeqData::Int(vec) => {
                if let Some(n) = int_from_val(&v) {
                    vec[i] = n;
                } else {
                    let mut any = VecDeque::with_capacity(vec.len());
                    for n in vec.drain(..) {
                        any.push_back(Value::Int(n).ref_cell());
                    }
                    any[i] = v;
                    *self = SeqData::Any(any);
                }
            }
            SeqData::Any(vec) => vec[i] = v,
        }
    }

    pub fn insert(&mut self, i: usize, v: ValueRef) {
        match self {
            SeqData::Int(vec) => {
                if let Some(n) = int_from_val(&v) {
                    vec.insert(i, n);
                } else {
                    let mut any = VecDeque::with_capacity(vec.len() + 1);
                    for n in vec.drain(..) {
                        any.push_back(Value::Int(n).ref_cell());
                    }
                    any.insert(i, v);
                    *self = SeqData::Any(any);
                }
            }
            SeqData::Any(vec) => vec.insert(i, v),
        }
    }

    pub fn remove(&mut self, i: usize) -> ValueRef {
        match self {
            SeqData::Int(vec) => Value::Int(vec.remove(i).unwrap()).ref_cell(),
            SeqData::Any(vec) => vec.remove(i).unwrap(),
        }
    }

    pub fn iter_refs(&self) -> Vec<ValueRef> {
        match self {
            SeqData::Int(vec) => vec.iter().map(|&n| Value::Int(n).ref_cell()).collect(),
            SeqData::Any(vec) => vec.iter().map(Rc::clone).collect(),
        }
    }

    pub fn from_elems(elems: Vec<ValueRef>) -> Self {
        let mut out = SeqData::Int(VecDeque::with_capacity(elems.len()));
        for e in elems {
            out.push_back(e);
        }
        out
    }

    pub fn make_contiguous_reverse(&mut self) {
        match self {
            SeqData::Int(vec) => vec.make_contiguous().reverse(),
            SeqData::Any(vec) => vec.make_contiguous().reverse(),
        }
    }
}

impl StackData {
    pub fn len(&self) -> usize {
        match self {
            StackData::Int(v) => v.len(),
            StackData::Any(v) => v.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn clear(&mut self) {
        match self {
            StackData::Int(v) => v.clear(),
            StackData::Any(v) => v.clear(),
        }
    }

    pub fn push(&mut self, v: ValueRef) {
        if let StackData::Int(vec) = self {
            if let Some(n) = int_from_val(&v) {
                vec.push(n);
                return;
            }
            let mut any = Vec::with_capacity(vec.len() + 1);
            for n in vec.drain(..) {
                any.push(Value::Int(n).ref_cell());
            }
            any.push(v);
            *self = StackData::Any(any);
        } else if let StackData::Any(vec) = self {
            vec.push(v);
        }
    }

    pub fn push_int(&mut self, n: i64) {
        match self {
            StackData::Int(vec) => vec.push(n),
            StackData::Any(vec) => vec.push(Value::Int(n).ref_cell()),
        }
    }

    pub fn push_range_int(&mut self, start: i64, limit: i64, mul: i64, add: i64) -> bool {
        let StackData::Int(vec) = self else {
            return false;
        };
        let count = (limit - start).max(0) as usize;
        vec.reserve(count);
        let mut i = start;
        while i < limit {
            vec.push(i.wrapping_mul(mul).wrapping_add(add));
            i += 1;
        }
        true
    }

    pub fn drain_sum(&mut self) -> Option<i64> {
        let StackData::Int(vec) = self else {
            return None;
        };
        let mut sum = 0i64;
        while let Some(n) = vec.pop() {
            sum += n;
        }
        Some(sum)
    }

    pub fn drain_count(&mut self) -> Option<i64> {
        let StackData::Int(vec) = self else {
            return None;
        };
        let n = vec.len() as i64;
        vec.clear();
        Some(n)
    }

    pub fn pop(&mut self) -> Option<ValueRef> {
        match self {
            StackData::Int(vec) => vec.pop().map(|n| Value::Int(n).ref_cell()),
            StackData::Any(vec) => vec.pop(),
        }
    }

    pub fn pop_int(&mut self) -> Option<i64> {
        match self {
            StackData::Int(vec) => vec.pop(),
            StackData::Any(vec) => match vec.pop() {
                Some(v) => int_from_val(&v),
                None => None,
            },
        }
    }

    pub fn last(&self) -> Option<ValueRef> {
        match self {
            StackData::Int(vec) => vec.last().copied().map(|n| Value::Int(n).ref_cell()),
            StackData::Any(vec) => vec.last().map(Rc::clone),
        }
    }

    pub fn iter_refs(&self) -> Vec<ValueRef> {
        match self {
            StackData::Int(vec) => vec.iter().map(|&n| Value::Int(n).ref_cell()).collect(),
            StackData::Any(vec) => vec.iter().map(Rc::clone).collect(),
        }
    }
}

impl SetData {
    pub fn len(&self) -> usize {
        match self {
            SetData::Int { set, .. } => set.len(),
            SetData::Any(s) => s.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn clear(&mut self) {
        match self {
            SetData::Int { set, order } => {
                set.clear();
                order.clear();
            }
            SetData::Any(s) => s.clear(),
        }
    }

    fn promote_any(&mut self) -> &mut AnySet {
        if let SetData::Int { set, order } = self {
            let mut any = AnySet::with_capacity_and_hasher(set.len(), RandomState::new());
            for k in order.drain(..) {
                any.insert(DsKey::Int(k));
            }
            set.clear();
            *self = SetData::Any(any);
        }
        match self {
            SetData::Any(s) => s,
            _ => unreachable!(),
        }
    }

    pub fn insert_key(&mut self, key: DsKey) -> bool {
        if let DsKey::Int(n) = key {
            if let SetData::Int { set, order } = self {
                if set.insert(n) {
                    order.push(n);
                    return true;
                }
                return false;
            }
        }
        let any = self.promote_any();
        any.insert(key)
    }

    pub fn insert_int(&mut self, n: i64) -> bool {
        self.insert_key(DsKey::Int(n))
    }

    pub fn contains_key(&self, key: &DsKey) -> bool {
        match (key, self) {
            (DsKey::Int(n), SetData::Int { set, .. }) => set.contains(n),
            (_, SetData::Int { .. }) => false,
            (_, SetData::Any(s)) => s.contains(key),
        }
    }

    pub fn contains_int(&self, n: i64) -> bool {
        self.contains_key(&DsKey::Int(n))
    }

    pub fn shift_remove(&mut self, key: &DsKey) -> bool {
        match (key, self) {
            (DsKey::Int(n), SetData::Int { set, order }) => {
                if !set.remove(n) {
                    return false;
                }
                if let Some(pos) = order.iter().position(|k| k == n) {
                    order.remove(pos);
                }
                true
            }
            (_, SetData::Int { .. }) => false,
            (_, SetData::Any(s)) => s.shift_remove(key),
        }
    }

    pub fn clone_keys(&self) -> AnySet {
        match self {
            SetData::Int { order, .. } => {
                let mut s = AnySet::with_capacity_and_hasher(order.len(), RandomState::new());
                for &n in order {
                    s.insert(DsKey::Int(n));
                }
                s
            }
            SetData::Any(s) => s.clone(),
        }
    }

    pub fn iter_values(&self) -> Vec<ValueRef> {
        match self {
            SetData::Int { order, .. } => order.iter().map(|&n| Value::Int(n).ref_cell()).collect(),
            SetData::Any(s) => s.iter().map(|k| key_to_value(k).ref_cell()).collect(),
        }
    }

    pub fn from_elems(elems: &[ValueRef]) -> Self {
        let mut out = SetData::default();
        for e in elems {
            let key = value_to_key(&e.borrow()).expect("hashable");
            out.insert_key(key);
        }
        out
    }

    pub fn from_any(s: AnySet) -> Self {
        let mut all_int = true;
        for k in s.iter() {
            if !matches!(k, DsKey::Int(_)) {
                all_int = false;
                break;
            }
        }
        if all_int {
            let mut set = IntHashSet::with_capacity_and_hasher(s.len(), RandomState::new());
            let mut order = Vec::with_capacity(s.len());
            for k in s.iter() {
                let DsKey::Int(n) = k else { unreachable!() };
                set.insert(*n);
                order.push(*n);
            }
            SetData::Int { set, order }
        } else {
            SetData::Any(s)
        }
    }
}

impl MapData {
    pub fn len(&self) -> usize {
        match self {
            MapData::Dense(v) => v.len(),
            MapData::IntInt { map, .. } => map.len(),
            MapData::Any(m) => m.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn clear(&mut self) {
        match self {
            MapData::Dense(v) => v.clear(),
            MapData::IntInt { map, keys } => {
                map.clear();
                keys.clear();
            }
            MapData::Any(m) => m.clear(),
        }
    }

    fn promote_any(&mut self) -> &mut AnyMap {
        match self {
            MapData::Dense(values) => {
                let mut any = AnyMap::with_capacity_and_hasher(values.len(), RandomState::new());
                for (i, &v) in values.iter().enumerate() {
                    any.insert(DsKey::Int(i as i64), Value::Int(v).ref_cell());
                }
                *self = MapData::Any(any);
            }
            MapData::IntInt { map, keys } => {
                let mut any = AnyMap::with_capacity_and_hasher(map.len(), RandomState::new());
                for k in keys.drain(..) {
                    if let Some(v) = map.remove(&k) {
                        any.insert(DsKey::Int(k), Value::Int(v).ref_cell());
                    }
                }
                map.clear();
                *self = MapData::Any(any);
            }
            MapData::Any(_) => {}
        }
        match self {
            MapData::Any(m) => m,
            _ => unreachable!(),
        }
    }

    pub fn insert(&mut self, key: DsKey, v: ValueRef) {
        if let DsKey::Int(k) = &key {
            if let Some(n) = int_from_val(&v) {
                self.insert_int_int(*k, n);
                return;
            }
        }
        let any = self.promote_any();
        any.insert(key, v);
    }

    pub fn insert_int_int(&mut self, k: i64, v: i64) {
        match self {
            MapData::Dense(values) => {
                if k >= 0 {
                    let idx = k as usize;
                    if idx == values.len() {
                        values.push(v);
                        return;
                    }
                    if idx < values.len() {
                        values[idx] = v;
                        return;
                    }
                }
                let old = std::mem::replace(self, MapData::IntInt {
                    map: IntHashMap::new(),
                    keys: Vec::new(),
                });
                if let MapData::Dense(vals) = old {
                    let mut map = IntHashMap::with_capacity(vals.len() + 1);
                    let mut keys = Vec::with_capacity(vals.len() + 1);
                    for (i, &val) in vals.iter().enumerate() {
                        let key = i as i64;
                        keys.push(key);
                        map.insert(key, val);
                    }
                    use std::collections::hash_map::Entry;
                    match map.entry(k) {
                        Entry::Vacant(e) => {
                            keys.push(k);
                            e.insert(v);
                        }
                        Entry::Occupied(mut e) => {
                            e.insert(v);
                        }
                    }
                    *self = MapData::IntInt { map, keys };
                }
            }
            MapData::IntInt { map, keys } => {
                use std::collections::hash_map::Entry;
                match map.entry(k) {
                    Entry::Vacant(e) => {
                        keys.push(k);
                        e.insert(v);
                    }
                    Entry::Occupied(mut e) => {
                        e.insert(v);
                    }
                }
            }
            MapData::Any(m) => {
                m.insert(DsKey::Int(k), Value::Int(v).ref_cell());
            }
        }
    }

    pub fn get(&self, key: &DsKey) -> Option<ValueRef> {
        match (key, self) {
            (DsKey::Int(k), MapData::Dense(values)) => {
                let idx = *k as usize;
                values.get(idx).copied().map(|n| Value::Int(n).ref_cell())
            }
            (DsKey::Int(k), MapData::IntInt { map, .. }) => {
                map.get(k).copied().map(|n| Value::Int(n).ref_cell())
            }
            (_, MapData::Dense(_) | MapData::IntInt { .. }) => None,
            (_, MapData::Any(m)) => m.get(key).map(Rc::clone),
        }
    }

    pub fn get_int(&self, k: i64) -> Option<i64> {
        match self {
            MapData::Dense(values) => {
                if k >= 0 {
                    values.get(k as usize).copied()
                } else {
                    None
                }
            }
            MapData::IntInt { map, .. } => map.get(&k).copied(),
            MapData::Any(m) => match m.get(&DsKey::Int(k)) {
                Some(v) => int_from_val(v),
                None => None,
            },
        }
    }

    pub fn contains_key(&self, key: &DsKey) -> bool {
        match (key, self) {
            (DsKey::Int(k), MapData::Dense(values)) => *k >= 0 && (*k as usize) < values.len(),
            (DsKey::Int(k), MapData::IntInt { map, .. }) => map.contains_key(k),
            (_, MapData::Dense(_) | MapData::IntInt { .. }) => false,
            (_, MapData::Any(m)) => m.contains_key(key),
        }
    }

    pub fn shift_remove(&mut self, key: &DsKey) -> Option<ValueRef> {
        match (key, self) {
            (DsKey::Int(k), MapData::Dense(values)) => {
                let idx = *k as usize;
                if idx < values.len() {
                    let v = values.remove(idx);
                    Some(Value::Int(v).ref_cell())
                } else {
                    None
                }
            }
            (DsKey::Int(k), MapData::IntInt { map, keys }) => {
                let v = map.remove(k)?;
                if let Some(pos) = keys.iter().position(|x| x == k) {
                    keys.remove(pos);
                }
                Some(Value::Int(v).ref_cell())
            }
            (_, MapData::Dense(_) | MapData::IntInt { .. }) => None,
            (_, MapData::Any(m)) => m.shift_remove(key),
        }
    }

    pub fn keys_values(&self) -> (Vec<ValueRef>, Vec<ValueRef>) {
        match self {
            MapData::Dense(values) => {
                let ks = (0..values.len() as i64)
                    .map(|k| Value::Int(k).ref_cell())
                    .collect();
                let vs = values.iter().map(|&n| Value::Int(n).ref_cell()).collect();
                (ks, vs)
            }
            MapData::IntInt { map, keys } => {
                let ks = keys.iter().map(|&k| Value::Int(k).ref_cell()).collect();
                let vs = keys
                    .iter()
                    .filter_map(|k| map.get(k).copied().map(|n| Value::Int(n).ref_cell()))
                    .collect();
                (ks, vs)
            }
            MapData::Any(m) => {
                let ks = m.keys().map(|k| key_to_value(k).ref_cell()).collect();
                let vs = m.values().map(Rc::clone).collect();
                (ks, vs)
            }
        }
    }
}
