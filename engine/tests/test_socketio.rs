mod helpers;

use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use rpress::{EioConfig, RpressIo, RpressRoutes};
use rpress::core::handler_response::ResponsePayload;
use rpress::types::definitions::RequestPayload;
use serde_json::Value;
use tokio::sync::Notify;
use tokio_tungstenite::tungstenite::Message;

use helpers::{parse_response, send_raw_request, start_test_server_custom};

/// Starts a test server with Socket.IO attached and returns the address.
async fn start_sio_server<F>(configure_io: F) -> (String, tokio::task::JoinHandle<()>)
where
    F: FnOnce(&RpressIo) + Send + 'static,
{
    let routes = RpressRoutes::new();
    start_test_server_custom(None, routes, move |app| {
        let io = RpressIo::new();
        configure_io(&io);
        app.attach_socketio(io);
    })
    .await
}

/// Starts a test server with Socket.IO using a custom EioConfig.
async fn start_sio_server_with_config<F>(
    config: EioConfig,
    configure_io: F,
) -> (String, tokio::task::JoinHandle<()>)
where
    F: FnOnce(&RpressIo) + Send + 'static,
{
    let routes = RpressRoutes::new();
    start_test_server_custom(None, routes, move |app| {
        let io = RpressIo::with_config(config);
        configure_io(&io);
        app.attach_socketio(io);
    })
    .await
}

// --- Engine.IO polling handshake ---

#[tokio::test]
async fn test_eio_polling_handshake() {
    let (addr, handle) = start_sio_server(|io| {
        io.on_connection(|_socket| async move {});
    })
    .await;

    let raw = send_raw_request(
        &addr,
        "GET /socket.io/?EIO=4&transport=polling HTTP/1.1\r\nHost: localhost\r\n\r\n",
    )
    .await;
    let resp = parse_response(&raw);
    assert_eq!(resp.status_code, 200);

    // Body should be an Engine.IO open packet: 0{...}
    assert!(resp.body.starts_with('0'), "Expected open packet, got: {}", resp.body);

    let json_str = &resp.body[1..];
    let handshake: Value = serde_json::from_str(json_str).unwrap();
    assert!(handshake["sid"].is_string());
    assert_eq!(handshake["upgrades"], serde_json::json!(["websocket"]));
    assert!(handshake["pingInterval"].is_number());
    assert!(handshake["pingTimeout"].is_number());

    handle.abort();
}

#[tokio::test]
async fn test_eio_polling_invalid_version() {
    let (addr, handle) = start_sio_server(|io| {
        io.on_connection(|_socket| async move {});
    })
    .await;

    let raw = send_raw_request(
        &addr,
        "GET /socket.io/?EIO=3&transport=polling HTTP/1.1\r\nHost: localhost\r\n\r\n",
    )
    .await;
    let resp = parse_response(&raw);
    assert_eq!(resp.status_code, 400);

    handle.abort();
}

// --- WebSocket direct connection ---

#[tokio::test]
async fn test_ws_direct_connection_and_event() {
    let received = Arc::new(Notify::new());
    let received_clone = received.clone();

    let (addr, handle) = start_sio_server(move |io| {
        let received = received_clone.clone();
        io.of("/").on_connection(move |socket| {
            let received = received.clone();
            async move {
                socket
                    .on("echo", move |socket, data| {
                        let received = received.clone();
                        async move {
                            socket.emit("echo_reply", &data[0]).await;
                            received.notify_one();
                            None
                        }
                    })
                    .await;
            }
        });
    })
    .await;

    let ws_url = format!("ws://{}/socket.io/?EIO=4&transport=websocket", addr);
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("WebSocket connect failed");

    // Should receive Engine.IO open packet
    let msg = ws.next().await.unwrap().unwrap();
    let text = msg.into_text().unwrap();
    assert!(text.starts_with('0'), "Expected open packet, got: {}", text);

    // Send Socket.IO CONNECT to namespace "/"
    // Engine.IO message (4) + Socket.IO CONNECT (0) = "40"
    ws.send(Message::Text("40".into())).await.unwrap();

    // Should receive CONNECT response: 40{"sid":"..."}
    let msg = ws.next().await.unwrap().unwrap();
    let text = msg.into_text().unwrap();
    assert!(
        text.starts_with("40"),
        "Expected SIO CONNECT response, got: {}",
        text
    );
    assert!(text.contains("\"sid\""));

    // Send an event: 42["echo","hello"]
    ws.send(Message::Text("42[\"echo\",\"hello\"]".into()))
        .await
        .unwrap();

    // Wait for the handler to process
    tokio::time::timeout(Duration::from_secs(2), received.notified())
        .await
        .expect("Handler was not called");

    // Should receive echo_reply event: 42["echo_reply","hello"]
    let msg = ws.next().await.unwrap().unwrap();
    let text = msg.into_text().unwrap();
    assert!(
        text.contains("echo_reply"),
        "Expected echo_reply event, got: {}",
        text
    );
    assert!(text.contains("hello"), "Expected data 'hello', got: {}", text);

    ws.close(None).await.ok();
    handle.abort();
}

