use engine::core::handler_response::RpressErrorExt;
use engine::types::definitions::{RequestPayload, StatusCode};
use engine::{
    Rpress,
    core::handler_response::{ResponsePayload, RpressError},
};
use serde::Serialize;
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
        ResponsePayload::json(&val)
    });

    app.route(":get/custom_erro", handler_external);
    app.route(":get/custom_success", custom_success);

    app.server("0.0.0.0:3434").await?;

    Ok(())
}

struct MyCustomError {
    message: String,
}

#[derive(Serialize)]
struct Success {
    message: String
}

impl RpressErrorExt for MyCustomError {
    fn into_rpress_error(self) -> (StatusCode, String) {
        (StatusCode::InternalServerError, self.message)
    }
}

async fn handler_external(_: RequestPayload) -> Result<ResponsePayload, MyCustomError> {
    Err(MyCustomError {
        message: "teste".to_string(),
    })
}

async fn custom_success(_: RequestPayload) -> ResponsePayload {
    ResponsePayload::json(&Success {
        message: "Hello world".to_string()
    })
} 
