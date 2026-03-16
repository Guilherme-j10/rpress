# Rpress

An async HTTP/1.1 and HTTP/2 framework in Rust, built on top of `tokio`. Designed to be lightweight, secure, and production-ready.

## Features

- Trie-based routing (static, dynamic, multi-method)
- Middleware (global and per route group)
- **TLS nativo via rustls** (HTTPS com certificados PEM)
- **HTTP/2 via h2** (negociação automática por ALPN sobre TLS)
- Request body streaming via `mpsc::channel`
- Automatic gzip/brotli compression
- Native CORS with builder pattern and **fail-fast validation** (RFC compliance)
- **Granular body size limits** (global and per route group)
- **Pluggable rate limiting** via `RateLimiter` trait (in-memory or distributed backends like Redis)
- Static file serving
- Cookies (parsing and Set-Cookie builder)
- Graceful shutdown
- Configurable timeouts (read and idle)
- Concurrent connection limits
- Automatic security headers (`X-Content-Type-Options: nosniff`)
- Automatic request ID (`X-Request-ID`)

## Quick Start

```rust
use rpress::{Rpress, RpressCors, RpressRoutes, RequestPayload, ResponsePayload};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let cors = RpressCors::new()
        .set_origins(vec!["*"])
        .set_methods(vec!["GET", "POST", "PUT", "DELETE"])
        .set_headers(vec!["Content-Type", "Authorization"]);

    let mut app = Rpress::new(Some(cors));

    let mut routes = RpressRoutes::new();
    routes.add(":get/hello", |_req: RequestPayload| async move {
        ResponsePayload::text("Hello, Rpress!")
    });

    app.add_route_group(routes);
    app.listen("0.0.0.0:3000").await?;

    Ok(())
}
```

## Routing

Routes use the format `:method/path`. Dynamic segments are prefixed with `:`.

### Static routes

```rust
let mut routes = RpressRoutes::new();

routes.add(":get/api/users", |_req: RequestPayload| async move {
    ResponsePayload::json(&serde_json::json!({"users": []})).unwrap()
});
```

### Dynamic route parameters

```rust
routes.add(":get/api/users/:id", |req: RequestPayload| async move {
    let id = req.get_param("id").unwrap_or("0");
    ResponsePayload::text(format!("User ID: {}", id))
});
```

### Multi-method on the same path

```rust
routes.add(":get/api/resource", |_req: RequestPayload| async move {
    ResponsePayload::text("GET resource")
});

routes.add(":post/api/resource", |_req: RequestPayload| async move {
    ResponsePayload::text("POST resource").with_status(StatusCode::Created)
});

routes.add(":delete/api/resource/:id", |req: RequestPayload| async move {
    let id = req.get_param("id").unwrap_or("?");
    ResponsePayload::text(format!("Deleted {}", id))
});
```

### Supported HTTP methods

`GET`, `POST`, `PUT`, `PATCH`, `DELETE`, `HEAD`, `OPTIONS`

## Middleware

### Global middleware

Applied to all routes:

```rust
app.use_middleware(|req, next| async move {
    let uri = req.uri().to_string();
    let method = req.method().to_string();

    tracing::info!("--> {} {}", method, uri);
    let start = std::time::Instant::now();

    let result = next(req).await;

    tracing::info!("<-- {} {} ({:?})", method, uri, start.elapsed());
    result
});
```

### Route group middleware

```rust
let mut routes = RpressRoutes::new();

routes.use_middleware(|req, next| async move {
    if req.header("authorization").is_none() {
        return Err(RpressError {
            status: StatusCode::Unauthorized,
            message: "Token required".to_string(),
        });
    }
    next(req).await
});

routes.add(":get/admin/dashboard", |_req: RequestPayload| async move {
    ResponsePayload::text("Admin area")
});
```

## Request

### Accessing request data

```rust
routes.add(":post/api/data", |req: RequestPayload| async move {
    // URI and method
    let uri = req.uri();
    let method = req.method();

    // Headers (keys are lowercase)
    let content_type = req.header("content-type").unwrap_or("unknown");
    let auth = req.header("authorization");

    // Route parameters
    let id = req.get_param("id");

    // Query string — GET /search?q=rust&page=1
    let query = req.get_query("q").unwrap_or("");
    let page = req.get_query("page").unwrap_or("1");

    // Cookies
    let cookies = req.cookies();
    let session = cookies.get("session_id");

    // Body as string
    let body_text = req.body_str().unwrap_or("invalid utf8");

    // Body as JSON
    let data: serde_json::Value = req.body_json().unwrap();

    ResponsePayload::text("ok")
});
```

