mod helpers;

use std::io::Read;
use rpress::core::cors::RpressCors;
use rpress::core::handler_response::{CookieBuilder, ResponsePayload};
use rpress::core::routes::RpressRoutes;
use rpress::types::definitions::{RequestPayload, StatusCode};
use helpers::{
    parse_response, send_raw_request, send_raw_request_bytes,
    split_http_response_bytes, start_test_server, start_test_server_custom,
};

// 4.1 — Multi-method same path
#[tokio::test]
async fn test_multi_method_same_path() {
    let mut routes = RpressRoutes::new();
    routes.add(":get/api", |_req: RequestPayload| async move {
        ResponsePayload::text("get_result")
    });
    routes.add(":post/api", |_req: RequestPayload| async move {
        ResponsePayload::text("post_result").with_status(StatusCode::Created)
    });

    let (addr, handle) = start_test_server(None, routes).await;

    let raw_get = send_raw_request(
        &addr,
        "GET /api HTTP/1.1\r\nHost: localhost\r\n\r\n",
    ).await;
    let resp_get = parse_response(&raw_get);
    assert_eq!(resp_get.status_code, 200);
    assert_eq!(resp_get.body, "get_result");

    let raw_post = send_raw_request(
        &addr,
        "POST /api HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\n\r\n",
    ).await;
    let resp_post = parse_response(&raw_post);
    assert_eq!(resp_post.status_code, 201);
    assert_eq!(resp_post.body, "post_result");

    let raw_delete = send_raw_request(
        &addr,
        "DELETE /api HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\n\r\n",
    ).await;
    let resp_delete = parse_response(&raw_delete);
    assert_eq!(resp_delete.status_code, 405);

    handle.abort();
}

// 4.2 — Rate limiting
#[tokio::test]
async fn test_rate_limiting() {
    let mut routes = RpressRoutes::new();
    routes.add(":get/limited", |_req: RequestPayload| async move {
        ResponsePayload::text("ok")
    });

    let (addr, handle) = start_test_server_custom(None, routes, |app| {
        app.set_rate_limit(3, 60);
    }).await;

    for i in 1..=3 {
        let raw = send_raw_request(
            &addr,
            "GET /limited HTTP/1.1\r\nHost: localhost\r\n\r\n",
        ).await;
        let resp = parse_response(&raw);
        assert_eq!(resp.status_code, 200, "Request {} should succeed", i);
    }

    let raw = send_raw_request(
        &addr,
        "GET /limited HTTP/1.1\r\nHost: localhost\r\n\r\n",
    ).await;
    let resp = parse_response(&raw);
    assert_eq!(resp.status_code, 429);

    handle.abort();
}

// 4.3 — Serve static files
#[tokio::test]
async fn test_serve_static() {
    let temp_dir = std::env::temp_dir().join("rpress_test_static");
    let _ = std::fs::create_dir_all(&temp_dir);
    std::fs::write(temp_dir.join("hello.txt"), "static content").unwrap();
    std::fs::write(temp_dir.join("page.html"), "<h1>Hello</h1>").unwrap();

    let routes = RpressRoutes::new();
    let dir_str = temp_dir.to_string_lossy().to_string();

    let (addr, handle) = start_test_server_custom(None, routes, |app| {
        app.serve_static("/static", &dir_str);
    }).await;

    let raw = send_raw_request(
        &addr,
        "GET /static/hello.txt HTTP/1.1\r\nHost: localhost\r\n\r\n",
    ).await;
    let resp = parse_response(&raw);
    assert_eq!(resp.status_code, 200);
    assert_eq!(resp.body, "static content");
    assert!(resp.get_header("Content-Type").unwrap().contains("text/plain"));

    let raw_html = send_raw_request(
        &addr,
        "GET /static/page.html HTTP/1.1\r\nHost: localhost\r\n\r\n",
    ).await;
    let resp_html = parse_response(&raw_html);
    assert_eq!(resp_html.status_code, 200);
    assert!(resp_html.get_header("Content-Type").unwrap().contains("text/html"));

    let raw_traversal = send_raw_request(
        &addr,
        "GET /static/../../../etc/passwd HTTP/1.1\r\nHost: localhost\r\n\r\n",
    ).await;
    let resp_traversal = parse_response(&raw_traversal);
    assert_eq!(resp_traversal.status_code, 404);

    let raw_missing = send_raw_request(
        &addr,
        "GET /static/nonexistent.txt HTTP/1.1\r\nHost: localhost\r\n\r\n",
    ).await;
    let resp_missing = parse_response(&raw_missing);
    assert_eq!(resp_missing.status_code, 404);

    let _ = std::fs::remove_dir_all(&temp_dir);
    handle.abort();
}

