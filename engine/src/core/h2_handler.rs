use std::collections::HashMap;
use std::sync::Arc;

use bytes::Bytes;
use h2::server;
use http::{Response, StatusCode as HttpStatusCode};
use tokio::io::{AsyncRead, AsyncWrite};

use crate::Rpress;
use crate::types::definitions::{RequestMetadata, RequestPayload};

fn h2_parts_to_payload(
    method: &http::Method,
    uri: &http::Uri,
    headers: &http::HeaderMap,
    body: Vec<u8>,
) -> RequestPayload {
    let method_str = method.as_str().to_string();
    let uri_str = uri.to_string();

    let (raw_path, query_path) = match uri_str.split_once('?') {
        Some((path, qs)) => (path.to_string(), qs.to_string()),
        None => (uri_str.clone(), String::new()),
    };

    let mut header_map: HashMap<String, String> = HashMap::new();
    for (key, value) in headers {
        let key_lower = key.as_str().to_lowercase();
        let val_str = value.to_str().unwrap_or("").to_string();
        header_map
            .entry(key_lower)
            .and_modify(|existing| {
                existing.push_str(", ");
                existing.push_str(&val_str);
            })
            .or_insert(val_str);
    }

    let query = parse_query_string(&query_path);

    let metadata = RequestMetadata {
        method: method_str,
        uri: raw_path,
        query_path,
        http_method: "HTTP/2".to_string(),
        headers: header_map,
    };

    RequestPayload {
        request_metadata: Some(metadata),
        payload: body,
        params: HashMap::default(),
        query,
        body_receiver: None,
    }
}

fn parse_query_string(query_path: &str) -> HashMap<String, String> {
    let mut result = HashMap::new();
    if query_path.is_empty() {
        return result;
    }
    for pair in query_path.split('&') {
        if let Some((key, value)) = pair.split_once('=') {
            if !key.is_empty() {
                result.insert(key.to_string(), value.replace('+', " "));
            }
        }
    }
    result
}

pub(crate) async fn handle_h2_connection<S: AsyncRead + AsyncWrite + Unpin>(
    server: &Arc<Rpress>,
    io: S,
    max_body_size: usize,
) {
    let mut connection = match server::handshake(io).await {
        Ok(conn) => conn,
        Err(e) => {
            tracing::error!("HTTP/2 handshake failed: {}", e);
            return;
        }
    };

    while let Some(result) = connection.accept().await {
        let (request, respond) = match result {
            Ok(pair) => pair,
            Err(e) => {
                tracing::error!("HTTP/2 accept error: {}", e);
                break;
            }
        };

        let server = Arc::clone(server);
        tokio::spawn(async move {
            handle_h2_stream(&server, request, respond, max_body_size).await;
        });
    }
}

async fn handle_h2_stream(
    server: &Arc<Rpress>,
    request: http::Request<h2::RecvStream>,
    mut respond: h2::server::SendResponse<Bytes>,
    max_body_size: usize,
) {
    let (parts, mut body_stream) = request.into_parts();

    let mut body_bytes = Vec::new();
    while let Some(chunk) = body_stream.data().await {
        match chunk {
            Ok(data) => {
                let _ = body_stream.flow_control().release_capacity(data.len());
                body_bytes.extend_from_slice(&data);
                if body_bytes.len() > max_body_size {
                    let response = Response::builder()
                        .status(HttpStatusCode::PAYLOAD_TOO_LARGE)
                        .body(())
                        .unwrap();
                    let _ = respond.send_response(response, true);
                    return;
                }
            }
            Err(e) => {
                tracing::error!("HTTP/2 body read error: {}", e);
                return;
            }
        }
    }

    let req_payload = h2_parts_to_payload(
        &parts.method,
        &parts.uri,
        &parts.headers,
        body_bytes,
    );

    let resolved = server.resolve_route(req_payload).await;

    let mut response_payload = resolved.payload;
    server.apply_cors_headers(&mut response_payload, resolved.req_origin.as_deref());
    response_payload
        .headers
        .push(("x-request-id".into(), resolved.request_id));

    let final_body = if resolved.is_head {
        Vec::new()
    } else {
        response_payload.body
    };

    let status_u16 = u16::from(response_payload.status);
    let http_status = HttpStatusCode::from_u16(status_u16)
        .unwrap_or(HttpStatusCode::INTERNAL_SERVER_ERROR);

    let mut builder = Response::builder()
        .status(http_status)
        .header("content-type", &response_payload.content_type)
        .header("content-length", final_body.len().to_string());

    for (key, value) in &response_payload.headers {
        builder = builder.header(key.as_str(), value.as_str());
    }

    let response = match builder.body(()) {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("HTTP/2 response build error: {}", e);
            return;
        }
    };

    let end_of_stream = final_body.is_empty();
    let mut send_stream = match respond.send_response(response, end_of_stream) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("HTTP/2 send_response error: {}", e);
            return;
        }
    };

    if !final_body.is_empty() {
        if let Err(e) = send_stream.send_data(Bytes::from(final_body), true) {
            tracing::error!("HTTP/2 send_data error: {}", e);
        }
    }
}