### Body Streaming

For large uploads, Rpress can stream the body in chunks via a channel instead of accumulating everything in memory. The threshold is configurable:

```rust
app.set_stream_threshold(64 * 1024); // stream bodies > 64KB
```

#### `collect_body()` — Simple usage (recommended)

Collects the entire body into a `Vec<u8>`. Works for both small bodies (already buffered) and streamed ones:

```rust
routes.add(":post/upload", |mut req: RequestPayload| async move {
    let body = req.collect_body().await;
    ResponsePayload::text(format!("Received {} bytes", body.len()))
});
```

#### `body_stream()` — Chunk-by-chunk processing

For processing data on demand without accumulating everything in memory:

```rust
routes.add(":post/stream", |mut req: RequestPayload| async move {
    let mut total = 0usize;

    if let Some(mut rx) = req.body_stream() {
        while let Some(chunk) = rx.recv().await {
            total += chunk.len();
        }
    }

    ResponsePayload::text(format!("Processed {} bytes in chunks", total))
});
```

## Response

### Available builders

```rust
// Plain text
ResponsePayload::text("Hello world")

// HTML
ResponsePayload::html("<h1>Welcome</h1>")

// JSON
ResponsePayload::json(&serde_json::json!({"status": "ok"})).unwrap()

// Bytes with custom content-type
ResponsePayload::bytes(vec![0x89, 0x50, 0x4E, 0x47], "image/png")

// Empty (204 No Content)
ResponsePayload::empty()

// Redirect
ResponsePayload::redirect("/new-location", StatusCode::Found)
```

### Chaining modifiers

```rust
ResponsePayload::text("data")
    .with_status(StatusCode::Created)
    .with_content_type("application/xml")
    .with_header("X-Custom", "value")
```

### Cookies

```rust
use rpress::CookieBuilder;

let cookie = CookieBuilder::new("token", "abc123")
    .path("/")
    .max_age(3600)
    .same_site("Strict")
    .http_only(true)
    .secure(true)
    .domain("example.com");

ResponsePayload::text("logged in")
    .set_cookie(&cookie)
```

Multiple `Set-Cookie` headers are supported — each `.set_cookie()` call adds a separate header.

## CORS

Native configuration via builder pattern:

```rust
let cors = RpressCors::new()
    .set_origins(vec!["https://app.example.com", "https://admin.example.com"])
    .set_methods(vec!["GET", "POST", "PUT", "DELETE"])
    .set_headers(vec!["Content-Type", "Authorization", "X-Custom-Header"])
    .set_expose_headers(vec!["X-Request-ID"])
    .set_max_age(3600)
    .set_credentials(true);

let mut app = Rpress::new(Some(cors));
```

Without CORS:

```rust
let mut app = Rpress::new(None);
```

Automatic headers: `Access-Control-Allow-Origin`, `Access-Control-Allow-Methods`, `Access-Control-Allow-Headers`, `Vary: Origin`. Preflight `OPTIONS` requests are handled automatically.

### CORS validation (fail-fast)

Rpress enforces RFC-compliant CORS at startup. Using wildcard origin `"*"` with `set_credentials(true)` will **panic** immediately, preventing the application from starting with an insecure configuration that browsers would silently reject:

```rust
// This will panic at startup:
let cors = RpressCors::new()
    .set_origins(vec!["*"])
    .set_credentials(true);
let app = Rpress::new(Some(cors)); // panics!

// Use explicit origins instead:
let cors = RpressCors::new()
    .set_origins(vec!["https://app.example.com"])
    .set_credentials(true);
let app = Rpress::new(Some(cors)); // ok
```

## Compression

Gzip and Brotli with automatic negotiation via `Accept-Encoding`:

```rust
app.enable_compression(true);
```

Behavior:
- Brotli is preferred when `Accept-Encoding: br` is present
- Gzip is used when `Accept-Encoding: gzip` is present
- Bodies smaller than 256 bytes are not compressed
- Already compressed types (image/*, video/*, audio/*, zip, gzip) are skipped
- SVG is compressed normally
- `Content-Encoding` and `Vary: Accept-Encoding` are added automatically

## Rate Limiting

Limit requests per IP with a sliding window counter:

```rust
app.set_rate_limit(100, 60); // 100 requests per 60 seconds
```

When the limit is exceeded, returns `429 Too Many Requests`.

By default, `set_rate_limit` uses an in-memory backend (`InMemoryRateLimiter`) suitable for single-instance deployments. Expired entries are automatically cleaned up when the store exceeds 10,000 records.

### Distributed rate limiting

For multi-instance environments (e.g. Kubernetes), inject a custom backend that implements the `RateLimiter` trait:

```rust
use rpress::RateLimiter;
use std::pin::Pin;

struct RedisRateLimiter { /* redis client */ }

