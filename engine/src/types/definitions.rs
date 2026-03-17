use std::{collections::HashMap, pin::Pin, sync::{Arc, LazyLock}};

use regex::Regex;

use crate::core::handler_response::{ResponsePayload, RpressError};

/// Regex for extracting the HTTP method and path from route definitions (e.g. `:get/users`).
pub static HTTP_METHOD_REG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^:([^\/]+)(.*)$").unwrap());
/// Regex for matching percent-encoded UTF-8 byte sequences in query strings.
pub static PERCENT_ENCODING: LazyLock<Regex> = 
    LazyLock::new(|| Regex::new(r"(%F[0-7](?:%[89AB][0-9A-F]){3})|(%E[0-F](?:%[89AB][0-9A-F]){2})|(%C[23]%[89AB][0-9A-F])|(%[0-9A-F]{2})").unwrap());

/// Result type alias for Rpress handler return values.
///
/// Handlers return `RpressResult` (or `RpressResult<E>` with a custom error type).
pub type RpressResult<E = RpressError> = Result<ResponsePayload, E>;

/// A boxed async handler function that receives a [`RequestPayload`] and returns a [`RpressResult`].
pub type Handler = Box<
    dyn Fn(RequestPayload) -> Pin<Box<dyn Future<Output = RpressResult> + Send + 'static>>
        + Send
        + Sync,
>;

/// The "next" function in the middleware chain, calling the next middleware or final handler.
pub type Next = Arc<
    dyn Fn(RequestPayload) -> Pin<Box<dyn Future<Output = RpressResult> + Send + 'static>>
        + Send
        + Sync,
>;

/// A boxed async middleware function that receives a [`RequestPayload`] and a [`Next`] callback.
pub type Middleware = Arc<
    dyn Fn(RequestPayload, Next) -> Pin<Box<dyn Future<Output = RpressResult> + Send + 'static>>
        + Send
        + Sync,
>;

/// Macro to create a handler closure from an `Arc<Controller>` and one of its async methods.
///
/// # Example
///
/// ```ignore
/// let controller = UserController::new(); // returns Arc<UserController>
/// routes.add(":get/users/:id", handler!(controller, get_user));
/// ```
#[macro_export]
macro_rules! handler {
    ($controller:ident, $method:ident) => {{
        let controller = std::sync::Arc::clone(&$controller);
        move |req| {
            let controller = std::sync::Arc::clone(&controller);
            async move { controller.$method(req).await }
        }
    }};
}

/// Parsed HTTP request metadata including method, URI, and headers.
#[derive(Debug)]
pub struct RequestMetadata {
    /// The HTTP method (e.g. `"GET"`, `"POST"`).
    pub method: String,
    /// The decoded request URI path (e.g. `"/users/42"`).
    pub uri: String,
    #[allow(dead_code)]
    pub(crate) query_path: String,
    /// The HTTP version string (e.g. `"HTTP/1.1"` or `"HTTP/2"`).
    pub http_method: String,
    /// Request headers as a map of lowercase keys to concatenated values.
    pub headers: HashMap<String, String>,
}

/// Incoming HTTP request with metadata, body, route params, and query params.
///
/// Use the helper methods ([`uri()`](RequestPayload::uri), [`method()`](RequestPayload::method),
/// [`header()`](RequestPayload::header), [`get_param()`](RequestPayload::get_param), etc.)
/// to access request data ergonomically.
pub struct RequestPayload {
    /// Parsed request metadata (method, URI, headers). `None` for chunked-only frames.
    pub request_metadata: Option<RequestMetadata>,
    /// The raw request body bytes (empty when using body streaming).
    pub payload: Vec<u8>,
    /// Route parameters extracted from dynamic segments (e.g. `:id`).
    pub params: HashMap<String, String>,
    /// Parsed query string parameters.
    pub query: HashMap<String, String>,
    pub(crate) body_receiver: Option<tokio::sync::mpsc::Receiver<Vec<u8>>>,
}

#[derive(Debug)]
pub(crate) enum HttpVerbs {
    Get,
    Post,
    Delete,
    Put,
    Patch,
    Head,
    Options,
}

