use std::io::Write;

use crate::types::definitions::{HeadersResponse, StatusCode};
use chrono::{DateTime, Utc};
use tokio::io::AsyncWriteExt;

const MIN_COMPRESS_SIZE: usize = 256;

const SKIP_COMPRESS_PREFIXES: &[&str] = &[
    "image/", "video/", "audio/", "application/zip", "application/gzip",
    "application/br", "application/zstd",
];

pub(crate) struct Response<'a, W: AsyncWriteExt + Unpin> {
    socket: &'a mut W,
    headers: Vec<(String, String)>,
}

impl<'a, W: AsyncWriteExt + Unpin> Response<'a, W> {
    pub(crate) fn new(socket: &'a mut W) -> Self {
        Self {
            socket,
            headers: Vec::new(),
        }
    }

    fn get_http_data(&self) -> String {
        let now: DateTime<Utc> = Utc::now();
        now.format("%a, %d %b %Y %H:%M:%S GMT").to_string()
    }

    fn write_headers<V: Into<String>>(&mut self, index: HeadersResponse, value: V) {
        let key: String = index.into();
        self.headers.push((key, value.into()));
    }

    fn sanitize_header(value: &str) -> String {
        value.chars().filter(|c| *c != '\r' && *c != '\n').collect()
    }

    fn should_compress(content_type: &str) -> bool {
        if content_type.starts_with("image/svg") {
            return true;
        }
        !SKIP_COMPRESS_PREFIXES
            .iter()
            .any(|prefix| content_type.starts_with(prefix))
    }

    fn compress_body(
        body: &[u8],
        accept_encoding: Option<&str>,
        content_type: &str,
    ) -> (Vec<u8>, Option<&'static str>) {
        let accept = match accept_encoding {
            Some(a) if body.len() >= MIN_COMPRESS_SIZE && Self::should_compress(content_type) => a,
            _ => return (body.to_vec(), None),
        };

        if accept.contains("br") {
            let mut output = Vec::new();
            {
                let mut compressor =
                    brotli::CompressorWriter::new(&mut output, 4096, 4, 22);
                let _ = compressor.write_all(body);
            }
            if !output.is_empty() {
                return (output, Some("br"));
            }
        }

        if accept.contains("gzip") {
            let mut encoder =
                flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
            let _ = encoder.write_all(body);
            if let Ok(compressed) = encoder.finish() {
                return (compressed, Some("gzip"));
            }
        }

        (body.to_vec(), None)
    }

    fn build_response(&mut self, protocol: String, body: Vec<u8>) -> Vec<u8> {
        self.write_headers(HeadersResponse::ContentLength, format!("{}", body.len()));

        let mut response_buffer = protocol.into_bytes();

        for (key, value) in self.headers.iter() {
            let safe_key = Self::sanitize_header(key);
            let safe_value = Self::sanitize_header(value);
            let header_line = format!("{}: {}\r\n", safe_key, safe_value);
            response_buffer.extend_from_slice(header_line.as_bytes());
        }

        response_buffer.extend_from_slice(b"\r\n");
        if !body.is_empty() {
            response_buffer.extend_from_slice(&body);
        }

        response_buffer
    }

    pub(crate) async fn send_response(
        &mut self,
        status_code: StatusCode,
        body: Vec<u8>,
        content_type: &str,
        extra_headers: &[(String, String)],
        accept_encoding: Option<&str>,
    ) -> std::io::Result<()> {
        let message = String::from(&status_code);
        let code = u16::from(status_code);
        let status_code_line = format!("HTTP/1.1 {} {}\r\n", code, message);

        self.write_headers(HeadersResponse::Date, self.get_http_data());
        self.write_headers(HeadersResponse::Server, "Rpress/1.0");
        self.write_headers(HeadersResponse::ContentType, content_type);
        self.write_headers(HeadersResponse::Connection, "keep-alive");
        self.write_headers(HeadersResponse::XContentTypeOptions, "nosniff");

        for (key, value) in extra_headers {
            self.headers.push((key.clone(), value.clone()));
        }

        let (final_body, encoding) = Self::compress_body(&body, accept_encoding, content_type);
        if let Some(enc) = encoding {
            self.write_headers(HeadersResponse::ContentEncoding, enc);
            self.headers.push(("Vary".into(), "Accept-Encoding".into()));
        }

        let protocol_response = self.build_response(status_code_line, final_body);
        self.socket.write_all(&protocol_response).await?;
        self.socket.flush().await?;
        self.headers.clear();

        Ok(())
    }
}
