//! Core framework components: routing, request parsing, response building, CORS,
//! TLS, rate limiting, error handling, and HTTP/2 support.

/// Route definitions and trie-based routing engine.
pub mod routes;
/// HTTP request parsing (headers, body, query strings, percent-decoding).
pub mod request;
pub(crate) mod response;
/// Response payload builders, cookie builder, error types, and handler result traits.
pub mod handler_response;
/// Engine-level error types.
pub mod error;
/// CORS (Cross-Origin Resource Sharing) configuration.
pub mod cors;
/// TLS configuration for HTTPS support via rustls.
pub mod tls;
/// Pluggable rate limiting with a default in-memory implementation.
pub mod rate_limiter;
pub(crate) mod h2_handler;