impl HttpVerbs {
    pub(crate) fn try_from_str(method: &str) -> Result<Self, crate::core::error::RpressEngineError> {
        match method {
            "delete" => Ok(HttpVerbs::Delete),
            "patch" => Ok(HttpVerbs::Patch),
            "post" => Ok(HttpVerbs::Post),
            "put" => Ok(HttpVerbs::Put),
            "get" => Ok(HttpVerbs::Get),
            "head" => Ok(HttpVerbs::Head),
            "options" => Ok(HttpVerbs::Options),
            _ => Err(crate::core::error::RpressEngineError::UnknownMethod(method.to_string())),
        }
    }
}

impl From<HttpVerbs> for String {
    fn from(verb: HttpVerbs) -> String {
        match verb {
            HttpVerbs::Delete => String::from("DELETE"),
            HttpVerbs::Get => String::from("GET"),
            HttpVerbs::Post => String::from("POST"),
            HttpVerbs::Put => String::from("PUT"),
            HttpVerbs::Patch => String::from("PATCH"),
            HttpVerbs::Head => String::from("HEAD"),
            HttpVerbs::Options => String::from("OPTIONS"),
        }
    }
}

#[allow(dead_code)]
pub(crate) enum HeadersResponse {
    Date,
    Content,
    ContentLength,
    ContentType,
    ContentEncoding,
    ContentLanguage,
    ContentLocation,
    ContentRange,
    ContentDisposition,
    ContentSecurityPolicy,
    ContentSecurityPolicyReportOnly,
    Expires,
    LastModified,
    Location,
    Pragma,
    RetryAfter,
    Server,
    SetCookie,
    Vary,
    WWWAuthenticate,
    XContentTypeOptions,
    XPoweredBy,
    XRequestID,
    XRobotsTag,
    XUACompatible,
    XFrameOptions,
    XXSSProtection,
    AltSvc,
    AcceptPatch,
    AcceptRanges,
    Age,
    Allow,
    AltUsed,
    CacheControl,
    Connection,
    ContentMD5,
    ETag,
    TransferEncoding,
    Upgrade,
}

impl From<HeadersResponse> for String {
    fn from(header: HeadersResponse) -> String {
        match header {
            HeadersResponse::Date => String::from("Date"),
            HeadersResponse::Content => String::from("Content"),
            HeadersResponse::ContentLength => String::from("Content-Length"),
            HeadersResponse::ContentType => String::from("Content-Type"),
            HeadersResponse::ContentEncoding => String::from("Content-Encoding"),
            HeadersResponse::ContentLanguage => String::from("Content-Language"),
            HeadersResponse::ContentLocation => String::from("Content-Location"),
            HeadersResponse::ContentRange => String::from("Content-Range"),
            HeadersResponse::ContentDisposition => String::from("Content-Disposition"),
            HeadersResponse::ContentSecurityPolicy => String::from("Content-Security-Policy"),
            HeadersResponse::ContentSecurityPolicyReportOnly => {
                String::from("Content-Security-Policy-Report-Only")
            }
            HeadersResponse::Expires => String::from("Expires"),
            HeadersResponse::LastModified => String::from("Last-Modified"),
            HeadersResponse::Location => String::from("Location"),
            HeadersResponse::Pragma => String::from("Pragma"),
            HeadersResponse::RetryAfter => String::from("Retry-After"),
            HeadersResponse::Server => String::from("Server"),
            HeadersResponse::SetCookie => String::from("Set-Cookie"),
            HeadersResponse::Vary => String::from("Vary"),
            HeadersResponse::WWWAuthenticate => String::from("WWW-Authenticate"),
            HeadersResponse::XContentTypeOptions => String::from("X-Content-Type-Options"),
            HeadersResponse::XPoweredBy => String::from("X-Powered-By"),
            HeadersResponse::XRequestID => String::from("X-Request-ID"),
            HeadersResponse::XRobotsTag => String::from("X-Robots-Tag"),
            HeadersResponse::XUACompatible => String::from("X-UA-Compatible"),
            HeadersResponse::XFrameOptions => String::from("X-Frame-Options"),
            HeadersResponse::XXSSProtection => String::from("X-XSS-Protection"),
            HeadersResponse::AltSvc => String::from("Alt-Svc"),
            HeadersResponse::AcceptPatch => String::from("Accept-Patch"),
            HeadersResponse::AcceptRanges => String::from("Accept-Ranges"),
            HeadersResponse::Age => String::from("Age"),
            HeadersResponse::Allow => String::from("Allow"),
            HeadersResponse::AltUsed => String::from("Alt-Used"),
            HeadersResponse::CacheControl => String::from("Cache-Control"),
            HeadersResponse::Connection => String::from("Connection"),
            HeadersResponse::ContentMD5 => String::from("Content-MD5"),
            HeadersResponse::ETag => String::from("ETag"),
            HeadersResponse::TransferEncoding => String::from("Transfer-Encoding"),
            HeadersResponse::Upgrade => String::from("Upgrade"),
        }
    }
}

