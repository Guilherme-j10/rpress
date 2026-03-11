use engine::types::definitions::StatusCode;
use engine::{
    Rpress,
    core::handler_response::{ResponsePayload, RpressError},
};
use serde_json::json;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut app = Rpress::new();

    app.route(":get/get_name/:user", |req| async move {
        let user = req.get_param("user").unwrap();

        if user == "1" {
            return Ok(ResponsePayload::json(&json!({
                "name": "Guilherme"
            })));
        }

        Err(RpressError {
            status: StatusCode::InternalServerError,
            message: json!({
                "error": "firsrtname not found"
            })
            .to_string(),
        })
    });

    app.route(":get/lastname", |_| async move {
        let val = json!({ "lastname": "Campos" });
        Ok(ResponsePayload::json(&val))
    });

    app.server("0.0.0.0:3434").await?;

    Ok(())
}
