pub mod core;
pub mod types;

use crate::{
    core::{
        handler_response::{IntoRpressResult, RpressErrorExt},
        request::Request,
        response::Response,
        routes::Route,
    },
    types::definitions::{HTTP_METHOD_REG, HttpVerbs, RequestPayload, StatusCode},
};
use std::sync::Arc;
use tokio::io::AsyncReadExt;

pub struct Rpress {
    routes_tree: Route,
    pub max_buffer_capacity: usize,
}

impl Rpress {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            routes_tree: Route::new(),
            max_buffer_capacity: 40096,
        })
    }

    pub fn set_buffer_capacity(self: &mut Arc<Self>, capacity: usize) -> () {
        if let Some(rpress) = Arc::get_mut(self) {
            rpress.max_buffer_capacity = capacity;
        }
    }

    // space can be (+) or (%20)
    pub fn route<T, F, Fut, R>(self: &mut Arc<Self>, name: T, handler: F)
    where
        T: Into<String>,
        R: IntoRpressResult + 'static,
        F: Fn(RequestPayload) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = R> + Send + 'static,
    {
        if let Some(rpress) = Arc::get_mut(self) {
            let route = name.into();
            let look_for_method = match HTTP_METHOD_REG.captures(&route) {
                Some(method) => method,
                None => panic!("HTTP method ot found"),
            };

            rpress.routes_tree.insert_route(
                &look_for_method[2],
                String::from(HttpVerbs::from(look_for_method[1].to_lowercase().as_str())).as_str(),
                Box::new(move |req| {
                    let fut = handler(req);

                    Box::pin(async move { fut.await.into_result() })
                }),
            );
        }
    }

    async fn dispatch_route(
        &self,
        mut req: RequestPayload,
        socket: &mut tokio::net::TcpStream,
    ) -> () {
        if let Some(ref meta) = req.request_metadata {
            let mut response = Response::new(socket);

            if let Some(route) = self.routes_tree.find(meta.uri.as_str()) {
                let handler = route.0;
                let method = route.1;
                let params = route.2;

                if meta.method == *method {
                    req.set_params(params);
                    match handler(req).await {
                        Ok(payload) => {
                            let _ = response
                                .send_response(payload.status, payload.body, payload.content_type)
                                .await;
                        }
                        Err(error) => {
                            let get_complete_errro = error.into_rpress_error();
                            let _ = response
                                .send_response(
                                    get_complete_errro.0,
                                    get_complete_errro.1.into_bytes(),
                                    "text/plain; charset=utf-8",
                                )
                                .await;
                        }
                    }
                } else {
                    let _ = response
                        .send_response(
                            StatusCode::MethodNotAllowed,
                            vec![],
                            "text/plain; charset=utf-8",
                        )
                        .await;
                }
            } else {
                let _ = response
                    .send_response(StatusCode::NotFound, vec![], "text/plain; charset=utf-8")
                    .await;
            }
        }
    }

    pub async fn server<T: Into<String>>(self: Arc<Self>, listener: T) -> anyhow::Result<()> {
        let listener = tokio::net::TcpListener::bind(listener.into()).await?;

        loop {
            let (mut socket, _) = listener.accept().await?;

            tokio::spawn({
                let thread_self = self.clone();

                async move {
                    let mut buffer: Vec<u8> = Vec::with_capacity(4096);
                    let mut temp_buffer = [0; 1024];

                    let chunk_header = b"Transfer-Encoding: chunked";
                    let mut is_chunked = false;

                    let request = Request::new();
                    let mut current_request: Vec<RequestPayload> = vec![];

                    loop {
                        loop {
                            if buffer.len() == 0 {
                                break;
                            }

                            if is_chunked == false {
                                if let Some(_) = buffer
                                    .windows(chunk_header.len())
                                    .position(|b| b == chunk_header)
                                {
                                    is_chunked = true;
                                }
                            }

                            match request.parse_http_protocol(&buffer, is_chunked) {
                                Ok(Some((request, consumed))) => {
                                    let has_metadata = request.request_metadata.is_some();
                                    let has_payload = !request.payload.is_empty();

                                    if has_metadata {
                                        current_request.push(request);
                                    } else if has_payload {
                                        if let Some(cr) = current_request.last_mut() {
                                            cr.payload.extend(request.payload);
                                        }
                                    }

                                    buffer.drain(..consumed);
                                }
                                Ok(None) => {
                                    println!("[POSSIBLE_MTU]: Incomplete message");
                                    break;
                                }
                                Err(err) => {
                                    println!("Error: {}", err);
                                    break;
                                }
                            }
                        }

                        for request in current_request {
                            thread_self.dispatch_route(request, &mut socket).await;
                        }

                        current_request = vec![];
                        is_chunked = false;

                        let n = match socket.read(&mut temp_buffer).await {
                            Ok(0) => break,
                            Ok(n) => n,
                            Err(e) => {
                                println!("Error in read socket: {}", e);
                                break;
                            }
                        };

                        buffer.extend_from_slice(&temp_buffer[..n]);

                        if buffer.len() > thread_self.max_buffer_capacity {
                            println!("Buffer capacity overflowed");
                            break;
                        }
                    }
                }
            });
        }
    }
}
