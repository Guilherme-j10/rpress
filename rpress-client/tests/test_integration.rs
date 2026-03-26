use std::sync::Arc;
use std::time::Duration;

use rpress::core::routes::RpressRoutes;
use rpress::{Rpress, RpressIo};
use rpress_client::SocketIoClient;
use serde_json::json;
use tokio::sync::Notify;

async fn start_sio_server<F>(configure_io: F) -> (String, tokio::task::JoinHandle<()>)
where
    F: FnOnce(&RpressIo) + Send + 'static,
{
    let routes = RpressRoutes::new();
    let mut app = Rpress::new(None);
    app.add_route_group(routes);

    let io = RpressIo::new();
    configure_io(&io);
    app.attach_socketio(io);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let addr_str = format!("127.0.0.1:{}", addr.port());

    let handle = tokio::spawn(async move {
        app.server_with_listener(listener).await.ok();
    });

    tokio::time::sleep(Duration::from_millis(50)).await;
    (addr_str, handle)
}

#[tokio::test]
async fn test_client_connect_and_disconnect() {
    let (addr, handle) = start_sio_server(|io| {
        io.on_connection(|_socket| async move {});
    })
    .await;

    let client = SocketIoClient::connect(&format!("http://{}", addr))
        .await
        .expect("connect failed");

    assert!(client.is_connected());
    assert!(!client.id().is_empty());
    assert_eq!(client.namespace(), "/");

    client.disconnect().await.expect("disconnect failed");
    assert!(!client.is_connected());

    handle.abort();
}

#[tokio::test]
async fn test_client_emit_and_receive_event() {
    let received = Arc::new(Notify::new());
    let received_clone = received.clone();

    let (addr, handle) = start_sio_server(move |io| {
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
    })
    .await;

    let client = SocketIoClient::connect(&format!("http://{}", addr))
        .await
        .unwrap();

    let got_pong = Arc::new(Notify::new());
    let got_pong_clone = got_pong.clone();

    client
        .on("pong", move |_args| {
            let got_pong = got_pong_clone.clone();
            async move {
                got_pong.notify_one();
            }
        })
        .await;

    client
        .emit("ping", &serde_json::json!("test"))
        .await
        .unwrap();

    tokio::time::timeout(Duration::from_secs(3), got_pong.notified())
        .await
        .expect("did not receive pong event");

    client.disconnect().await.unwrap();
    handle.abort();
}

#[tokio::test]
async fn test_client_emit_with_ack() {
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

    let client = SocketIoClient::connect(&format!("http://{}", addr))
        .await
        .unwrap();

    let ack = client
        .emit_with_ack("greet", &serde_json::json!("Rpress"))
        .await
        .expect("ack failed");

    let ack_arr = ack.as_array().expect("ack should be an array");
    let ack_str = ack_arr[0].as_str().expect("ack data should be a string");
    assert_eq!(ack_str, "hello, Rpress!");

    client.disconnect().await.unwrap();
    handle.abort();
}

#[tokio::test]
async fn test_client_namespace() {
    let (addr, handle) = start_sio_server(|io| {
        io.of("/admin").on_connection(|socket| async move {
            socket
                .on("who", |_socket, _data| async move {
                    Some(serde_json::json!("admin-ns"))
                })
                .await;
        });
    })
    .await;

    let client = SocketIoClient::connect_to(&format!("http://{}", addr), "/admin")
        .await
        .expect("namespace connect failed");

    assert_eq!(client.namespace(), "/admin");

    let ack = client
        .emit_with_ack("who", &serde_json::json!(null))
        .await
        .expect("ack failed");

    let ack_arr = ack.as_array().expect("ack should be an array");
    assert_eq!(ack_arr[0].as_str().unwrap(), "admin-ns");

    client.disconnect().await.unwrap();
    handle.abort();
}

#[tokio::test]
async fn test_client_server_initiated_event() {
    let (addr, handle) = start_sio_server(|io| {
        io.on_connection(|socket| async move {
            socket.emit("welcome", &serde_json::json!("hi")).await;
        });
    })
    .await;

    let client = SocketIoClient::connect(&format!("http://{}", addr))
        .await
        .unwrap();

    let got_welcome = Arc::new(Notify::new());
    let got_welcome_clone = got_welcome.clone();

    client
        .on("welcome", move |_args| {
            let got_welcome = got_welcome_clone.clone();
            async move {
                got_welcome.notify_one();
            }
        })
        .await;

    tokio::time::timeout(Duration::from_secs(3), got_welcome.notified())
        .await
        .expect("did not receive welcome event");

    client.disconnect().await.unwrap();
    handle.abort();
}

// --- Authentication tests ---

#[tokio::test]
async fn test_client_connect_with_auth_valid() {
    let connected = Arc::new(Notify::new());
    let connected_clone = connected.clone();

    let (addr, handle) = start_sio_server(move |io| {
        io.use_auth(|auth| async move {
            let token = auth
                .get("token")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing token".to_string())?;
            if token == "secret-123" {
                Ok(json!({"user": "alice"}))
            } else {
                Err("Bad token".to_string())
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

    let client = SocketIoClient::connect_with_auth(
        &format!("http://{}", addr),
        json!({"token": "secret-123"}),
    )
    .await
    .expect("connect_with_auth should succeed");

    assert!(client.is_connected());

    tokio::time::timeout(Duration::from_secs(2), connected.notified())
        .await
        .expect("on_connection was not called");

    client.disconnect().await.unwrap();
    handle.abort();
}

#[tokio::test]
async fn test_client_connect_with_auth_invalid() {
    let (addr, handle) = start_sio_server(|io| {
        io.use_auth(|auth| async move {
            let token = auth
                .get("token")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing token".to_string())?;
            if token == "secret-123" {
                Ok(json!({}))
            } else {
                Err("Bad token".to_string())
            }
        });
        io.on_connection(|_socket| async move {});
    })
    .await;

    let result = SocketIoClient::connect_with_auth(
        &format!("http://{}", addr),
        json!({"token": "wrong-token"}),
    )
    .await;

    match result {
        Ok(_) => panic!("connect_with_auth should fail with bad token"),
        Err(e) => {
            let err_chain = format!("{e:?}");
            assert!(
                err_chain.contains("Bad token"),
                "Error chain should contain rejection reason, got: {}",
                err_chain
            );
        }
    }

    handle.abort();
}
