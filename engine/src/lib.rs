pub mod core;
pub mod types;

pub use core::cors::RpressCors;
pub use core::handler_response::{
    CookieBuilder, IntoRpressResult, ResponsePayload, RpressError, RpressErrorExt,
};
pub use core::routes::RpressRoutes;
pub use types::definitions::{RequestPayload, RpressResult, StatusCode};

use crate::{
    core::{
        request::Request,
        response::Response,
        routes::{Route, RouteMatch},
    },
    types::definitions::{HTTP_METHOD_REG, Handler, HttpVerbs, Middleware, Next},
};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tokio::sync::{Mutex, Semaphore};
use tokio::time::{Duration, Instant, timeout};

pub struct Rpress {
    routes_tree: Route,
    routes_group: Vec<Option<RpressRoutes>>,
    max_buffer_capacity: usize,
    middlewares: Vec<Middleware>,
    cors: Option<RpressCors>,
    read_timeout: Duration,
    idle_timeout: Duration,
    max_connections: usize,
    static_dirs: Vec<(String, String)>,
    rate_limit: Option<(u32, u64)>,
    rate_limit_store: Option<Arc<Mutex<HashMap<String, (u32, Instant)>>>>,
    compression_enabled: bool,
    stream_threshold: usize,
}

impl Rpress {
    pub fn new(cors: Option<RpressCors>) -> Self {
        Self {
            routes_tree: Route::new(),
            routes_group: Vec::default(),
            max_buffer_capacity: 40096,
            middlewares: Vec::new(),
            cors,
            read_timeout: Duration::from_secs(30),
            idle_timeout: Duration::from_secs(60),
            max_connections: 1024,
            static_dirs: Vec::new(),
            rate_limit: None,
            rate_limit_store: None,
            compression_enabled: false,
            stream_threshold: 64 * 1024,
        }
    }

    pub fn set_buffer_capacity(&mut self, capacity: usize) {
        self.max_buffer_capacity = capacity;
    }

    pub fn set_read_timeout(&mut self, duration: Duration) {
        self.read_timeout = duration;
    }

    pub fn set_idle_timeout(&mut self, duration: Duration) {
        self.idle_timeout = duration;
    }

    pub fn set_max_connections(&mut self, max: usize) {
        self.max_connections = max;
    }

    pub fn use_middleware<F, Fut>(&mut self, middleware: F)
    where
        F: Fn(RequestPayload, Next) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = RpressResult> + Send + 'static,
    {
        self.middlewares
            .push(Arc::new(move |req, next| Box::pin(middleware(req, next))));
    }

    pub fn add_route_group(&mut self, group: RpressRoutes) {
        self.routes_group.push(Some(group));
    }

    pub fn serve_static(&mut self, url_prefix: &str, dir: &str) {
        let prefix = url_prefix.trim_end_matches('/').to_string();
        self.static_dirs.push((prefix, dir.to_string()));
    }

    pub fn set_rate_limit(&mut self, max_requests: u32, window_secs: u64) {
        self.rate_limit = Some((max_requests, window_secs));
    }

    pub fn enable_compression(&mut self, enabled: bool) {
        self.compression_enabled = enabled;
    }

    pub fn set_stream_threshold(&mut self, bytes: usize) {
        self.stream_threshold = bytes;
    }

    fn initialize_routes(&mut self) {
        for route_group in self.routes_group.iter_mut() {
            if let Some(mut group) = route_group.take() {
                let group_middlewares: Vec<Middleware> = group.middlewares.drain(..).collect();

                for (route, handler) in group.routes.iter_mut() {
                    let look_for_method = match HTTP_METHOD_REG.captures(&route) {
                        Some(method) => method,
                        None => {
                            tracing::error!("HTTP method not found in route: {}", route);
                            continue;
                        }
                    };

                    let verb = match HttpVerbs::try_from_str(
                        look_for_method[1].to_lowercase().as_str(),
                    ) {
                        Ok(v) => v,
                        Err(e) => {
                            tracing::error!("Route '{}': {}", route, e);
                            continue;
                        }
                    };

                    if let Some(handler) = handler.take() {
                        let final_handler = if group_middlewares.is_empty() {
                            handler
                        } else {
                            Self::wrap_handler_with_middlewares(handler, &group_middlewares)
                        };

                        self.routes_tree.insert_route(
                            &look_for_method[2],
                            String::from(verb).as_str(),
                            final_handler,
                        );
                    }
                }
            }
        }
    }

