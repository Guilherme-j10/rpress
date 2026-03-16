use engine::Rpress;

use crate::routes::user::get_user_routes;

mod routes;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut app = Rpress::new();
    let user_routes = get_user_routes();

    app.add_route_group(user_routes);

    app.server("0.0.0.0:3434").await?;

    Ok(())
}