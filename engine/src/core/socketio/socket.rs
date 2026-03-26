//! Individual Socket.IO socket: represents one client connected to one namespace.

use std::collections::HashSet;
use std::sync::Arc;

use serde::Serialize;
use serde_json::Value;
use tokio::sync::{mpsc, RwLock};

use std::future::Future;

use super::adapter::Adapter;
use super::engine_io::EioPacket;
use super::engine_io::EioPacketType;
use super::socket_io::SioPacket;
use super::{
    EventHandler, DisconnectHandler, RpressIoInner,
    register_event_handler, register_disconnect_handler,
};

/// A connected Socket.IO socket within a namespace.
///
/// Each socket has a unique `id`, belongs to a namespace, and can join/leave rooms.
/// It holds a sender channel to push Engine.IO message packets to the underlying transport.
pub struct Socket {
    id: String,
    engine_sid: String,
    namespace: String,
    rooms: RwLock<HashSet<String>>,
    /// Sends Engine.IO packets to the transport layer.
    eio_tx: mpsc::Sender<EioPacket>,
    adapter: Arc<dyn Adapter>,
    io_inner: Arc<RpressIoInner>,
    auth: Value,
}

impl Socket {
    pub(crate) fn new(
        id: String,
        engine_sid: String,
        namespace: String,
        eio_tx: mpsc::Sender<EioPacket>,
        adapter: Arc<dyn Adapter>,
        io_inner: Arc<RpressIoInner>,
        auth: Value,
    ) -> Self {
        let mut initial_rooms = HashSet::new();
        initial_rooms.insert(id.clone());

        Self {
            id,
            engine_sid,
            namespace,
            rooms: RwLock::new(initial_rooms),
            eio_tx,
            adapter,
            io_inner,
            auth,
        }
    }

    /// Returns the unique Socket.IO socket ID.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the underlying Engine.IO session ID.
    pub fn engine_sid(&self) -> &str {
        &self.engine_sid
    }

    /// Returns the namespace this socket is connected to.
    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    /// Returns the authentication claims stored during connection.
    ///
    /// If the server has an [`AuthHandler`](super::AuthHandler) configured,
    /// this contains the `Ok(value)` returned by the handler.
    /// Otherwise it is [`Value::Null`].
    pub fn auth(&self) -> &Value {
        &self.auth
    }

    /// Returns the set of rooms this socket has joined.
    pub async fn rooms(&self) -> HashSet<String> {
        self.rooms.read().await.clone()
    }

    /// Joins a room.
    pub async fn join(&self, room: &str) {
        self.rooms.write().await.insert(room.to_string());
        self.adapter
            .join(&self.namespace, room, &self.id)
            .await;
    }

    /// Leaves a room.
    pub async fn leave(&self, room: &str) {
        self.rooms.write().await.remove(room);
        self.adapter
            .leave(&self.namespace, room, &self.id)
            .await;
    }

    /// Emits an event to this socket.
    pub async fn emit<T: Serialize>(&self, event: &str, data: &T) {
        let value = serde_json::to_value(data).unwrap_or(Value::Null);
        let sio_pkt = SioPacket::event(&self.namespace, event, &value, None);
        let eio_pkt = EioPacket::new(EioPacketType::Message, Some(sio_pkt.encode()));
        let _ = self.eio_tx.send(eio_pkt).await;
    }

    /// Emits an event with an acknowledgement ID.
    pub async fn emit_with_ack<T: Serialize>(&self, event: &str, data: &T, ack_id: u64) {
        let value = serde_json::to_value(data).unwrap_or(Value::Null);
        let sio_pkt = SioPacket::event(&self.namespace, event, &value, Some(ack_id));
        let eio_pkt = EioPacket::new(EioPacketType::Message, Some(sio_pkt.encode()));
        let _ = self.eio_tx.send(eio_pkt).await;
    }

    /// Sends a raw Socket.IO packet through the Engine.IO transport.
    pub(crate) async fn send_sio_packet(&self, pkt: &SioPacket) {
        let eio_pkt = EioPacket::new(EioPacketType::Message, Some(pkt.encode()));
        let _ = self.eio_tx.send(eio_pkt).await;
    }

    /// Returns a [`BroadcastBuilder`] that emits to all sockets in the namespace
    /// **except** this one.
    pub fn broadcast(&self) -> BroadcastBuilder {
        BroadcastBuilder {
            namespace: self.namespace.clone(),
            exclude: Some(self.id.clone()),
            rooms: Vec::new(),
            adapter: self.adapter.clone(),
        }
    }

    /// Returns a [`BroadcastBuilder`] targeting a specific room.
    pub fn to(&self, room: &str) -> BroadcastBuilder {
        BroadcastBuilder {
            namespace: self.namespace.clone(),
            exclude: Some(self.id.clone()),
            rooms: vec![room.to_string()],
            adapter: self.adapter.clone(),
        }
    }

    /// Registers an event handler for this socket.
    ///
    /// The handler receives the socket and the event arguments, and may return
    /// a value for acknowledgement.
    pub async fn on<F, Fut>(&self, event: &str, handler: F)
    where
        F: Fn(Arc<Socket>, Vec<Value>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Option<Value>> + Send + 'static,
    {
        let handler: EventHandler = Arc::new(move |socket, data| Box::pin(handler(socket, data)));
        register_event_handler(&self.io_inner, &self.id, event, handler).await;
    }

    /// Registers a disconnect handler for this socket.
    pub async fn on_disconnect<F, Fut>(&self, handler: F)
    where
        F: Fn(Arc<Socket>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let handler: DisconnectHandler = Arc::new(move |socket| Box::pin(handler(socket)));
        register_disconnect_handler(&self.io_inner, &self.id, handler).await;
    }

    /// Sends a DISCONNECT packet and cleans up.
    pub async fn disconnect(&self) {
        let pkt = SioPacket::disconnect(&self.namespace);
        self.send_sio_packet(&pkt).await;
    }

    /// Removes this socket from all rooms.
    pub(crate) async fn leave_all_rooms(&self) {
        let rooms = self.rooms.read().await.clone();
        for room in rooms {
            self.adapter
                .leave(&self.namespace, &room, &self.id)
                .await;
        }
        self.rooms.write().await.clear();
    }
}

/// Builder for broadcasting events to multiple sockets.
pub struct BroadcastBuilder {
    namespace: String,
    exclude: Option<String>,
    rooms: Vec<String>,
    adapter: Arc<dyn Adapter>,
}

impl BroadcastBuilder {
    /// Further restricts the broadcast to a specific room.
    pub fn to(mut self, room: &str) -> Self {
        self.rooms.push(room.to_string());
        self
    }

    /// Emits an event to the targeted sockets.
    pub async fn emit<T: Serialize>(&self, event: &str, data: &T) {
        let value = serde_json::to_value(data).unwrap_or(Value::Null);
        let sio_pkt = SioPacket::event(&self.namespace, event, &value, None);
        let eio_pkt = EioPacket::new(EioPacketType::Message, Some(sio_pkt.encode()));

        if self.rooms.is_empty() {
            self.adapter
                .broadcast_namespace(&self.namespace, &eio_pkt, self.exclude.as_deref())
                .await;
        } else {
            for room in &self.rooms {
                self.adapter
                    .broadcast_room(&self.namespace, room, &eio_pkt, self.exclude.as_deref())
                    .await;
            }
        }
    }
}
