use crate::types::definitions::StatusCode;

fn sanitize_header_value(value: &str) -> String {
    value.chars().filter(|c| *c != '\r' && *c != '\n').collect()
}

pub struct ResponsePayload {
    pub status: StatusCode,
    pub body: Vec<u8>,
    pub content_type: String,
    pub headers: Vec<(String, String)>,
}

impl ResponsePayload {
    pub fn text<T: Into<String>>(content: T) -> Self {
        Self {
            status: StatusCode::OK,
            body: content.into().into_bytes(),
            content_type: "text/plain; charset=utf-8".into(),
            headers: Vec::new(),
        }
    }

    pub fn html<T: Into<String>>(content: T) -> Self {
        Self {
            status: StatusCode::OK,
            body: content.into().into_bytes(),
            content_type: "text/html; charset=utf-8".into(),
            headers: Vec::new(),
        }
    }

    pub fn json<T: serde::Serialize>(data: &T) -> Result<Self, serde_json::Error> {
        let body = serde_json::to_vec(data)?;
        Ok(Self {
            status: StatusCode::OK,
            body,
            content_type: "application/json".into(),
            headers: Vec::new(),
        })
    }

    pub fn bytes(data: Vec<u8>, content_type: &str) -> Self {
        Self {
            status: StatusCode::OK,
            body: data,
            content_type: content_type.into(),
            headers: Vec::new(),
        }
    }

    pub fn empty() -> Self {
        Self {
            status: StatusCode::NoContent,
            body: vec![],
            content_type: "text/plain; charset=utf-8".into(),
            headers: Vec::new(),
        }
    }

    pub fn with_status(mut self, status: StatusCode) -> Self {
        self.status = status;
        self
    }

    pub fn with_content_type(mut self, ct: impl Into<String>) -> Self {
        self.content_type = ct.into();
        self
    }

    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((
            sanitize_header_value(&key.into()),
            sanitize_header_value(&value.into()),
        ));
        self
    }

    pub fn redirect(location: &str, status: StatusCode) -> Self {
        let safe_location: String = location
            .chars()
            .filter(|c| *c != '\r' && *c != '\n')
            .collect();

        Self {
            status,
            body: vec![],
            content_type: "text/plain; charset=utf-8".into(),
            headers: vec![("Location".to_string(), safe_location)],
        }
    }

    pub fn set_cookie(mut self, cookie: &CookieBuilder) -> Self {
        let mut parts = vec![format!("{}={}", cookie.name, cookie.value)];

        if let Some(ref path) = cookie.path {
            parts.push(format!("Path={}", path));
        }
        if let Some(max_age) = cookie.max_age {
            parts.push(format!("Max-Age={}", max_age));
        }
        if let Some(ref same_site) = cookie.same_site {
            parts.push(format!("SameSite={}", same_site));
        }
        if cookie.http_only {
            parts.push("HttpOnly".into());
        }
        if cookie.secure {
            parts.push("Secure".into());
        }
        if let Some(ref domain) = cookie.domain {
            parts.push(format!("Domain={}", domain));
        }

        let header_value = parts.join("; ");
        self.headers.push(("Set-Cookie".to_string(), header_value));
        self
    }
}

pub struct CookieBuilder {
    pub name: String,
    pub value: String,
    pub path: Option<String>,
    pub domain: Option<String>,
    pub max_age: Option<i64>,
    pub same_site: Option<String>,
    pub http_only: bool,
    pub secure: bool,
}

impl CookieBuilder {
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
            path: Some("/".into()),
            domain: None,
            max_age: None,
            same_site: Some("Lax".into()),
            http_only: true,
            secure: false,
        }
    }

    pub fn path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    pub fn domain(mut self, domain: impl Into<String>) -> Self {
        self.domain = Some(domain.into());
        self
    }

    pub fn max_age(mut self, seconds: i64) -> Self {
        self.max_age = Some(seconds);
        self
    }

    pub fn same_site(mut self, same_site: impl Into<String>) -> Self {
        self.same_site = Some(same_site.into());
        self
    }

    pub fn http_only(mut self, http_only: bool) -> Self {
        self.http_only = http_only;
        self
    }

    pub fn secure(mut self, secure: bool) -> Self {
        self.secure = secure;
        self
    }
}

pub trait IntoRpressResult {
    fn into_result(self) -> Result<ResponsePayload, RpressError>;
}

pub trait RpressErrorExt {
    fn into_rpress_error(self) -> (StatusCode, String);
}

impl IntoRpressResult for ResponsePayload {
    fn into_result(self) -> Result<ResponsePayload, RpressError> {
        Ok(self)
    }
}

impl IntoRpressResult for () {
    fn into_result(self) -> Result<ResponsePayload, RpressError> {
        Ok(ResponsePayload {
            status: StatusCode::Accepted,
            body: vec![],
            content_type: "text/plain".into(),
            headers: Vec::new(),
        })
    }
}

impl<E: RpressErrorExt> IntoRpressResult for E {
    fn into_result(self) -> Result<ResponsePayload, RpressError> {
        let (status, message) = self.into_rpress_error();
        Err(RpressError { status, message })
    }
}

impl<E: RpressErrorExt> IntoRpressResult for Result<ResponsePayload, E> {
    fn into_result(self) -> Result<ResponsePayload, RpressError> {
        self.map_err(|e| {
            let (status, message) = e.into_rpress_error();
            RpressError { status, message }
        })
    }
}

#[derive(Debug)]
pub struct RpressError {
    pub status: StatusCode,
    pub message: String,
}

impl std::fmt::Display for RpressError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for RpressError {}

impl RpressErrorExt for RpressError {
    fn into_rpress_error(self) -> (StatusCode, String) {
        (self.status, self.message)
    }
}

impl From<serde_json::Error> for RpressError {
    fn from(err: serde_json::Error) -> Self {
        Self {
            status: StatusCode::InternalServerError,
            message: err.to_string(),
        }
    }
}
