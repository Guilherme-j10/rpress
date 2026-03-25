# Rpress

An async HTTP/1.1 and HTTP/2 framework in Rust, built on top of `tokio`. Designed to be lightweight, secure, and production-ready.

## Features

- Trie-based routing (static, dynamic, multi-method)
- Middleware (global and per route group)
- **Native TLS via rustls** (HTTPS with PEM certificates)
- **HTTP/2 via h2** (automatic ALPN negotiation over TLS)
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
- Automatic security headers (`X-Content-Type-Options: nosniff`) with configurable CSP, X-Frame-Options, and more
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

## Observability (Distributed Tracing)

Rpress automatically creates structured [tracing](https://docs.rs/tracing) spans for every request. This makes the framework compatible with distributed tracing backends like **Jaeger**, **Datadog**, **Grafana Tempo**, and **Zipkin** out of the box.

### Automatic spans

Every incoming request is wrapped in an `http.request` span with these fields:

| Field | Description |
|-------|-------------|
| `http.method` | HTTP method (GET, POST, etc.) |
| `http.route` | Request URI path |
| `http.request_id` | Unique UUID v4 (same as X-Request-ID header) |
| `http.status_code` | Response status code (recorded after handler completes) |
| `http.latency_ms` | Total processing time in milliseconds |

Each connection also gets a parent span:

| Span | Fields | Description |
|------|--------|-------------|
| `http.connection` | `peer.addr` | Per-connection span (HTTP/1.1 and TLS) |
| `h2.stream` | — | Per-stream span for HTTP/2 multiplexed streams |

The hierarchy looks like this:

```
http.connection (peer.addr=192.168.1.10)
  └── http.request (method=GET, route=/users/1, request_id=abc-123, status_code=200, latency_ms=3)
        └── app.request (your middleware span)
              └── tracing::info!("...")   ← inherits full context
```

Any `tracing::info!`, `tracing::warn!`, or `tracing::error!` emitted inside a middleware or handler automatically inherits the parent span context — no manual propagation needed.

### Adding custom fields in middleware

The framework span already exists when your middleware runs. Create a child span to add application-specific fields:

```rust
app.use_middleware(|req, next| async move {
    let uri = req.uri().to_string();
    let method = req.method().to_string();

    let span = tracing::info_span!(
        "app.request",
        app.route = %uri,
        app.method = %method,
        app.user_id = tracing::field::Empty,
    );
    let _guard = span.enter();

    tracing::info!("processing request");
    let result = next(req).await;

    // After authentication, record the user:
    // tracing::Span::current().record("app.user_id", &"user-123");

    result
});
```

### Exporting to Jaeger / Datadog / Tempo

Rpress uses the standard `tracing` crate. To export spans to a distributed tracing backend, configure `tracing-subscriber` with an OpenTelemetry layer in your `main()`:

```rust
// Cargo.toml:
// tracing-subscriber = { version = "0.3", features = ["env-filter"] }
// opentelemetry = "0.27"
// opentelemetry-otlp = "0.27"
// tracing-opentelemetry = "0.28"

use opentelemetry::trace::TracerProvider;
use opentelemetry_otlp::WithExportConfig;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

fn init_tracing() {
    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint("http://localhost:4317")
        .build()
        .unwrap();

    let provider = opentelemetry::sdk::trace::TracerProvider::builder()
        .with_batch_exporter(exporter)
        .build();

    let tracer = provider.tracer("rpress-app");
    let telemetry = tracing_opentelemetry::layer().with_tracer(tracer);

    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(telemetry)
        .init();
}
```

With this setup, every `http.request` span (and its children) is automatically exported as a trace to your backend. The `http.request_id` field matches the `X-Request-ID` response header, making it easy to correlate logs with traces.

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
- **Compression runs inside `tokio::task::spawn_blocking`** — CPU-bound work (Brotli/Gzip encoding) never blocks the async event loop, even under high concurrency

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
- Path traversal is prevented with `canonicalize()` — both the base directory and the requested path are resolved and compared before any read is performed
- File reads use `tokio::fs::read` and path resolution uses `tokio::fs::canonicalize` — **no blocking syscalls on the event loop**
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

## State Management

Shared state — database pools, config, caches, service clients — is passed into route groups as function parameters and stored inside controllers wrapped in `Arc`.

### The pattern

```
main()
  └── Arc::new(MyPool::new())   — created once
        ├── .clone() → get_user_routes(db)
        │       └── UserController { db }
        │             └── self.db.query(…).await
        └── .clone() → get_order_routes(db)
                └── OrderController { db }
```

### Example — database pool

```rust
// db.rs — your database pool (e.g. sqlx::PgPool or a mock)
pub struct DbPool { /* connection pool */ }

impl DbPool {
    pub async fn find_user(&self, id: u32) -> Option<User> { /* … */ }
    pub async fn create_user(&self, name: String, email: String) -> User { /* … */ }
}
```

```rust
// routes/user.rs
use std::sync::Arc;
use rpress::{handler, RpressRoutes, RequestPayload, ResponsePayload, RpressError, StatusCode};
use crate::db::DbPool;

pub struct UserController {
    db: Arc<DbPool>,   // shared, cloning Arc is O(1)
}

impl UserController {
    pub fn new(db: Arc<DbPool>) -> Arc<Self> {
        Arc::new(Self { db })
    }

    async fn get_user(&self, req: RequestPayload) -> Result<ResponsePayload, RpressError> {
        let id: u32 = req.get_param("id")
            .and_then(|v| v.parse().ok())
            .ok_or(RpressError { status: StatusCode::BadRequest, message: "bad id".into() })?;

        let user = self.db.find_user(id).await
            .ok_or(RpressError { status: StatusCode::NotFound, message: "not found".into() })?;

        Ok(ResponsePayload::json(&user)?)
    }

    async fn create_user(&self, mut req: RequestPayload) -> Result<ResponsePayload, RpressError> {
        let body = req.collect_body().await;
        let data: serde_json::Value = serde_json::from_slice(&body)?;

        let user = self.db.create_user(
            data["name"].as_str().unwrap_or("").to_string(),
            data["email"].as_str().unwrap_or("").to_string(),
        ).await;

        Ok(ResponsePayload::json(&user)?.with_status(StatusCode::Created))
    }
}

// The pool is injected here — route groups are plain functions.
pub fn get_user_routes(db: Arc<DbPool>) -> RpressRoutes {
    let controller = UserController::new(db);
    let mut routes = RpressRoutes::new();

    routes.add(":get/users/:id", handler!(controller, get_user));
    routes.add(":post/users",    handler!(controller, create_user));

    routes
}
```

```rust
// main.rs — create the pool once, share it via Arc::clone
use std::sync::Arc;
use rpress::Rpress;
use crate::db::DbPool;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // With sqlx: let db = Arc::new(PgPool::connect(&database_url).await?);
    let db = Arc::new(DbPool::new());

    let mut app = Rpress::new(None);

    // Each route group gets a cheap Arc clone — no data is copied.
    app.add_route_group(get_user_routes(db.clone()));
    app.add_route_group(get_order_routes(db.clone()));

    app.listen("0.0.0.0:3000").await?;
    Ok(())
}
```

### Multiple state types

Pass additional state the same way — just add more parameters:

```rust
pub fn get_auth_routes(
    db:    Arc<DbPool>,
    cache: Arc<RedisClient>,
    cfg:   Arc<AppConfig>,
) -> RpressRoutes {
    let controller = AuthController::new(db, cache, cfg);
    // …
}
```

```rust
// main.rs
let db    = Arc::new(DbPool::new());
let cache = Arc::new(RedisClient::connect("redis://localhost")?);
let cfg   = Arc::new(AppConfig::from_env());

app.add_route_group(get_auth_routes(db.clone(), cache.clone(), cfg.clone()));
```

Any type that is `Send + Sync + 'static` can be wrapped in `Arc` and shared this way, including `tokio::sync::RwLock` and `tokio::sync::Mutex` for mutable shared state.

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

### Always Applied

These headers are sent automatically on every response:

| Header | Value |
|--------|-------|
| `X-Content-Type-Options` | `nosniff` |
| `X-Request-ID` | Unique UUID v4 per request |
| `Server` | `Rpress/1.0` |
| `Connection` | `keep-alive` |

### Configurable Security Headers

Use `RpressSecurityHeaders` to opt-in to additional security headers such as
`Content-Security-Policy`, `X-Frame-Options`, `X-XSS-Protection`, and any custom
header. These are injected into every response **unless** the handler already set
the same header via `with_header()`.

```rust
use rpress::{Rpress, RpressSecurityHeaders};

let mut app = Rpress::new(None);
app.set_security_headers(
    RpressSecurityHeaders::new()
        .content_security_policy("default-src 'self'; script-src 'self'")
        .x_frame_options("DENY")
        .x_xss_protection("1; mode=block")
        .custom("Permissions-Policy", "camera=(), microphone=()")
        .custom("Referrer-Policy", "strict-origin-when-cross-origin"),
);
```

If a handler needs a different policy for a specific route, it can override by
setting the header directly:

```rust
ResponsePayload::html(page)
    .with_header("Content-Security-Policy", "default-src 'self'; script-src 'self' 'unsafe-inline'")
```

The handler-set value takes priority and the global default is skipped for that header.

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

## Socket.IO (Real-time Communication)

Rpress includes a built-in Socket.IO server compatible with `socket.io-client` v4+
(Engine.IO v4, Socket.IO protocol v5). It supports HTTP long-polling and WebSocket
transports, namespaces, rooms, event-based messaging, acknowledgements, and broadcasting.

### Basic Setup

```rust
use rpress::{Rpress, RpressIo};
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let io = RpressIo::new();

    io.on_connection(|socket| async move {
        println!("Connected: {}", socket.id());

        socket.on("message", |socket, data| async move {
            // Broadcast to all other sockets
            socket.broadcast().emit("message", &data[0]).await;
            None
        }).await;

        socket.on_disconnect(|socket| async move {
            println!("Disconnected: {}", socket.id());
        }).await;
    });

    let mut app = Rpress::new(None);
    app.attach_socketio(io);
    app.listen("0.0.0.0:3000").await
}
```

### Namespaces

```rust
let io = RpressIo::new();

// Default namespace "/"
io.on_connection(|socket| async move { /* ... */ });

// Custom namespace "/admin"
io.of("/admin").on_connection(|socket| async move {
    println!("Admin connected: {}", socket.id());
});
```

### Rooms

```rust
socket.on("join_room", |socket, data| async move {
    if let Some(room) = data.first().and_then(|v| v.as_str()) {
        socket.join(room).await;
        socket.to(room).emit("user_joined", &socket.id()).await;
    }
    None
}).await;
```

### Acknowledgements

```rust
socket.on("greet", |_socket, data| async move {
    let name = data.first().and_then(|v| v.as_str()).unwrap_or("world");
    Some(serde_json::json!(format!("Hello, {}!", name)))
}).await;
```

On the client side (JavaScript):

```javascript
socket.emit("greet", "Rpress", (response) => {
    console.log(response); // "Hello, Rpress!"
});
```

### Client Connection (JavaScript)

```javascript
import { io } from "socket.io-client";

const socket = io("http://localhost:3000");

socket.on("connect", () => {
    console.log("Connected:", socket.id);
});

socket.emit("message", "Hello from client");

socket.on("message", (data) => {
    console.log("Received:", data);
});
```

## License

MIT
