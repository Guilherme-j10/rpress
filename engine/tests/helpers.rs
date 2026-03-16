use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use rpress::core::cors::RpressCors;
use rpress::core::routes::RpressRoutes;
use rpress::Rpress;

#[allow(dead_code)]
pub struct TestResponse {
    pub status_code: u16,
    pub status_text: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl TestResponse {
    #[allow(dead_code)]
    pub fn get_header(&self, key: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_str())
    }

    #[allow(dead_code)]
    pub fn get_all_headers(&self, key: &str) -> Vec<&str> {
        self.headers
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_str())
            .collect()
    }
}

pub fn parse_response(raw: &str) -> TestResponse {
    let (head, body) = raw.split_once("\r\n\r\n").unwrap_or((raw, ""));
    let mut lines = head.split("\r\n");

    let status_line = lines.next().unwrap_or("");
    let parts: Vec<&str> = status_line.splitn(3, ' ').collect();
    let status_code = parts.get(1).unwrap_or(&"0").parse().unwrap_or(0);
    let status_text = parts.get(2).unwrap_or(&"").to_string();

    let mut headers = vec![];
    for line in lines {
        if let Some((key, value)) = line.split_once(": ") {
            headers.push((key.to_string(), value.to_string()));
        }
    }

    TestResponse {
        status_code,
        status_text,
        headers,
        body: body.to_string(),
    }
}

pub async fn send_raw_request(addr: &str, request: &str) -> String {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream.write_all(request.as_bytes()).await.unwrap();
    stream.flush().await.unwrap();

    let mut buf = vec![0u8; 8192];
    let n = stream.read(&mut buf).await.unwrap();
    String::from_utf8_lossy(&buf[..n]).to_string()
}

#[allow(dead_code)]
pub async fn send_raw_request_bytes(addr: &str, request: &[u8]) -> Vec<u8> {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream.write_all(request).await.unwrap();
    stream.flush().await.unwrap();

    let mut buf = Vec::new();
    let mut tmp = [0u8; 8192];
    loop {
        match tokio::time::timeout(
            tokio::time::Duration::from_millis(500),
            stream.read(&mut tmp),
        ).await {
            Ok(Ok(0)) => break,
            Ok(Ok(n)) => buf.extend_from_slice(&tmp[..n]),
            _ => break,
        }
    }
    buf
}

#[allow(dead_code)]
pub fn split_http_response_bytes(raw: &[u8]) -> (String, Vec<u8>) {
    let separator = b"\r\n\r\n";
    if let Some(pos) = raw.windows(4).position(|w| w == separator) {
        let head = String::from_utf8_lossy(&raw[..pos]).to_string();
        let body = raw[pos + 4..].to_vec();
        (head, body)
    } else {
        (String::from_utf8_lossy(raw).to_string(), Vec::new())
    }
}

pub async fn start_test_server(
    cors: Option<RpressCors>,
    routes: RpressRoutes,
) -> (String, tokio::task::JoinHandle<()>) {
    start_test_server_custom(cors, routes, |_| {}).await
}

#[allow(dead_code)]
pub async fn start_test_server_custom<F: FnOnce(&mut Rpress)>(
    cors: Option<RpressCors>,
    routes: RpressRoutes,
    configure: F,
) -> (String, tokio::task::JoinHandle<()>) {
    let mut app = Rpress::new(cors);
    app.add_route_group(routes);
    configure(&mut app);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let addr_str = format!("127.0.0.1:{}", addr.port());

    let handle = tokio::spawn(async move {
        app.server_with_listener(listener).await.ok();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    (addr_str, handle)
}
