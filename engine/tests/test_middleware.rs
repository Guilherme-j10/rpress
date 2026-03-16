mod helpers;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use rpress::core::handler_response::{ResponsePayload, RpressError};
use rpress::core::routes::RpressRoutes;
use rpress::types::definitions::{RequestPayload, StatusCode};
use helpers::{parse_response, send_raw_request, start_test_server};

#[tokio::test]
async fn test_group_middleware_runs() {
    let counter = Arc::new(AtomicUsize::new(0));
    let counter_clone = counter.clone();

    let mut routes = RpressRoutes::new();

    let mw_counter = counter_clone.clone();
    routes.use_middleware(move |req, next| {
        let mw_counter = mw_counter.clone();
        async move {
            mw_counter.fetch_add(1, Ordering::SeqCst);
            next(req).await
        }
    });

    routes.add(":get/counted", |_req: RequestPayload| async move {
        ResponsePayload::text("ok")
    });

    let (addr, handle) = start_test_server(None, routes).await;

    send_raw_request(
        &addr,
        "GET /counted HTTP/1.1\r\nHost: localhost\r\n\r\n",
    )
    .await;

    assert_eq!(counter.load(Ordering::SeqCst), 1);

    send_raw_request(
        &addr,
        "GET /counted HTTP/1.1\r\nHost: localhost\r\n\r\n",
    )
    .await;

    assert_eq!(counter.load(Ordering::SeqCst), 2);

    handle.abort();
}

#[tokio::test]
async fn test_middleware_short_circuit() {
    let mut routes = RpressRoutes::new();

    routes.use_middleware(|_req, _next| async move {
        Err(RpressError {
            status: StatusCode::Unauthorized,
            message: "blocked".to_string(),
        })
    });

    routes.add(":get/protected", |_req: RequestPayload| async move {
        ResponsePayload::text("should not reach here")
    });

    let (addr, handle) = start_test_server(None, routes).await;

    let raw = send_raw_request(
        &addr,
        "GET /protected HTTP/1.1\r\nHost: localhost\r\n\r\n",
    )
    .await;
    let resp = parse_response(&raw);

    assert_eq!(resp.status_code, 401);
    assert!(resp.body.contains("blocked"));

    handle.abort();
}

#[tokio::test]
async fn test_middleware_chain_order() {
    let order = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));

    let mut routes = RpressRoutes::new();

    let order1 = order.clone();
    routes.use_middleware(move |req, next| {
        let order1 = order1.clone();
        async move {
            order1.lock().unwrap().push("mw1_before".to_string());
            let result = next(req).await;
            order1.lock().unwrap().push("mw1_after".to_string());
            result
        }
    });

    let order2 = order.clone();
    routes.use_middleware(move |req, next| {
        let order2 = order2.clone();
        async move {
            order2.lock().unwrap().push("mw2_before".to_string());
            let result = next(req).await;
            order2.lock().unwrap().push("mw2_after".to_string());
            result
        }
    });

    let order_handler = order.clone();
    routes.add(":get/order", move |_req: RequestPayload| {
        let order_handler = order_handler.clone();
        async move {
            order_handler.lock().unwrap().push("handler".to_string());
            ResponsePayload::text("ok")
        }
    });

    let (addr, handle) = start_test_server(None, routes).await;

    send_raw_request(
        &addr,
        "GET /order HTTP/1.1\r\nHost: localhost\r\n\r\n",
    )
    .await;

    let recorded = order.lock().unwrap().clone();
    assert_eq!(
        recorded,
        vec!["mw1_before", "mw2_before", "handler", "mw2_after", "mw1_after"]
    );

    handle.abort();
}
