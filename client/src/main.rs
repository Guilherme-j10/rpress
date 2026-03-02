use engine::Rpress;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut app = Rpress::build();

    app.add_route("/firstname", |req| async {});
    app.add_route("/lastname", |req| async {});

    app.server("0.0.0.0:3434").await?;

    Ok(())
}
