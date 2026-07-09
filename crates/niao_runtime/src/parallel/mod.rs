//! Native `parallel` standard library — threading, mutexes, channels, and worker pools.

mod channel;
mod common;
mod mutex;
mod poll;
mod pool;
mod registry;
mod sendable;
mod thread;

use crate::{NativeFn, Value, ValueRef};
use std::collections::HashMap;
use std::rc::Rc;

pub use sendable::{value_to_sendable, SendableValue};

macro_rules! bind_fn {
    ($map:expr, $name:expr, $func:expr) => {
        $map.insert(
            $name.to_string(),
            Value::NativeFunction($func).ref_cell(),
        );
    };
}

fn method_object(methods: Vec<(&'static str, NativeFn)>) -> ValueRef {
    let mut map = HashMap::new();
    for (method, func) in methods {
        map.insert(method.to_string(), Value::NativeFunction(func).ref_cell());
    }
    Value::Object(map).ref_cell()
}

/// `parallel.Thread`, `parallel.Mutex`, `parallel.Channel`, `parallel.Pool` namespace.
pub fn namespace() -> Value {
    let mut map = HashMap::new();

    map.insert(
        "Thread".to_string(),
        method_object(vec![
            ("spawn", Rc::new(thread::parallel_thread_spawn)),
            ("join", Rc::new(thread::parallel_thread_join)),
            ("detach", Rc::new(thread::parallel_thread_detach)),
            ("is_alive", Rc::new(thread::parallel_thread_is_alive)),
            ("id", Rc::new(thread::parallel_thread_id)),
            ("yield", Rc::new(thread::parallel_thread_yield)),
            ("sleep", Rc::new(thread::parallel_thread_sleep)),
        ]),
    );

    map.insert(
        "Mutex".to_string(),
        method_object(vec![
            ("new", Rc::new(mutex::parallel_mutex_new)),
            ("lock", Rc::new(mutex::parallel_mutex_lock)),
            ("unlock", Rc::new(mutex::parallel_mutex_unlock)),
            ("try_lock", Rc::new(mutex::parallel_mutex_try_lock)),
            ("get", Rc::new(mutex::parallel_mutex_get)),
            ("set", Rc::new(mutex::parallel_mutex_set)),
            ("run", Rc::new(mutex::parallel_mutex_run)),
        ]),
    );

    map.insert(
        "Channel".to_string(),
        method_object(vec![
            ("new", Rc::new(channel::parallel_channel_new)),
            ("send", Rc::new(channel::parallel_channel_send)),
            ("recv", Rc::new(channel::parallel_channel_recv)),
            ("try_recv", Rc::new(channel::parallel_channel_try_recv)),
            ("close", Rc::new(channel::parallel_channel_close)),
            ("recv_timeout", Rc::new(channel::parallel_channel_recv_timeout)),
            ("is_closed", Rc::new(channel::parallel_channel_is_closed)),
        ]),
    );

    map.insert(
        "Pool".to_string(),
        method_object(vec![
            ("new", Rc::new(pool::parallel_pool_new)),
            ("submit", Rc::new(pool::parallel_pool_submit)),
            ("wait", Rc::new(pool::parallel_pool_wait)),
            ("shutdown", Rc::new(pool::parallel_pool_shutdown)),
            ("active", Rc::new(pool::parallel_pool_active)),
        ]),
    );

    bind_fn!(map, "poll", Rc::new(pool::parallel_poll));
    bind_fn!(map, "poll_all", Rc::new(pool::parallel_poll_all));
    bind_fn!(map, "cpu_count", Rc::new(thread::parallel_cpu_count));

    Value::Object(map)
}

pub const MODULE_NAME: &str = "parallel";
pub const MODULE_PATHS: &[&str] = &["parallel", "std/parallel"];

pub fn builtins() -> Vec<(&'static str, NativeFn)> {
    vec![
        // thread
        ("parallel_thread_spawn", Rc::new(thread::parallel_thread_spawn)),
        ("parallel_thread_join", Rc::new(thread::parallel_thread_join)),
        ("parallel_thread_detach", Rc::new(thread::parallel_thread_detach)),
        ("parallel_thread_is_alive", Rc::new(thread::parallel_thread_is_alive)),
        ("parallel_thread_id", Rc::new(thread::parallel_thread_id)),
        ("parallel_thread_yield", Rc::new(thread::parallel_thread_yield)),
        ("parallel_thread_sleep", Rc::new(thread::parallel_thread_sleep)),
        ("parallel_cpu_count", Rc::new(thread::parallel_cpu_count)),
        // mutex
        ("parallel_mutex_new", Rc::new(mutex::parallel_mutex_new)),
        ("parallel_mutex_lock", Rc::new(mutex::parallel_mutex_lock)),
        ("parallel_mutex_unlock", Rc::new(mutex::parallel_mutex_unlock)),
        ("parallel_mutex_try_lock", Rc::new(mutex::parallel_mutex_try_lock)),
        ("parallel_mutex_get", Rc::new(mutex::parallel_mutex_get)),
        ("parallel_mutex_set", Rc::new(mutex::parallel_mutex_set)),
        ("parallel_mutex_run", Rc::new(mutex::parallel_mutex_run)),
        // channel
        ("parallel_channel_new", Rc::new(channel::parallel_channel_new)),
        ("parallel_channel_send", Rc::new(channel::parallel_channel_send)),
        ("parallel_channel_recv", Rc::new(channel::parallel_channel_recv)),
        ("parallel_channel_try_recv", Rc::new(channel::parallel_channel_try_recv)),
        ("parallel_channel_close", Rc::new(channel::parallel_channel_close)),
        ("parallel_channel_recv_timeout", Rc::new(channel::parallel_channel_recv_timeout)),
        ("parallel_channel_is_closed", Rc::new(channel::parallel_channel_is_closed)),
        // pool
        ("parallel_pool_new", Rc::new(pool::parallel_pool_new)),
        ("parallel_pool_submit", Rc::new(pool::parallel_pool_submit)),
        ("parallel_pool_wait", Rc::new(pool::parallel_pool_wait)),
        ("parallel_pool_shutdown", Rc::new(pool::parallel_pool_shutdown)),
        ("parallel_pool_active", Rc::new(pool::parallel_pool_active)),
        // poll
        ("parallel_poll", Rc::new(pool::parallel_poll)),
        ("parallel_poll_all", Rc::new(pool::parallel_poll_all)),
    ]
}
