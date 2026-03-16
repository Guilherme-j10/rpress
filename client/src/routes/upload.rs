use std::sync::Arc;

use engine::{
    core::handler_response::{ResponsePayload, RpressError},
    core::routes::RpressRoutes,
    handler,
    types::definitions::{RequestPayload, StatusCode},
};
use serde_json::json;

pub struct UploadController;

impl UploadController {
    pub fn new() -> Arc<Self> {
        Arc::new(Self)
    }

    async fn collect_json(&self, mut req: RequestPayload) -> Result<ResponsePayload, RpressError> {
        let body = req.collect_body().await;

        let data: serde_json::Value = serde_json::from_slice(&body).map_err(|e| RpressError {
            status: StatusCode::BadRequest,
            message: format!("Invalid JSON: {}", e),
        })?;

        let name = data
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        Ok(ResponsePayload::json(&json!({
            "received": true,
            "name": name,
            "body_size": body.len()
        }))?)
    }

    async fn stream_upload(
        &self,
        mut req: RequestPayload,
    ) -> Result<ResponsePayload, RpressError> {
        let content_length = req
            .header("content-length")
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(0);

        let mut total_bytes = 0usize;
        let mut chunk_count = 0u32;

        if let Some(mut rx) = req.body_stream() {
            while let Some(chunk) = rx.recv().await {
                chunk_count += 1;
                total_bytes += chunk.len();
                tracing::debug!(
                    "Chunk #{}: {} bytes ({}/{})",
                    chunk_count,
                    chunk.len(),
                    total_bytes,
                    content_length
                );
            }
        } else {
            total_bytes = req.payload.len();
            chunk_count = if total_bytes > 0 { 1 } else { 0 };
        }

        Ok(ResponsePayload::json(&json!({
            "streamed": true,
            "chunks": chunk_count,
            "total_bytes": total_bytes,
            "expected_bytes": content_length,
        }))?)
    }

    async fn echo(&self, mut req: RequestPayload) -> ResponsePayload {
        let body = req.collect_body().await;
        let ct = req
            .header("content-type")
            .unwrap_or("application/octet-stream")
            .to_string();
        ResponsePayload::bytes(body, &ct)
    }
}

pub fn get_upload_routes() -> RpressRoutes {
    let controller = UploadController::new();
    let mut routes = RpressRoutes::new();

    routes.use_middleware(|req, next| async move {
        tracing::info!("[UPLOAD] {} {}", req.method(), req.uri());
        next(req).await
    });

    routes.add(":post/upload/json", handler!(controller, collect_json));
    routes.add(":post/upload/stream", handler!(controller, stream_upload));
    routes.add(":post/upload/echo", handler!(controller, echo));

    routes
}
