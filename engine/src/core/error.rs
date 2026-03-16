/// Internal engine errors that can occur during request processing.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum RpressEngineError {
    #[error("Unknown HTTP method: {0}")]
    UnknownMethod(String),
    #[error("Malformed request: {0}")]
    MalformedRequest(String),
    #[error("Payload exceeds maximum allowed size")]
    PayloadTooLarge,
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