// 4.4 — Cookies
#[tokio::test]
async fn test_cookies_parsing_and_set_cookie() {
    let mut routes = RpressRoutes::new();

    routes.add(":get/read_cookies", |req: RequestPayload| async move {
        let cookies = req.cookies();
        let session = cookies.get("session").cloned().unwrap_or_default();
        ResponsePayload::text(format!("session:{}", session))
    });

    routes.add(":get/set_cookies", |_req: RequestPayload| async move {
        let cookie1 = CookieBuilder::new("token", "abc123")
            .max_age(3600)
            .secure(true);
        let cookie2 = CookieBuilder::new("lang", "pt-BR")
            .http_only(false);

        ResponsePayload::text("cookies set")
            .set_cookie(&cookie1)
            .set_cookie(&cookie2)
    });

    let (addr, handle) = start_test_server(None, routes).await;

    let raw = send_raw_request(
        &addr,
        "GET /read_cookies HTTP/1.1\r\nHost: localhost\r\nCookie: session=xyz789; theme=dark\r\n\r\n",
    ).await;
    let resp = parse_response(&raw);
    assert_eq!(resp.status_code, 200);
    assert_eq!(resp.body, "session:xyz789");

    let raw_set = send_raw_request(
        &addr,
        "GET /set_cookies HTTP/1.1\r\nHost: localhost\r\n\r\n",
    ).await;
    let resp_set = parse_response(&raw_set);
    assert_eq!(resp_set.status_code, 200);
    let set_cookie_headers = resp_set.get_all_headers("Set-Cookie");
    assert_eq!(set_cookie_headers.len(), 2, "Should have 2 separate Set-Cookie headers");
    assert!(set_cookie_headers.iter().any(|h| h.contains("token=abc123")));
    assert!(set_cookie_headers.iter().any(|h| h.contains("lang=pt-BR")));

    handle.abort();
}

// 4.5 — Percent-decoding URI and + in query
#[tokio::test]
async fn test_percent_decode_uri() {
    let mut routes = RpressRoutes::new();
    routes.add(":get/users/:id", |req: RequestPayload| async move {
        let id = req.get_param("id").unwrap_or("none");
        ResponsePayload::text(format!("id:{}", id))
    });

    let (addr, handle) = start_test_server(None, routes).await;

    let raw = send_raw_request(
        &addr,
        "GET /us%65rs/42 HTTP/1.1\r\nHost: localhost\r\n\r\n",
    ).await;
    let resp = parse_response(&raw);
    assert_eq!(resp.status_code, 200);
    assert_eq!(resp.body, "id:42");

    handle.abort();
}

#[tokio::test]
async fn test_plus_in_query_string() {
    let mut routes = RpressRoutes::new();
    routes.add(":get/search", |req: RequestPayload| async move {
        let q = req.get_query("q").unwrap_or_default();
        ResponsePayload::text(format!("query:{}", q))
    });

    let (addr, handle) = start_test_server(None, routes).await;

    let raw = send_raw_request(
        &addr,
        "GET /search?q=hello+world HTTP/1.1\r\nHost: localhost\r\n\r\n",
    ).await;
    let resp = parse_response(&raw);
    assert_eq!(resp.status_code, 200);
    assert_eq!(resp.body, "query:hello world");

    handle.abort();
}

// 4.6 — Response builders and request helpers
#[tokio::test]
async fn test_html_builder() {
    let mut routes = RpressRoutes::new();
    routes.add(":get/page", |_req: RequestPayload| async move {
        ResponsePayload::html("<h1>Hello</h1>")
    });

    let (addr, handle) = start_test_server(None, routes).await;

    let raw = send_raw_request(
        &addr,
        "GET /page HTTP/1.1\r\nHost: localhost\r\n\r\n",
    ).await;
    let resp = parse_response(&raw);
    assert_eq!(resp.status_code, 200);
    assert!(resp.get_header("Content-Type").unwrap().contains("text/html"));
    assert_eq!(resp.body, "<h1>Hello</h1>");

    handle.abort();
}

