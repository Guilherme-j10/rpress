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
            content_type: "text/plain",
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

pub struct RpressError {
    pub status: StatusCode,
    pub message: String,
}

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
