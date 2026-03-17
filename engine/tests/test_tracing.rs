mod helpers;

use std::sync::{Arc, Mutex, Once};

use rpress::core::handler_response::{ResponsePayload, RpressError};
use rpress::core::routes::RpressRoutes;
use rpress::types::definitions::{RequestPayload, StatusCode};

use helpers::{parse_response, send_raw_request, start_test_server, start_test_server_custom};

static INIT_SUBSCRIBER: Once = Once::new();

fn ensure_subscriber() {
    INIT_SUBSCRIBER.call_once(|| {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_test_writer()
            .try_init()
            .ok();
    });
}

// -- X-Request-ID tests --
// Every response must include a valid X-Request-ID header (UUID v4)
// because resolve_route generates it before the span is created.

#[tokio::test]
async fn test_request_id_present_on_success() {
    let mut routes = RpressRoutes::new();
    routes.add(":get/hello", |_req: RequestPayload| async move {
        ResponsePayload::text("ok")
    });

    let (addr, handle) = start_test_server(None, routes).await;

    let raw = send_raw_request(
        &addr,
        "GET /hello HTTP/1.1\r\nHost: localhost\r\n\r\n",
    )
    .await;
    let resp = parse_response(&raw);

    assert_eq!(resp.status_code, 200);
    let request_id = resp.get_header("X-Request-ID");
    assert!(request_id.is_some(), "X-Request-ID header must be present");
    assert!(
        request_id.unwrap().len() == 36,
        "X-Request-ID must be a UUID v4 (36 chars), got: {}",
        request_id.unwrap()
    );

    handle.abort();
}

#[tokio::test]
async fn test_request_id_present_on_404() {
    let routes = RpressRoutes::new();
    let (addr, handle) = start_test_server(None, routes).await;

    let raw = send_raw_request(
        &addr,
        "GET /nonexistent HTTP/1.1\r\nHost: localhost\r\n\r\n",
    )
    .await;
    let resp = parse_response(&raw);

    assert_eq!(resp.status_code, 404);
    let request_id = resp.get_header("X-Request-ID");
    assert!(request_id.is_some(), "X-Request-ID must be present on 404");
    assert_eq!(request_id.unwrap().len(), 36);

    handle.abort();
}

#[tokio::test]
async fn test_request_id_present_on_405() {
    let mut routes = RpressRoutes::new();
    routes.add(":get/only-get", |_req: RequestPayload| async move {
        ResponsePayload::text("ok")
    });

    let (addr, handle) = start_test_server(None, routes).await;

    let raw = send_raw_request(
        &addr,
        "POST /only-get HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\n\r\n",
    )
    .await;
    let resp = parse_response(&raw);

    assert_eq!(resp.status_code, 405);
    let request_id = resp.get_header("X-Request-ID");
    assert!(request_id.is_some(), "X-Request-ID must be present on 405");

    handle.abort();
}

#[tokio::test]
async fn test_request_ids_are_unique() {
    let mut routes = RpressRoutes::new();
    routes.add(":get/ping", |_req: RequestPayload| async move {
        ResponsePayload::text("pong")
    });

    let (addr, handle) = start_test_server(None, routes).await;

    let raw1 = send_raw_request(
        &addr,
        "GET /ping HTTP/1.1\r\nHost: localhost\r\n\r\n",
    )
    .await;
    let raw2 = send_raw_request(
        &addr,
        "GET /ping HTTP/1.1\r\nHost: localhost\r\n\r\n",
    )
    .await;

    let id1 = parse_response(&raw1).get_header("X-Request-ID").unwrap().to_string();
    let id2 = parse_response(&raw2).get_header("X-Request-ID").unwrap().to_string();

    assert_ne!(id1, id2, "Each request must get a unique X-Request-ID");

    handle.abort();
}

// -- Span context propagation tests --
// These verify that the automatic "http.request" span created by the
// framework is active when user middleware/handlers run.