#[tokio::test]
async fn test_bytes_builder() {
    let mut routes = RpressRoutes::new();
    routes.add(":get/bin", |_req: RequestPayload| async move {
        ResponsePayload::bytes(vec![0x89, 0x50, 0x4E, 0x47], "image/png")
    });

    let (addr, handle) = start_test_server(None, routes).await;

    let raw = send_raw_request(
        &addr,
        "GET /bin HTTP/1.1\r\nHost: localhost\r\n\r\n",
    ).await;
    let resp = parse_response(&raw);
    assert_eq!(resp.status_code, 200);
    assert!(resp.get_header("Content-Type").unwrap().contains("image/png"));

    handle.abort();
}

#[tokio::test]
async fn test_with_content_type() {
    let mut routes = RpressRoutes::new();
    routes.add(":get/custom_ct", |_req: RequestPayload| async move {
        ResponsePayload::text("data").with_content_type("application/xml")
    });

    let (addr, handle) = start_test_server(None, routes).await;

    let raw = send_raw_request(
        &addr,
        "GET /custom_ct HTTP/1.1\r\nHost: localhost\r\n\r\n",
    ).await;
    let resp = parse_response(&raw);
    assert_eq!(resp.status_code, 200);
    assert!(resp.get_header("Content-Type").unwrap().contains("application/xml"));

    handle.abort();
}

#[tokio::test]
async fn test_request_helpers() {
    let mut routes = RpressRoutes::new();
    routes.add(":post/echo_helpers", |req: RequestPayload| async move {
        let uri = req.uri().to_string();
        let method = req.method().to_string();
        let ct = req.header("content-type").unwrap_or("none").to_string();
        let body = req.body_str().unwrap_or("invalid utf8").to_string();
        ResponsePayload::text(format!("{} {} ct:{} body:{}", method, uri, ct, body))
    });

    let (addr, handle) = start_test_server(None, routes).await;

    let body = "test body";
    let request = format!(
        "POST /echo_helpers HTTP/1.1\r\nHost: localhost\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{}",
        body.len(), body
    );

    let raw = send_raw_request(&addr, &request).await;
    let resp = parse_response(&raw);
    assert_eq!(resp.status_code, 200);
    assert!(resp.body.contains("POST"));
    assert!(resp.body.contains("/echo_helpers"));
    assert!(resp.body.contains("ct:text/plain"));
    assert!(resp.body.contains("body:test body"));

    handle.abort();
}

#[tokio::test]
async fn test_body_json_helper() {
    let mut routes = RpressRoutes::new();
    routes.add(":post/json_echo", |req: RequestPayload| async move {
        let data: serde_json::Value = req.body_json().unwrap();
        let name = data.get("name").and_then(|v| v.as_str()).unwrap_or("unknown");
        ResponsePayload::text(format!("name:{}", name))
    });

    let (addr, handle) = start_test_server(None, routes).await;

    let body = r#"{"name":"Rpress"}"#;
    let request = format!(
        "POST /json_echo HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        body.len(), body
    );

    let raw = send_raw_request(&addr, &request).await;
    let resp = parse_response(&raw);
    assert_eq!(resp.status_code, 200);
    assert_eq!(resp.body, "name:Rpress");

    handle.abort();
}

// 4.7 — X-Request-ID present
#[tokio::test]
async fn test_x_request_id_present() {
    let mut routes = RpressRoutes::new();
    routes.add(":get/rid", |_req: RequestPayload| async move {
        ResponsePayload::text("ok")
    });

    let (addr, handle) = start_test_server(None, routes).await;

    let raw = send_raw_request(
        &addr,
        "GET /rid HTTP/1.1\r\nHost: localhost\r\n\r\n",
    ).await;
    let resp = parse_response(&raw);
    assert_eq!(resp.status_code, 200);
    let rid = resp.get_header("X-Request-ID");
    assert!(rid.is_some(), "X-Request-ID header must be present");
    assert!(!rid.unwrap().is_empty(), "X-Request-ID must not be empty");

    handle.abort();
}

