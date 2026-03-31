use std::collections::HashMap;

use crate::types::definitions::{PERCENT_ENCODING, RequestMetadata, RequestPayload};

const MAX_REQUEST_LINE_SIZE: usize = 8192;
const MAX_HEADER_SIZE: usize = 8192;
const MAX_HEADERS_COUNT: usize = 100;
pub(crate) const DEFAULT_MAX_BODY_SIZE: usize = 10 * 1024 * 1024; // 10MB
const MAX_CHUNK_SIZE: usize = 1024 * 1024; // 1MB

pub(crate) struct HeadersParseResult {
    pub metadata: RequestMetadata,
    pub query: HashMap<String, String>,
    pub content_length: usize,
    pub is_chunked: bool,
    pub body_start: usize,
}

fn parse_head(buffer: &[u8], max_body_size: usize) -> Result<Option<HeadersParseResult>, &'static str> {
    let rq_line = buffer.windows(2).position(|b| b == b"\r\n");

    let rq_bytes = match rq_line {
        Some(pos) if pos >= 3 => {
            if pos > MAX_REQUEST_LINE_SIZE {
                return Err("Request line too long");
            }
            if !String::from_utf8_lossy(&buffer[..pos]).contains("HTTP/1.1") {
                return Err("Request line possibly malformed");
            }
            pos
        }
        Some(_) => return Err("Request line possibly malformed"),
        None => return Ok(None),
    };

    let h_bytes = match buffer.windows(4).position(|b| b == b"\r\n\r\n") {
        Some(val) => val,
        None => return Ok(None),
    };

    let header_section_size = h_bytes
        .checked_sub(rq_bytes + 2)
        .ok_or("Malformed header section")?;
    if header_section_size > MAX_HEADER_SIZE {
        return Err("Headers too large");
    }

    let request_line = buffer[..rq_bytes]
        .split(|&b| [b] == *b" ")
        .map(|v| String::from_utf8_lossy(v).into_owned())
        .collect::<Vec<String>>();

    if request_line.len() < 3 {
        return Err("Invalid request line size");
    }

    let header_lines = &buffer[rq_bytes + 2..h_bytes];
    let headers_str = String::from_utf8_lossy(header_lines);
    let mut header_map: HashMap<String, String> = HashMap::new();
    let mut header_count: usize = 0;
    let mut content_length: usize = 0;
    let mut has_content_length = false;
    let mut has_transfer_encoding = false;
    let mut is_chunked = false;

    for header in headers_str.split("\r\n") {
        let (key, value) = match header.split_once(": ") {
            Some((k, v)) => (k.to_lowercase(), v.to_string()),
            None => continue,
        };

        header_count += 1;
        if header_count > MAX_HEADERS_COUNT {
            return Err("Too many headers");
        }

        if key == "content-length" {
            content_length = value.parse().unwrap_or(0);
            has_content_length = true;
        }
        if key == "transfer-encoding" {
            has_transfer_encoding = true;
            is_chunked = value.to_lowercase().contains("chunked");
        }

        header_map
            .entry(key)
            .and_modify(|existing| {
                existing.push_str(", ");
                existing.push_str(&value);
            })
            .or_insert(value);
    }

    if has_content_length && has_transfer_encoding {
        return Err("Request contains both Content-Length and Transfer-Encoding");
    }

    if content_length > max_body_size {
        return Err("Content-Length exceeds maximum allowed size");
    }

    let raw_uri = &request_line[1];
    let (raw_path, query_path) = match raw_uri.split_once("?") {
        Some((path, qs)) => (path.to_string(), qs.to_string()),
        None => (raw_uri.to_string(), String::new()),
    };

    let uri = percent_decode(&raw_path);
    let query = parse_query_string(&query_path);

    let metadata = RequestMetadata {
        uri,
        query_path,
        method: request_line[0].clone(),
        http_method: request_line[2].clone(),
        headers: header_map,
    };

    Ok(Some(HeadersParseResult {
        metadata,
        query,
        content_length,
        is_chunked,
        body_start: h_bytes + 4,
    }))
}

