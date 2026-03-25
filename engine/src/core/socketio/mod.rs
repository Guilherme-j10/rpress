//! Socket.IO server implementation compatible with Socket.IO v4+ clients.
//!
//! Built on Engine.IO v4 (HTTP long-polling + WebSocket transport) and
//! Socket.IO protocol v5 (namespaces, events, acknowledgements, rooms).

pub(crate) mod engine_io;
pub(crate) mod socket_io;
pub mod socket;
pub mod room;
pub(crate) mod transport;
pub mod adapter;
pub(crate) mod handler;

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde::Serialize;
use serde_json::Value;
use tokio::sync::RwLock;

/// Sync RwLock for namespace configuration (registered before server starts).
type NamespaceLock = std::sync::RwLock<HashMap<String, NamespaceConfig>>;

use engine_io::{EioConfig, EioPacket, EioPacketType, EioSessionStore};
use room::RoomManager;
use socket::Socket;
use socket_io::SioPacket;

/// Type alias for the connection handler callback.
pub type ConnectionHandler = Arc<
    dyn Fn(Arc<Socket>) -> Pin<Box<dyn Future<Output = ()> + Send + 'static>>
        + Send
        + Sync
        + 'static,
>;

/// Type alias for event handler callbacks on a socket.
pub type EventHandler = Arc<
    dyn Fn(Arc<Socket>, Vec<Value>) -> Pin<Box<dyn Future<Output = Option<Value>> + Send + 'static>>
        + Send
        + Sync
        + 'static,
>;

/// Type alias for disconnect handler callbacks.
pub type DisconnectHandler = Arc<
    dyn Fn(Arc<Socket>) -> Pin<Box<dyn Future<Output = ()> + Send + 'static>>
        + Send
        + Sync
        + 'static,
>;

/// Configuration for a Socket.IO namespace.
pub(crate) struct NamespaceConfig {
    pub connection_handler: Option<ConnectionHandler>,
}

impl Default for NamespaceConfig {
    fn default() -> Self {
        Self {
            connection_handler: None,
        }
    }
}

/// A namespace handle for registering connection handlers.
pub struct NamespaceBuilder {
    io: Arc<RpressIoInner>,
    namespace: String,
}

impl NamespaceBuilder {
    /// Registers a handler called when a socket connects to this namespace.
    pub fn on_connection<F, Fut>(&self, handler: F)
    where
        F: Fn(Arc<Socket>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let handler: ConnectionHandler = Arc::new(move |socket| Box::pin(handler(socket)));
        let mut namespaces = self.io.namespaces.write().unwrap();
        let config = namespaces.entry(self.namespace.clone()).or_default();
        config.connection_handler = Some(handler);
    }
}

/// Stores per-socket event handlers and disconnect handler.
pub(crate) struct SocketHandlers {
    pub events: HashMap<String, EventHandler>,
    pub disconnect_handler: Option<DisconnectHandler>,
}

impl Default for SocketHandlers {
    fn default() -> Self {
        Self {
            events: HashMap::new(),
            disconnect_handler: None,
        }
    }
}

pub(crate) struct RpressIoInner {
    pub(crate) namespaces: NamespaceLock,
    pub(crate) session_store: EioSessionStore,
    pub(crate) room_manager: Arc<RoomManager>,
    pub(crate) socket_handlers: RwLock<HashMap<String, SocketHandlers>>,
    pub(crate) sockets: RwLock<HashMap<String, Arc<Socket>>>,
    pub(crate) config: EioConfig,
    pub(crate) path: String,
}

/// Socket.IO server — the main entry point for real-time communication.
///
/// Compatible with `socket.io-client` v4+ (Engine.IO v4, Socket.IO protocol v5).
///
/// # Example
///
/// ```rust,no_run
/// use rpress::{Rpress, RpressIo};
/// use std::sync::Arc;
///
/// #[tokio::main]
/// async fn main() -> anyhow::Result<()> {
///     let io = RpressIo::new();
///     io.on_connection(|socket| async move {
///         println!("Connected: {}", socket.id());
///     });
///
///     let mut app = Rpress::new(None);
///     app.attach_socketio(io);
///     app.listen("0.0.0.0:3000").await
/// }
/// ```
pub struct RpressIo {
    pub(crate) inner: Arc<RpressIoInner>,
}

impl RpressIo {
    /// Creates a new Socket.IO server with default configuration.
    pub fn new() -> Self {
        Self::with_config(EioConfig::default())
    }

    /// Creates a new Socket.IO server with custom Engine.IO configuration.
    pub fn with_config(config: EioConfig) -> Self {
        let room_manager = Arc::new(RoomManager::new());
        let inner = Arc::new(RpressIoInner {
            namespaces: std::sync::RwLock::new(HashMap::new()),
            session_store: EioSessionStore::new(config.clone()),
            room_manager,
            socket_handlers: RwLock::new(HashMap::new()),
            sockets: RwLock::new(HashMap::new()),
            config,
            path: "/socket.io/".to_string(),
        });
        Self { inner }
    }

    /// Sets the path prefix for Socket.IO requests (default: `/socket.io/`).
    pub fn set_path(&mut self, path: &str) {
        let inner = Arc::get_mut(&mut self.inner)
            .expect("set_path must be called before attach_socketio");
        inner.path = if path.ends_with('/') {
            path.to_string()
        } else {
            format!("{path}/")
        };
    }

    /// Registers a connection handler for the default namespace (`/`).
    pub fn on_connection<F, Fut>(&self, handler: F)
    where
        F: Fn(Arc<Socket>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.of("/").on_connection(handler);
    }

    /// Returns a [`NamespaceBuilder`] for configuring a specific namespace.
    pub fn of(&self, namespace: &str) -> NamespaceBuilder {
        NamespaceBuilder {
            io: self.inner.clone(),
            namespace: namespace.to_string(),
        }
    }

    /// Emits an event to all sockets in the default namespace.
    pub async fn emit<T: Serialize>(&self, event: &str, data: &T) {
        let value = serde_json::to_value(data).unwrap_or(Value::Null);
        let sio_pkt = SioPacket::event("/", event, &value, None);
        let eio_pkt = EioPacket::new(EioPacketType::Message, Some(sio_pkt.encode()));
        self.inner
            .room_manager
            .broadcast_namespace("/", &eio_pkt, None)
            .await;
    }

    /// Returns the configured path prefix.
    pub fn path(&self) -> &str {
        &self.inner.path
    }
}

impl Default for RpressIo {
    fn default() -> Self {
        Self::new()
    }
}

/// Registers an event handler on a socket (called from user-facing API).
pub(crate) async fn register_event_handler(
    inner: &RpressIoInner,
    socket_id: &str,
    event: &str,
    handler: EventHandler,
) {
    let mut handlers = inner.socket_handlers.write().await;
    let entry = handlers.entry(socket_id.to_string()).or_default();
    entry.events.insert(event.to_string(), handler);
}

/// Registers a disconnect handler on a socket.
pub(crate) async fn register_disconnect_handler(
    inner: &RpressIoInner,
    socket_id: &str,
    handler: DisconnectHandler,
) {
    let mut handlers = inner.socket_handlers.write().await;
    let entry = handlers.entry(socket_id.to_string()).or_default();
    entry.disconnect_handler = Some(handler);
}