    fn wrap_handler_with_middlewares(handler: Handler, middlewares: &[Middleware]) -> Handler {
        let handler = Arc::new(handler);
        let middlewares: Vec<Middleware> = middlewares.to_vec();

        Box::new(move |req| {
            let handler = Arc::clone(&handler);
            let middlewares = middlewares.clone();

            Box::pin(async move {
                let final_next: Next = Arc::new(move |req| handler(req));

                let chain = middlewares.iter().rev().fold(final_next, |next, mw| {
                    let mw = Arc::clone(mw);
                    Arc::new(move |req| {
                        let mw = Arc::clone(&mw);
                        let next = Arc::clone(&next);
                        Box::pin(async move { mw(req, next).await })
                    })
                });

                chain(req).await
            })
        })
    }

    fn apply_cors_headers(&self, payload: &mut ResponsePayload, req_origin: Option<&str>) {
        if let Some(ref cors) = self.cors {
            payload.headers.retain(|(k, _)| !k.starts_with("Access-Control-") && k != "Vary");
            payload.headers.push(("Vary".into(), "Origin".into()));

            let origin_allowed = cors
                .allowed_origins
                .iter()
                .any(|o| o == "*" || Some(o.as_str()) == req_origin);

            if origin_allowed {
                let origin_value = if cors.allow_credentials {
                    req_origin.unwrap_or("").to_string()
                } else if cors.allowed_origins.iter().any(|o| o == "*") {
                    "*".to_string()
                } else {
                    req_origin.unwrap_or("").to_string()
                };

                payload.headers.push((
                    "Access-Control-Allow-Origin".into(),
                    origin_value,
                ));
                payload.headers.push((
                    "Access-Control-Allow-Methods".into(),
                    cors.allowed_methods.join(", "),
                ));
                payload.headers.push((
                    "Access-Control-Allow-Headers".into(),
                    cors.allowed_headers.join(", "),
                ));

                if cors.allow_credentials {
                    payload
                        .headers
                        .push(("Access-Control-Allow-Credentials".into(), "true".into()));
                }
                if let Some(max_age) = cors.max_age {
                    payload
                        .headers
                        .push(("Access-Control-Max-Age".into(), max_age.to_string()));
                }
                if !cors.expose_headers.is_empty() {
                    payload.headers.push((
                        "Access-Control-Expose-Headers".into(),
                        cors.expose_headers.join(", "),
                    ));
                }
            }
        }
    }

    fn resolve_accept_encoding(&self, req: &RequestPayload) -> Option<String> {
        if !self.compression_enabled {
            return None;
        }
        req.request_metadata
            .as_ref()
            .and_then(|m| m.headers.get("accept-encoding").cloned())
    }

    async fn send_payload<W: tokio::io::AsyncWriteExt + Unpin>(
        &self,
        mut payload: ResponsePayload,
        socket: &mut W,
        req_origin: Option<&str>,
        is_head: bool,
        request_id: &str,
        accept_encoding: Option<&str>,
    ) {
        self.apply_cors_headers(&mut payload, req_origin);
        payload.headers.push(("X-Request-ID".into(), request_id.into()));

        let mut response = Response::new(socket);
        let body = if is_head { vec![] } else { payload.body };
        let _ = response
            .send_response(payload.status, body, &payload.content_type, &payload.headers, accept_encoding)
            .await;
    }

    async fn send_error_status<W: tokio::io::AsyncWriteExt + Unpin>(
        &self,
        status: StatusCode,
        socket: &mut W,
        req_origin: Option<&str>,
        request_id: &str,
    ) {
        let mut payload = ResponsePayload::empty().with_status(status);
        self.apply_cors_headers(&mut payload, req_origin);
        payload.headers.push(("X-Request-ID".into(), request_id.into()));

        let mut response = Response::new(socket);
        let _ = response
            .send_response(
                payload.status,
                payload.body,
                &payload.content_type,
                &payload.headers,
                None,
            )
            .await;
    }

