use std::collections::HashMap;

use crate::types::definitions::{RequestMetadata, RequestPayload};
pub struct Request;

impl Request {
    pub fn new() -> Self {
        Request
    }

    pub fn parse_http_protocol(
        &self,
        buffer: &[u8],
        is_chunk: bool,
    ) -> Result<Option<(RequestPayload, usize)>, &'static str> {
        let mut parse_only_chunk = false;
        let rq_line = buffer.windows(2).position(|b| b == b"\r\n");

        if rq_line.is_some() && rq_line.unwrap() < 3
            || String::from_utf8_lossy(&buffer[..rq_line.unwrap()]).contains("HTTP/1.1") == false
        {
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
            let mut content_lenght = 0;

            let mut header_map: HashMap<String, String> = HashMap::new();
            for header in headers {
                let data = header.split(": ").collect::<Vec<_>>();

                let index = data.get(0).unwrap().to_string();
                let value = data.get(1).unwrap().to_string();

                if index == "Content-Length" {
                    content_lenght = value.parse().unwrap();
                }

                header_map.insert(index, value);
            }

            let body_start = h_bytes + 4;
            let body_end = body_start + content_lenght;

            if buffer.len() < body_end {
                return Ok(None);
            }

            let complete_uri = request_line_content
                .get(1)
                .unwrap()
                .split("?")
                .collect::<Vec<&str>>();

            request_metadata = Some(RequestMetadata {
                uri: complete_uri
                    .get(0)
                    .map(|s| s.to_string())
                    .unwrap_or_default(),
                query_path: complete_uri
                    .get(1)
                    .map(|s| s.to_string())
                    .unwrap_or_default(),
                method: request_line_content.get(0).unwrap().to_owned(),
                http_method: request_line_content.get(2).unwrap().to_owned(),
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
    pub fn set_params(&mut self, params: HashMap<String, String>) -> () {
        self.params = params;
    }

    pub fn get_param(&self, index: &str) -> Result<String, &'static str> {
        match self.params.get(index) {
            Some(value) => Ok(value.to_owned()),
            None => Err("Param not found."),
        }
    }
}