fn parse_chunked_body(buffer: &[u8]) -> Result<Option<(Vec<u8>, usize)>, &'static str> {
    let mut cursor: usize = 0;
    let mut accumulator: Vec<u8> = vec![];

    while let Some(relative_hex_end) =
        buffer[cursor..].windows(2).position(|p| p == b"\r\n")
    {
        let hex_line_start = cursor;
        let hex_line_end = cursor + relative_hex_end;

        let hex_content_end = buffer[hex_line_start..hex_line_end]
            .iter()
            .position(|&b| b == b';')
            .map(|pos| hex_line_start + pos)
            .unwrap_or(hex_line_end);

        let hex_str = String::from_utf8_lossy(&buffer[hex_line_start..hex_content_end]);
        let decimal_size = match usize::from_str_radix(hex_str.trim(), 16) {
            Ok(size) if size <= MAX_CHUNK_SIZE => size,
            Ok(_) => return Err("Chunk size exceeds maximum"),
            Err(_) => break,
        };

        if decimal_size == 0 {
            cursor = hex_line_end.checked_add(4).ok_or("Chunk cursor overflow")?;
            break;
        }

        let data_start = hex_line_end.checked_add(2).ok_or("Chunk offset overflow")?;
        let data_end = data_start
            .checked_add(decimal_size)
            .ok_or("Chunk data_end overflow")?;

        let required_len = data_end.checked_add(2).ok_or("Chunk length overflow")?;
        if buffer.len() < required_len {
            break;
        }

        accumulator.extend_from_slice(&buffer[data_start..data_end]);
        cursor = data_end + 2;
    }

    Ok(Some((accumulator, cursor)))
}

pub(crate) fn parse_headers_only(
    buffer: &[u8],
    max_body_size: usize,
) -> Result<Option<HeadersParseResult>, &'static str> {
    parse_head(buffer, max_body_size)
}

pub(crate) fn parse_http_protocol(
    buffer: &[u8],
    is_chunk: bool,
    max_body_size: usize,
) -> Result<Option<(RequestPayload, usize)>, &'static str> {
    let has_valid_request_line = match buffer.windows(2).position(|b| b == b"\r\n") {
        Some(pos) if pos >= 3 => {
            if pos > MAX_REQUEST_LINE_SIZE {
                return Err("Request line too long");
            }
            String::from_utf8_lossy(&buffer[..pos]).contains("HTTP/1.1")
        }
        Some(_) => false,
        None => false,
    };

    if !has_valid_request_line && !is_chunk {
        return Err("Request line possibly malformed");
    }

    if !has_valid_request_line {
        let (payload, consumed) = match parse_chunked_body(buffer)? {
            Some(result) => result,
            None => return Ok(None),
        };

        return Ok(Some((
            RequestPayload {
                request_metadata: None,
                payload,
                params: HashMap::default(),
                query: HashMap::default(),
                extensions: HashMap::default(),
                body_receiver: None,
            },
            consumed,
        )));
    }

    let head = match parse_head(buffer, max_body_size)? {
        Some(h) => h,
        None => return Ok(None),
    };

    let body_end = head
        .body_start
        .checked_add(head.content_length)
        .ok_or("Content-Length overflow")?;

    if buffer.len() < body_end {
        return Ok(None);
    }

    let payload = buffer[head.body_start..body_end].to_vec();

    Ok(Some((
        RequestPayload {
            request_metadata: Some(head.metadata),
            payload,
            params: HashMap::default(),
            query: head.query,
            extensions: HashMap::default(),
            body_receiver: None,
        },
        body_end,
    )))
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut decoded_bytes = Vec::with_capacity(bytes.len());
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(byte) = u8::from_str_radix(&input[i + 1..i + 3], 16)
        {
            decoded_bytes.push(byte);
            i += 3;
            continue;
        }
        decoded_bytes.push(bytes[i]);
        i += 1;
    }

    String::from_utf8_lossy(&decoded_bytes).into_owned()
}

