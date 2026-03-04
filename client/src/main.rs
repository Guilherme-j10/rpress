use engine::Rpress;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut app = Rpress::build();

    app.add_route("/firstname", |req| async move {
        println!("{:?}", req);
    });

    app.add_route("/lastname", |req| async move {
        dbg!(req);
    });

    app.server("0.0.0.0:3434").await?;

    Ok(())
}