//! Room management: join, leave, and broadcast within namespaces.

use std::collections::{HashMap, HashSet};

use tokio::sync::{mpsc, RwLock};

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

    /// Registers a socket's outbound channel.
    pub async fn register_sender(&self, socket_id: &str, tx: mpsc::Sender<EioPacket>) {
        self.senders
            .write()
            .await
            .insert(socket_id.to_string(), tx);
    }

    /// Removes a socket's outbound channel.
    pub async fn unregister_sender(&self, socket_id: &str) {
        self.senders.write().await.remove(socket_id);
    }

    /// Adds a socket to a room.
    pub async fn join(&self, namespace: &str, room: &str, socket_id: &str) {
        let key = RoomKey {
            namespace: namespace.to_string(),
            room: room.to_string(),
        };
        self.rooms
            .write()
            .await
            .entry(key)
            .or_default()
            .insert(socket_id.to_string());
    }

    /// Removes a socket from a room.
    pub async fn leave(&self, namespace: &str, room: &str, socket_id: &str) {
        let key = RoomKey {
            namespace: namespace.to_string(),
            room: room.to_string(),
        };
        let mut rooms = self.rooms.write().await;
        if let Some(members) = rooms.get_mut(&key) {
            members.remove(socket_id);
            if members.is_empty() {
                rooms.remove(&key);
            }
        }
    }

    /// Removes a socket from all rooms in all namespaces.
    pub async fn leave_all(&self, socket_id: &str) {
        let mut rooms = self.rooms.write().await;
        rooms.retain(|_, members| {
            members.remove(socket_id);
            !members.is_empty()
        });
    }

    /// Returns all socket IDs in a specific room.
    pub async fn room_members(&self, namespace: &str, room: &str) -> HashSet<String> {
        let key = RoomKey {
            namespace: namespace.to_string(),
            room: room.to_string(),
        };
        self.rooms
            .read()
            .await
            .get(&key)
            .cloned()
            .unwrap_or_default()
    }

    /// Broadcasts a packet to all members of a room, optionally excluding one socket.
    pub async fn broadcast_room(
        &self,
        namespace: &str,
        room: &str,
        packet: &EioPacket,
        exclude: Option<&str>,
    ) {
        let members = self.room_members(namespace, room).await;
        let senders = self.senders.read().await;

        for member_id in &members {
            if Some(member_id.as_str()) == exclude {
                continue;
            }
            if let Some(tx) = senders.get(member_id) {
                let _ = tx.send(packet.clone()).await;
            }
        }
    }

    /// Broadcasts a packet to all sockets in a namespace, optionally excluding one.
    pub async fn broadcast_namespace(
        &self,
        namespace: &str,
        packet: &EioPacket,
        exclude: Option<&str>,
    ) {
        let rooms = self.rooms.read().await;
        let senders = self.senders.read().await;

        let mut sent = HashSet::new();
        for (key, members) in rooms.iter() {
            if key.namespace != namespace {
                continue;
            }
            for member_id in members {
                if Some(member_id.as_str()) == exclude {
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
    }
}

impl Default for RoomManager {
    fn default() -> Self {
        Self::new()
    }
}
