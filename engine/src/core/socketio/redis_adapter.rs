//! Redis-backed adapter for horizontal scaling of Socket.IO across multiple server instances.
//!
//! Uses Redis Pub/Sub to propagate broadcasts between nodes. Each node maintains
//! its own local room state and senders; the Redis channel synchronizes events
//! so that a broadcast on Node A reaches sockets connected to Node B.
//!
//! # Example
//!
//! ```rust,no_run
//! use rpress::{Rpress, RpressIo};
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let io = RpressIo::with_redis("redis://127.0.0.1:6379").await?;
//!     io.on_connection(|socket| async move {
//!         println!("Connected: {}", socket.id());
//!     });
//!
//!     let mut app = Rpress::new(None);
//!     app.attach_socketio(io);
//!     app.listen("0.0.0.0:3000").await
//! }
//! ```

use std::collections::HashSet;
use std::pin::Pin;
use std::sync::Arc;

use futures_util::StreamExt;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use super::adapter::Adapter;
use super::engine_io::{EioPacket, EioPacketType};
use super::room::RoomManager;

const CHANNEL_PREFIX: &str = "rpress:sio";

/// Wire format for broadcast messages sent through Redis Pub/Sub.
#[derive(Serialize, Deserialize)]
struct BroadcastMessage {
    node_id: String,
    namespace: String,
    rooms: Option<Vec<String>>,
    packet_type: u8,
    packet_data: Option<String>,
    exclude: Option<String>,
}

/// Redis-backed [`Adapter`] for scaling Socket.IO across multiple server instances.
///
/// Wraps the in-memory [`RoomManager`] for local delivery and uses Redis Pub/Sub
/// to fan out broadcasts to other nodes in the cluster.
///
/// Requires the `redis` feature: `cargo add rpress --features redis`.
pub struct RedisAdapter {
    local: RoomManager,
    node_id: String,
    client: redis::Client,
}

impl RedisAdapter {
    /// Creates a new Redis adapter.
    ///
    /// # Arguments
    ///
    /// * `redis_url` — Redis connection string, e.g. `"redis://127.0.0.1:6379"`.
    pub async fn new(redis_url: &str) -> anyhow::Result<Self> {
        let client = redis::Client::open(redis_url)?;
        let node_id = uuid::Uuid::new_v4().to_string();

        Ok(Self {
            local: RoomManager::new(),
            node_id,
            client,
        })
    }

    /// Starts the background Redis subscriber task. Called internally by
    /// [`RpressIo::with_redis`](super::RpressIo::with_redis) after wrapping
    /// the adapter in `Arc`.
    pub(crate) fn start_subscriber(self: &Arc<Self>) {
        let adapter = Arc::clone(self);
        tokio::spawn(async move {
            loop {
                if let Err(e) = adapter.run_subscriber().await {
                    tracing::error!("Redis subscriber error (will retry in 2s): {}", e);
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                }
            }
        });
    }

    async fn run_subscriber(&self) -> anyhow::Result<()> {
        let mut pubsub = self.client.get_async_pubsub().await?;
        let pattern = format!("{CHANNEL_PREFIX}:*");
        pubsub.psubscribe(&pattern).await?;

        tracing::info!(
            node_id = %self.node_id,
            "Redis adapter subscribed to {}",
            pattern
        );

        let mut stream = pubsub.into_on_message();

        while let Some(msg) = stream.next().await {
            let payload: String = match msg.get_payload() {
                Ok(p) => p,
                Err(e) => {
                    tracing::debug!("Failed to read Redis message payload: {}", e);
                    continue;
                }
            };

            let broadcast: BroadcastMessage = match serde_json::from_str(&payload) {
                Ok(b) => b,
                Err(e) => {
                    tracing::debug!("Failed to deserialize broadcast message: {}", e);
                    continue;
                }
            };

            if broadcast.node_id == self.node_id {
                continue;
            }

            let packet = EioPacket::new(
                EioPacketType::from_byte(broadcast.packet_type + b'0')
                    .unwrap_or(EioPacketType::Message),
                broadcast.packet_data,
            );

            if let Some(rooms) = broadcast.rooms {
                for room in &rooms {
                    self.local
                        .broadcast_room(
                            &broadcast.namespace,
                            room,
                            &packet,
                            broadcast.exclude.as_deref(),
                        )
                        .await;
                }
            } else {
                self.local
                    .broadcast_namespace(
                        &broadcast.namespace,
                        &packet,
                        broadcast.exclude.as_deref(),
                    )
                    .await;
            }
        }

        anyhow::bail!("Redis subscription stream ended unexpectedly")
    }

