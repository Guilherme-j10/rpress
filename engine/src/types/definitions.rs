use std::{collections::HashMap, pin::Pin, sync::LazyLock};

use regex::Regex;

use crate::core::handler_response::{ResponsePayload, RpressError};

pub static HTTP_METHOD_REG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^:([^\/]+)(.*)$").unwrap());
pub static PERCENT_ENCODING: LazyLock<Regex> = 
    LazyLock::new(|| Regex::new(r"(%F[0-7](?:%[89AB][0-9A-F]){3})|(%E[0-F](?:%[89AB][0-9A-F]){2})|(%C[23]%[89AB][0-9A-F])|(%[0-9A-F]{2})").unwrap());

pub type RpressResult<E = RpressError> = Result<ResponsePayload, E>;
pub type Handler = Box<
    dyn Fn(RequestPayload) -> Pin<Box<dyn Future<Output = RpressResult> + Send + 'static>>
        + Send
        + Sync,
>;

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

#[derive(Debug)]
pub struct RequestMetadata {
    pub method: String,
    pub uri: String,
    pub(crate) query_path: String,
    pub http_method: String,
    pub headers: HashMap<String, String>,
}

#[derive(Debug)]
pub struct RequestPayload {
    pub request_metadata: Option<RequestMetadata>,
    pub payload: Vec<u8>,
    pub params: HashMap<String, String>,
    pub query: HashMap<String, String>
}

#[derive(Debug)]
pub(crate) enum HttpVerbs {
    GET,
    POST,
    DELETE,
    PUT,
    PATCH,
}

impl From<&str> for HttpVerbs {
    fn from(method: &str) -> HttpVerbs {
        match method {
            "delete" => HttpVerbs::DELETE,
            "patch" => HttpVerbs::PATCH,
            "post" => HttpVerbs::POST,
            "put" => HttpVerbs::PUT,
            "get" => HttpVerbs::GET,
            &_ => panic!("Unknown http method: {}", method),
        }
    }
}

impl From<HttpVerbs> for String {
    fn from(verb: HttpVerbs) -> String {
        match verb {
            HttpVerbs::DELETE => String::from("DELETE"),
            HttpVerbs::GET => String::from("GET"),
            HttpVerbs::POST => String::from("POST"),
            HttpVerbs::PUT => String::from("PUT"),
            HttpVerbs::PATCH => String::from("PATCH"),
        }
    }
}

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

#[derive(Debug)]
pub enum StatusCode {
    Continue = 100,
    SwitchingProtocols = 101,
    Ok = 200,
    Created = 201,
    Accepted = 202,
    NonAuthoritativeInformation = 203,
    NoContent = 204,
    ResetContent = 205,
    PartialContent = 206,
    MultiStatus = 207,
    AlreadyReported = 208,
    ImaTeapot = 418,
    UnprocessableEntity = 422,
    Forbidden = 403,
    NotFound = 404,
    MethodNotAllowed = 405,
    NotAcceptable = 406,
    ProxyAuthenticationRequired = 407,
    RequestTimeout = 408,
    Conflict = 409,
    Gone = 410,
    LengthRequired = 411,
    PreconditionFailed = 412,
    PayloadTooLarge = 413,
    UriTooLong = 414,
    UnsupportedMediaType = 415,
    RangeNotSatisfiable = 416,
    ExpectationFailed = 417,
    Locked = 423,
    FailedDependency = 424,
    UnorderedCollection = 425,
    UpgradeRequired = 426,
    PreconditionRequired = 428,
    TooManyRequests = 429,
    RequestHeaderFieldsTooLarge = 431,
    InternalServerError = 500,
    NotImplemented = 501,
    BadGateway = 502,
    ServiceUnavailable = 503,
    GatewayTimeout = 504,
    HttpVersionNotSupported = 505,
    VariantAlsoNegotiates = 506,
    InsufficientStorage = 507,
    LoopDetected = 508,
    NotExtended = 510,
    NetworkAuthenticationRequired = 511,
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
            StatusCode::Ok => String::from("Ok"),
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
            StatusCode::ImaTeapot => String::from("I'm a Teapot"),
            StatusCode::UnprocessableEntity => String::from("Unprocessable Entity"),
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