// 4.8 — Vary: Origin with CORS
#[tokio::test]
async fn test_vary_origin_header() {
    let cors = RpressCors::new()
        .set_origins(vec!["https://example.com"]);

    let mut routes = RpressRoutes::new();
    routes.add(":get/vary_test", |_req: RequestPayload| async move {
        ResponsePayload::text("ok")
    });

    let (addr, handle) = start_test_server(Some(cors), routes).await;

    let raw = send_raw_request(
        &addr,
        "GET /vary_test HTTP/1.1\r\nHost: localhost\r\nOrigin: https://example.com\r\n\r\n",
    ).await;
    let resp = parse_response(&raw);
    assert_eq!(resp.status_code, 200);
    assert_eq!(resp.get_header("Vary"), Some("Origin"));

    handle.abort();
}

// Extra — X-Content-Type-Options: nosniff
#[tokio::test]
async fn test_nosniff_header() {
    let mut routes = RpressRoutes::new();
    routes.add(":get/nosniff", |_req: RequestPayload| async move {
        ResponsePayload::text("ok")
    });

    let (addr, handle) = start_test_server(None, routes).await;

    let raw = send_raw_request(
        &addr,
        "GET /nosniff HTTP/1.1\r\nHost: localhost\r\n\r\n",
    ).await;
    let resp = parse_response(&raw);
    assert_eq!(resp.status_code, 200);
    assert_eq!(resp.get_header("X-Content-Type-Options"), Some("nosniff"));

    handle.abort();
}

// --- Body Streaming tests ---

#[tokio::test]
async fn test_body_streaming_collect() {
    let mut routes = RpressRoutes::new();
    routes.add(":post/upload", |mut req: RequestPayload| async move {
        let body = req.collect_body().await;
        ResponsePayload::text(format!("received:{}", body.len()))
    });

    let (addr, handle) = start_test_server_custom(None, routes, |app| {
        app.set_stream_threshold(100);
    }).await;

    let body_data = "X".repeat(500);
    let request = format!(
        "POST /upload HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{}",
        body_data.len(), body_data
    );

    let raw = send_raw_request(&addr, &request).await;
    let resp = parse_response(&raw);
    assert_eq!(resp.status_code, 200);
    assert_eq!(resp.body, "received:500");

    handle.abort();
}

#[tokio::test]
async fn test_body_stream_chunks() {
    let mut routes = RpressRoutes::new();
    routes.add(":post/stream_count", |mut req: RequestPayload| async move {
        let mut chunk_count = 0u32;
        let mut total_bytes = 0usize;

        if let Some(mut rx) = req.body_stream() {
            while let Some(chunk) = rx.recv().await {
                chunk_count += 1;
                total_bytes += chunk.len();
            }
        }

        ResponsePayload::text(format!("chunks:{},bytes:{}", chunk_count, total_bytes))
    });

    let (addr, handle) = start_test_server_custom(None, routes, |app| {
        app.set_stream_threshold(50);
    }).await;

    let body_data = "A".repeat(300);
    let request = format!(
        "POST /stream_count HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{}",
        body_data.len(), body_data
    );

    let raw = send_raw_request(&addr, &request).await;
    let resp = parse_response(&raw);
    assert_eq!(resp.status_code, 200);
    assert!(resp.body.contains("bytes:300"), "Body: {}", resp.body);

    handle.abort();
}

#[tokio::test]
async fn test_stream_threshold_small_body() {
    let mut routes = RpressRoutes::new();
    routes.add(":post/small", |req: RequestPayload| async move {
        let body = std::str::from_utf8(&req.payload).unwrap_or("").to_string();
        ResponsePayload::text(format!("got:{}", body))
    });

    let (addr, handle) = start_test_server_custom(None, routes, |app| {
        app.set_stream_threshold(1024);
    }).await;

    let body = "hello";
    let request = format!(
        "POST /small HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{}",
        body.len(), body
    );

    let raw = send_raw_request(&addr, &request).await;
    let resp = parse_response(&raw);
    assert_eq!(resp.status_code, 200);
    assert_eq!(resp.body, "got:hello");

    handle.abort();
}

