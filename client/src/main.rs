use std::time::Duration;

use rpress::{Rpress, RpressCors};

use crate::routes::examples::get_example_routes;
use crate::routes::security::get_security_routes;
use crate::routes::upload::get_upload_routes;
use crate::routes::user::get_user_routes;

mod routes;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    // CORS: always use explicit origins when credentials are enabled.
    // Using set_origins(vec!["*"]) + set_credentials(true) would panic at startup —
    // Rpress enforces RFC compliance to prevent misconfiguration.
    let cors = RpressCors::new()
        .set_origins(vec!["http://localhost:5173", "https://app.example.com"])
        .set_methods(vec!["GET", "POST", "PUT", "DELETE"])
        .set_headers(vec!["Content-Type", "Authorization"])
        .set_expose_headers(vec!["X-Request-ID"])
        .set_credentials(true)
        .set_max_age(3600);

    let mut app = Rpress::new(Some(cors));

    app.set_buffer_capacity(1024 * 1024);
    app.set_read_timeout(Duration::from_secs(30));
    app.set_idle_timeout(Duration::from_secs(120));
    app.set_max_connections(2048);

    // Global body size limit (default: 10 MB). Route groups can override this.
    // See get_security_routes() for a 8 KB login limit and
    // get_upload_routes() for a 20 MB upload limit.
    app.set_max_body_size(1 * 1024 * 1024); // 1 MB global default

    // Rate limiting: the default set_rate_limit() uses InMemoryRateLimiter.
    // For distributed environments (Kubernetes), inject a custom backend via
    // set_rate_limiter() before calling set_rate_limit(). See the comments in
    // get_security_routes() for an example of the RateLimiter trait.
    app.set_rate_limit(100, 60);

    // Alternatively, inject a custom limiter first (uncomment to try):
    // app.set_rate_limiter(MyRedisRateLimiter::new("redis://localhost:6379"));
    // app.set_rate_limit(100, 60);

    app.set_stream_threshold(64 * 1024);
    app.enable_compression(true);
    app.serve_static("/assets", "./public");

    app.use_middleware(|req, next| async move {
        let uri = req.uri().to_string();
        let method = req.method().to_string();

        tracing::info!("--> {} {}", method, uri);
        let start = std::time::Instant::now();

        let result = next(req).await;

        tracing::info!("<-- {} {} ({:?})", method, uri, start.elapsed());
        result
    });

    app.add_route_group(get_user_routes());
    app.add_route_group(get_upload_routes());
    app.add_route_group(get_example_routes());
    app.add_route_group(get_security_routes());

    // Start without TLS (HTTP/1.1 only). To enable HTTPS + HTTP/2:
    //
    //   use rpress::RpressTlsConfig;
    //   let tls = RpressTlsConfig::from_pem("cert.pem", "key.pem")?;
    //   app.listen_tls("0.0.0.0:443", tls).await?;
    //
    // HTTP/2 is negotiated automatically via ALPN — no handler changes needed.
    app.listen("0.0.0.0:3434").await?;

    Ok(())
}

// Example of a custom rate limiter for distributed deployments.
// Implement the RateLimiter trait backed by Redis, a database, or any
// shared store accessible from all your service replicas.
//
// use rpress::RateLimiter;
// use std::pin::Pin;
//
// struct MyRedisRateLimiter { url: String }
//
// impl MyRedisRateLimiter {
//     fn new(url: &str) -> Self { Self { url: url.to_string() } }
// }
//
// impl RateLimiter for MyRedisRateLimiter {
//     fn check(
//         &self,
//         key: &str,
//         max_requests: u32,
//         window_secs: u64,
//     ) -> Pin<Box<dyn Future<Output = bool> + Send + '_>> {
//         let key = key.to_string();
//         Box::pin(async move {
//             // call INCR key, set EXPIRE, return count <= max_requests
//             true
//         })
//     }
// }
