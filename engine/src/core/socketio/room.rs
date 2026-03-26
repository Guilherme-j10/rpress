//! Room management: join, leave, and broadcast within namespaces.

use std::collections::{HashMap, HashSet};
use std::pin::Pin;

use tokio::sync::{mpsc, RwLock};

use super::adapter::Adapter;
use super::engine_io::EioPacket;

/// A key combining namespace + room name.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct RoomKey {
    namespace: String,
    room: String,
}

/// Manages rooms and socket membership across namespaces.
///
/// Each socket's outbound channel is registered so that broadcasts can
/// fan-out packets to all members of a room efficiently.
///
/// This is the default in-memory [`Adapter`] implementation. It works for
/// single-server deployments. For horizontal scaling, use `RedisAdapter`
/// (requires the `redis` feature).
pub struct RoomManager {
    /// `(namespace, room)` -> set of socket IDs
    rooms: RwLock<HashMap<RoomKey, HashSet<String>>>,
    /// socket_id -> sender for Engine.IO packets
    senders: RwLock<HashMap<String, mpsc::Sender<EioPacket>>>,
}

impl RoomManager {
    pub fn new() -> Self {
        Self {
            rooms: RwLock::new(HashMap::new()),
            senders: RwLock::new(HashMap::new()),
        }
    }
}

impl Default for RoomManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Adapter for RoomManager {
    fn register_sender(
        &self,
        socket_id: &str,
        tx: mpsc::Sender<EioPacket>,
    ) -> Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        let socket_id = socket_id.to_string();
        Box::pin(async move {
            self.senders.write().await.insert(socket_id, tx);
        })
    }

    fn unregister_sender(
        &self,
        socket_id: &str,
    ) -> Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        let socket_id = socket_id.to_string();
        Box::pin(async move {
            self.senders.write().await.remove(&socket_id);
        })
    }

    fn join(
        &self,
        namespace: &str,
        room: &str,
        socket_id: &str,
    ) -> Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        let key = RoomKey {
            namespace: namespace.to_string(),
            room: room.to_string(),
        };
        let socket_id = socket_id.to_string();
        Box::pin(async move {
            self.rooms
                .write()
                .await
                .entry(key)
                .or_default()
                .insert(socket_id);
        })
    }

    fn leave(
        &self,
        namespace: &str,
        room: &str,
        socket_id: &str,
    ) -> Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        let key = RoomKey {
            namespace: namespace.to_string(),
            room: room.to_string(),
        };
        let socket_id = socket_id.to_string();
        Box::pin(async move {
            let mut rooms = self.rooms.write().await;
            if let Some(members) = rooms.get_mut(&key) {
                members.remove(&socket_id);
                if members.is_empty() {
                    rooms.remove(&key);
                }
            }
        })
    }

    fn leave_all(
        &self,
        socket_id: &str,
    ) -> Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        let socket_id = socket_id.to_string();
        Box::pin(async move {
            let mut rooms = self.rooms.write().await;
            rooms.retain(|_, members| {
                members.remove(&socket_id);
                !members.is_empty()
            });
        })
    }

    fn broadcast_room(
        &self,
        namespace: &str,
        room: &str,
        packet: &EioPacket,
        exclude: Option<&str>,
    ) -> Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        let key = RoomKey {
            namespace: namespace.to_string(),
            room: room.to_string(),
        };
        let packet = packet.clone();
        let exclude = exclude.map(|s| s.to_string());
        Box::pin(async move {
            let members = {
                self.rooms
                    .read()
                    .await
                    .get(&key)
                    .cloned()
                    .unwrap_or_default()
            };
            let senders = self.senders.read().await;
            for member_id in &members {
                if exclude.as_deref() == Some(member_id.as_str()) {
                    continue;
                }
                if let Some(tx) = senders.get(member_id) {
                    let _ = tx.send(packet.clone()).await;
                }
            }
        })
    }

    fn broadcast_namespace(
        &self,
        namespace: &str,
        packet: &EioPacket,
        exclude: Option<&str>,
    ) -> Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        let namespace = namespace.to_string();
        let packet = packet.clone();
        let exclude = exclude.map(|s| s.to_string());
        Box::pin(async move {
            let rooms = self.rooms.read().await;
            let senders = self.senders.read().await;
            let mut sent = HashSet::new();
            for (key, members) in rooms.iter() {
                if key.namespace != namespace {
                    continue;
                }
                for member_id in members {
                    if exclude.as_deref() == Some(member_id.as_str()) {
                        continue;
                    }
                    if sent.contains(member_id) {
                        continue;
                    }
                    if let Some(tx) = senders.get(member_id) {
                        let _ = tx.send(packet.clone()).await;
                        sent.insert(member_id.clone());
                    }
                }
            }
        })
    }

    fn room_members(
        &self,
        namespace: &str,
        room: &str,
    ) -> Pin<Box<dyn std::future::Future<Output = HashSet<String>> + Send + '_>> {
        let key = RoomKey {
            namespace: namespace.to_string(),
            room: room.to_string(),
        };
        Box::pin(async move {
            self.rooms
                .read()
                .await
                .get(&key)
                .cloned()
                .unwrap_or_default()
        })
    }
}