// --- Acknowledgements ---

#[tokio::test]
async fn test_ws_event_with_ack() {
    let (addr, handle) = start_sio_server(|io| {
        io.on_connection(|socket| async move {
            socket
                .on("greet", |_socket, data| async move {
                    let name = data
                        .first()
                        .and_then(|v| v.as_str())
                        .unwrap_or("world");
                    Some(serde_json::json!(format!("hello, {}!", name)))
                })
                .await;
        });
    })
    .await;

    let ws_url = format!("ws://{}/socket.io/?EIO=4&transport=websocket", addr);
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .unwrap();

    // Open packet
    ws.next().await.unwrap().unwrap();

    // Connect
    ws.send(Message::Text("40".into())).await.unwrap();
    ws.next().await.unwrap().unwrap(); // connect response

    // Send event with ack id 7: 427["greet","Rpress"]
    ws.send(Message::Text("427[\"greet\",\"Rpress\"]".into()))
        .await
        .unwrap();

    // Should receive ack: 37["hello, Rpress!"]
    let msg = tokio::time::timeout(Duration::from_secs(2), ws.next())
        .await
        .expect("Ack timeout")
        .unwrap()
        .unwrap();
    let text = msg.into_text().unwrap();
    // Engine.IO message (4) + Socket.IO ACK (3) + ack_id + data
    assert!(
        text.starts_with("43"),
        "Expected ACK packet (43...), got: {}",
        text
    );
    assert!(text.contains("hello, Rpress!"), "Expected ack data, got: {}", text);

    ws.close(None).await.ok();
    handle.abort();
}

// --- Rooms and broadcast ---

#[tokio::test]
async fn test_ws_rooms_broadcast() {
    let (addr, handle) = start_sio_server(|io| {
        io.on_connection(|socket| async move {
            socket
                .on("join", |socket, data| async move {
                    if let Some(room) = data.first().and_then(|v| v.as_str()) {
                        socket.join(room).await;
                    }
                    None
                })
                .await;
            socket
                .on("broadcast_to_room", |socket, data| async move {
                    if let (Some(room), Some(msg)) = (
                        data.first().and_then(|v| v.as_str()),
                        data.get(1),
                    ) {
                        socket.to(room).emit("room_msg", msg).await;
                    }
                    None
                })
                .await;
        });
    })
    .await;

    let ws_url = format!("ws://{}/socket.io/?EIO=4&transport=websocket", addr);

    // Client 1 joins "test-room"
    let (mut ws1, _) = tokio_tungstenite::connect_async(&ws_url).await.unwrap();
    ws1.next().await; // open
    ws1.send(Message::Text("40".into())).await.unwrap();
    ws1.next().await; // connect
    ws1.send(Message::Text("42[\"join\",\"test-room\"]".into()))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Client 2 joins "test-room"
    let (mut ws2, _) = tokio_tungstenite::connect_async(&ws_url).await.unwrap();
    ws2.next().await; // open
    ws2.send(Message::Text("40".into())).await.unwrap();
    ws2.next().await; // connect
    ws2.send(Message::Text("42[\"join\",\"test-room\"]".into()))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Client 2 broadcasts to room (excludes self by default)
    ws2.send(
        Message::Text("42[\"broadcast_to_room\",\"test-room\",\"hi from ws2\"]".into()),
    )
    .await
    .unwrap();

    // Client 1 should receive the room message
    let msg = tokio::time::timeout(Duration::from_secs(2), ws1.next())
        .await
        .expect("Broadcast timeout")
        .unwrap()
        .unwrap();
    let text = msg.into_text().unwrap();
    assert!(
        text.contains("room_msg"),
        "Expected room_msg event, got: {}",
        text
    );
    assert!(
        text.contains("hi from ws2"),
        "Expected broadcast data, got: {}",
        text
    );

    ws1.close(None).await.ok();
    ws2.close(None).await.ok();
    handle.abort();
}

// --- Non-SIO requests still work ---

#[tokio::test]
async fn test_normal_routes_with_socketio() {
    let mut routes = RpressRoutes::new();
    routes.add(":get/api/hello", |_req: RequestPayload| async {
        ResponsePayload::text("hello from api")
    });

    let (addr, handle) = start_test_server_custom(None, routes, |app| {
        let io = RpressIo::new();
        io.on_connection(|_socket| async move {});
        app.attach_socketio(io);
    })
    .await;

    let raw = send_raw_request(
        &addr,
        "GET /api/hello HTTP/1.1\r\nHost: localhost\r\n\r\n",
    )
    .await;
    let resp = parse_response(&raw);
    assert_eq!(resp.status_code, 200);
    assert_eq!(resp.body, "hello from api");

    handle.abort();
}

