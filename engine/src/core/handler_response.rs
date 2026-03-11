use crate::types::definitions::StatusCode;

pub struct ResponsePayload {
    pub status: StatusCode,
    pub body: Vec<u8>,
    pub content_type: &'static str,
}

impl ResponsePayload {
    pub fn text<T: Into<String>>(content: T) -> Self {
        Self {
            status: StatusCode::Ok,
            body: content.into().into_bytes(),
            content_type: "text/plain; charset=utf-8",
        }
    }

    pub fn json<T: serde::Serialize>(data: &T) -> Self {
        let body = serde_json::to_vec(data).unwrap_or_default();
        Self {
            status: StatusCode::Ok,
            body,
            content_type: "application/json",
        }
    }

    pub fn with_status(mut self, status: StatusCode) -> Self {
        self.status = status;
        self
    }
}

pub struct RpressError {
    pub status: StatusCode,
    pub message: String,
}

impl From<serde_json::Error> for RpressError {
    fn from(err: serde_json::Error) -> Self {
        Self {
            status: StatusCode::InternalServerError,
            message: err.to_string(),
        }
    }
}