#[tokio::test]
async fn test_span_context_available_in_middleware() {
    ensure_subscriber();

    let span_names: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let span_capture = span_names.clone();

    let mut routes = RpressRoutes::new();
    routes.add(":get/traced", |_req: RequestPayload| async move {
        ResponsePayload::text("ok")
    });

    let (addr, handle) = start_test_server_custom(None, routes, move |app| {
        let span_capture = span_capture.clone();
        app.use_middleware(move |req, next| {
            let span_capture = span_capture.clone();
            async move {
                let current = tracing::Span::current();
                if let Some(name) = current.metadata().map(|m| m.name().to_string()) {
                    span_capture.lock().unwrap().push(name);
                }
                next(req).await
            }
        });
    })
    .await;

    let raw = send_raw_request(
        &addr,
        "GET /traced HTTP/1.1\r\nHost: localhost\r\n\r\n",
    )
    .await;
    let resp = parse_response(&raw);
    assert_eq!(resp.status_code, 200);

    let names = span_names.lock().unwrap();
    assert!(
        names.iter().any(|n| n == "http.request"),
        "Middleware must run inside 'http.request' span, got: {:?}",
        *names
    );

    handle.abort();
}

#[tokio::test]
async fn test_span_context_available_in_handler() {
    ensure_subscriber();

    let span_names: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let handler_capture = span_names.clone();

    let mut routes = RpressRoutes::new();
    routes.add(":get/in-handler", move |_req: RequestPayload| {
        let handler_capture = handler_capture.clone();
        async move {
            let current = tracing::Span::current();
            if let Some(name) = current.metadata().map(|m| m.name().to_string()) {
                handler_capture.lock().unwrap().push(name);
            }
            ResponsePayload::text("ok")
        }
    });

    let (addr, handle) = start_test_server(None, routes).await;

    let raw = send_raw_request(
        &addr,
        "GET /in-handler HTTP/1.1\r\nHost: localhost\r\n\r\n",
    )
    .await;
    let resp = parse_response(&raw);
    assert_eq!(resp.status_code, 200);

    let names = span_names.lock().unwrap();
    assert!(
        names.iter().any(|n| n == "http.request"),
        "Handler must run inside 'http.request' span, got: {:?}",
        *names
    );

    handle.abort();
}

#[tokio::test]
async fn test_request_id_on_error_handler() {
    let mut routes = RpressRoutes::new();
    routes.add(
        ":get/fail",
        |_req: RequestPayload| async move {
            Err::<ResponsePayload, RpressError>(RpressError {
                status: StatusCode::InternalServerError,
                message: "intentional error".to_string(),
            })
        },
    );

    let (addr, handle) = start_test_server(None, routes).await;

    let raw = send_raw_request(
        &addr,
        "GET /fail HTTP/1.1\r\nHost: localhost\r\n\r\n",
    )
    .await;
    let resp = parse_response(&raw);

    assert_eq!(resp.status_code, 500);
    let request_id = resp.get_header("X-Request-ID");
    assert!(
        request_id.is_some(),
        "X-Request-ID must be present even on 500 errors"
    );
    assert_eq!(request_id.unwrap().len(), 36);

    handle.abort();
}

#[tokio::test]
async fn test_request_id_on_cors_preflight() {
    use rpress::core::cors::RpressCors;

    let cors = RpressCors::new()
        .set_origins(vec!["https://example.com"])
        .set_methods(vec!["GET", "POST"]);

    let routes = RpressRoutes::new();
    let (addr, handle) = start_test_server(Some(cors), routes).await;

    let raw = send_raw_request(
        &addr,
        "OPTIONS /anything HTTP/1.1\r\nHost: localhost\r\nOrigin: https://example.com\r\nAccess-Control-Request-Method: POST\r\n\r\n",
    )
    .await;
    let resp = parse_response(&raw);

    assert_eq!(resp.status_code, 204);
    let request_id = resp.get_header("X-Request-ID");
    assert!(
        request_id.is_some(),
        "X-Request-ID must be present on CORS preflight responses"
    );

    handle.abort();
}