// --- Authentication ---

#[tokio::test]
async fn test_ws_auth_valid_token() {
    let connected = Arc::new(Notify::new());
    let connected_clone = connected.clone();

    let (addr, handle) = start_sio_server(move |io| {
        io.use_auth(|auth| async move {
            let token = auth
                .get("token")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing token".to_string())?;
            if token == "valid-secret" {
                Ok(serde_json::json!({"user_id": "u1", "role": "admin"}))
            } else {
                Err("Invalid token".to_string())
            }
        });
        let connected = connected_clone.clone();
        io.on_connection(move |socket| {
            let connected = connected.clone();
            async move {
                assert_eq!(
                    socket.auth().get("user_id").and_then(|v| v.as_str()),
                    Some("u1")
                );
                assert_eq!(
                    socket.auth().get("role").and_then(|v| v.as_str()),
                    Some("admin")
                );
                connected.notify_one();
            }
        });
    })
    .await;

    let ws_url = format!("ws://{}/socket.io/?EIO=4&transport=websocket", addr);
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url).await.unwrap();
    ws.next().await; // open

    ws.send(Message::Text(r#"40{"token":"valid-secret"}"#.into()))
        .await
        .unwrap();

    let msg = ws.next().await.unwrap().unwrap();
    let text = msg.into_text().unwrap();
    assert!(text.starts_with("40"), "Expected CONNECT OK, got: {}", text);
    assert!(text.contains("\"sid\""));

    tokio::time::timeout(Duration::from_secs(2), connected.notified())
        .await
        .expect("on_connection was not called with valid auth");

    ws.close(None).await.ok();
    handle.abort();
}

#[tokio::test]
async fn test_ws_auth_invalid_token() {
    let connected = Arc::new(Notify::new());
    let connected_clone = connected.clone();

    let (addr, handle) = start_sio_server(move |io| {
        io.use_auth(|auth| async move {
            let token = auth
                .get("token")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing token".to_string())?;
            if token == "valid-secret" {
                Ok(serde_json::json!({}))
            } else {
                Err("Invalid token".to_string())
            }
        });
        let connected = connected_clone.clone();
        io.on_connection(move |_socket| {
            let connected = connected.clone();
            async move {
                connected.notify_one();
            }
        });
    })
    .await;

    let ws_url = format!("ws://{}/socket.io/?EIO=4&transport=websocket", addr);
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url).await.unwrap();
    ws.next().await; // open

    ws.send(Message::Text(r#"40{"token":"wrong"}"#.into()))
        .await
        .unwrap();

    let msg = ws.next().await.unwrap().unwrap();
    let text = msg.into_text().unwrap();
    assert!(
        text.starts_with("44"),
        "Expected CONNECT_ERROR (44), got: {}",
        text
    );
    assert!(text.contains("Invalid token"));

    let result = tokio::time::timeout(Duration::from_millis(200), connected.notified()).await;
    assert!(result.is_err(), "on_connection should NOT have been called");

    ws.close(None).await.ok();
    handle.abort();
}

#[tokio::test]
async fn test_ws_auth_missing_when_required() {
    let (addr, handle) = start_sio_server(|io| {
        io.use_auth(|auth| async move {
            auth.get("token")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing token".to_string())?;
            Ok(serde_json::json!({}))
        });
        io.on_connection(|_socket| async move {});
    })
    .await;

    let ws_url = format!("ws://{}/socket.io/?EIO=4&transport=websocket", addr);
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url).await.unwrap();
    ws.next().await; // open

    // CONNECT without auth data
    ws.send(Message::Text("40".into())).await.unwrap();

    let msg = ws.next().await.unwrap().unwrap();
    let text = msg.into_text().unwrap();
    assert!(
        text.starts_with("44"),
        "Expected CONNECT_ERROR (44), got: {}",
        text
    );
    assert!(text.contains("Missing token"));

    ws.close(None).await.ok();
    handle.abort();
}

#[tokio::test]
async fn test_ws_no_auth_handler_accepts_all() {
    let connected = Arc::new(Notify::new());
    let connected_clone = connected.clone();

    let (addr, handle) = start_sio_server(move |io| {
        let connected = connected_clone.clone();
        io.on_connection(move |socket| {
            let connected = connected.clone();
            async move {
                assert!(socket.auth().is_null());
                connected.notify_one();
            }
        });
    })
    .await;

    let ws_url = format!("ws://{}/socket.io/?EIO=4&transport=websocket", addr);
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url).await.unwrap();
    ws.next().await; // open

    ws.send(Message::Text("40".into())).await.unwrap();

    let msg = ws.next().await.unwrap().unwrap();
    let text = msg.into_text().unwrap();
    assert!(text.starts_with("40"), "Expected CONNECT OK, got: {}", text);

    tokio::time::timeout(Duration::from_secs(2), connected.notified())
        .await
        .expect("on_connection was not called");

    ws.close(None).await.ok();
    handle.abort();
}

