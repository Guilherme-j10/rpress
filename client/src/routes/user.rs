use std::sync::Arc;

use engine::{
    core::{
        handler_response::{ResponsePayload, RpressError, RpressErrorExt},
        routes::RpressRoutes,
    },
    handler,
    types::definitions::{RequestPayload, StatusCode},
};
use serde::Serialize;
use serde_json::json;

pub struct User;

impl User {
    pub fn new() -> Arc<Self> {
        Arc::new(Self)
    }

    async fn handler_test(&self, req: RequestPayload) -> Result<ResponsePayload, RpressError> {
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
    }

    async fn handler_external(&self, _: RequestPayload) -> Result<ResponsePayload, MyCustomError> {
        Err(MyCustomError {
            message: "teste".to_string(),
        })
    }

    async fn handler_external_no_result(&self, _: RequestPayload) -> MyCustomError {
        MyCustomError {
            message: "teste".to_string(),
        }
    }

    async fn custom_success(&self, _: RequestPayload) -> ResponsePayload {
        ResponsePayload::json(&Success {
            message: "Hello world".to_string(),
        })
    }
}

pub fn get_user_routes() -> RpressRoutes {
    let user_controller = User::new();
    let mut routes = RpressRoutes::new();

    routes.add(
        ":get/get_name/:user",
        handler!(user_controller, handler_test),
    );

    routes.add(":get/lastname", |req| async move {
        if let Some(value) = req.get_query("client") {
            let val = json!({ "lastname": value });
            return Ok(ResponsePayload::json(&val));
        }

        Err(MyCustomError {
            message: "client not provided".to_string(),
        })
    });

    routes.add(
        ":get/custom_erro",
        handler!(user_controller, handler_external),
    );
    routes.add(
        ":get/custom_erro_without_result",
        handler!(user_controller, handler_external_no_result),
    );
    routes.add(
        ":get/custom_success",
        handler!(user_controller, custom_success),
    );

    routes
}

struct MyCustomError {
    message: String,
}

#[derive(Serialize)]
struct Success {
    message: String,
}

impl RpressErrorExt for MyCustomError {
    fn into_rpress_error(self) -> (StatusCode, String) {
        (StatusCode::InternalServerError, self.message)
    }
}