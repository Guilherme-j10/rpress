// user.rs — Demonstrates passing shared state (a database pool) into
// route groups and controllers.
//
// Pattern:
//   1. Wrap your state in Arc so it can be shared across async handlers.
//   2. Pass the Arc<State> as a parameter to the route group factory.
//   3. Forward it to the controller constructor so every method can access it.
//
// Replace Arc<DbPool> with any shared state: config, cache, mailer, etc.

use std::sync::Arc;

use rpress::{
    core::{
        handler_response::{ResponsePayload, RpressError},
        routes::RpressRoutes,
    },
    handler,
    types::definitions::{RequestPayload, StatusCode},
};
use serde_json::json;

use crate::db::DbPool;

pub struct UserController {
    // Shared database pool — cloning an Arc is cheap (just a reference count bump).
    db: Arc<DbPool>,
}

impl UserController {
    pub fn new(db: Arc<DbPool>) -> Arc<Self> {
        Arc::new(Self { db })
    }

    // GET /users — list all users
    async fn list_users(&self, _req: RequestPayload) -> Result<ResponsePayload, RpressError> {
        let users = self.db.list_users().await;
        Ok(ResponsePayload::json(&json!({
            "users": users,
            "total": users.len()
        }))?)
    }

    // GET /users/:id — fetch a single user
    async fn get_user(&self, req: RequestPayload) -> Result<ResponsePayload, RpressError> {
        let id: u32 = req
            .get_param("id")
            .and_then(|v| v.parse().ok())
            .ok_or(RpressError {
                status: StatusCode::BadRequest,
                message: "id must be a positive integer".to_string(),
            })?;

        let user = self.db.find_user(id).await.ok_or(RpressError {
            status: StatusCode::NotFound,
            message: format!("user {} not found", id),
        })?;

        Ok(ResponsePayload::json(&user)?)
    }

    // POST /users — create a user
    async fn create_user(
        &self,
        mut req: RequestPayload,
    ) -> Result<ResponsePayload, RpressError> {
        let body = req.collect_body().await;

        let data: serde_json::Value =
            serde_json::from_slice(&body).map_err(|_| RpressError {
                status: StatusCode::BadRequest,
                message: "body must be valid JSON with 'name' and 'email'".to_string(),
            })?;

        let name = data
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or(RpressError {
                status: StatusCode::UnprocessableEntity,
                message: "'name' is required".to_string(),
            })?
            .to_string();

        let email = data
            .get("email")
            .and_then(|v| v.as_str())
            .ok_or(RpressError {
                status: StatusCode::UnprocessableEntity,
                message: "'email' is required".to_string(),
            })?
            .to_string();

        let user = self.db.create_user(name, email).await;

        Ok(ResponsePayload::json(&user)?.with_status(StatusCode::Created))
    }

    // DELETE /users/:id — remove a user
    async fn delete_user(&self, req: RequestPayload) -> Result<ResponsePayload, RpressError> {
        let id: u32 = req
            .get_param("id")
            .and_then(|v| v.parse().ok())
            .ok_or(RpressError {
                status: StatusCode::BadRequest,
                message: "id must be a positive integer".to_string(),
            })?;

        let deleted = self.db.delete_user(id).await;

        if deleted {
            Ok(ResponsePayload::json(&json!({ "deleted": true, "id": id }))?)
        } else {
            Err(RpressError {
                status: StatusCode::NotFound,
                message: format!("user {} not found", id),
            })
        }
    }
}

/// Builds the user route group.
///
/// `db` is an `Arc<DbPool>` created once in `main` and shared across all
/// controllers that need database access. Cloning the `Arc` is O(1).
pub fn get_user_routes(db: Arc<DbPool>) -> RpressRoutes {
    let controller = UserController::new(db);
    let mut routes = RpressRoutes::new();

    routes.use_middleware(|req, next| async move {
        tracing::info!("[USERS] {} {}", req.method(), req.uri());
        next(req).await
    });

    routes.add(":get/users", handler!(controller, list_users));
    routes.add(":get/users/:id", handler!(controller, get_user));
    routes.add(":post/users", handler!(controller, create_user));
    routes.add(":delete/users/:id", handler!(controller, delete_user));

    routes
}