    async fn publish_broadcast(
        &self,
        namespace: &str,
        rooms: Option<Vec<String>>,
        packet: &EioPacket,
        exclude: Option<&str>,
    ) {
        let msg = BroadcastMessage {
            node_id: self.node_id.clone(),
            namespace: namespace.to_string(),
            rooms,
            packet_type: packet.packet_type as u8,
            packet_data: packet.data.clone(),
            exclude: exclude.map(|s| s.to_string()),
        };

        let channel = format!("{CHANNEL_PREFIX}:{namespace}");
        let payload = match serde_json::to_string(&msg) {
            Ok(p) => p,
            Err(e) => {
                tracing::error!("Failed to serialize broadcast message: {}", e);
                return;
            }
        };

        match self.client.get_multiplexed_async_connection().await {
            Ok(mut conn) => {
                let result: Result<(), redis::RedisError> =
                    conn.publish(channel, payload).await;
                if let Err(e) = result {
                    tracing::error!("Failed to PUBLISH to Redis: {}", e);
                }
            }
            Err(e) => {
                tracing::error!("Failed to get Redis connection: {}", e);
            }
        }
    }
}

impl Adapter for RedisAdapter {
    fn register_sender(
        &self,
        socket_id: &str,
        tx: mpsc::Sender<EioPacket>,
    ) -> Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        self.local.register_sender(socket_id, tx)
    }

    fn unregister_sender(
        &self,
        socket_id: &str,
    ) -> Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        self.local.unregister_sender(socket_id)
    }

    fn join(
        &self,
        namespace: &str,
        room: &str,
        socket_id: &str,
    ) -> Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        self.local.join(namespace, room, socket_id)
    }

    fn leave(
        &self,
        namespace: &str,
        room: &str,
        socket_id: &str,
    ) -> Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        self.local.leave(namespace, room, socket_id)
    }

    fn leave_all(
        &self,
        socket_id: &str,
    ) -> Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        self.local.leave_all(socket_id)
    }

    fn broadcast_room(
        &self,
        namespace: &str,
        room: &str,
        packet: &EioPacket,
        exclude: Option<&str>,
    ) -> Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        let namespace_owned = namespace.to_string();
        let room_owned = room.to_string();
        let packet = packet.clone();
        let exclude_owned = exclude.map(|s| s.to_string());
        Box::pin(async move {
            self.local
                .broadcast_room(&namespace_owned, &room_owned, &packet, exclude_owned.as_deref())
                .await;
            self.publish_broadcast(
                &namespace_owned,
                Some(vec![room_owned]),
                &packet,
                exclude_owned.as_deref(),
            )
            .await;
        })
    }

    fn broadcast_namespace(
        &self,
        namespace: &str,
        packet: &EioPacket,
        exclude: Option<&str>,
    ) -> Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        let namespace_owned = namespace.to_string();
        let packet = packet.clone();
        let exclude_owned = exclude.map(|s| s.to_string());
        Box::pin(async move {
            self.local
                .broadcast_namespace(&namespace_owned, &packet, exclude_owned.as_deref())
                .await;
            self.publish_broadcast(
                &namespace_owned,
                None,
                &packet,
                exclude_owned.as_deref(),
            )
            .await;
        })
    }

    fn room_members(
        &self,
        namespace: &str,
        room: &str,
    ) -> Pin<Box<dyn std::future::Future<Output = HashSet<String>> + Send + '_>> {
        self.local.room_members(namespace, room)
    }
}
