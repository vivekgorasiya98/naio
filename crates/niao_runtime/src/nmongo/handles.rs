//! Global handle tables for MongoDB clients and sessions (shared across VM worker threads).

use mongodb::options::ClientOptions;
use mongodb::Client;
use mongodb::ClientSession;
use niao_ast::Span;
use niao_errors::codes;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

fn build_parallel_clients(opts: &ClientOptions, primary: &Client) -> Vec<Client> {
    const POOL: usize = 8;
    let mut pool = Vec::with_capacity(POOL);
    pool.push(primary.clone());
    let mut parallel_opts = opts.clone();
    parallel_opts.min_pool_size = Some(8);
    for _ in 1..POOL {
        let opts = parallel_opts.clone();
        // Spawn on a fresh thread so block_on never nests inside the driver's runtime.
        match std::thread::spawn(move || Client::with_options(opts)).join() {
            Ok(Ok(c)) => pool.push(c),
            _ => {}
        }
    }
    pool
}

fn ensure_parallel_client_pool(id: u64) {
    let mut map = clients().lock().unwrap();
    let Some(handle) = map.get_mut(&id) else {
        return;
    };
    if !handle.parallel_clients.is_empty() {
        return;
    }
    handle.parallel_clients = build_parallel_clients(&handle.options, &handle.client);
}

pub struct ClientHandle {
    pub client: Client,
    pub options: ClientOptions,
    /// Extra driver clients for true parallel ops (separate connection pools).
    pub parallel_clients: Vec<Client>,
}

pub struct SessionHandle {
    pub client_id: u64,
    pub session: ClientSession,
}

static NEXT_CLIENT: AtomicU64 = AtomicU64::new(1);
static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);
static CLIENTS: OnceLock<Mutex<HashMap<u64, ClientHandle>>> = OnceLock::new();
static SESSIONS: OnceLock<Mutex<HashMap<u64, SessionHandle>>> = OnceLock::new();

fn clients() -> &'static Mutex<HashMap<u64, ClientHandle>> {
    CLIENTS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn sessions() -> &'static Mutex<HashMap<u64, SessionHandle>> {
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn alloc_client(client: Client, options: ClientOptions) -> u64 {
    alloc_client_with_pool(client, options, Vec::new())
}

pub fn alloc_client_with_pool(
    client: Client,
    options: ClientOptions,
    parallel_clients: Vec<Client>,
) -> u64 {
    let id = NEXT_CLIENT.fetch_add(1, Ordering::Relaxed);
    clients().lock().unwrap().insert(
        id,
        ClientHandle {
            client,
            options,
            parallel_clients,
        },
    );
    id
}

pub fn remove_client(id: u64) -> Option<ClientHandle> {
    sessions().lock().unwrap().retain(|_, s| s.client_id != id);
    clients().lock().unwrap().remove(&id)
}

pub fn client_options(id: u64) -> Option<ClientOptions> {
    clients()
        .lock()
        .unwrap()
        .get(&id)
        .map(|h| h.options.clone())
}

/// Cheap clone of the pooled driver client (shared connection pool via `Arc`).
pub fn client_clone(id: u64) -> Option<Client> {
    clients()
        .lock()
        .unwrap()
        .get(&id)
        .map(|h| h.client.clone())
}

pub fn warm_parallel_client_pool(id: u64) {
    ensure_parallel_client_pool(id);
}

pub fn parallel_client_pool(id: u64) -> Option<Vec<Client>> {
    ensure_parallel_client_pool(id);
    clients().lock().unwrap().get(&id).map(|h| {
        if h.parallel_clients.is_empty() {
            vec![h.client.clone()]
        } else {
            h.parallel_clients.clone()
        }
    })
}

pub fn with_client<F, T>(id: u64, name: &str, span: Span, f: F) -> Result<T, crate::RuntimeError>
where
    F: FnOnce(Client) -> Result<T, String>,
{
    let client = client_clone(id).ok_or_else(|| {
        crate::RuntimeError::at(
            span,
            codes::E1922_NMONGO_INVALID_HANDLE,
            format!("{name}(): invalid client handle {id}"),
        )
    })?;
    f(client).map_err(|msg| crate::RuntimeError::at(span, codes::E1921_NMONGO_ERROR, msg))
}

pub fn with_client_mut<F, T>(id: u64, name: &str, span: Span, f: F) -> Result<T, crate::RuntimeError>
where
    F: FnOnce(&mut ClientHandle) -> Result<T, String>,
{
    let mut map = clients().lock().unwrap();
    let handle = map.get_mut(&id).ok_or_else(|| {
        crate::RuntimeError::at(
            span,
            codes::E1922_NMONGO_INVALID_HANDLE,
            format!("{name}(): invalid client handle {id}"),
        )
    })?;
    f(handle).map_err(|msg| crate::RuntimeError::at(span, codes::E1921_NMONGO_ERROR, msg))
}

pub fn alloc_session(client_id: u64, session: ClientSession) -> u64 {
    let id = NEXT_SESSION.fetch_add(1, Ordering::Relaxed);
    sessions().lock().unwrap().insert(
        id,
        SessionHandle {
            client_id,
            session,
        },
    );
    id
}

pub fn remove_session(id: u64) -> Option<SessionHandle> {
    sessions().lock().unwrap().remove(&id)
}

pub fn with_session_mut<F, T>(id: u64, name: &str, span: Span, f: F) -> Result<T, crate::RuntimeError>
where
    F: FnOnce(&mut ClientSession) -> Result<T, String>,
{
    let mut map = sessions().lock().unwrap();
    let handle = map.get_mut(&id).ok_or_else(|| {
        crate::RuntimeError::at(
            span,
            codes::E1922_NMONGO_INVALID_HANDLE,
            format!("{name}(): invalid session handle {id}"),
        )
    })?;
    f(&mut handle.session).map_err(|msg| {
        crate::RuntimeError::at(span, codes::E1926_NMONGO_TRANSACTION, msg)
    })
}

pub fn session_client_id(id: u64) -> Option<u64> {
    sessions()
        .lock()
        .unwrap()
        .get(&id)
        .map(|s| s.client_id)
}

pub fn with_optional_session<F, T>(
    session_id: Option<u64>,
    name: &str,
    span: Span,
    f: F,
) -> Result<T, String>
where
    F: FnOnce(Option<&mut ClientSession>) -> Result<T, String>,
{
    if let Some(sid) = session_id {
        with_session_mut(sid, name, span, |session| f(Some(session)))
            .map_err(|e| e.message())
    } else {
        f(None)
    }
}
