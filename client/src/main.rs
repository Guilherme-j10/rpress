use engine::Rpress;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut app = Rpress::new();

    app.route(":get/firstname", |req| async move {
        println!("{:?}", req);
    });

    app.route(":get/lastname", |req| async move {
        println!("{:?}", req);
    });

    app.server("0.0.0.0:3434").await?;

    Ok(())
}
