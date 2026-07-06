use dashmap::DashMap;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::broadcast;

pub type ConnId = u64;

#[derive(Clone)]
pub struct WsHub {
    rooms: Arc<DashMap<String, HashSet<ConnId>>>,
    broadcasters: Arc<DashMap<String, broadcast::Sender<String>>>,
    presence: Arc<DashMap<String, usize>>,
}

impl WsHub {
    pub fn new() -> Self {
        Self {
            rooms: Arc::new(DashMap::new()),
            broadcasters: Arc::new(DashMap::new()),
            presence: Arc::new(DashMap::new()),
        }
    }

    pub fn join(&self, room: &str, conn_id: ConnId) {
        self.rooms
            .entry(room.to_string())
            .or_default()
            .insert(conn_id);
        *self.presence.entry(room.to_string()).or_insert(0) += 1;
        if !self.broadcasters.contains_key(room) {
            let (tx, _) = broadcast::channel(256);
            self.broadcasters.insert(room.to_string(), tx);
        }
    }

    pub fn leave(&self, room: &str, conn_id: ConnId) {
        if let Some(mut set) = self.rooms.get_mut(room) {
            set.remove(&conn_id);
        }
        if let Some(mut count) = self.presence.get_mut(room) {
            if *count > 0 {
                *count -= 1;
            }
        }
    }

    pub fn broadcast(&self, room: &str, msg: &str) -> usize {
        if let Some(tx) = self.broadcasters.get(room) {
            tx.send(msg.to_string()).ok();
            tx.receiver_count()
        } else {
            0
        }
    }

    pub fn presence_count(&self, room: &str) -> usize {
        self.presence.get(room).map(|c| *c).unwrap_or(0)
    }

    pub fn subscribe(&self, room: &str) -> Option<broadcast::Receiver<String>> {
        self.broadcasters.get(room).map(|tx| tx.subscribe())
    }
}

pub type SharedWsHub = Arc<WsHub>;