impl RateLimiter for RedisRateLimiter {
    fn check(
        &self,
        key: &str,
        max_requests: u32,
        window_secs: u64,
    ) -> Pin<Box<dyn Future<Output = bool> + Send + '_>> {
        let key = key.to_string();
        Box::pin(async move {
            // Query Redis INCR + EXPIRE and return whether under limit
            true
        })
    }
}

let mut app = Rpress::new(None);
app.set_rate_limiter(RedisRateLimiter { /* ... */ });
app.set_rate_limit(100, 60);
```

The `set_rate_limiter` call must come **before** `set_rate_limit`, or after it to replace the default in-memory limiter. The framework does **not** ship a Redis implementation -- it only provides the trait and the in-memory default.

## Body Size Limits

By default, Rpress rejects request bodies larger than 10 MB with `413 Payload Too Large`.

### Global limit

```rust
app.set_max_body_size(5 * 1024 * 1024); // 5 MB for all routes
```

### Per route group limit

Individual route groups can override the global limit. This allows a file upload group to accept large bodies while keeping the rest of the API tightly restricted:

```rust
let mut api_routes = RpressRoutes::new();
api_routes.set_max_body_size(8 * 1024); // 8 KB for API routes
api_routes.add(":post/login", |req: RequestPayload| async move {
    ResponsePayload::text("ok")
});

let mut upload_routes = RpressRoutes::new();
upload_routes.set_max_body_size(50 * 1024 * 1024); // 50 MB for uploads
upload_routes.add(":post/upload", |mut req: RequestPayload| async move {
    let body = req.collect_body().await;
    ResponsePayload::text(format!("Received {} bytes", body.len()))
});

app.set_max_body_size(1024 * 1024); // 1 MB global default
app.add_route_group(api_routes);
app.add_route_group(upload_routes);
```

When a route group has its own limit, that limit takes precedence over the global one -- even if the group limit is larger. The global limit acts as the baseline for routes without a specific override.

## Static Files

```rust
app.serve_static("/assets", "./public");
app.serve_static("/uploads", "/var/data/uploads");
```

- Content-Type is detected by file extension
- Path traversal is prevented with `canonicalize()`
- Supports: HTML, CSS, JS, JSON, images (PNG, JPG, GIF, SVG, WebP, ICO), fonts (WOFF, WOFF2, TTF), PDF, XML, videos (MP4, WebM)

## TLS (HTTPS)

Rpress supports native TLS via `rustls`. Use `listen_tls` instead of `listen` to serve over HTTPS:

```rust
use rpress::{Rpress, RpressTlsConfig, RpressRoutes, RequestPayload, ResponsePayload};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut app = Rpress::new(None);

    let mut routes = RpressRoutes::new();
    routes.add(":get/hello", |_req: RequestPayload| async {
        ResponsePayload::text("Hello, HTTPS!")
    });
    app.add_route_group(routes);

    let tls = RpressTlsConfig::from_pem("cert.pem", "key.pem")?;
    app.listen_tls("0.0.0.0:443", tls).await
}
```

### `RpressTlsConfig`

| Method | Description |
|--------|-------------|
| `from_pem(cert_path, key_path)` | Loads a PEM certificate chain and private key from files |
| `from_config(rustls::ServerConfig)` | Uses an existing `rustls::ServerConfig` for full control |

Both methods automatically configure ALPN to support HTTP/2 (`h2`) and HTTP/1.1.

### Plaintext and TLS side by side

The `listen()` method continues to work for plaintext HTTP. You can use either one depending on your environment:

```rust
// Development — plaintext
app.listen("0.0.0.0:3000").await?;

// Production — TLS
let tls = RpressTlsConfig::from_pem("cert.pem", "key.pem")?;
app.listen_tls("0.0.0.0:443", tls).await?;
```

## HTTP/2

HTTP/2 is supported automatically over TLS connections. When a client negotiates the `h2` protocol via ALPN during the TLS handshake, Rpress routes the connection through its HTTP/2 handler.

- All routes, middleware, CORS, and response features work identically over HTTP/2
- No code changes required — the same `RpressRoutes` and handlers serve both protocols
- HTTP/2 multiplexing is fully supported (concurrent streams on a single connection)
- Plaintext connections (`listen()`) always use HTTP/1.1

```rust
// This handler serves both HTTP/1.1 and HTTP/2 clients transparently
routes.add(":get/api/data", |_req: RequestPayload| async {
    ResponsePayload::json(&serde_json::json!({"protocol": "auto"})).unwrap()
});
```

## Full Configuration

```rust
use std::time::Duration;
use rpress::{Rpress, RpressTlsConfig};