fn parse_query_string(query_path: &str) -> HashMap<String, String> {
    let mut result = HashMap::new();
    if query_path.is_empty() {
        return result;
    }

    for query in query_path.split("&") {
        if let Some((key, raw_value)) = query.split_once("=") {
            if key.is_empty() {
                continue;
            }

            let raw_value = raw_value.replace('+', " ");
            let mut final_value = raw_value.clone();

            if raw_value.contains("%") {
                let mut encodes: Vec<String> = vec![];
                let mut bytes: Vec<u8> = vec![];

                for (_, [percent]) in PERCENT_ENCODING
                    .captures_iter(&raw_value)
                    .map(|c| c.extract::<1>())
                {
                    let owned = percent.to_string();
                    if !encodes.contains(&owned) {
                        encodes.push(owned);
                    }
                }

                for encode in encodes.iter() {
                    bytes.extend(
                        encode
                            .split("%")
                            .filter(|f| !f.is_empty())
                            .flat_map(|a| u8::from_str_radix(a, 16)),
                    );

                    if let Ok(hex_string) = std::str::from_utf8(&bytes) {
                        final_value = final_value.replace(encode.as_str(), hex_string);
                    }

                    bytes.clear();
                }
            }

            result.insert(key.to_string(), final_value);
        }
    }

    result
}

impl RequestPayload {
    pub(crate) fn set_params(&mut self, params: HashMap<String, String>) {
        self.params = params;
    }

    /// Returns a route parameter value by name (e.g. `:id` in `/users/:id`).
    pub fn get_param(&self, key: &str) -> Option<&str> {
        self.params.get(key).map(|s| s.as_str())
    }

    /// Stores a key-value pair in the request extensions.
    ///
    /// Designed for middleware to attach data that downstream handlers can read.
    /// Typical use: an auth middleware validates a JWT and stores the extracted
    /// claims so that handlers can access them without re-parsing the token.
    ///
    /// ```ignore
    /// // In a middleware:
    /// req.set_extension("user_id", "42");
    /// req.set_extension("role", "admin");
    /// next(req).await
    ///
    /// // In a handler:
    /// let user_id = req.get_extension("user_id").unwrap();
    /// ```
    pub fn set_extension(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.extensions.insert(key.into(), value.into());
    }

    /// Returns an extension value previously set by middleware via [`set_extension`](Self::set_extension).
    pub fn get_extension(&self, key: &str) -> Option<&str> {
        self.extensions.get(key).map(|s| s.as_str())
    }

    /// Returns a query string parameter value by name.
    pub fn get_query(&self, key: &str) -> Option<&str> {
        self.query.get(key).map(|s| s.as_str())
    }

    /// Returns the decoded request URI path.
    pub fn uri(&self) -> &str {
        self.request_metadata
            .as_ref()
            .map(|m| m.uri.as_str())
            .unwrap_or("")
    }

    /// Returns the HTTP method (GET, POST, etc.).
    pub fn method(&self) -> &str {
        self.request_metadata
            .as_ref()
            .map(|m| m.method.as_str())
            .unwrap_or("")
    }

    /// Returns a request header value by name (case-insensitive lookup).
    pub fn header(&self, key: &str) -> Option<&str> {
        self.request_metadata
            .as_ref()
            .and_then(|m| m.headers.get(key))
            .map(|s| s.as_str())
    }

    /// Returns the request body as a UTF-8 string slice.
    pub fn body_str(&self) -> Result<&str, std::str::Utf8Error> {
        std::str::from_utf8(&self.payload)
    }

    /// Deserializes the request body from JSON into the given type.
    pub fn body_json<T: serde::de::DeserializeOwned>(&self) -> Result<T, serde_json::Error> {
        serde_json::from_slice(&self.payload)
    }

    /// Parses the Cookie header and returns all cookies as a key-value map.
    pub fn cookies(&self) -> HashMap<String, String> {
        let mut cookies = HashMap::new();
        if let Some(cookie_header) = self.header("cookie") {
            for pair in cookie_header.split(';') {
                let pair = pair.trim();
                if let Some((name, value)) = pair.split_once('=') {
                    cookies.insert(name.trim().to_string(), value.trim().to_string());
                }
            }
        }
        cookies
    }

    /// Collects the full request body, consuming either the buffered payload or the body stream.
    pub async fn collect_body(&mut self) -> Vec<u8> {
        if !self.payload.is_empty() {
            return std::mem::take(&mut self.payload);
        }
        if let Some(mut rx) = self.body_receiver.take() {
            let mut body = Vec::new();
            while let Some(chunk) = rx.recv().await {
                body.extend(chunk);
            }
            return body;
        }
        Vec::new()
    }

    /// Takes the body stream receiver for incremental chunk processing.
    pub fn body_stream(&mut self) -> Option<tokio::sync::mpsc::Receiver<Vec<u8>>> {
        self.body_receiver.take()
    }
}
