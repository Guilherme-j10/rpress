//! Transport layer: abstractions for HTTP long-polling and WebSocket transports.

use base64::Engine as Base64Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use sha1::{Digest, Sha1};

/// The WebSocket GUID used in the Sec-WebSocket-Accept calculation (RFC 6455).
const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

/// Computes the `Sec-WebSocket-Accept` value from a client's `Sec-WebSocket-Key`.
pub(crate) fn compute_ws_accept(key: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(key.as_bytes());
    hasher.update(WS_GUID.as_bytes());
    let hash = hasher.finalize();
    BASE64_STANDARD.encode(hash)
}

/// Parses Socket.IO/Engine.IO query parameters from a URI query string.
#[derive(Debug, Default)]
pub(crate) struct EioQueryParams {
    pub eio: Option<u8>,
    pub transport: Option<String>,
    pub sid: Option<String>,
    pub t: Option<String>,
}

impl EioQueryParams {
    pub fn parse(query: &str) -> Self {
        let mut params = Self::default();
        for pair in query.split('&') {
            if let Some((key, value)) = pair.split_once('=') {
                match key {
                    "EIO" => params.eio = value.parse().ok(),
                    "transport" => params.transport = Some(value.to_string()),
                    "sid" => params.sid = Some(value.to_string()),
                    "t" => params.t = Some(value.to_string()),
                    _ => {}
                }
            }
        }
        params
    }

    pub fn is_polling(&self) -> bool {
        self.transport.as_deref() == Some("polling")
    }

    pub fn is_websocket(&self) -> bool {
        self.transport.as_deref() == Some("websocket")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ws_accept_rfc_example() {
        let accept = compute_ws_accept("dGhlIHNhbXBsZSBub25jZQ==");
        assert_eq!(accept, "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
    }

    #[test]
    fn test_query_params_parsing() {
        let params = EioQueryParams::parse("EIO=4&transport=polling&t=N8hyd6w");
        assert_eq!(params.eio, Some(4));
        assert!(params.is_polling());
        assert!(params.sid.is_none());
        assert_eq!(params.t.as_deref(), Some("N8hyd6w"));
    }

    #[test]
    fn test_query_params_with_sid() {
        let params = EioQueryParams::parse("EIO=4&transport=websocket&sid=abc123");
        assert!(params.is_websocket());
        assert_eq!(params.sid.as_deref(), Some("abc123"));
    }
}
