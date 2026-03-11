use std::collections::HashMap;

use crate::types::definitions::{HeadersResponse, StatusCode};
use chrono::{DateTime, Utc};
use tokio::{io::AsyncWriteExt, net::TcpStream};

pub struct Response<'a> {
    socket: &'a mut TcpStream,
    headers: HashMap<String, String>,
}

impl<'a> Response<'a> {
    pub fn new(socket: &'a mut TcpStream) -> Self {
        Self {
            socket,
            headers: HashMap::default(),
        }
    }

    fn get_http_data(&self) -> String {
        let now: DateTime<Utc> = Utc::now();
        now.format("%a, %d %b %Y %H:%M:%S GMT").to_string()
    }

    fn write_headers<V: Into<String>>(&mut self, index: HeadersResponse, value: V) -> () {
        self.headers.insert(index.into(), value.into());
    }

    fn build_response(&mut self, protocol: String, body: Vec<u8>) -> Vec<u8> {
        let body_bytes_len = body.len();
        self.write_headers(HeadersResponse::ContentLength, format!("{}", body.len()));

        let mut response_buffer = protocol.into_bytes();

        for (key, value) in self.headers.iter() {
            let header_line = format!("{}: {}\r\n", key, value);
            response_buffer.extend_from_slice(header_line.as_bytes());
        }

        response_buffer.extend_from_slice(b"\r\n");
        if body_bytes_len > 0 {
            response_buffer.extend_from_slice(&body);
        }

        response_buffer
    }

    pub async fn send_response(&mut self, status_code: StatusCode) -> () {
        let message = String::from(&status_code);
        let code = u16::from(status_code);
        let status_code_line = format!("HTTP/1.1 {} {}\r\n", code, message);

        self.write_headers(HeadersResponse::Date, self.get_http_data());
        self.write_headers(HeadersResponse::Server, "Rpress/1.0");
        self.write_headers(HeadersResponse::ContentType, "text/plain; charset=utf-8");
        self.write_headers(HeadersResponse::Connection, "keep-alive");

        let protocol_response = self.build_response(status_code_line, vec![]);
        self.socket.write_all(&protocol_response).await.unwrap();
        self.socket.flush().await.unwrap();
        self.headers.clear();
    }
}
