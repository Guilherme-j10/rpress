mod helpers;

use helpers::{parse_response, send_tls_request, start_test_server_tls};
use rpress::core::routes::RpressRoutes;
use rpress::{RequestPayload, ResponsePayload};

#[tokio::test]
async fn test_tls_get_request() {
    let mut routes = RpressRoutes::new();
    routes.add(":get/hello", |_req: RequestPayload| async {
        ResponsePayload::text("Hello TLS!")
    });

    let (addr, _handle, client_config) = start_test_server_tls(None, routes).await;

    let raw = send_tls_request(
        &addr,
        "GET /hello HTTP/1.1\r\nHost: localhost\r\n\r\n",
        &client_config,
    )
    .await;

    let resp = parse_response(&raw);
    assert_eq!(resp.status_code, 200);
    assert_eq!(resp.body, "Hello TLS!");
}

#[tokio::test]
async fn test_tls_post_with_body() {
    let mut routes = RpressRoutes::new();
    routes.add(":post/echo", |req: RequestPayload| async move {
        let body = req.body_str().unwrap_or("").to_string();
        ResponsePayload::text(body)
    });

    let (addr, _handle, client_config) = start_test_server_tls(None, routes).await;

    let body = "test body content";
    let request = format!(
        "POST /echo HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body,
    );

    let raw = send_tls_request(&addr, &request, &client_config).await;
    let resp = parse_response(&raw);
    assert_eq!(resp.status_code, 200);
    assert_eq!(resp.body, "test body content");
}

#[tokio::test]
async fn test_tls_404_not_found() {
    let routes = RpressRoutes::new();
    let (addr, _handle, client_config) = start_test_server_tls(None, routes).await;

    let raw = send_tls_request(
        &addr,
        "GET /nonexistent HTTP/1.1\r\nHost: localhost\r\n\r\n",
        &client_config,
    )
    .await;

    let resp = parse_response(&raw);
    assert_eq!(resp.status_code, 404);
}

#[tokio::test]
async fn test_tls_json_response() {
    let mut routes = RpressRoutes::new();
    routes.add(":get/data", |_req: RequestPayload| async {
        ResponsePayload::json(&serde_json::json!({"tls": true})).unwrap()
    });

    let (addr, _handle, client_config) = start_test_server_tls(None, routes).await;

    let raw = send_tls_request(
        &addr,
        "GET /data HTTP/1.1\r\nHost: localhost\r\n\r\n",
        &client_config,
    )
    .await;

    let resp = parse_response(&raw);
    assert_eq!(resp.status_code, 200);
    assert!(resp.body.contains("\"tls\":true"));
    assert_eq!(
        resp.get_header("Content-Type").unwrap(),
        "application/json"
    );
}
