mod helpers;

use rpress::core::handler_response::ResponsePayload;
use rpress::core::routes::RpressRoutes;
use rpress::types::definitions::{RequestPayload, StatusCode};
use helpers::{parse_response, send_raw_request, start_test_server};

#[tokio::test]
async fn test_static_route_get() {
    let mut routes = RpressRoutes::new();
    routes.add(":get/hello", |_req: RequestPayload| async move {
        ResponsePayload::text("Hello World")
    });

    let (addr, handle) = start_test_server(None, routes).await;

    let raw = send_raw_request(
        &addr,
        "GET /hello HTTP/1.1\r\nHost: localhost\r\n\r\n",
    )
    .await;
    let resp = parse_response(&raw);

    assert_eq!(resp.status_code, 200);
    assert_eq!(resp.body, "Hello World");

    handle.abort();
}

#[tokio::test]
async fn test_dynamic_route_param() {
    let mut routes = RpressRoutes::new();
    routes.add(":get/users/:id", |req: RequestPayload| async move {
        let id = req.get_param("id").unwrap();
        ResponsePayload::text(format!("user:{}", id))
    });

    let (addr, handle) = start_test_server(None, routes).await;

    let raw = send_raw_request(
        &addr,
        "GET /users/42 HTTP/1.1\r\nHost: localhost\r\n\r\n",
    )
    .await;
    let resp = parse_response(&raw);

    assert_eq!(resp.status_code, 200);
    assert_eq!(resp.body, "user:42");

    handle.abort();
}

#[tokio::test]
async fn test_404_not_found() {
    let mut routes = RpressRoutes::new();
    routes.add(":get/exists", |_req: RequestPayload| async move {
        ResponsePayload::text("ok")
    });

    let (addr, handle) = start_test_server(None, routes).await;

    let raw = send_raw_request(
        &addr,
        "GET /does_not_exist HTTP/1.1\r\nHost: localhost\r\n\r\n",
    )
    .await;
    let resp = parse_response(&raw);

    assert_eq!(resp.status_code, 404);

    handle.abort();
}

#[tokio::test]
async fn test_405_method_not_allowed() {
    let mut routes = RpressRoutes::new();
    routes.add(":get/only_get", |_req: RequestPayload| async move {
        ResponsePayload::text("ok")
    });

    let (addr, handle) = start_test_server(None, routes).await;

    let raw = send_raw_request(
        &addr,
        "POST /only_get HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\n\r\n",
    )
    .await;
    let resp = parse_response(&raw);

    assert_eq!(resp.status_code, 405);

    handle.abort();
}

#[tokio::test]
async fn test_post_route() {
    let mut routes = RpressRoutes::new();
    routes.add(":post/data", |_req: RequestPayload| async move {
        ResponsePayload::text("created").with_status(StatusCode::Created)
    });

    let (addr, handle) = start_test_server(None, routes).await;

    let raw = send_raw_request(
        &addr,
        "POST /data HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\n\r\n",
    )
    .await;
    let resp = parse_response(&raw);

    assert_eq!(resp.status_code, 201);
    assert_eq!(resp.body, "created");

    handle.abort();
}

#[tokio::test]
async fn test_head_returns_no_body() {
    let mut routes = RpressRoutes::new();
    routes.add(":get/headtest", |_req: RequestPayload| async move {
        ResponsePayload::text("this body should not appear in HEAD")
    });

    let (addr, handle) = start_test_server(None, routes).await;

    let raw = send_raw_request(
        &addr,
        "HEAD /headtest HTTP/1.1\r\nHost: localhost\r\n\r\n",
    )
    .await;
    let resp = parse_response(&raw);

    assert_eq!(resp.status_code, 200);
    assert_eq!(resp.body, "");

    handle.abort();
}

#[tokio::test]
async fn test_same_dynamic_param_name_is_allowed() {
    let mut routes = RpressRoutes::new();
    routes.add(":get/tenants/:tenant_id/users", |req: RequestPayload| async move {
        let tid = req.get_param("tenant_id").unwrap_or("?");
        ResponsePayload::text(format!("users of {}", tid))
    });
    routes.add(":get/tenants/:tenant_id/settings", |req: RequestPayload| async move {
        let tid = req.get_param("tenant_id").unwrap_or("?");
        ResponsePayload::text(format!("settings of {}", tid))
    });

    let (addr, handle) = start_test_server(None, routes).await;

    let raw = send_raw_request(
        &addr,
        "GET /tenants/abc/users HTTP/1.1\r\nHost: localhost\r\n\r\n",
    )
    .await;
    let resp = parse_response(&raw);
    assert_eq!(resp.status_code, 200);
    assert_eq!(resp.body, "users of abc");

    let raw2 = send_raw_request(
        &addr,
        "GET /tenants/abc/settings HTTP/1.1\r\nHost: localhost\r\n\r\n",
    )
    .await;
    let resp2 = parse_response(&raw2);
    assert_eq!(resp2.status_code, 200);
    assert_eq!(resp2.body, "settings of abc");

    handle.abort();
}

#[tokio::test]
async fn test_conflicting_dynamic_param_names_panics() {
    let mut routes = RpressRoutes::new();
    routes.add(":get/tenants/:tenant_id/users", |_req: RequestPayload| async move {
        ResponsePayload::text("ok")
    });
    routes.add(":get/tenants/:id", |_req: RequestPayload| async move {
        ResponsePayload::text("ok")
    });

    let mut app = rpress::Rpress::new(None);
    app.add_route_group(routes);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let handle = tokio::spawn(async move {
        app.server_with_listener(listener).await.ok();
    });

    let result = handle.await;
    assert!(result.is_err(), "task should have panicked on conflicting param names");
}
