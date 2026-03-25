//! Public Socket.IO client API.

use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::{Context, Result};
use serde_json::Value;
use tokio::sync::{RwLock, mpsc, oneshot};
use tracing::debug;

use crate::engine_io::{EioPacket, EioPacketType};
use crate::socket_io::{SioPacket, SioPacketType};
use crate::transport;

type EventCallback = Arc<dyn Fn(Vec<Value>) -> std::pin::Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;

/// A Socket.IO client that connects to a remote server.
///
/// # Example
///
/// ```rust,no_run
/// use rpress_client::SocketIoClient;
///
/// #[tokio::main]
/// async fn main() -> anyhow::Result<()> {
///     let mut client = SocketIoClient::connect("http://localhost:3000").await?;
///
///     client.on("chat message", |data| async move {
///         println!("Received: {:?}", data);
///     }).await;
///
///     client.emit("chat message", &serde_json::json!("Hello!")).await?;
///     client.disconnect().await?;
///     Ok(())
/// }
/// ```
pub struct SocketIoClient {
    sid: String,
    #[allow(dead_code)]
    engine_sid: String,
    namespace: String,
    tx: mpsc::Sender<EioPacket>,
    event_handlers: Arc<RwLock<HashMap<String, EventCallback>>>,
    ack_pending: Arc<RwLock<HashMap<u64, oneshot::Sender<Value>>>>,
    ack_counter: Arc<AtomicU64>,
    connected: Arc<AtomicBool>,
}

impl SocketIoClient {
    /// Connects to a Socket.IO server on the default namespace (`/`).
    ///
    /// # Arguments
    ///
    /// * `url` — The server URL, e.g. `"http://localhost:3000"`.
    pub async fn connect(url: &str) -> Result<Self> {
        Self::connect_to(url, "/").await
    }

    /// Connects to a Socket.IO server on a specific namespace.
    ///
    /// # Arguments
    ///
    /// * `url` — The server URL.
    /// * `namespace` — The namespace to connect to (e.g. `"/admin"`).
    pub async fn connect_to(url: &str, namespace: &str) -> Result<Self> {
        let handle = transport::connect(url, namespace).await?;

        let engine_sid = handle.handshake.sid.clone();
        let tx = handle.tx;
        let mut rx = handle.rx;

        let sio_connect = SioPacket::connect(namespace, None);
        let eio_msg = EioPacket::new(EioPacketType::Message, Some(sio_connect.encode()));
        tx.send(eio_msg).await.context("failed to send SIO CONNECT")?;

        let connect_response = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            Self::wait_for_connect(&mut rx),
        )
        .await
        .context("timeout waiting for SIO CONNECT response")?
        .context("failed to receive SIO CONNECT response")?;

        let sid = connect_response
            .data
            .as_ref()
            .and_then(|d| d.get("sid"))
            .and_then(|s| s.as_str())
            .unwrap_or(&engine_sid)
            .to_string();

        debug!(sid = %sid, namespace = %namespace, "Socket.IO connected");

        let event_handlers: Arc<RwLock<HashMap<String, EventCallback>>> =
            Arc::new(RwLock::new(HashMap::new()));
        let ack_pending: Arc<RwLock<HashMap<u64, oneshot::Sender<Value>>>> =
            Arc::new(RwLock::new(HashMap::new()));
        let connected = Arc::new(AtomicBool::new(true));

        let client = Self {
            sid,
            engine_sid,
            namespace: namespace.to_string(),
            tx,
            event_handlers: Arc::clone(&event_handlers),
            ack_pending: Arc::clone(&ack_pending),
            ack_counter: Arc::new(AtomicU64::new(0)),
            connected: Arc::clone(&connected),
        };

        tokio::spawn(Self::dispatch_loop(
            rx,
            event_handlers,
            ack_pending,
            connected,
        ));