let mut app = Rpress::new(Some(cors));

// Read buffer capacity (default: 40KB)
app.set_buffer_capacity(1024 * 1024);

// Read timeout per request (default: 30s)
app.set_read_timeout(Duration::from_secs(30));

// Idle timeout between keep-alive requests (default: 60s)
app.set_idle_timeout(Duration::from_secs(120));

// Maximum concurrent connections (default: 1024)
app.set_max_connections(2048);

// Global max body size (default: 10MB)
app.set_max_body_size(5 * 1024 * 1024);

// Rate limiting (in-memory by default)
app.set_rate_limit(100, 60);
// Or inject a custom backend:
// app.set_rate_limiter(my_redis_limiter);

// Body streaming threshold (default: 64KB)
app.set_stream_threshold(64 * 1024);

// Gzip/brotli compression (default: disabled)
app.enable_compression(true);

// Static files
app.serve_static("/assets", "./public");

// Routes and middleware
app.use_middleware(|req, next| async move { next(req).await });
app.add_route_group(routes);

// Start the server (choose one)
app.listen("0.0.0.0:3000").await?;             // HTTP
// or
let tls = RpressTlsConfig::from_pem("cert.pem", "key.pem")?;
app.listen_tls("0.0.0.0:443", tls).await?;     // HTTPS + HTTP/2
```

## Controllers with the `handler!` macro

Organize handlers in structs with `Arc`:

```rust
use rpress::handler;

pub struct UserController;

impl UserController {
    pub fn new() -> Arc<Self> {
        Arc::new(Self)
    }

    async fn get_user(&self, req: RequestPayload) -> Result<ResponsePayload, RpressError> {
        let id = req.get_param("id").ok_or_else(|| RpressError {
            status: StatusCode::BadRequest,
            message: "Missing id".to_string(),
        })?;

        Ok(ResponsePayload::json(&serde_json::json!({
            "id": id,
            "name": "Guilherme"
        }))?)
    }

    async fn create_user(&self, mut req: RequestPayload) -> Result<ResponsePayload, RpressError> {
        let body = req.collect_body().await;
        let data: serde_json::Value = serde_json::from_slice(&body)?;

        Ok(ResponsePayload::json(&serde_json::json!({
            "created": true,
            "name": data["name"]
        }))?.with_status(StatusCode::Created))
    }
}

pub fn get_user_routes() -> RpressRoutes {
    let controller = UserController::new();
    let mut routes = RpressRoutes::new();

    routes.add(":get/users/:id", handler!(controller, get_user));
    routes.add(":post/users", handler!(controller, create_user));

    routes
}
```

## Custom Errors

Implement `RpressErrorExt` to return errors with custom status codes:

```rust
use rpress::{RpressErrorExt, StatusCode};

struct NotFoundError {
    resource: String,
}

impl RpressErrorExt for NotFoundError {
    fn into_rpress_error(self) -> (StatusCode, String) {
        (StatusCode::NotFound, format!("{} not found", self.resource))
    }
}

routes.add(":get/items/:id", |req: RequestPayload| async move {
    let id = req.get_param("id").unwrap_or("0");
    if id == "0" {
        return Err(NotFoundError { resource: "Item".into() });
    }
    Ok(ResponsePayload::text(format!("Item {}", id)))
});
```

Handlers can return:
- `ResponsePayload` (implicit 200)
- `Result<ResponsePayload, RpressError>`
- `Result<ResponsePayload, E>` where `E: RpressErrorExt`
- Any `E: RpressErrorExt` directly (error without Result)
- `()` (202 Accepted with no body)

## Security Headers

Automatically applied to all responses:

| Header | Value |
|--------|-------|
| `X-Content-Type-Options` | `nosniff` |
| `X-Request-ID` | Unique UUID v4 per request |
| `Server` | `Rpress/1.0` |
| `Connection` | `keep-alive` |

## Graceful Shutdown

The server responds to `SIGINT` (Ctrl+C):

1. Stops accepting new connections
2. Waits for active connections to finish
3. Shuts down cleanly

## Security Limits

| Resource | Limit |
|----------|-------|
| Request line | 8 KB |
| Headers (size) | 8 KB |
| Headers (count) | 100 |
| Body (Content-Length) | Configurable per route group (default 10 MB) |
| Individual chunk | 1 MB |
| Connection buffer | Configurable (default 40 KB) |

## License

MIT
