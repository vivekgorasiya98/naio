//! Reusable f32 buffer pool to reduce allocator churn during training.

use std::collections::HashMap;
use std::sync::Mutex;

pub struct BufferPool {
    buckets: Mutex<HashMap<usize, Vec<Vec<f32>>>>,
}

impl Default for BufferPool {
    fn default() -> Self {
        Self::new()
    }
}

impl BufferPool {
    pub fn new() -> Self {
        Self {
            buckets: Mutex::new(HashMap::new()),
        }
    }

    pub fn acquire(&self, len: usize) -> Vec<f32> {
        let mut buckets = self.buckets.lock().unwrap();
        if let Some(stack) = buckets.get_mut(&len) {
            if let Some(buf) = stack.pop() {
                return buf;
            }
        }
        vec![0.0; len]
    }

    pub fn release(&self, mut buf: Vec<f32>) {
        let len = buf.len();
        buf.fill(0.0);
        let mut buckets = self.buckets.lock().unwrap();
        buckets.entry(len).or_default().push(buf);
    }

    pub fn clear(&self) {
        self.buckets.lock().unwrap().clear();
    }
}

static GLOBAL_POOL: std::sync::OnceLock<BufferPool> = std::sync::OnceLock::new();
static MEMORY_BUDGET: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static MEMORY_USED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

pub fn set_memory_budget(bytes: usize) {
    MEMORY_BUDGET.store(bytes, std::sync::atomic::Ordering::Relaxed);
}

pub fn memory_used() -> usize {
    MEMORY_USED.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn memory_budget() -> usize {
    MEMORY_BUDGET.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn try_alloc(len: usize) -> Result<Vec<f32>, String> {
    let budget = memory_budget();
    if budget > 0 {
        let need = len * std::mem::size_of::<f32>();
        let used = MEMORY_USED.load(std::sync::atomic::Ordering::Relaxed);
        if used + need > budget {
            return Err(format!("memory budget exceeded (E1976): need {need}, used {used}, budget {budget}"));
        }
        MEMORY_USED.fetch_add(need, std::sync::atomic::Ordering::Relaxed);
    }
    Ok(vec![0.0; len])
}

pub fn release_alloc(len: usize) {
    let budget = memory_budget();
    if budget > 0 {
        let need = len * std::mem::size_of::<f32>();
        MEMORY_USED.fetch_sub(need, std::sync::atomic::Ordering::Relaxed);
    }
}

pub fn global_pool() -> &'static BufferPool {
    GLOBAL_POOL.get_or_init(BufferPool::new)
}
