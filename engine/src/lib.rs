#![allow(dead_code)]
pub mod core;
pub mod types;

use crate::{
    core::{
        handler_response::RpressErrorExt,
        request::Request,
        response::Response,
        routes::{Route, RpressRoutes},
    },
    types::definitions::{HTTP_METHOD_REG, HttpVerbs, RequestPayload, StatusCode},
};
use std::sync::Arc;
use tokio::io::AsyncReadExt;

pub struct Rpress {
    routes_tree: Route,
    pub routes_group: Vec<Option<RpressRoutes>>,
    pub max_buffer_capacity: usize,
}

impl Rpress {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            routes_tree: Route::new(),
            routes_group: Vec::default(),
            max_buffer_capacity: 40096,
        })
    }

    pub fn set_buffer_capacity(self: &mut Arc<Self>, capacity: usize) -> () {
        if let Some(rpress) = Arc::get_mut(self) {
            rpress.max_buffer_capacity = capacity;
        }
    }

    pub fn add_route_group(self: &mut Arc<Self>, group: RpressRoutes) -> () {
        if let Some(rpress) = Arc::get_mut(self) {
            rpress.routes_group.push(Some(group));
        }
    }

    fn initialize_routes(self: &mut Arc<Self>) -> () {
        if let Some(rpress) = Arc::get_mut(self) {
            for route_group in rpress.routes_group.iter_mut() {
                if let Some(mut group) = route_group.take() {
                    for (route, handler) in group.routes.iter_mut() {
                        let look_for_method = match HTTP_METHOD_REG.captures(&route) {
                            Some(method) => method,
                            None => panic!("HTTP method not found in route: {}", route),
                        };

                        if let Some(handler) = handler.take() {
                            rpress.routes_tree.insert_route(
                                &look_for_method[2],
                                String::from(HttpVerbs::from(
                                    look_for_method[1].to_lowercase().as_str(),
                                ))
                                .as_str(),
                                handler,
                            );
                        }
                    }
                }
            }
        }
    }

    async fn dispatch_route(
        &self,
        mut req: RequestPayload,
        socket: &mut tokio::net::TcpStream,
    ) -> () {
        if let Some(ref meta) = req.request_metadata {
            if let Some(route) = self.routes_tree.find(meta.uri.as_str()) {
                let (handler, method, params) = route;

                if meta.method == *method {
                    req.set_params(params);
                    let result = handler(req).await;

                    let mut response = Response::new(socket);
                    match result {
                        Ok(payload) => {
                            let _ = response
                                .send_response(payload.status, payload.body, payload.content_type)
                                .await;
                        }
                        Err(error) => {
                            let (status, message) = error.into_rpress_error();
                            let _ = response
                                .send_response(
                                    status,
                                    message.into_bytes(),
                                    "text/plain; charset=utf-8",
                                )
                                .await;
                        }
                    }
                } else {
                    let mut response = Response::new(socket);
                    let _ = response
                        .send_response(
                            StatusCode::MethodNotAllowed,
                            vec![],
                            "text/plain; charset=utf-8",
                        )
                        .await;
                }
            } else {
                let mut response = Response::new(socket);
                let _ = response
                    .send_response(StatusCode::NotFound, vec![], "text/plain; charset=utf-8")
                    .await;
            }
        }
    }

    pub async fn server<T: Into<String>>(self: &mut Arc<Self>, listener: T) -> anyhow::Result<()> {
        self.initialize_routes();
        let listener = tokio::net::TcpListener::bind(listener.into()).await?;

        loop {
            let (mut socket, _) = listener.accept().await?;

            tokio::spawn({
                let thread_self = self.clone();

                async move {
                    let mut buffer: Vec<u8> = Vec::with_capacity(4096);
                    let mut temp_buffer = [0; 1024];

                    let chunk_header = b"Transfer-Encoding: chunked";
                    let request = Request::new();

                    loop {
                        let n = match socket.read(&mut temp_buffer).await {
                            Ok(0) => break,
                            Ok(n) => n,
                            Err(e) => {
                                eprintln!("Error in read socket: {}", e);
                                break;
                            }
                        };

                        buffer.extend_from_slice(&temp_buffer[..n]);

                        if buffer.len() > thread_self.max_buffer_capacity {
                            eprintln!("Buffer capacity overflowed");
                            break;
                        }

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
                                    let has_metadata = parsed.request_metadata.is_some();
                                    let has_payload = !parsed.payload.is_empty();

                                    if has_metadata {
                                        current_requests.push(parsed);
                                    } else if has_payload {
                                        if let Some(cr) = current_requests.last_mut() {
                                            cr.payload.extend(parsed.payload);
                                        }
                                    }

                                    buffer.drain(..consumed);
                                }
                                Ok(None) => break,
                                Err(err) => {
                                    eprintln!("Parse error: {}", err);
                                    break;
                                }
                            }
                        }

                        for req in current_requests {
                            thread_self.dispatch_route(req, &mut socket).await;
                        }
                    }
                }
            });
        }
    }
}
