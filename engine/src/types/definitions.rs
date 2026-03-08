use std::{collections::HashMap, pin::Pin, sync::LazyLock};

use regex::Regex;

pub static HTTP_METHOD_REG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\:(.*)\/").unwrap());

pub type Handler =
    Box<dyn Fn(RequestPayload) -> Pin<Box<dyn Future<Output = ()> + Send + 'static>> + Send + Sync>;

#[derive(Debug)]
pub struct RequestMetadata {
    pub method: String,
    pub uri: String,
    pub http_method: String,
    pub headers: HashMap<String, String>,
}

#[derive(Debug)]
pub struct RequestPayload {
    pub request_metadata: Option<RequestMetadata>,
    pub payload: Vec<u8>,
}

#[derive(Debug)]
pub enum HttpVerbs {
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

impl Into<u16> for StatusCode {
    fn into(self) -> u16 {
        self as u16
    }
}

impl From<StatusCode> for String {
    fn from(status: StatusCode) -> Self {
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
