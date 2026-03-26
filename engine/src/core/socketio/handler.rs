//! Engine.IO / Socket.IO request handler: processes polling and WebSocket requests.

use std::sync::Arc;

use serde_json::Value;
use tokio::sync::mpsc;

use crate::core::handler_response::ResponsePayload;
use crate::types::definitions::StatusCode;

use super::engine_io::{EioPacket, EioPacketType, TransportType};
use super::socket::Socket;
use super::socket_io::{SioPacket, SioPacketType};
use super::transport::EioQueryParams;
use super::RpressIoInner;

/// Result of handling a Socket.IO HTTP request.
pub(crate) enum SioHttpResult {
    /// A normal HTTP response (for polling).
    Response(ResponsePayload),
    /// The request is a WebSocket upgrade — the caller should upgrade the socket.
    WebSocketUpgrade(String),
}

/// Handles an incoming HTTP request on the Socket.IO path.
pub(crate) async fn handle_sio_request(
    inner: &Arc<RpressIoInner>,
    method: &str,
    _uri: &str,
    query: &str,
    body: &[u8],
    has_upgrade_header: bool,
) -> SioHttpResult {
    let params = EioQueryParams::parse(query);

    if params.eio != Some(4) {
        return SioHttpResult::Response(
            ResponsePayload::text(r#"{"code":5,"message":"UNSUPPORTED"}"#)
                .with_status(StatusCode::BadRequest)
                .with_content_type("application/json"),
        );
    }

    if params.is_websocket() {
        if has_upgrade_header {
            let sid = params.sid.unwrap_or_default();
            return SioHttpResult::WebSocketUpgrade(sid);
        }
        return SioHttpResult::Response(
            ResponsePayload::text(r#"{"code":3,"message":"BAD_REQUEST"}"#)
                .with_status(StatusCode::BadRequest)
                .with_content_type("application/json"),
        );
    }

    if !params.is_polling() {
        return SioHttpResult::Response(
            ResponsePayload::text(r#"{"code":0,"message":"Transport unknown"}"#)
                .with_status(StatusCode::BadRequest)
                .with_content_type("application/json"),
        );
    }

    if inner.config.websocket_only {
        return SioHttpResult::Response(
            ResponsePayload::text(r#"{"code":3,"message":"Polling transport disabled. Use WebSocket: { transports: [\"websocket\"] }"}"#)
                .with_status(StatusCode::BadRequest)
                .with_content_type("application/json"),
        );
    }

    match (method, params.sid.as_deref()) {
        ("GET", None) => handle_polling_handshake(inner).await,
        ("GET", Some(sid)) => handle_polling_get(inner, sid).await,
        ("POST", Some(sid)) => handle_polling_post(inner, sid, body).await,
        ("POST", None) => SioHttpResult::Response(
            ResponsePayload::text(r#"{"code":1,"message":"Session ID unknown"}"#)
                .with_status(StatusCode::BadRequest)
                .with_content_type("application/json"),
        ),
        _ => SioHttpResult::Response(
            ResponsePayload::text(r#"{"code":2,"message":"Bad handshake method"}"#)
                .with_status(StatusCode::BadRequest)
                .with_content_type("application/json"),
        ),
    }
}

/// Creates a new Engine.IO session (polling handshake).
async fn handle_polling_handshake(inner: &Arc<RpressIoInner>) -> SioHttpResult {
    let (sid, mut rx) = inner.session_store.create_session();
    let handshake = inner.session_store.get_handshake_data(&sid).unwrap();

    let open_json = serde_json::to_string(&handshake).unwrap();
    let open_packet = EioPacket::new(EioPacketType::Open, Some(open_json));

    let inner_clone = inner.clone();
    let sid_clone = sid.clone();
    tokio::spawn(async move {
        run_session_loop(&inner_clone, &sid_clone, &mut rx).await;
    });

    SioHttpResult::Response(
        ResponsePayload::text(open_packet.encode())
            .with_content_type("text/plain; charset=UTF-8"),
    )
}

/// Handles a polling GET: waits for data to send to the client.
async fn handle_polling_get(inner: &Arc<RpressIoInner>, sid: &str) -> SioHttpResult {
    if !inner.session_store.is_valid(sid) {
        return SioHttpResult::Response(
            ResponsePayload::text(r#"{"code":1,"message":"Session ID unknown"}"#)
                .with_status(StatusCode::BadRequest)
                .with_content_type("application/json"),
        );
    }

    let (poll_notify, poll_buffer) = {
        let session = inner.session_store.sessions.get(sid);
        match session {
            Some(s) => (s.poll_notify.clone(), s.poll_buffer.clone()),
            None => {
                return SioHttpResult::Response(
                    ResponsePayload::text(r#"{"code":1,"message":"Session ID unknown"}"#)
                        .with_status(StatusCode::BadRequest)
                        .with_content_type("application/json"),
                )
            }
        }
    };

    // Wait for data or timeout
    let wait_result = tokio::time::timeout(
        inner.config.ping_interval + inner.config.ping_timeout,
        poll_notify.notified(),
    )
    .await;

    let mut buffer = poll_buffer.write().await;
    if buffer.is_empty() {
        if wait_result.is_err() {
            // Timed out with no data — send noop
            buffer.push(EioPacket::new(EioPacketType::Noop, None));
        }
    }

    let packets: Vec<EioPacket> = buffer.drain(..).collect();
    drop(buffer);

    if packets.is_empty() {
        return SioHttpResult::Response(
            ResponsePayload::text(EioPacket::new(EioPacketType::Noop, None).encode())
                .with_content_type("text/plain; charset=UTF-8"),
        );
    }

    let encoded = EioPacket::encode_polling_payload(&packets);
    SioHttpResult::Response(
        ResponsePayload::text(encoded).with_content_type("text/plain; charset=UTF-8"),
    )
}

/// Handles a polling POST: receives packets from the client.
async fn handle_polling_post(
    inner: &Arc<RpressIoInner>,
    sid: &str,
    body: &[u8],
) -> SioHttpResult {
    if !inner.session_store.is_valid(sid) {
        return SioHttpResult::Response(
            ResponsePayload::text(r#"{"code":1,"message":"Session ID unknown"}"#)
                .with_status(StatusCode::BadRequest)
                .with_content_type("application/json"),
        );
    }

    let body_str = String::from_utf8_lossy(body);
    let packets = EioPacket::decode_polling_payload(&body_str);

    for pkt in packets {
        process_incoming_eio_packet(inner, sid, &pkt).await;
    }

    SioHttpResult::Response(
        ResponsePayload::text("ok").with_content_type("text/plain; charset=UTF-8"),
    )
}

/// Processes a single incoming Engine.IO packet (from any transport).
pub(crate) async fn process_incoming_eio_packet(
    inner: &Arc<RpressIoInner>,
    sid: &str,
    pkt: &EioPacket,
) {
    match pkt.packet_type {
        EioPacketType::Pong => {
            inner.session_store.mark_pong(sid);
        }
        EioPacketType::Message => {
            if let Some(ref data) = pkt.data {
                if let Some(sio_pkt) = SioPacket::decode(data) {
                    process_sio_packet(inner, sid, &sio_pkt).await;
                }
            }
        }
        EioPacketType::Close => {
            handle_disconnect(inner, sid).await;
        }
        EioPacketType::Upgrade => {
            inner
                .session_store
                .set_transport(sid, TransportType::WebSocket);
        }
        _ => {}
    }
}

/// Processes a decoded Socket.IO packet.
async fn process_sio_packet(inner: &Arc<RpressIoInner>, engine_sid: &str, pkt: &SioPacket) {
    match pkt.packet_type {
        SioPacketType::Connect => {
            handle_sio_connect(inner, engine_sid, &pkt.namespace, pkt.data.as_ref()).await;
        }
        SioPacketType::Disconnect => {
            handle_sio_disconnect(inner, engine_sid, &pkt.namespace).await;
        }
        SioPacketType::Event => {
            if let (Some(event_name), Some(event_data)) = (pkt.event_name(), pkt.event_data()) {
                handle_sio_event(
                    inner,
                    engine_sid,
                    &pkt.namespace,
                    event_name,
                    event_data,
                    pkt.id,
                )
                .await;
            }
        }
        SioPacketType::Ack => {
            // Client-side acks — currently no server-side awaiting acks
        }
        SioPacketType::BinaryEvent | SioPacketType::BinaryAck => {
            // Binary events are forwarded as regular events for now
            if let (Some(event_name), Some(event_data)) = (pkt.event_name(), pkt.event_data()) {
                handle_sio_event(
                    inner,
                    engine_sid,
                    &pkt.namespace,
                    event_name,
                    event_data,
                    pkt.id,
                )
                .await;
            }
        }
        _ => {}
    }
}

/// Handles Socket.IO CONNECT: validates auth, creates a socket, joins namespace, calls connection handler.
async fn handle_sio_connect(
    inner: &Arc<RpressIoInner>,
    engine_sid: &str,
    namespace: &str,
    auth_data: Option<&Value>,
) {
    let (has_namespace, auth_handler) = {
        let namespaces = inner.namespaces.read().unwrap();
        match namespaces.get(namespace) {
            Some(ns) => (true, ns.auth_handler.clone()),
            None => (false, None),
        }
    };

    let eio_tx = {
        let session = inner.session_store.sessions.get(engine_sid);
        match session {
            Some(s) => s.tx.clone(),
            None => return,
        }
    };

    if !has_namespace {
        let err_pkt = SioPacket::connect_error(namespace, "Invalid namespace");
        let eio_pkt = EioPacket::new(EioPacketType::Message, Some(err_pkt.encode()));
        let _ = eio_tx.send(eio_pkt).await;
        return;
    }

    let auth_claims = if let Some(handler) = auth_handler {
        let data = auth_data.cloned().unwrap_or(Value::Null);
        match handler(data).await {
            Ok(claims) => claims,
            Err(msg) => {
                tracing::debug!("Socket.IO auth rejected for namespace {}: {}", namespace, msg);
                let err_pkt = SioPacket::connect_error(namespace, &msg);
                let eio_pkt = EioPacket::new(EioPacketType::Message, Some(err_pkt.encode()));
                let _ = eio_tx.send(eio_pkt).await;
                return;
            }
        }
    } else {
        Value::Null
    };

    let socket_id = uuid::Uuid::new_v4().to_string();

    let socket = Arc::new(Socket::new(
        socket_id.clone(),
        engine_sid.to_string(),
        namespace.to_string(),
        eio_tx.clone(),
        inner.adapter.clone(),
        inner.clone(),
        auth_claims,
    ));

    inner
        .adapter
        .register_sender(&socket_id, eio_tx.clone())
        .await;

    inner
        .adapter
        .join(namespace, &socket_id, &socket_id)
        .await;

    inner
        .sockets
        .write()
        .await
        .insert(socket_id.clone(), socket.clone());

    let connect_ok = SioPacket::connect_ok(namespace, &socket_id);
    let eio_pkt = EioPacket::new(EioPacketType::Message, Some(connect_ok.encode()));
    let _ = eio_tx.send(eio_pkt).await;

    let handler = {
        let namespaces = inner.namespaces.read().unwrap();
        namespaces
            .get(namespace)
            .and_then(|ns| ns.connection_handler.clone())
    };

    if let Some(handler) = handler {
        handler(socket).await;
    }
}

/// Handles Socket.IO EVENT: dispatches to registered event handlers.
async fn handle_sio_event(
    inner: &Arc<RpressIoInner>,
    engine_sid: &str,
    namespace: &str,
    event_name: &str,
    event_data: Vec<Value>,
    ack_id: Option<u64>,
) {
    // Find the socket for this engine_sid + namespace
    let socket_id = {
        let sockets = inner.sockets.read().await;
        sockets
            .iter()
            .find(|(_, s)| s.engine_sid() == engine_sid && s.namespace() == namespace)
            .map(|(id, _)| id.clone())
    };

    let Some(socket_id) = socket_id else {
        return;
    };

    // Get the event handler
    let handler = {
        let handlers = inner.socket_handlers.read().await;
        handlers
            .get(&socket_id)
            .and_then(|h| h.events.get(event_name).cloned())
    };

    let socket = {
        let sockets = inner.sockets.read().await;
        sockets.get(&socket_id).cloned()
    };

    if let (Some(handler), Some(socket)) = (handler, socket) {
        let ack_result = handler(socket.clone(), event_data).await;

        if let (Some(ack_id), Some(ack_data)) = (ack_id, ack_result) {
            let ack_pkt = SioPacket::ack(namespace, ack_id, &ack_data);
            socket.send_sio_packet(&ack_pkt).await;
        }
    }
}

/// Handles Socket.IO DISCONNECT for a specific namespace.
async fn handle_sio_disconnect(inner: &Arc<RpressIoInner>, engine_sid: &str, namespace: &str) {
    let socket_id = {
        let sockets = inner.sockets.read().await;
        sockets
            .iter()
            .find(|(_, s)| s.engine_sid() == engine_sid && s.namespace() == namespace)
            .map(|(id, _)| id.clone())
    };

    if let Some(socket_id) = socket_id {
        cleanup_socket(inner, &socket_id).await;
    }
}

/// Full disconnect: removes all sockets for an Engine.IO session.
async fn handle_disconnect(inner: &Arc<RpressIoInner>, engine_sid: &str) {
    let socket_ids: Vec<String> = {
        let sockets = inner.sockets.read().await;
        sockets
            .iter()
            .filter(|(_, s)| s.engine_sid() == engine_sid)
            .map(|(id, _)| id.clone())
            .collect()
    };

    for socket_id in socket_ids {
        cleanup_socket(inner, &socket_id).await;
    }

    inner.session_store.remove_session(engine_sid);
}

/// Cleans up a socket: calls disconnect handler, leaves rooms, removes from stores.
async fn cleanup_socket(inner: &Arc<RpressIoInner>, socket_id: &str) {
    // Call disconnect handler
    let handler = {
        let handlers = inner.socket_handlers.read().await;
        handlers
            .get(socket_id)
            .and_then(|h| h.disconnect_handler.clone())
    };

    let socket = inner.sockets.write().await.remove(socket_id);
    if let Some(socket) = socket {
        if let Some(handler) = handler {
            handler(socket.clone()).await;
        }
        socket.leave_all_rooms().await;
    }

    inner.adapter.leave_all(socket_id).await;
    inner.adapter.unregister_sender(socket_id).await;
    inner.socket_handlers.write().await.remove(socket_id);
}

/// Background loop for an Engine.IO session: delivers packets to polling buffer or WS,
/// and runs heartbeat pings.
async fn run_session_loop(
    inner: &Arc<RpressIoInner>,
    sid: &str,
    rx: &mut mpsc::Receiver<EioPacket>,
) {
    let ping_interval = inner.config.ping_interval;
    let ping_timeout = inner.config.ping_timeout;

    let mut ping_timer = tokio::time::interval(ping_interval);
    ping_timer.tick().await; // consume immediate tick

    loop {
        tokio::select! {
            _ = ping_timer.tick() => {
                // Send ping
                let session = inner.session_store.sessions.get(sid);
                let Some(session) = session else { break };

                if session.last_pong.elapsed() > ping_interval + ping_timeout {
                    tracing::debug!("Engine.IO session {} timed out (no pong)", sid);
                    drop(session);
                    handle_disconnect(inner, sid).await;
                    break;
                }

                match session.transport {
                    TransportType::Polling => {
                        let ping = EioPacket::new(EioPacketType::Ping, None);
                        session.poll_buffer.write().await.push(ping);
                        session.poll_notify.notify_one();
                    }
                    TransportType::WebSocket | TransportType::Upgrading => {
                        let ping = EioPacket::new(EioPacketType::Ping, None);
                        let _ = session.tx.send(ping).await;
                    }
                }
            }
            pkt = rx.recv() => {
                let Some(pkt) = pkt else { break };

                let session = inner.session_store.sessions.get(sid);
                let Some(session) = session else { break };

                match session.transport {
                    TransportType::Polling => {
                        session.poll_buffer.write().await.push(pkt);
                        session.poll_notify.notify_one();
                    }
                    TransportType::WebSocket | TransportType::Upgrading => {
                        // For WebSocket transport, packets go directly through the
                        // WebSocket writer task, not through the polling buffer.
                        // The ws writer reads from its own channel copy.
                        // This path handles messages that get routed through
                        // the session tx (e.g. from broadcast/room emit).
                        session.poll_buffer.write().await.push(pkt);
                        session.poll_notify.notify_one();
                    }
                }
            }
        }
    }
}

/// Handles the WebSocket connection after upgrade (called from lib.rs).
pub(crate) async fn handle_websocket_connection<S>(
    inner: Arc<RpressIoInner>,
    ws_stream: tokio_tungstenite::WebSocketStream<S>,
    sid: String,
) where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;

    let (mut ws_tx, mut ws_rx) = ws_stream.split();

    let is_upgrade = !sid.is_empty() && inner.session_store.is_valid(&sid);

    let effective_sid = if is_upgrade {
        // Upgrading from polling — session already exists
        sid
    } else {
        // Direct WebSocket connection — create new session
        let (new_sid, mut rx) = inner.session_store.create_session();
        inner
            .session_store
            .set_transport(&new_sid, TransportType::WebSocket);

        // Send open packet
        let handshake = inner.session_store.get_handshake_data(&new_sid).unwrap();
        let open_json = serde_json::to_string(&handshake).unwrap();
        let open_pkt = EioPacket::new(EioPacketType::Open, Some(open_json));
        let _ = ws_tx.send(Message::Text(open_pkt.encode().into())).await;

        // Start session loop
        let inner_clone = inner.clone();
        let sid_clone = new_sid.clone();
        tokio::spawn(async move {
            run_session_loop(&inner_clone, &sid_clone, &mut rx).await;
        });

        new_sid
    };

    // Writer task: reads from poll_buffer and sends to WebSocket
    let (poll_notify, poll_buffer) = {
        let session = inner.session_store.sessions.get(&effective_sid);
        match session {
            Some(s) => (s.poll_notify.clone(), s.poll_buffer.clone()),
            None => return,
        }
    };

    let writer_handle = tokio::spawn(async move {
        loop {
            poll_notify.notified().await;

            let packets: Vec<EioPacket> = {
                let mut buf = poll_buffer.write().await;
                buf.drain(..).collect()
            };

            for pkt in packets {
                let msg = Message::Text(pkt.encode().into());
                if ws_tx.send(msg).await.is_err() {
                    return;
                }
            }
        }
    });

    // Reader loop: reads from WebSocket and processes packets
    while let Some(msg) = ws_rx.next().await {
        let msg = match msg {
            Ok(m) => m,
            Err(_) => break,
        };

        match msg {
            Message::Text(text) => {
                if let Some(pkt) = EioPacket::decode(&text) {
                    // Handle probe during upgrade
                    if pkt.packet_type == EioPacketType::Ping
                        && pkt.data.as_deref() == Some("probe")
                    {
                        let pong = EioPacket::new(
                            EioPacketType::Pong,
                            Some("probe".to_string()),
                        );
                        // Send pong probe directly via poll_buffer+notify
                        let session = inner.session_store.sessions.get(&effective_sid);
                        if let Some(s) = session {
                            s.poll_buffer.write().await.push(pong);
                            s.poll_notify.notify_one();
                        }
                        continue;
                    }

                    if pkt.packet_type == EioPacketType::Upgrade {
                        inner
                            .session_store
                            .set_transport(&effective_sid, TransportType::WebSocket);
                        continue;
                    }

                    process_incoming_eio_packet(&inner, &effective_sid, &pkt).await;
                }
            }
            Message::Binary(_) => {
                // Binary frames for BINARY_EVENT attachments — future extension
            }
            Message::Close(_) => break,
            Message::Ping(_data) => {
                // WebSocket-level ping (not Engine.IO ping)
                let session = inner.session_store.sessions.get(&effective_sid);
                if let Some(s) = session {
                    let pong_pkt = EioPacket::new(EioPacketType::Pong, None);
                    s.poll_buffer.write().await.push(pong_pkt);
                    s.poll_notify.notify_one();
                }
            }
            _ => {}
        }
    }

    writer_handle.abort();
    handle_disconnect(&inner, &effective_sid).await;
}