/// HTTP status codes supported by Rpress.
///
/// Covers informational (1xx), success (2xx), redirection (3xx), client error (4xx),
/// and server error (5xx) responses as defined by RFC 9110 and common extensions.
#[non_exhaustive]
#[derive(Debug, Clone, Copy)]
pub enum StatusCode {
    /// 100 Continue
    Continue = 100,
    /// 101 Switching Protocols
    SwitchingProtocols = 101,
    /// 200 OK
    OK = 200,
    /// 201 Created
    Created = 201,
    /// 202 Accepted
    Accepted = 202,
    /// 203 Non-Authoritative Information
    NonAuthoritativeInformation = 203,
    /// 204 No Content
    NoContent = 204,
    /// 205 Reset Content
    ResetContent = 205,
    /// 206 Partial Content
    PartialContent = 206,
    /// 207 Multi-Status (WebDAV)
    MultiStatus = 207,
    /// 208 Already Reported (WebDAV)
    AlreadyReported = 208,
    /// 301 Moved Permanently
    MovedPermanently = 301,
    /// 302 Found (temporary redirect)
    Found = 302,
    /// 303 See Other
    SeeOther = 303,
    /// 304 Not Modified
    NotModified = 304,
    /// 307 Temporary Redirect
    TemporaryRedirect = 307,
    /// 308 Permanent Redirect
    PermanentRedirect = 308,
    /// 400 Bad Request
    BadRequest = 400,
    /// 401 Unauthorized
    Unauthorized = 401,
    /// 403 Forbidden
    Forbidden = 403,
    /// 404 Not Found
    NotFound = 404,
    /// 405 Method Not Allowed
    MethodNotAllowed = 405,
    /// 406 Not Acceptable
    NotAcceptable = 406,
    /// 407 Proxy Authentication Required
    ProxyAuthenticationRequired = 407,
    /// 408 Request Timeout
    RequestTimeout = 408,
    /// 409 Conflict
    Conflict = 409,
    /// 410 Gone
    Gone = 410,
    /// 411 Length Required
    LengthRequired = 411,
    /// 412 Precondition Failed
    PreconditionFailed = 412,
    /// 413 Payload Too Large
    PayloadTooLarge = 413,
    /// 414 URI Too Long
    UriTooLong = 414,
    /// 415 Unsupported Media Type
    UnsupportedMediaType = 415,
    /// 416 Range Not Satisfiable
    RangeNotSatisfiable = 416,
    /// 417 Expectation Failed
    ExpectationFailed = 417,
    /// 418 I'm a Teapot (RFC 2324)
    ImaTeapot = 418,
    /// 422 Unprocessable Entity (WebDAV)
    UnprocessableEntity = 422,
    /// 423 Locked (WebDAV)
    Locked = 423,
    /// 424 Failed Dependency (WebDAV)
    FailedDependency = 424,
    /// 425 Unordered Collection
    UnorderedCollection = 425,
    /// 426 Upgrade Required
    UpgradeRequired = 426,
    /// 428 Precondition Required
    PreconditionRequired = 428,
    /// 429 Too Many Requests
    TooManyRequests = 429,
    /// 431 Request Header Fields Too Large
    RequestHeaderFieldsTooLarge = 431,
    /// 500 Internal Server Error
    InternalServerError = 500,
    /// 501 Not Implemented
    NotImplemented = 501,
    /// 502 Bad Gateway
    BadGateway = 502,
    /// 503 Service Unavailable
    ServiceUnavailable = 503,
    /// 504 Gateway Timeout
    GatewayTimeout = 504,
    /// 505 HTTP Version Not Supported
    HttpVersionNotSupported = 505,
    /// 506 Variant Also Negotiates
    VariantAlsoNegotiates = 506,
    /// 507 Insufficient Storage (WebDAV)
    InsufficientStorage = 507,
    /// 508 Loop Detected (WebDAV)
    LoopDetected = 508,
    /// 510 Not Extended
    NotExtended = 510,
    /// 511 Network Authentication Required
    NetworkAuthenticationRequired = 511,
    /// 520 Unknown Error
    UnknownError = 520,
}

