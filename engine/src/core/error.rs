/// Internal engine errors that can occur during request processing.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum RpressEngineError {
    /// The HTTP method in the route definition is not recognized.
    #[error("Unknown HTTP method: {0}")]
    UnknownMethod(String),
    /// The incoming request could not be parsed.
    #[error("Malformed request: {0}")]
    MalformedRequest(String),
    /// The request body exceeds the configured maximum size.
    #[error("Payload exceeds maximum allowed size")]
    PayloadTooLarge,
    /// An I/O error occurred while reading or writing.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
