use std::time::Duration;

use engine::{Rpress, RpressCors};

use crate::routes::examples::get_example_routes;
use crate::routes::upload::get_upload_routes;
use crate::routes::user::get_user_routes;

mod routes;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let cors = RpressCors::new()
        .set_origins(vec!["*"])
        .set_methods(vec!["GET", "POST", "PUT", "DELETE"])
        .set_headers(vec!["Content-Type", "Authorization"]);

    let mut app = Rpress::new(Some(cors));

    app.set_buffer_capacity(1024 * 1024);
    app.set_read_timeout(Duration::from_secs(30));
    app.set_idle_timeout(Duration::from_secs(120));
    app.set_max_connections(2048);
    app.set_rate_limit(100, 60);
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

    app.listen("0.0.0.0:3434").await?;

    Ok(())
}