// --- Adapter trait (set_adapter with RoomManager) ---

#[tokio::test]
async fn test_set_adapter_with_room_manager() {
    use rpress::core::socketio::room::RoomManager;

    let received = Arc::new(Notify::new());
    let received_clone = received.clone();

    let routes = RpressRoutes::new();
    let (addr, handle) = start_test_server_custom(None, routes, move |app| {
        let mut io = RpressIo::new();
        io.set_adapter(RoomManager::new());

        let received = received_clone.clone();
        io.on_connection(move |socket| {
            let received = received.clone();
            async move {
                socket
                    .on("ping", move |socket, _data| {
                        let received = received.clone();
                        async move {
                            socket.emit("pong", &serde_json::json!("ok")).await;
                            received.notify_one();
                            None
                        }
                    })
                    .await;
            }
        });

        app.attach_socketio(io);
    })
    .await;

    let ws_url = format!("ws://{}/socket.io/?EIO=4&transport=websocket", addr);
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url).await.unwrap();
    ws.next().await; // open
    ws.send(Message::Text("40".into())).await.unwrap();
    ws.next().await; // connect

    ws.send(Message::Text(r#"42["ping","test"]"#.into()))
        .await
        .unwrap();

    tokio::time::timeout(Duration::from_secs(2), received.notified())
        .await
        .expect("Handler was not called through custom adapter");

    let msg = ws.next().await.unwrap().unwrap();
    let text = msg.into_text().unwrap();
    assert!(text.contains("pong"), "Expected pong event, got: {}", text);

    ws.close(None).await.ok();
    handle.abort();
}

// --- Disconnect handler ---

#[tokio::test]
async fn test_ws_disconnect_handler() {
    let disconnected = Arc::new(Notify::new());
    let disconnected_clone = disconnected.clone();

    let (addr, handle) = start_sio_server(move |io| {
        let disconnected = disconnected_clone.clone();
        io.on_connection(move |socket| {
            let disconnected = disconnected.clone();
            async move {
                socket
                    .on_disconnect(move |_socket| {
                        let disconnected = disconnected.clone();
                        async move {
                            disconnected.notify_one();
                        }
                    })
                    .await;
            }
        });
    })
    .await;

    let ws_url = format!("ws://{}/socket.io/?EIO=4&transport=websocket", addr);
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url).await.unwrap();
    ws.next().await; // open
    ws.send(Message::Text("40".into())).await.unwrap();
    ws.next().await; // connect

    // Close the WebSocket
    ws.close(None).await.ok();

    // Disconnect handler should be called
    tokio::time::timeout(Duration::from_secs(2), disconnected.notified())
        .await
        .expect("Disconnect handler was not called");

    handle.abort();
}

// --- WebSocket-only mode ---

#[tokio::test]
async fn test_websocket_only_rejects_polling() {
    let config = EioConfig {
        websocket_only: true,
        ..EioConfig::default()
    };
    let (addr, handle) = start_sio_server_with_config(config, |io| {
        io.on_connection(|_socket| async move {});
    })
    .await;

    let raw = send_raw_request(
        &addr,
        "GET /socket.io/?EIO=4&transport=polling HTTP/1.1\r\nHost: localhost\r\n\r\n",
    )
    .await;
    let resp = parse_response(&raw);
    assert_eq!(resp.status_code, 400);
    assert!(resp.body.contains("Polling transport disabled"));

    handle.abort();
}

#[tokio::test]
async fn test_websocket_only_allows_websocket() {
    let config = EioConfig {
        websocket_only: true,
        ..EioConfig::default()
    };
    let (addr, handle) = start_sio_server_with_config(config, |io| {
        io.on_connection(|_socket| async move {});
    })
    .await;

    let ws_url = format!("ws://{}/socket.io/?EIO=4&transport=websocket", addr);
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url).await.unwrap();

    let msg = ws.next().await.unwrap().unwrap();
    let text = msg.into_text().unwrap();
    assert!(text.starts_with('0'), "Expected EIO open packet, got: {text}");

    let json_str = &text[1..];
    let handshake: Value = serde_json::from_str(json_str).unwrap();
    assert!(handshake["sid"].is_string());
    assert_eq!(handshake["upgrades"], serde_json::json!([]));

    ws.close(None).await.ok();
    handle.abort();
}
