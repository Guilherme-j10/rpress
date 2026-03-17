// tracing_demo.rs — Demonstrates how Rpress's automatic request spans
// integrate with application-level tracing.
//
// The framework creates an "http.request" span for every request with:
//   http.method, http.route, http.request_id, http.status_code, http.latency_ms
//
// These examples show how to:
//   1. Create child spans with custom application fields
//   2. Record deferred fields after computation
//   3. Instrument async work (DB queries, external calls)
//   4. Propagate trace context through nested function calls

use std::sync::Arc;

use rpress::{
    core::handler_response::{ResponsePayload, RpressError},
    core::routes::RpressRoutes,
    handler,
    types::definitions::{RequestPayload, StatusCode},
};
use serde_json::json;

use crate::db::DbPool;

pub struct TracingController {
    db: Arc<DbPool>,
}

impl TracingController {
    pub fn new(db: Arc<DbPool>) -> Arc<Self> {
        Arc::new(Self { db })
    }

    // GET /tracing/basic
    // The framework's "http.request" span already wraps this handler.
    // All tracing::info! calls automatically inherit the request context.
    async fn basic(&self, _req: RequestPayload) -> ResponsePayload {
        tracing::info!("this log line inherits http.method, http.route, http.request_id");
        tracing::info!(custom_field = "hello", "structured fields work too");
        ResponsePayload::json(&json!({
            "message": "Check your logs — these events include the request span fields."
        }))
        .unwrap()
    }

    // GET /tracing/child-span
    // Creates a child span to isolate a logical unit of work.
    // Useful for profiling specific operations within a request.
    async fn child_span(&self, _req: RequestPayload) -> ResponsePayload {
        let users = {
            let span = tracing::info_span!("db.query", table = "users");
            let _guard = span.enter();

            tracing::info!("executing query");
            let result = self.db.list_users().await;
            tracing::info!(rows = result.len(), "query complete");
            result
        };

        // Trace hierarchy:
        //   http.connection (peer.addr=...)
        //     └── http.request (method=GET, route=/tracing/child-span, ...)
        //           └── db.query (table=users)
        //                 └── info "executing query"
        //                 └── info "query complete" { rows=2 }

        ResponsePayload::json(&json!({
            "users": users,
            "trace_note": "See the 'db.query' child span in your tracing backend."
        }))
        .unwrap()
    }

    // GET /tracing/deferred-fields
    // Shows how to record span fields after computation completes.
    // This pattern is common for recording computed values like cache hits.
    async fn deferred_fields(&self, req: RequestPayload) -> ResponsePayload {
        let span = tracing::info_span!(
            "app.lookup",
            cache.hit = tracing::field::Empty,
            result.count = tracing::field::Empty,
        );
        let _guard = span.enter();

        let search = req.get_query("q").unwrap_or("").to_string();
        let users = self.db.list_users().await;

        let results: Vec<_> = users
            .iter()
            .filter(|u| u.name.to_lowercase().contains(&search.to_lowercase()))
            .collect();

        let cache_hit = false;
        span.record("cache.hit", cache_hit);
        span.record("result.count", results.len() as u64);

        ResponsePayload::json(&json!({
            "query": search,
            "results": results,
            "cache_hit": cache_hit,
            "note": "The 'app.lookup' span now has cache.hit and result.count fields."
        }))
        .unwrap()
    }

    // GET /tracing/nested-calls
    // Demonstrates tracing across multiple layers of async function calls.
    // Each function creates its own span, building a complete call tree.
    async fn nested_calls(&self, req: RequestPayload) -> Result<ResponsePayload, RpressError> {
        let id: u32 = req
            .get_param("id")
            .and_then(|v| v.parse().ok())
            .ok_or(RpressError {
                status: StatusCode::BadRequest,
                message: "id must be a positive integer".to_string(),
            })?;

        let user = self.fetch_user_with_tracing(id).await.ok_or(RpressError {
            status: StatusCode::NotFound,
            message: format!("user {} not found", id),
        })?;

        Ok(ResponsePayload::json(&json!({
            "user": user,
            "trace_note": "Look for the 'service.fetch_user' → 'db.find' span chain."
        }))?)
    }

    async fn fetch_user_with_tracing(&self, id: u32) -> Option<crate::db::User> {
        let span = tracing::info_span!("service.fetch_user", user.id = id);
        let _guard = span.enter();

        tracing::info!("fetching user from database");

        let db_span = tracing::info_span!("db.find", table = "users");
        let _db_guard = db_span.enter();
        self.db.find_user(id).await
    }
}

/// Builds the tracing demo route group.
/// Pass the shared DB pool to demonstrate traced database calls.
pub fn get_tracing_routes(db: Arc<DbPool>) -> RpressRoutes {
    let controller = TracingController::new(db);
    let mut routes = RpressRoutes::new();

    // Group middleware: logs every request in this group with a group-level span
    routes.use_middleware(|req, next| async move {
        let span = tracing::info_span!("tracing_demo.group");
        let _guard = span.enter();
        tracing::info!(path = %req.uri(), "entering tracing demo group");
        next(req).await
    });

    routes.add(":get/tracing/basic", handler!(controller, basic));
    routes.add(":get/tracing/child-span", handler!(controller, child_span));
    routes.add(
        ":get/tracing/deferred-fields",
        handler!(controller, deferred_fields),
    );
    routes.add(
        ":get/tracing/nested/:id",
        handler!(controller, nested_calls),
    );

    routes
}
