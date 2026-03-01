use std::{collections::HashMap, sync::Arc};
use tokio::io::AsyncReadExt;

#[derive(Debug)]
pub struct Route {
    pub name: String,
}

#[derive(Debug)]
pub struct RequestMetadata {
    pub method: String,
    pub uri: String,
    pub http_method: String,
    pub headers: HashMap<String, String>,
}

#[derive(Debug)]
pub struct Request {
    pub request_metadata: Option<RequestMetadata>,
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

    pub fn parse_http_protocol(
        &self,
        buffer: &[u8],
        is_chunk: bool,
    ) -> Result<Option<(Request, usize)>, &'static str> {
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
        let mut total_consumed = 0;

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

            request_metadata = Some(RequestMetadata {
                uri: request_line_content.get(1).unwrap().to_owned(),
                method: request_line_content.get(0).unwrap().to_owned(),
                http_method: request_line_content.get(2).unwrap().to_owned(),
                headers: header_map,
            });
            payload = buffer[body_start..body_end].to_vec();
            total_consumed = body_end;
        } else {
            let hexsize_bytes = buffer.windows(2).position(|p| p == b"\r\n");
            let hexline_bytes = if let Some(value) = hexsize_bytes {
                value
            } else {
                return Err("Hex position dot found on chunk");
            };

            let decimal_bytes = match buffer[..hexline_bytes].iter().position(|b| &[*b] == b";") {
                Some(position) => position,
                None => hexline_bytes,
            };

            let decimal_size =
                match usize::from_str_radix(&String::from_utf8_lossy(&buffer[..decimal_bytes]), 16)
                {
                    Ok(decimal) => decimal,
                    Err(err) => {
                        println!("Error in parse hex value: {:?}", err);
                        0
                    }
                };

            let start = hexline_bytes + 2;
            let end = start + decimal_size;
            let payload_chunk = &buffer[start..end];

            println!(
                "decilmal size: {:?}",
                String::from_utf8_lossy(payload_chunk)
            );
        }

        Ok(Some((
            Request {
                request_metadata,
                payload,
            },
            total_consumed,
        )))
    }

    pub async fn server<T: Into<String>>(self: Arc<Self>, listener: T) -> anyhow::Result<()> {
        let listener = tokio::net::TcpListener::bind(listener.into()).await?;

        loop {
            let (mut socket, _) = listener.accept().await?;

            tokio::spawn({
                let thread_self = self.clone();

                async move {
                    let max_capacity = 40096;
                    let mut buffer: Vec<u8> = Vec::with_capacity(4096);
                    let mut temp_buffer = [0; 1024];

                    let chunk_header = b"Transfer-Encoding: chunked";
                    let mut is_chunked = false;

                    loop {
                        loop {
                            if buffer.len() == 0 {
                                break;
                            }

                            if is_chunked == false {
                                if let Some(_) = buffer
                                    .windows(chunk_header.len())
                                    .position(|b| b == chunk_header)
                                {
                                    is_chunked = true;
                                }
                            }

                            match thread_self.parse_http_protocol(&buffer, is_chunked) {
                                Ok(Some((request, consumed))) => {
                                    dbg!("{:?}", request);
                                    buffer.drain(..consumed);
                                }
                                Ok(None) => {
                                    println!("[POSSIBLE_MTU]: Incomplete message");
                                    break;
                                }
                                Err(err) => {
                                    println!("Error: {}", err);
                                    break;
                                }
                            }
                        }

                        let n = match socket.read(&mut temp_buffer).await {
                            Ok(0) => break,
                            Ok(n) => n,
                            Err(e) => {
                                println!("Error in read socket: {}", e);
                                break;
                            }
                        };

                        buffer.extend_from_slice(&temp_buffer[..n]);

                        if buffer.len() > max_capacity {
                            println!("Buffer capacity overflowed");
                            break;
                        }
                    }
                }
            });
        }
    }
}