impl From<StatusCode> for u16 {
    fn from(status: StatusCode) -> Self {
        status as u16
    }
}

impl From<&StatusCode> for String {
    fn from(status: &StatusCode) -> Self {
        match status {
            StatusCode::Continue => String::from("Continue"),
            StatusCode::SwitchingProtocols => String::from("Switching Protocols"),
            StatusCode::OK => String::from("OK"),
            StatusCode::Created => String::from("Created"),
            StatusCode::Accepted => String::from("Accepted"),
            StatusCode::NonAuthoritativeInformation => {
                String::from("Non-Authoritative Information")
            }
            StatusCode::NoContent => String::from("No Content"),
            StatusCode::ResetContent => String::from("Reset Content"),
            StatusCode::PartialContent => String::from("Partial Content"),
            StatusCode::MultiStatus => String::from("Multi-Status"),
            StatusCode::AlreadyReported => String::from("Already Reported"),
            StatusCode::MovedPermanently => String::from("Moved Permanently"),
            StatusCode::Found => String::from("Found"),
            StatusCode::SeeOther => String::from("See Other"),
            StatusCode::NotModified => String::from("Not Modified"),
            StatusCode::TemporaryRedirect => String::from("Temporary Redirect"),
            StatusCode::PermanentRedirect => String::from("Permanent Redirect"),
            StatusCode::BadRequest => String::from("Bad Request"),
            StatusCode::Unauthorized => String::from("Unauthorized"),
            StatusCode::Forbidden => String::from("Forbidden"),
            StatusCode::NotFound => String::from("Not Found"),
            StatusCode::MethodNotAllowed => String::from("Method Not Allowed"),
            StatusCode::NotAcceptable => String::from("Not Acceptable"),
            StatusCode::ProxyAuthenticationRequired => {
                String::from("Proxy Authentication Required")
            }
            StatusCode::RequestTimeout => String::from("Request Timeout"),
            StatusCode::Conflict => String::from("Conflict"),
            StatusCode::Gone => String::from("Gone"),
            StatusCode::LengthRequired => String::from("Length Required"),
            StatusCode::PreconditionFailed => String::from("Precondition Failed"),
            StatusCode::PayloadTooLarge => String::from("Payload Too Large"),
            StatusCode::UriTooLong => String::from("URI Too Long"),
            StatusCode::UnsupportedMediaType => String::from("Unsupported Media Type"),
            StatusCode::RangeNotSatisfiable => String::from("Range Not Satisfiable"),
            StatusCode::ExpectationFailed => String::from("Expectation Failed"),
            StatusCode::ImaTeapot => String::from("I'm a Teapot"),
            StatusCode::UnprocessableEntity => String::from("Unprocessable Entity"),
            StatusCode::Locked => String::from("Locked"),
            StatusCode::FailedDependency => String::from("Failed Dependency"),
            StatusCode::UnorderedCollection => String::from("Unordered Collection"),
            StatusCode::UpgradeRequired => String::from("Upgrade Required"),
            StatusCode::PreconditionRequired => String::from("Precondition Required"),
            StatusCode::TooManyRequests => String::from("Too Many Requests"),
            StatusCode::RequestHeaderFieldsTooLarge => {
                String::from("Request Header Fields Too Large")
            }
            StatusCode::InternalServerError => String::from("Internal Server Error"),
            StatusCode::NotImplemented => String::from("Not Implemented"),
            StatusCode::BadGateway => String::from("Bad Gateway"),
            StatusCode::ServiceUnavailable => String::from("Service Unavailable"),
            StatusCode::GatewayTimeout => String::from("Gateway Timeout"),
            StatusCode::HttpVersionNotSupported => String::from("HTTP Version Not Supported"),
            StatusCode::VariantAlsoNegotiates => String::from("Variant Also Negotiates"),
            StatusCode::InsufficientStorage => String::from("Insufficient Storage"),
            StatusCode::LoopDetected => String::from("Loop Detected"),
            StatusCode::NotExtended => String::from("Not Extended"),
            StatusCode::NetworkAuthenticationRequired => {
                String::from("Network Authentication Required")
            }
            StatusCode::UnknownError => String::from("Unknown Error"),
        }
    }
}
