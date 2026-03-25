use std::time::Duration;

use rpress::{EioConfig, Rpress, RpressIo, RpressRoutes, RequestPayload, ResponsePayload};

fn get_bench_routes() -> RpressRoutes {
    let mut routes = RpressRoutes::new();

    routes.add(":get/health", |_req: RequestPayload| async {
        ResponsePayload::text("ok")
    });

    routes.add(":get/api/json", |_req: RequestPayload| async {
        ResponsePayload::json(&serde_json::json!({
            "status": "success",
            "message": "Benchmark response from Rpress",
            "server": {
                "name": "rpress",
                "version": "0.3.2",
                "features": ["http1", "http2", "tls", "websocket", "socketio", "compression"]
            },
            "data": {
                "id": 42,
                "timestamp": "2026-03-16T00:00:00Z",
                "items": [
                    {"name": "alpha", "value": 100},
                    {"name": "beta", "value": 200},
                    {"name": "gamma", "value": 300},
                    {"name": "delta", "value": 400}
                ]
            }
        }))
        .unwrap()
    });

    routes.add(":post/api/echo", |req: RequestPayload| async move {
        let body = req.body_str().unwrap_or("").to_string();
        ResponsePayload::text(&body)
    });

    routes.add(":get/api/heavy", |_req: RequestPayload| async {
        let mut ids = Vec::with_capacity(100);
        for _ in 0..100 {
            ids.push(uuid::Uuid::new_v4().to_string());
        }
        ResponsePayload::json(&serde_json::json!({
            "count": ids.len(),
            "sample": ids[0]
        }))
        .unwrap()
    });

    routes.add(":get/api/large", |_req: RequestPayload| async {
        let chunk = "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ\n";
        let body = chunk.repeat(1600); // ~100KB
        ResponsePayload::text(&body)
    });

    routes
}

fn setup_socketio() -> RpressIo {
    let config = EioConfig {
        ping_interval: Duration::from_secs(5),
        ping_timeout: Duration::from_secs(3),
        ..EioConfig::default()
    };
    let io = RpressIo::with_config(config);

    io.on_connection(|socket| async move {
        socket
            .on("ping", |socket, data| async move {
                let payload = data.first().cloned().unwrap_or(serde_json::json!("pong"));
                socket.emit("pong", &payload).await;
                None
            })
            .await;

        socket
            .on("broadcast", |socket, data| async move {
                let payload = data.first().cloned().unwrap_or(serde_json::json!("broadcast"));
                socket.broadcast().emit("broadcast_msg", &payload).await;
                None
            })
            .await;

        socket
            .on("join_room", |socket, data| async move {
                if let Some(room) = data.first().and_then(|v| v.as_str()) {
                    socket.join(room).await;
                }
                None
            })
            .await;

        socket
            .on("room_msg", |socket, data| async move {
                if let (Some(room), Some(msg)) = (
                    data.first().and_then(|v| v.as_str()),
                    data.get(1),
                ) {
                    socket.to(room).emit("room_event", msg).await;
                }
                None
            })
            .await;
    });

    io
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_target(false)
        .with_level(true)
        .init();

    let port = std::env::var("BENCH_PORT").unwrap_or_else(|_| "9090".into());
    let max_conn: usize = std::env::var("BENCH_MAX_CONN")
        .unwrap_or_else(|_| "4096".into())
        .parse()
        .unwrap_or(4096);
    let compression: bool = std::env::var("BENCH_COMPRESSION")
        .unwrap_or_else(|_| "true".into())
        .parse()
        .unwrap_or(true);
    let rate_limit: Option<u32> = std::env::var("BENCH_RATE_LIMIT")
        .ok()
        .filter(|v| !v.is_empty())
        .and_then(|v| v.parse().ok());
    let read_timeout: u64 = std::env::var("BENCH_READ_TIMEOUT")
        .unwrap_or_else(|_| "30".into())
        .parse()
        .unwrap_or(30);
    let idle_timeout: u64 = std::env::var("BENCH_IDLE_TIMEOUT")
        .unwrap_or_else(|_| "60".into())
        .parse()
        .unwrap_or(60);
    let max_body: usize = std::env::var("BENCH_MAX_BODY_MB")
        .unwrap_or_else(|_| "10".into())
        .parse()
        .map(|mb: usize| mb * 1024 * 1024)
        .unwrap_or(10 * 1024 * 1024);

    let mut app = Rpress::new(None);

    app.set_max_connections(max_conn);
    app.set_read_timeout(Duration::from_secs(read_timeout));
    app.set_idle_timeout(Duration::from_secs(idle_timeout));
    app.set_max_body_size(max_body);
    app.set_buffer_capacity(64 * 1024);
    app.enable_compression(compression);
    app.serve_static("/assets", "./bench/public");

    if let Some(limit) = rate_limit {
        app.set_rate_limit(limit, 60);
        tracing::info!("Rate limiting enabled: {} req/min", limit);
    }

    app.add_route_group(get_bench_routes());
    app.attach_socketio(setup_socketio());

    let addr = format!("0.0.0.0:{}", port);
    tracing::info!(
        "Bench server starting on {} (max_conn={}, compression={}, read_timeout={}s, idle_timeout={}s, max_body={}MB, rate_limit={:?})",
        addr, max_conn, compression, read_timeout, idle_timeout,
        max_body / (1024 * 1024), rate_limit
    );

    app.listen(&addr).await?;
    Ok(())
}