        Ok(client)
    }

    async fn wait_for_connect(rx: &mut mpsc::Receiver<EioPacket>) -> Result<SioPacket> {
        while let Some(pkt) = rx.recv().await {
            if pkt.packet_type != EioPacketType::Message {
                continue;
            }
            let Some(data) = &pkt.data else { continue };
            let Some(sio) = SioPacket::decode(data) else {
                continue;
            };

            match sio.packet_type {
                SioPacketType::Connect => return Ok(sio),
                SioPacketType::ConnectError => {
                    let msg = sio
                        .data
                        .as_ref()
                        .and_then(|d| d.get("message"))
                        .and_then(|m| m.as_str())
                        .unwrap_or("unknown error");
                    anyhow::bail!("Socket.IO connection refused: {msg}");
                }
                _ => continue,
            }
        }
        anyhow::bail!("connection closed before CONNECT response")
    }

    /// Background task that routes incoming packets to event handlers and ack resolvers.
    async fn dispatch_loop(
        mut rx: mpsc::Receiver<EioPacket>,
        handlers: Arc<RwLock<HashMap<String, EventCallback>>>,
        ack_pending: Arc<RwLock<HashMap<u64, oneshot::Sender<Value>>>>,
        connected: Arc<AtomicBool>,
    ) {
        while let Some(pkt) = rx.recv().await {
            if pkt.packet_type != EioPacketType::Message {
                continue;
            }
            let Some(data) = &pkt.data else { continue };
            let Some(sio) = SioPacket::decode(data) else {
                continue;
            };

            match sio.packet_type {
                SioPacketType::Event => {
                    if let Some(name) = sio.event_name() {
                        let args = sio.event_data().unwrap_or_default();
                        let guard = handlers.read().await;
                        if let Some(handler) = guard.get(name) {
                            let handler = Arc::clone(handler);
                            drop(guard);
                            handler(args).await;
                        }
                    }
                }
                SioPacketType::Ack => {
                    if let Some(ack_id) = sio.id {
                        let mut guard = ack_pending.write().await;
                        if let Some(sender) = guard.remove(&ack_id) {
                            let value = sio.data.unwrap_or(Value::Null);
                            let _ = sender.send(value);
                        }
                    }
                }
                SioPacketType::Disconnect => {
                    debug!("server-side disconnect received");
                    connected.store(false, Ordering::SeqCst);
                    break;
                }
                _ => {}
            }
        }
        connected.store(false, Ordering::SeqCst);
        debug!("dispatch loop exiting");
    }

    /// Registers an event handler. The callback receives event arguments as `Vec<Value>`.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use rpress_client::SocketIoClient;
    /// # async fn example(client: &mut SocketIoClient) {
    /// client.on("message", |args| async move {
    ///     println!("got message: {:?}", args);
    /// }).await;
    /// # }
    /// ```
    pub async fn on<F, Fut>(&self, event: &str, handler: F)
    where
        F: Fn(Vec<Value>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let handler: EventCallback =
            Arc::new(move |args| Box::pin(handler(args)));
        self.event_handlers
            .write()
            .await
            .insert(event.to_string(), handler);
    }

    /// Emits an event to the server without expecting an acknowledgement.
    ///
    /// # Arguments
    ///
    /// * `event` — The event name.
    /// * `data` — A JSON value to send as payload.
    pub async fn emit(&self, event: &str, data: &Value) -> Result<()> {
        if !self.connected.load(Ordering::SeqCst) {
            anyhow::bail!("not connected");
        }
        let sio = SioPacket::event(&self.namespace, event, data, None);
        let eio = EioPacket::new(EioPacketType::Message, Some(sio.encode()));
        self.tx
            .send(eio)
            .await
            .context("failed to send event")?;
        Ok(())
    }

    /// Emits an event and waits for the server's acknowledgement (default 5s timeout).
    ///
    /// Returns the acknowledgement payload as a [`serde_json::Value`].
    pub async fn emit_with_ack(&self, event: &str, data: &Value) -> Result<Value> {
        self.emit_with_ack_timeout(event, data, std::time::Duration::from_secs(5))
            .await
    }

    /// Emits an event and waits for the server's acknowledgement with a custom timeout.
    pub async fn emit_with_ack_timeout(
        &self,
        event: &str,
        data: &Value,
        timeout: std::time::Duration,
    ) -> Result<Value> {
        if !self.connected.load(Ordering::SeqCst) {
            anyhow::bail!("not connected");
        }

        let ack_id = self.ack_counter.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();

        self.ack_pending.write().await.insert(ack_id, tx);

        let sio = SioPacket::event(&self.namespace, event, data, Some(ack_id));
        let eio = EioPacket::new(EioPacketType::Message, Some(sio.encode()));
        self.tx
            .send(eio)
            .await
            .context("failed to send event with ack")?;

        let value = tokio::time::timeout(timeout, rx)
            .await
            .context("ack timeout")?
            .context("ack channel closed")?;

        Ok(value)
    }

    /// Gracefully disconnects from the server.
    pub async fn disconnect(&self) -> Result<()> {
        if !self.connected.load(Ordering::SeqCst) {
            return Ok(());
        }

        let sio = SioPacket::disconnect(&self.namespace);
        let eio = EioPacket::new(EioPacketType::Message, Some(sio.encode()));
        let _ = self.tx.send(eio).await;
        self.connected.store(false, Ordering::SeqCst);

        debug!("disconnected");
        Ok(())
    }

    /// Returns the Socket.IO session ID.
    pub fn id(&self) -> &str {
        &self.sid
    }

    /// Returns the namespace this client is connected to.
    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    /// Returns `true` if the client is still connected.
    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }
}
