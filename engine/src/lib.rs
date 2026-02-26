use std::{collections::HashMap, sync::Arc};
use tokio::io::AsyncReadExt;

#[derive(Debug)]
pub struct Route {
    pub name: String,
}

#[derive(Debug)]
pub struct Request {
    pub method: String,
    pub uri: String,
    pub http_method: String,
    pub headers: HashMap<String, String>,
    pub payload: Vec<u8>,
}

pub struct Rpress {
    pub routes: Vec<Route>,
}

impl Rpress {
    pub fn build() -> Arc<Self> {
        Arc::new(Self { routes: vec![] })
    }

    pub fn add_route<T: Into<String>>(self: &mut Arc<Self>, name: T) -> () {
        if let Some(rpress) = Arc::get_mut(self) {
            rpress.routes.push(Route { name: name.into() });
        }
    }

    pub fn parse_http_protocol(&self, buffer: &[u8]) -> Result<Option<Request>, &'static str> {
        let rq_line = buffer.windows(2).position(|b| b == b"\r\n");
        let rq_bytes = if let Some(val) = rq_line {
            val
        } else {
            return Err("Invalid request line");
        };

        let mut request_line = buffer[..rq_bytes]
            .split(|&b| [b] == *b" ")
            .collect::<Vec<&[u8]>>();

        let request_line_content = request_line
            .iter_mut()
            .map(|v| String::from_utf8_lossy(v).into_owned())
            .collect::<Vec<String>>();

        if request_line_content.len() < 3 {
            return Err("Invalid request line");
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
            // the request body is incomplete
            return Ok(None);
        }

        Ok(Some(Request {
            uri: request_line_content.get(1).unwrap().to_owned(),
            method: request_line_content.get(0).unwrap().to_owned(),
            http_method: request_line_content.get(2).unwrap().to_owned(),
            headers: header_map,
            payload: buffer[body_start..body_end].to_vec(),
        }))
    }

    pub async fn server<T: Into<String>>(self: Arc<Self>, listener: T) -> anyhow::Result<()> {
        let listener = tokio::net::TcpListener::bind(listener.into()).await?;

        loop {
            let (mut socket, _) = listener.accept().await?;

            tokio::spawn({
                let thread_self = self.clone();

                async move {
                    let mut buffer: Vec<u8> = Vec::with_capacity(4096);
                    let mut temp_buffer = [0; 1024];

                    loop {
                        let n = socket.read(&mut temp_buffer).await.unwrap();
                        if n == 0 {
                            break;
                        }

                        buffer.extend_from_slice(&temp_buffer[..n]);

                        match thread_self.parse_http_protocol(&buffer) {
                            Ok(parsed_message) => {
                                if let Some(request) = parsed_message {
                                    dbg!("{:?}", request);
                                } else {
                                    println!("Incomplete request waiting for the end")
                                }
                            }
                            Err(err) => println!("Error in parsing http content: {}", err),
                        }
                    }
                }
            });
        }
    }
}
