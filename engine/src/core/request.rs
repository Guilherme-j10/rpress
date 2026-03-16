use std::{borrow::Cow, collections::HashMap};

use crate::types::definitions::{PERCENT_ENCODING, RequestMetadata, RequestPayload};
pub(crate) struct Request;

impl Request {
    pub(crate) fn new() -> Self {
        Request
    }

    pub(crate) fn parse_http_protocol(
        &self,
        buffer: &[u8],
        is_chunk: bool,
    ) -> Result<Option<(RequestPayload, usize)>, &'static str> {
        let mut parse_only_chunk = false;
        let rq_line = buffer.windows(2).position(|b| b == b"\r\n");

        let is_valid_request_line = match rq_line {
            Some(pos) if pos >= 3 => {
                String::from_utf8_lossy(&buffer[..pos]).contains("HTTP/1.1")
            }
            Some(_) => false,
            None => false,
        };

        if !is_valid_request_line {
            if is_chunk {
                parse_only_chunk = true;
            } else {
                return Err("Request line possibly malformed");
            }
        }

        let mut request_metadata: Option<RequestMetadata> = None;
        let mut payload = vec![];
        let total_consumed: usize;

        if parse_only_chunk == false && is_chunk || parse_only_chunk == false && is_chunk == false {
            let rq_bytes = if let Some(request_bytes) = rq_line {
                request_bytes
            } else {
                return Err("Request line not found");
            };

            let mut request_line = buffer[..rq_bytes]
                .split(|&b| [b] == *b" ")
                .collect::<Vec<&[u8]>>();

            let request_line_content = request_line
                .iter_mut()
                .map(|v| String::from_utf8_lossy(v).into_owned())
                .collect::<Vec<String>>();

            if request_line_content.len() < 3 {
                return Err("Invalid request line size");
            }

            let h_lines = buffer.windows(4).position(|b| b == b"\r\n\r\n");
            let h_bytes = if let Some(val) = h_lines {
                val
            } else {
                return Err("Invalid headers");
            };

            let header_lines = &buffer[rq_bytes + 2..h_bytes];
            let headers_str = String::from_utf8_lossy(header_lines).to_owned();
            let headers = headers_str.split("\r\n").collect::<Vec<&str>>();
            let mut content_length = 0;

            let mut header_map: HashMap<String, String> = HashMap::new();
            for header in headers {
                let (key, value) = match header.split_once(": ") {
                    Some((k, v)) => (k.to_string(), v.to_string()),
                    None => continue,
                };

                if key == "Content-Length" {
                    content_length = value.parse().unwrap_or(0);
                }

                header_map.insert(key, value);
            }

            let body_start = h_bytes + 4;
            let body_end = body_start + content_length;

            if buffer.len() < body_end {
                return Ok(None);
            }

            let raw_uri = &request_line_content[1];
            let (uri, query_path) = match raw_uri.split_once("?") {
                Some((path, qs)) => (path.to_string(), qs.to_string()),
                None => (raw_uri.to_string(), String::new()),
            };

            request_metadata = Some(RequestMetadata {
                uri,
                query_path,
                method: request_line_content[0].clone(),
                http_method: request_line_content[2].clone(),
                headers: header_map,
            });
            payload = buffer[body_start..body_end].to_vec();
            total_consumed = body_end;
        } else {
            let mut cursor: usize = 0;
            let mut accumulator: Vec<u8> = vec![];

            loop {
                let relative_hex_end = match buffer[cursor..].windows(2).position(|p| p == b"\r\n")
                {
                    Some(pos) => pos,
                    None => break,
                };

                let hex_line_start = cursor;
                let hex_line_end = cursor + relative_hex_end;

                let hex_content_end = buffer[hex_line_start..hex_line_end]
                    .iter()
                    .position(|&b| b == b';')
                    .map(|pos| hex_line_start + pos)
                    .unwrap_or(hex_line_end);

                let hex_str = String::from_utf8_lossy(&buffer[hex_line_start..hex_content_end]);
                let decimal_size = match usize::from_str_radix(hex_str.trim(), 16) {
                    Ok(size) => size,
                    Err(_) => break,
                };

                if decimal_size == 0 {
                    cursor = hex_line_end + 4;
                    break;
                }

                let data_start = hex_line_end + 2;
                let data_end = data_start + decimal_size;

                if buffer.len() < data_end + 2 {
                    break;
                }

                accumulator.extend_from_slice(&buffer[data_start..data_end]);
                cursor = data_end + 2;
            }

            total_consumed = cursor;
            payload = accumulator
        }

        Ok(Some((
            RequestPayload {
                request_metadata,
                payload,
                params: HashMap::default(),
                query: HashMap::default(),
            },
            total_consumed,
        )))
    }
}

impl RequestPayload {
    pub(crate) fn set_params(&mut self, params: HashMap<String, String>) -> () {
        self.params = params;
    }

    pub fn get_param(&self, index: &str) -> Result<String, &'static str> {
        match self.params.get(index) {
            Some(value) => Ok(value.to_owned()),
            None => Err("Param not found."),
        }
    }

    fn parse_query<'a>(&'a self) -> Vec<(&'a str, Cow<'a, str>)> {
        if let Some(ref meta) = self.request_metadata {
            let mut key_value: Vec<(&str, Cow<str>)> = vec![];
            let mut encodes: Vec<&str> = vec![];
            let mut bytes: Vec<u8> = vec![];

            for query in meta.query_path.split("&") {
                let decoded = match query.split_once("=") {
                    Some((key, raw_value)) => {
                        let mut final_value = Cow::Borrowed(raw_value);

                        if raw_value.contains("%") {
                            for (_, [percent]) in PERCENT_ENCODING
                                .captures_iter(&raw_value)
                                .map(|c| c.extract::<1>())
                            {
                                if !encodes.contains(&percent) {
                                    encodes.push(percent);
                                }
                            }

                            for encode in encodes.iter() {
                                bytes.extend(
                                    encode
                                        .split("%")
                                        .filter(|f| !f.is_empty())
                                        .map(|a| u8::from_str_radix(a, 16))
                                        .flatten(),
                                );

                                if let Ok(hex_string) = std::str::from_utf8(&bytes) {
                                    final_value =
                                        Cow::Owned(final_value.replace(encode, hex_string));
                                }

                                bytes.clear();
                            }
                        }

                        (key, final_value)
                    }
                    None => ("", Cow::from("")),
                };

                if !decoded.0.is_empty() {
                    key_value.push((decoded.0, decoded.1));
                }

                encodes.clear();
            }

            return key_value;
        }

        vec![]
    }

    pub fn get_query(&self, query: &str) -> Option<String> {
        if let Some(ref occ) = self.parse_query().iter().find(|f| f.0 == query) {
            return Some(occ.1.to_string());
        }

        None
    }
}
