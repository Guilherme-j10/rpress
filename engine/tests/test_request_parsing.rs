mod helpers;

use rpress::core::handler_response::ResponsePayload;
use rpress::core::routes::RpressRoutes;
use rpress::types::definitions::RequestPayload;
use helpers::{parse_response, send_raw_request, start_test_server};

#[tokio::test]
async fn test_valid_get_request() {
    let mut routes = RpressRoutes::new();
    routes.add(":get/parse_test", |_req: RequestPayload| async move {
        ResponsePayload::text("parsed ok")
    });

    let (addr, handle) = start_test_server(None, routes).await;

    let raw = send_raw_request(
        &addr,
        "GET /parse_test HTTP/1.1\r\nHost: localhost\r\n\r\n",
    )
    .await;
    let resp = parse_response(&raw);

    assert_eq!(resp.status_code, 200);
    assert_eq!(resp.body, "parsed ok");

    handle.abort();
}

#[tokio::test]
async fn test_query_string_parsing() {
    let mut routes = RpressRoutes::new();
    routes.add(":get/search", |req: RequestPayload| async move {
        let q = req.get_query("q").unwrap_or_default();
        ResponsePayload::text(format!("query:{}", q))
    });

    let (addr, handle) = start_test_server(None, routes).await;

    let raw = send_raw_request(
        &addr,
        "GET /search?q=hello HTTP/1.1\r\nHost: localhost\r\n\r\n",
    )
    .await;
    let resp = parse_response(&raw);

    assert_eq!(resp.status_code, 200);
    assert_eq!(resp.body, "query:hello");

    handle.abort();
}

#[tokio::test]
async fn test_post_with_body() {
    let mut routes = RpressRoutes::new();
    routes.add(":post/echo", |req: RequestPayload| async move {
        let body = String::from_utf8_lossy(&req.payload).to_string();
        ResponsePayload::text(body)
    });

    let (addr, handle) = start_test_server(None, routes).await;

    let body = r#"{"name":"test"}"#;
    let request = format!(
        "POST /echo HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );

    let raw = send_raw_request(&addr, &request).await;
    let resp = parse_response(&raw);

    assert_eq!(resp.status_code, 200);
    assert_eq!(resp.body, r#"{"name":"test"}"#);

    handle.abort();
}

#[tokio::test]
async fn test_request_headers_accessible() {
    let mut routes = RpressRoutes::new();
    routes.add(":get/headers", |req: RequestPayload| async move {
        let auth = req
            .request_metadata
            .as_ref()
            .and_then(|m| m.headers.get("authorization"))
            .cloned()
            .unwrap_or_default();
        ResponsePayload::text(format!("auth:{}", auth))
    });

    let (addr, handle) = start_test_server(None, routes).await;

    let raw = send_raw_request(
        &addr,
        "GET /headers HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer token123\r\n\r\n",
    )
    .await;
    let resp = parse_response(&raw);

    assert_eq!(resp.status_code, 200);
    assert_eq!(resp.body, "auth:Bearer token123");

    handle.abort();
}
