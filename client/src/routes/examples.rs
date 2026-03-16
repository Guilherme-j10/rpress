use rpress::{
    core::handler_response::{CookieBuilder, ResponsePayload, RpressError, RpressErrorExt},
    core::routes::RpressRoutes,
    types::definitions::{RequestPayload, StatusCode},
};
use serde::Serialize;
use serde_json::json;

struct AppError {
    status: StatusCode,
    message: String,
}

impl RpressErrorExt for AppError {
    fn into_rpress_error(self) -> (StatusCode, String) {
        (self.status, self.message)
    }
}

#[derive(Serialize)]
struct ApiResponse<T: Serialize> {
    success: bool,
    data: T,
}

pub fn get_example_routes() -> RpressRoutes {
    let mut routes = RpressRoutes::new();

    routes.add(":get/examples/html", |_req: RequestPayload| async move {
        ResponsePayload::html(
            r#"<!DOCTYPE html>
<html>
<head><title>Rpress</title></head>
<body>
    <h1>Hello from Rpress</h1>
    <p>This is an HTML response.</p>
</body>
</html>"#,
        )
    });

    routes.add(
        ":get/examples/json",
        |_req: RequestPayload| async move {
            ResponsePayload::json(&ApiResponse {
                success: true,
                data: json!({
                    "framework": "Rpress",
                    "version": "1.0",
                    "features": ["streaming", "compression", "cors"]
                }),
            })
            .unwrap()
        },
    );

    routes.add(":get/examples/bytes", |_req: RequestPayload| async move {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100">
            <circle cx="50" cy="50" r="40" fill="blue"/>
        </svg>"#;
        ResponsePayload::bytes(svg.to_vec(), "image/svg+xml")
    });

    routes.add(
        ":get/examples/redirect",
        |_req: RequestPayload| async move {
            ResponsePayload::redirect("/examples/html", StatusCode::Found)
        },
    );

    routes.add(
        ":get/examples/redirect/permanent",
        |_req: RequestPayload| async move {
            ResponsePayload::redirect("/examples/html", StatusCode::MovedPermanently)
        },
    );

    routes.add(
        ":get/examples/cookies/set",
        |_req: RequestPayload| async move {
            let session = CookieBuilder::new("session_id", "abc123xyz")
                .path("/")
                .max_age(3600)
                .http_only(true)
                .secure(true)
                .same_site("Strict");

            let preference = CookieBuilder::new("theme", "dark")
                .path("/")
                .max_age(86400 * 30)
                .http_only(false);

            ResponsePayload::json(&json!({"cookies": "set"}))
                .unwrap()
                .set_cookie(&session)
                .set_cookie(&preference)
        },
    );

    routes.add(
        ":get/examples/cookies/read",
        |req: RequestPayload| async move {
            let cookies = req.cookies();
            ResponsePayload::json(&json!({
                "cookies_found": cookies.len(),
                "session_id": cookies.get("session_id"),
                "theme": cookies.get("theme"),
            }))
            .unwrap()
        },
    );

    routes.add(
        ":get/examples/error/not_found",
        |_req: RequestPayload| async move {
            Err::<ResponsePayload, _>(AppError {
                status: StatusCode::NotFound,
                message: json!({"error": "Resource not found", "code": "NOT_FOUND"}).to_string(),
            })
        },
    );

    routes.add(
        ":get/examples/error/validation",
        |_req: RequestPayload| async move {
            Err::<ResponsePayload, _>(RpressError {
                status: StatusCode::UnprocessableEntity,
                message: json!({
                    "error": "Validation failed",
                    "fields": {
                        "email": "must be a valid email",
                        "age": "must be >= 18"
                    }
                })
                .to_string(),
            })
        },
    );

    routes.add(
        ":get/examples/query",
        |req: RequestPayload| async move {
            let search = req.get_query("q").unwrap_or("");
            let page: u32 = req
                .get_query("page")
                .and_then(|p| p.parse().ok())
                .unwrap_or(1);
            let per_page: u32 = req
                .get_query("per_page")
                .and_then(|p| p.parse().ok())
                .unwrap_or(10);

            ResponsePayload::json(&json!({
                "search": search,
                "page": page,
                "per_page": per_page,
                "results": []
            }))
            .unwrap()
        },
    );

    routes.add(
        ":get/examples/headers",
        |req: RequestPayload| async move {
            let user_agent = req.header("user-agent").unwrap_or("unknown").to_string();
            let accept = req.header("accept").unwrap_or("*/*").to_string();

            ResponsePayload::json(&json!({
                "your_user_agent": user_agent,
                "your_accept": accept,
            }))
            .unwrap()
            .with_header("X-Powered-By", "Rpress")
            .with_header("X-Custom-Header", "example-value")
            .with_header("Cache-Control", "no-cache, no-store, must-revalidate")
        },
    );

    routes.add(
        ":get/examples/status/:code",
        |req: RequestPayload| async move {
            let code: u16 = req
                .get_param("code")
                .and_then(|c| c.parse().ok())
                .unwrap_or(200);

            let status = match code {
                200 => StatusCode::OK,
                201 => StatusCode::Created,
                204 => StatusCode::NoContent,
                400 => StatusCode::BadRequest,
                401 => StatusCode::Unauthorized,
                403 => StatusCode::Forbidden,
                404 => StatusCode::NotFound,
                422 => StatusCode::UnprocessableEntity,
                429 => StatusCode::TooManyRequests,
                500 => StatusCode::InternalServerError,
                _ => StatusCode::OK,
            };

            ResponsePayload::json(&json!({"status_code": code}))
                .unwrap()
                .with_status(status)
        },
    );

    routes.add(
        ":post/examples/form",
        |mut req: RequestPayload| async move {
            let body = req.collect_body().await;
            let text = String::from_utf8_lossy(&body);

            let mut form_data = std::collections::HashMap::new();
            for pair in text.split('&') {
                if let Some((key, value)) = pair.split_once('=') {
                    form_data.insert(key.to_string(), value.to_string());
                }
            }

            ResponsePayload::json(&json!({
                "parsed_form": form_data,
                "raw_length": body.len()
            }))
            .unwrap()
        },
    );

    routes.add(
        ":get/examples/content-type",
        |_req: RequestPayload| async move {
            ResponsePayload::text("<data>xml content</data>")
                .with_content_type("application/xml; charset=utf-8")
        },
    );

    routes
}
