mod helpers;

use engine::core::handler_response::ResponsePayload;
use engine::core::routes::RpressRoutes;
use engine::types::definitions::{RequestPayload, StatusCode};
use helpers::{parse_response, send_raw_request, start_test_server};

#[tokio::test]
async fn test_json_response() {
    let mut routes = RpressRoutes::new();
    routes.add(":get/json", |_req: RequestPayload| async move {
        ResponsePayload::json(&serde_json::json!({"key": "value"})).unwrap()
    });

    let (addr, handle) = start_test_server(None, routes).await;

    let raw = send_raw_request(
        &addr,
        "GET /json HTTP/1.1\r\nHost: localhost\r\n\r\n",
    )
    .await;
    let resp = parse_response(&raw);

    assert_eq!(resp.status_code, 200);
    assert!(resp.get_header("Content-Type").unwrap().contains("application/json"));
    assert!(resp.body.contains("\"key\":\"value\""));

    handle.abort();
}

#[tokio::test]
async fn test_custom_status_code() {
    let mut routes = RpressRoutes::new();
    routes.add(":post/created", |_req: RequestPayload| async move {
        ResponsePayload::text("done").with_status(StatusCode::Created)
    });

    let (addr, handle) = start_test_server(None, routes).await;

    let raw = send_raw_request(
        &addr,
        "POST /created HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\n\r\n",
    )
    .await;
    let resp = parse_response(&raw);

    assert_eq!(resp.status_code, 201);

    handle.abort();
}

#[tokio::test]
async fn test_custom_headers() {
    let mut routes = RpressRoutes::new();
    routes.add(":get/custom", |_req: RequestPayload| async move {
        ResponsePayload::text("ok")
            .with_header("X-Custom-Header", "custom-value")
            .with_header("X-Another", "another-value")
    });

    let (addr, handle) = start_test_server(None, routes).await;

    let raw = send_raw_request(
        &addr,
        "GET /custom HTTP/1.1\r\nHost: localhost\r\n\r\n",
    )
    .await;
    let resp = parse_response(&raw);

    assert_eq!(resp.status_code, 200);
    assert_eq!(resp.get_header("X-Custom-Header"), Some("custom-value"));
    assert_eq!(resp.get_header("X-Another"), Some("another-value"));

    handle.abort();
}

#[tokio::test]
async fn test_cors_headers_present() {
    use engine::core::cors::RpressCors;

    let cors = RpressCors::new()
        .set_origins(vec!["https://example.com"])
        .set_methods(vec!["GET", "POST"]);

    let mut routes = RpressRoutes::new();
    routes.add(":get/cors_test", |_req: RequestPayload| async move {
        ResponsePayload::text("ok")
    });

    let (addr, handle) = start_test_server(Some(cors), routes).await;

    let raw = send_raw_request(
        &addr,
        "GET /cors_test HTTP/1.1\r\nHost: localhost\r\nOrigin: https://example.com\r\n\r\n",
    )
    .await;
    let resp = parse_response(&raw);

    assert_eq!(resp.status_code, 200);
    assert_eq!(
        resp.get_header("Access-Control-Allow-Origin"),
        Some("https://example.com")
    );
    assert!(resp
        .get_header("Access-Control-Allow-Methods")
        .unwrap()
        .contains("GET"));

    handle.abort();
}

#[tokio::test]
async fn test_cors_preflight_options() {
    use engine::core::cors::RpressCors;

    let cors = RpressCors::new()
        .set_origins(vec!["*"])
        .set_methods(vec!["GET", "POST", "DELETE"]);

    let mut routes = RpressRoutes::new();
    routes.add(":get/preflight", |_req: RequestPayload| async move {
        ResponsePayload::text("should not reach")
    });

    let (addr, handle) = start_test_server(Some(cors), routes).await;

    let raw = send_raw_request(
        &addr,
        "OPTIONS /preflight HTTP/1.1\r\nHost: localhost\r\nOrigin: https://any.com\r\n\r\n",
    )
    .await;
    let resp = parse_response(&raw);

    assert_eq!(resp.status_code, 204);
    assert_eq!(resp.get_header("Access-Control-Allow-Origin"), Some("*"));
    assert_eq!(resp.body, "");

    handle.abort();
}

#[tokio::test]
async fn test_cors_wrong_origin_no_headers() {
    use engine::core::cors::RpressCors;

    let cors = RpressCors::new()
        .set_origins(vec!["https://allowed.com"]);

    let mut routes = RpressRoutes::new();
    routes.add(":get/restricted", |_req: RequestPayload| async move {
        ResponsePayload::text("ok")
    });

    let (addr, handle) = start_test_server(Some(cors), routes).await;

    let raw = send_raw_request(
        &addr,
        "GET /restricted HTTP/1.1\r\nHost: localhost\r\nOrigin: https://evil.com\r\n\r\n",
    )
    .await;
    let resp = parse_response(&raw);

    assert_eq!(resp.status_code, 200);
    assert!(resp.get_header("Access-Control-Allow-Origin").is_none());

    handle.abort();
}
