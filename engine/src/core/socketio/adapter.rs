//! Adapter trait for pluggable Socket.IO backends (in-memory, Redis, etc.).

use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;

use tokio::sync::mpsc;

use super::engine_io::EioPacket;

/// Trait for pluggable room/broadcast backends.
///
/// The default in-memory implementation ([`RoomManager`](super::room::RoomManager))
/// works for single-server deployments. For distributed setups (e.g. multiple
/// Rpress instances behind a load balancer), implement this trait with a
/// shared store like Redis.
pub trait Adapter: Send + Sync + 'static {
    /// Registers a socket's outbound channel for packet delivery.
    fn register_sender(
        &self,
        socket_id: &str,
        tx: mpsc::Sender<EioPacket>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>>;

    /// Removes a socket's outbound channel.
    fn unregister_sender(
        &self,
        socket_id: &str,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>>;

    /// Adds a socket to a room.
    fn join(
        &self,
        namespace: &str,
        room: &str,
        socket_id: &str,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>>;

    /// Removes a socket from a room.
    fn leave(
        &self,
        namespace: &str,
        room: &str,
        socket_id: &str,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>>;

    /// Removes a socket from all rooms in all namespaces.
    fn leave_all(
        &self,
        socket_id: &str,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>>;

    /// Broadcasts a packet to all members of a room, optionally excluding one socket.
    fn broadcast_room(
        &self,
        namespace: &str,
        room: &str,
        packet: &EioPacket,
        exclude: Option<&str>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>>;

    /// Broadcasts a packet to all sockets in a namespace, optionally excluding one.
    fn broadcast_namespace(
        &self,
        namespace: &str,
        packet: &EioPacket,
        exclude: Option<&str>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>>;

    /// Returns all socket IDs in a room.
    fn room_members(
        &self,
        namespace: &str,
        room: &str,
    ) -> Pin<Box<dyn Future<Output = HashSet<String>> + Send + '_>>;
}
