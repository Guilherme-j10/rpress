// security.rs — Demonstrates granular body size limits per route group.
//
// Problem: a single global body limit forces a trade-off between security
// and functionality. A login route should refuse anything above ~8 KB
// (prevents DoS via huge payloads), while an API endpoint might legitimately
// receive larger JSON documents.
//
// Solution: call set_max_body_size() on a RpressRoutes group. That limit
// overrides the global one for every route in the group — even if the group
// limit is *larger* than the global.

use rpress::{
    core::handler_response::{ResponsePayload, RpressError},
    core::routes::RpressRoutes,
    types::definitions::{RequestPayload, StatusCode},
};
use serde_json::json;

pub fn get_security_routes() -> RpressRoutes {
    let mut routes = RpressRoutes::new();

    // Strict 8 KB limit for authentication endpoints.
    // Any request body larger than 8 192 bytes is rejected with 413
    // before the handler is ever called.
    routes.set_max_body_size(8 * 1024); // 8 KB

    // POST /security/login
    // Simulates credential validation. The tiny body limit mitigates DoS
    // attacks that attempt to exhaust memory by sending enormous payloads.
    routes.add(
        ":post/security/login",
        |mut req: RequestPayload| async move {
            let body = req.collect_body().await;

            let data: serde_json::Value =
                serde_json::from_slice(&body).map_err(|_| RpressError {
                    status: StatusCode::BadRequest,
                    message: "Expected JSON body with 'email' and 'password'".to_string(),
                })?;

            let email = data
                .get("email")
                .and_then(|v| v.as_str())
                .ok_or_else(|| RpressError {
                    status: StatusCode::UnprocessableEntity,
                    message: "'email' is required".to_string(),
                })?;

            let _password = data
                .get("password")
                .and_then(|v| v.as_str())
                .ok_or_else(|| RpressError {
                    status: StatusCode::UnprocessableEntity,
                    message: "'password' is required".to_string(),
                })?;

            // In a real app: verify credentials, issue JWT, etc.
            let res = ResponsePayload::json(&json!({
                "token": "eyJhbGciOiJIUzI1NiJ9.example",
                "email": email,
                "expires_in": 3600
            }))
            .map_err(|e| RpressError {
                status: StatusCode::InternalServerError,
                message: e.to_string(),
            })?
            .with_status(StatusCode::OK);
            Ok::<ResponsePayload, RpressError>(res)
        },
    );

    // POST /security/verify-token
    // A second endpoint in the same group shares the same 8 KB limit.
    routes.add(
        ":post/security/verify-token",
        |mut req: RequestPayload| async move {
            let body = req.collect_body().await;

            let data: serde_json::Value =
                serde_json::from_slice(&body).map_err(|_| RpressError {
                    status: StatusCode::BadRequest,
                    message: "Expected JSON body with 'token'".to_string(),
                })?;

            let token = data
                .get("token")
                .and_then(|v| v.as_str())
                .ok_or_else(|| RpressError {
                    status: StatusCode::UnprocessableEntity,
                    message: "'token' is required".to_string(),
                })?;

            ResponsePayload::json(&json!({
                "valid": !token.is_empty(),
                "subject": "user@example.com",
            }))
            .map_err(|e| RpressError {
                status: StatusCode::InternalServerError,
                message: e.to_string(),
            })
        },
    );

    // GET /security/limits
    // Returns the active limits for this group so clients can understand
    // what is enforced. Useful during development.
    routes.add(
        ":get/security/limits",
        |_req: RequestPayload| async move {
            ResponsePayload::json(&json!({
                "group": "security",
                "max_body_bytes": 8 * 1024,
                "max_body_human": "8 KB",
                "note": "Bodies exceeding this limit are rejected with 413 Payload Too Large before the handler is called."
            }))
            .unwrap()
        },
    );

    routes
}
