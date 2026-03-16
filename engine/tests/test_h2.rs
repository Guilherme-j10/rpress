mod helpers;

use std::sync::Arc;

use helpers::start_test_server_tls;
use rpress::core::routes::RpressRoutes;
use rpress::{RequestPayload, ResponsePayload};

async fn h2_get(
    addr: &str,
    path: &str,
    client_config: &Arc<rustls::ClientConfig>,
) -> (u16, String, Vec<(String, String)>) {
    let mut h2_client_config = (**client_config).clone();
    h2_client_config.alpn_protocols = vec![b"h2".to_vec()];
    let h2_client_config = Arc::new(h2_client_config);

    let tcp = tokio::net::TcpStream::connect(addr).await.unwrap();
    let connector = tokio_rustls::TlsConnector::from(h2_client_config);
    let server_name = rustls_pki_types::ServerName::try_from("localhost").unwrap();
    let tls_stream = connector.connect(server_name, tcp).await.unwrap();

    let (mut client, h2_conn) = h2::client::handshake(tls_stream).await.unwrap();
    tokio::spawn(async move {
        h2_conn.await.ok();
    });

    let request = http::Request::builder()
        .method("GET")
        .uri(path)
        .body(())
        .unwrap();

    let (response, _) = client.send_request(request, true).unwrap();
    let response = response.await.unwrap();

    let status = response.status().as_u16();
    let mut headers = Vec::new();
    for (key, value) in response.headers() {
        headers.push((
            key.as_str().to_string(),
            value.to_str().unwrap_or("").to_string(),
        ));
    }

    let mut body_bytes = Vec::new();
    let mut body_stream = response.into_body();
    while let Some(chunk) = body_stream.data().await {
        let data = chunk.unwrap();
        let _ = body_stream.flow_control().release_capacity(data.len());
        body_bytes.extend_from_slice(&data);
    }

    let body = String::from_utf8_lossy(&body_bytes).to_string();
    (status, body, headers)
}

async fn h2_post(
    addr: &str,
    path: &str,
    body: &[u8],
    client_config: &Arc<rustls::ClientConfig>,
) -> (u16, String, Vec<(String, String)>) {
    let mut h2_client_config = (**client_config).clone();
    h2_client_config.alpn_protocols = vec![b"h2".to_vec()];
    let h2_client_config = Arc::new(h2_client_config);

    let tcp = tokio::net::TcpStream::connect(addr).await.unwrap();
    let connector = tokio_rustls::TlsConnector::from(h2_client_config);
    let server_name = rustls_pki_types::ServerName::try_from("localhost").unwrap();
    let tls_stream = connector.connect(server_name, tcp).await.unwrap();

    let (mut client, h2_conn) = h2::client::handshake(tls_stream).await.unwrap();
    tokio::spawn(async move {
        h2_conn.await.ok();
    });

    let request = http::Request::builder()
        .method("POST")
        .uri(path)
        .body(())
        .unwrap();

    let (response, mut send_stream) = client.send_request(request, false).unwrap();
    send_stream.send_data(body.to_vec().into(), true).unwrap();

    let response = response.await.unwrap();
    let status = response.status().as_u16();
    let mut headers = Vec::new();
    for (key, value) in response.headers() {
        headers.push((
            key.as_str().to_string(),
            value.to_str().unwrap_or("").to_string(),
        ));
    }

    let mut body_bytes = Vec::new();
    let mut body_stream = response.into_body();
    while let Some(chunk) = body_stream.data().await {
        let data = chunk.unwrap();
        let _ = body_stream.flow_control().release_capacity(data.len());
        body_bytes.extend_from_slice(&data);
    }

    let body_str = String::from_utf8_lossy(&body_bytes).to_string();
    (status, body_str, headers)
}

#[tokio::test]
async fn test_h2_get_request() {
    let mut routes = RpressRoutes::new();
    routes.add(":get/hello", |_req: RequestPayload| async {
        ResponsePayload::text("Hello HTTP/2!")
    });

    let (addr, _handle, client_config) = start_test_server_tls(None, routes).await;
    let (status, body, _headers) = h2_get(&addr, "/hello", &client_config).await;

    assert_eq!(status, 200);
    assert_eq!(body, "Hello HTTP/2!");
}

#[tokio::test]
async fn test_h2_404_not_found() {
    let routes = RpressRoutes::new();
    let (addr, _handle, client_config) = start_test_server_tls(None, routes).await;

    let (status, _body, _headers) = h2_get(&addr, "/nonexistent", &client_config).await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn test_h2_post_with_body() {
    let mut routes = RpressRoutes::new();
    routes.add(":post/echo", |req: RequestPayload| async move {
        let body = req.body_str().unwrap_or("").to_string();
        ResponsePayload::text(body)
    });

    let (addr, _handle, client_config) = start_test_server_tls(None, routes).await;
    let (status, body, _headers) = h2_post(&addr, "/echo", b"http2 body", &client_config).await;

    assert_eq!(status, 200);
    assert_eq!(body, "http2 body");
}

#[tokio::test]
async fn test_h2_json_response() {
    let mut routes = RpressRoutes::new();
    routes.add(":get/data", |_req: RequestPayload| async {
        ResponsePayload::json(&serde_json::json!({"protocol": "h2"})).unwrap()
    });

    let (addr, _handle, client_config) = start_test_server_tls(None, routes).await;
    let (status, body, headers) = h2_get(&addr, "/data", &client_config).await;

    assert_eq!(status, 200);
    assert!(body.contains("\"protocol\":\"h2\""));

    let ct = headers.iter().find(|(k, _)| k == "content-type");
    assert!(ct.is_some());
    assert_eq!(ct.unwrap().1, "application/json");
}

#[tokio::test]
async fn test_h2_request_id_header() {
    let mut routes = RpressRoutes::new();
    routes.add(":get/ping", |_req: RequestPayload| async {
        ResponsePayload::text("pong")
    });

    let (addr, _handle, client_config) = start_test_server_tls(None, routes).await;
    let (_status, _body, headers) = h2_get(&addr, "/ping", &client_config).await;

    let request_id = headers.iter().find(|(k, _)| k == "x-request-id");
    assert!(request_id.is_some());
    assert!(!request_id.unwrap().1.is_empty());
}

#[tokio::test]
async fn test_h2_dynamic_route_param() {
    let mut routes = RpressRoutes::new();
    routes.add(":get/users/:id", |req: RequestPayload| async move {
        let id = req.get_param("id").unwrap_or("unknown").to_string();
        ResponsePayload::text(format!("user:{}", id))
    });

    let (addr, _handle, client_config) = start_test_server_tls(None, routes).await;
    let (status, body, _headers) = h2_get(&addr, "/users/42", &client_config).await;

    assert_eq!(status, 200);
    assert_eq!(body, "user:42");
}