    async fn try_serve_static(&self, uri: &str) -> Option<ResponsePayload> {
        for (prefix, dir) in &self.static_dirs {
            if let Some(relative) = uri.strip_prefix(prefix.as_str()) {
                let relative = relative.trim_start_matches('/');
                if relative.is_empty() || relative.contains("..") {
                    continue;
                }

                let base = std::path::Path::new(dir).canonicalize().ok()?;
                let full = base.join(relative);
                let canonical = match full.canonicalize() {
                    Ok(p) => p,
                    Err(_) => continue,
                };
                if !canonical.starts_with(&base) {
                    continue;
                }

                if let Ok(contents) = tokio::fs::read(&canonical).await {
                    let file_path = canonical.to_string_lossy();
                    let content_type = Self::guess_content_type(&file_path);
                    return Some(ResponsePayload::bytes(contents, content_type));
                }
            }
        }
        None
    }

    fn guess_content_type(path: &str) -> &'static str {
        match path.rsplit('.').next().unwrap_or("") {
            "html" | "htm" => "text/html; charset=utf-8",
            "css" => "text/css; charset=utf-8",
            "js" | "mjs" => "application/javascript; charset=utf-8",
            "json" => "application/json; charset=utf-8",
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "svg" => "image/svg+xml",
            "ico" => "image/x-icon",
            "woff" => "font/woff",
            "woff2" => "font/woff2",
            "ttf" => "font/ttf",
            "pdf" => "application/pdf",
            "xml" => "application/xml",
            "txt" => "text/plain; charset=utf-8",
            "webp" => "image/webp",
            "mp4" => "video/mp4",
            "webm" => "video/webm",
            _ => "application/octet-stream",
        }
    }

    async fn check_rate_limit(&self, addr: &SocketAddr) -> bool {
        if let (Some((max_requests, window_secs)), Some(store)) =
            (self.rate_limit, &self.rate_limit_store)
        {
            let ip = addr.ip().to_string();
            let now = Instant::now();
            let window = Duration::from_secs(window_secs);

            let mut map = store.lock().await;
            let entry = map.entry(ip).or_insert((0, now));

            if now.duration_since(entry.1) > window {
                *entry = (1, now);
                return true;
            }

            entry.0 += 1;
            let allowed = entry.0 <= max_requests;

            if map.len() > 10_000 {
                map.retain(|_, (_, ts)| now.duration_since(*ts) <= window);
            }

            allowed
        } else {
            true
        }
    }

    async fn dispatch_route<W: tokio::io::AsyncWriteExt + Unpin>(
        &self,
        mut req: RequestPayload,
        socket: &mut W,
    ) {
        let request_id = uuid::Uuid::new_v4().to_string();

        let accept_encoding = self.resolve_accept_encoding(&req);
        let accept_enc_ref = accept_encoding.as_deref();

        let req_origin = req
            .request_metadata
            .as_ref()
            .and_then(|m| m.headers.get("origin").cloned());
        let origin_ref = req_origin.as_deref();

        if let Some(ref meta) = req.request_metadata {
            if meta.method == "OPTIONS" && self.cors.is_some() {
                self.send_error_status(StatusCode::NoContent, socket, origin_ref, &request_id)
                    .await;
                return;
            }

            let is_head = meta.method == "HEAD";
            let lookup_method = if is_head { "GET" } else { meta.method.as_str() };

            match self.routes_tree.find(meta.uri.as_str(), lookup_method) {
                RouteMatch::Found(handler, params) => {
                    req.set_params(params);

                    let result = if self.middlewares.is_empty() {
                        handler(req).await
                    } else {
                        let final_next: Next = Arc::new(move |req| handler(req));

                        let chain =
                            self.middlewares
                                .iter()
                                .rev()
                                .fold(final_next, |next, mw| {
                                    let mw = Arc::clone(mw);
                                    Arc::new(move |req| {
                                        let mw = Arc::clone(&mw);
                                        let next = Arc::clone(&next);
                                        Box::pin(async move { mw(req, next).await })
                                    })
                                });

                        chain(req).await
                    };

                    match result {
                        Ok(payload) => {
                            self.send_payload(payload, socket, origin_ref, is_head, &request_id, accept_enc_ref)
                                .await;
                        }
                        Err(error) => {
                            let (status, message) = error.into_rpress_error();
                            let mut payload = ResponsePayload::text(message).with_status(status);
                            self.apply_cors_headers(&mut payload, origin_ref);
                            payload.headers.push(("X-Request-ID".into(), request_id.clone()));
                            let mut response = Response::new(socket);
                            let _ = response
                                .send_response(
                                    payload.status,
                                    payload.body,
                                    &payload.content_type,
                                    &payload.headers,
                                    accept_enc_ref,
                                )
                                .await;
                        }
                    }
                }
                RouteMatch::WrongMethod => {
                    self.send_error_status(StatusCode::MethodNotAllowed, socket, origin_ref, &request_id)
                        .await;
                }
                RouteMatch::NotFound => {
                    if is_head || lookup_method == "GET" {
                        if let Some(payload) = self.try_serve_static(meta.uri.as_str()).await {
                            self.send_payload(payload, socket, origin_ref, is_head, &request_id, accept_enc_ref)
                                .await;
                            return;
                        }
                    }
                    self.send_error_status(StatusCode::NotFound, socket, origin_ref, &request_id)
                        .await;
                }
            }
        }
    }

    pub async fn listen<T: Into<String>>(mut self, addr: T) -> anyhow::Result<()> {
        self.initialize_routes();
        if self.rate_limit.is_some() {
            self.rate_limit_store = Some(Arc::new(Mutex::new(HashMap::new())));
        }
        let listener = tokio::net::TcpListener::bind(addr.into()).await?;
        let arc_self = Arc::new(self);
        Self::run_server(&arc_self, listener).await
    }

    pub async fn server_with_listener(
        mut self,
        listener: tokio::net::TcpListener,
    ) -> anyhow::Result<()> {
        self.initialize_routes();
        if self.rate_limit.is_some() {
            self.rate_limit_store = Some(Arc::new(Mutex::new(HashMap::new())));
        }
        let arc_self = Arc::new(self);
        Self::run_server(&arc_self, listener).await
    }

    async fn run_server(arc_self: &Arc<Self>, listener: tokio::net::TcpListener) -> anyhow::Result<()> {
        let semaphore = Arc::new(Semaphore::new(arc_self.max_connections));
        let tracker = tokio_util::task::TaskTracker::new();
        let shutdown = tokio::signal::ctrl_c();
        tokio::pin!(shutdown);

        tracing::info!("Rpress server started");

        loop {
            tokio::select! {
                result = listener.accept() => {
                    let (mut socket, addr) = result?;
                    let permit = match semaphore.clone().try_acquire_owned() {
                        Ok(p) => p,
                        Err(_) => {
                            tracing::warn!("Connection limit reached, rejecting {}", addr);
                            continue;
                        }
                    };

                    if !arc_self.check_rate_limit(&addr).await {
                        tracing::debug!("Rate limited: {}", addr);
                        let request_id = uuid::Uuid::new_v4().to_string();
                        arc_self.send_error_status(
                            StatusCode::TooManyRequests,
                            &mut socket,
                            None,
                            &request_id,
                        ).await;
                        continue;
                    }

                    tracker.spawn({
                        let thread_self = arc_self.clone();

                        async move {
                            let _permit = permit;
                            let mut buffer: Vec<u8> = Vec::with_capacity(4096);
                            let mut temp_buffer = [0; 1024];
                            let chunk_header = b"Transfer-Encoding: chunked";
                            let request = Request::new();
                            let read_dur = thread_self.read_timeout;
                            let idle_dur = thread_self.idle_timeout;
                            let mut use_idle_timeout = true;

                            loop {
                                let dur = if use_idle_timeout { idle_dur } else { read_dur };

                                let n = match timeout(dur, socket.read(&mut temp_buffer)).await {
                                    Ok(Ok(0)) => break,
                                    Ok(Ok(n)) => n,
                                    Ok(Err(e)) => {
                                        tracing::error!("Socket read error: {}", e);
                                        break;
                                    }
                                    Err(_) => {
                                        tracing::debug!("Connection timeout for {}", addr);
                                        let rid = uuid::Uuid::new_v4().to_string();
                                        thread_self.send_error_status(
                                            StatusCode::RequestTimeout,
                                            &mut socket,
                                            None,
                                            &rid,
                                        ).await;
                                        break;
                                    }
                                };

                                buffer.extend_from_slice(&temp_buffer[..n]);

                                if buffer.len() > thread_self.max_buffer_capacity {
                                    tracing::warn!("Buffer overflow for {}", addr);
                                    let rid = uuid::Uuid::new_v4().to_string();
                                    thread_self.send_error_status(
                                        StatusCode::PayloadTooLarge,
                                        &mut socket,
                                        None,
                                        &rid,
                                    ).await;
                                    break;
                                }

                                let threshold = thread_self.stream_threshold;
                                let should_stream = if threshold > 0 {
                                    match request.parse_headers_only(&buffer) {
                                        Ok(Some(ref h)) => !h.is_chunked && h.content_length > threshold,
                                        _ => false,
                                    }
                                } else {
                                    false
                                };

                                if should_stream {
                                    let parsed = request.parse_headers_only(&buffer).unwrap().unwrap();
                                    let body_start = parsed.body_start;
                                    let content_length = parsed.content_length;
                                    let (tx, rx) = tokio::sync::mpsc::channel::<Vec<u8>>(8);

                                    let req = RequestPayload {
                                        request_metadata: Some(parsed.metadata),
                                        payload: Vec::new(),
                                        params: HashMap::default(),
                                        query: parsed.query,
                                        body_receiver: Some(rx),
                                    };

                                    let already_in_buffer = buffer.len().saturating_sub(body_start);
                                    if already_in_buffer > 0 {
                                        let chunk = buffer[body_start..].to_vec();
                                        let _ = tx.send(chunk).await;
                                    }
                                    let mut remaining = content_length.saturating_sub(already_in_buffer);
                                    buffer.clear();

                                    let (mut read_half, mut write_half) = socket.into_split();

                                    let read_handle = tokio::spawn(async move {
                                        let mut tmp = [0u8; 4096];
                                        while remaining > 0 {
                                            match timeout(read_dur, read_half.read(&mut tmp)).await {
                                                Ok(Ok(0)) => break,
                                                Ok(Ok(n)) => {
                                                    let take = n.min(remaining);
                                                    if tx.send(tmp[..take].to_vec()).await.is_err() {
                                                        break;
                                                    }
                                                    remaining -= take;
                                                }
                                                Ok(Err(_)) | Err(_) => break,
                                            }
                                        }
                                        drop(tx);
                                        read_half
                                    });

                                    thread_self.dispatch_route(req, &mut write_half).await;

                                    let read_half = read_handle.await.unwrap();
                                    socket = read_half.reunite(write_half).unwrap();
                                    use_idle_timeout = true;
                                } else {
                                    let mut is_chunked = buffer
                                        .windows(chunk_header.len())
                                        .any(|b| b == chunk_header);

                                    let mut current_requests: Vec<RequestPayload> = vec![];

                                    loop {
                                        if buffer.is_empty() {
                                            break;
                                        }

                                        if !is_chunked {
                                            is_chunked = buffer
                                                .windows(chunk_header.len())
                                                .any(|b| b == chunk_header);
                                        }

                                        match request.parse_http_protocol(&buffer, is_chunked) {
                                            Ok(Some((parsed, consumed))) => {
                                                let has_metadata =
                                                    parsed.request_metadata.is_some();
                                                let has_payload = !parsed.payload.is_empty();

                                                if has_metadata {
                                                    current_requests.push(parsed);
                                                } else if has_payload {
                                                    if let Some(cr) =
                                                        current_requests.last_mut()
                                                    {
                                                        cr.payload.extend(parsed.payload);
                                                    }
                                                }

                                                buffer.drain(..consumed);
                                            }
                                            Ok(None) => break,
                                            Err(err) => {
                                                tracing::warn!("Parse error from {}: {}", addr, err);
                                                let rid = uuid::Uuid::new_v4().to_string();
                                                let mut payload = ResponsePayload::text(err)
                                                    .with_status(StatusCode::BadRequest);
                                                thread_self.apply_cors_headers(&mut payload, None);
                                                payload.headers.push(("X-Request-ID".into(), rid));
                                                let mut response = Response::new(&mut socket);
                                                let _ = response
                                                    .send_response(
                                                        payload.status,
                                                        payload.body,
                                                        &payload.content_type,
                                                        &payload.headers,
                                                        None,
                                                    )
                                                    .await;
                                                buffer.clear();
                                                break;
                                            }
                                        }
                                    }

                                    use_idle_timeout = buffer.is_empty();
                                    for req in current_requests {
                                        thread_self
                                            .dispatch_route(req, &mut socket)
                                            .await;
                                    }
                                }
                            }
                        }
                    });
                }
                _ = &mut shutdown => {
                    tracing::info!("Shutdown signal received, waiting for active connections...");
                    break;
                }
            }
        }

        tracker.close();
        tracker.wait().await;
        tracing::info!("Rpress server stopped");

        Ok(())
    }
}