// --- Compression tests ---

#[tokio::test]
async fn test_gzip_compression() {
    let mut routes = RpressRoutes::new();
    let big_text = "Hello Rpress! ".repeat(100);
    let big_text_clone = big_text.clone();
    routes.add(":get/big", move |_req: RequestPayload| {
        let text = big_text_clone.clone();
        async move { ResponsePayload::text(text) }
    });

    let (addr, handle) = start_test_server_custom(None, routes, |app| {
        app.enable_compression(true);
    }).await;

    let request = "GET /big HTTP/1.1\r\nHost: localhost\r\nAccept-Encoding: gzip\r\n\r\n";
    let raw_bytes = send_raw_request_bytes(&addr, request.as_bytes()).await;
    let (head, body_bytes) = split_http_response_bytes(&raw_bytes);

    assert!(head.contains("Content-Encoding: gzip"), "Should have gzip encoding, head: {}", head);
    assert!(head.contains("Vary: Accept-Encoding"), "Should have Vary header");

    let mut decoder = flate2::read::GzDecoder::new(&body_bytes[..]);
    let mut decompressed = String::new();
    decoder.read_to_string(&mut decompressed).unwrap();
    assert_eq!(decompressed, big_text);

    handle.abort();
}

#[tokio::test]
async fn test_brotli_compression() {
    let mut routes = RpressRoutes::new();
    let big_text = "Brotli test data! ".repeat(100);
    let big_text_clone = big_text.clone();
    routes.add(":get/brotli", move |_req: RequestPayload| {
        let text = big_text_clone.clone();
        async move { ResponsePayload::text(text) }
    });

    let (addr, handle) = start_test_server_custom(None, routes, |app| {
        app.enable_compression(true);
    }).await;

    let request = "GET /brotli HTTP/1.1\r\nHost: localhost\r\nAccept-Encoding: br\r\n\r\n";
    let raw_bytes = send_raw_request_bytes(&addr, request.as_bytes()).await;
    let (head, body_bytes) = split_http_response_bytes(&raw_bytes);

    assert!(head.contains("Content-Encoding: br"), "Should have brotli encoding, head: {}", head);

    let mut decompressed = Vec::new();
    let mut decoder = brotli::Decompressor::new(&body_bytes[..], 4096);
    decoder.read_to_end(&mut decompressed).unwrap();
    assert_eq!(String::from_utf8(decompressed).unwrap(), big_text);

    handle.abort();
}

#[tokio::test]
async fn test_no_compression_small_body() {
    let mut routes = RpressRoutes::new();
    routes.add(":get/tiny", |_req: RequestPayload| async move {
        ResponsePayload::text("hi")
    });

    let (addr, handle) = start_test_server_custom(None, routes, |app| {
        app.enable_compression(true);
    }).await;

    let raw = send_raw_request(
        &addr,
        "GET /tiny HTTP/1.1\r\nHost: localhost\r\nAccept-Encoding: gzip\r\n\r\n",
    ).await;
    let resp = parse_response(&raw);
    assert_eq!(resp.status_code, 200);
    assert!(resp.get_header("Content-Encoding").is_none(), "Small body should not be compressed");
    assert_eq!(resp.body, "hi");

    handle.abort();
}

#[tokio::test]
async fn test_no_compression_disabled() {
    let mut routes = RpressRoutes::new();
    let big_text = "No compress ".repeat(100);
    let big_text_clone = big_text.clone();
    routes.add(":get/nocompress", move |_req: RequestPayload| {
        let text = big_text_clone.clone();
        async move { ResponsePayload::text(text) }
    });

    let (addr, handle) = start_test_server(None, routes).await;

    let raw = send_raw_request(
        &addr,
        "GET /nocompress HTTP/1.1\r\nHost: localhost\r\nAccept-Encoding: gzip, br\r\n\r\n",
    ).await;
    let resp = parse_response(&raw);
    assert_eq!(resp.status_code, 200);
    assert!(resp.get_header("Content-Encoding").is_none(), "Compression disabled by default");
    assert_eq!(resp.body, big_text);

    handle.abort();
}
