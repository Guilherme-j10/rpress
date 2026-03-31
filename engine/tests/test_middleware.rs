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

#[tokio::test]
async fn test_middleware_sets_extension_readable_by_handler() {
    let mut routes = RpressRoutes::new();

    routes.use_middleware(|mut req, next| async move {
        req.set_extension("user_id", "42");
        req.set_extension("role", "admin");
        next(req).await
    });

    routes.add(":get/me", |req: RequestPayload| async move {
        let user_id = req.get_extension("user_id").unwrap_or("none");
        let role = req.get_extension("role").unwrap_or("none");
        ResponsePayload::text(format!("{}:{}", user_id, role))
    });

    let (addr, handle) = start_test_server(None, routes).await;

    let raw = send_raw_request(
        &addr,
        "GET /me HTTP/1.1\r\nHost: localhost\r\n\r\n",
    )
    .await;
    let resp = parse_response(&raw);

    assert_eq!(resp.status_code, 200);
    assert_eq!(resp.body, "42:admin");

    handle.abort();
}

#[tokio::test]
async fn test_extension_missing_returns_none() {
    let mut routes = RpressRoutes::new();

    routes.add(":get/no-ext", |req: RequestPayload| async move {
        let val = req.get_extension("missing").unwrap_or("absent");
        ResponsePayload::text(val)
    });

    let (addr, handle) = start_test_server(None, routes).await;

    let raw = send_raw_request(
        &addr,
        "GET /no-ext HTTP/1.1\r\nHost: localhost\r\n\r\n",
    )
    .await;
    let resp = parse_response(&raw);

    assert_eq!(resp.status_code, 200);
    assert_eq!(resp.body, "absent");

    handle.abort();
}

#[tokio::test]
async fn test_extension_overwritten_by_later_middleware() {
    let mut routes = RpressRoutes::new();

    routes.use_middleware(|mut req, next| async move {
        req.set_extension("role", "user");
        next(req).await
    });

    routes.use_middleware(|mut req, next| async move {
        req.set_extension("role", "superadmin");
        next(req).await
    });

    routes.add(":get/role", |req: RequestPayload| async move {
        let role = req.get_extension("role").unwrap_or("none");
        ResponsePayload::text(role)
    });

    let (addr, handle) = start_test_server(None, routes).await;

    let raw = send_raw_request(
        &addr,
        "GET /role HTTP/1.1\r\nHost: localhost\r\n\r\n",
    )
    .await;
    let resp = parse_response(&raw);

    assert_eq!(resp.status_code, 200);
    assert_eq!(resp.body, "superadmin");

    handle.abort();
}

#[tokio::test]
async fn test_extension_auth_guard_pattern() {
    let mut routes = RpressRoutes::new();

    routes.use_middleware(|mut req, next| async move {
        let token = req.header("authorization");

        match token {
            Some("Bearer valid-token") => {
                req.set_extension("user_id", "99");
                req.set_extension("tenant_id", "tenant-abc");
                next(req).await
            }
            _ => Err(RpressError {
                status: StatusCode::Unauthorized,
                message: "invalid token".to_string(),
            }),
        }
    });

    routes.add(":get/protected", |req: RequestPayload| async move {
        let user_id = req.get_extension("user_id").unwrap_or("?");
        let tenant = req.get_extension("tenant_id").unwrap_or("?");
        ResponsePayload::text(format!("user={} tenant={}", user_id, tenant))
    });

    let (addr, handle) = start_test_server(None, routes).await;

    let raw_ok = send_raw_request(
        &addr,
        "GET /protected HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer valid-token\r\n\r\n",
    )
    .await;
    let resp_ok = parse_response(&raw_ok);
    assert_eq!(resp_ok.status_code, 200);
    assert_eq!(resp_ok.body, "user=99 tenant=tenant-abc");

    let raw_fail = send_raw_request(
        &addr,
        "GET /protected HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer bad\r\n\r\n",
    )
    .await;
    let resp_fail = parse_response(&raw_fail);
    assert_eq!(resp_fail.status_code, 401);
    assert!(resp_fail.body.contains("invalid token"));

    handle.abort();
}
