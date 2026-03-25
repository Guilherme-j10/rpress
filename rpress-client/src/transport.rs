//! WebSocket transport layer for Engine.IO connections.

use anyhow::{Context, Result};
use futures_util::stream::SplitSink;
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};
use tracing::{debug, error, warn};
use url::Url;

use crate::engine_io::{EioHandshake, EioPacket, EioPacketType};

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;
type WsWriter = SplitSink<WsStream, Message>;

/// Converts an HTTP(S) URL to a WebSocket Engine.IO URL.
fn build_ws_url(server_url: &str, namespace: &str) -> Result<Url> {
    let mut url = Url::parse(server_url).context("invalid server URL")?;

    match url.scheme() {
        "http" => url.set_scheme("ws").unwrap(),
        "https" => url.set_scheme("wss").unwrap(),
        "ws" | "wss" => {}
        other => anyhow::bail!("unsupported scheme: {other}"),
    }

    url.set_path("/socket.io/");
    url.set_query(Some(&format!(
        "EIO=4&transport=websocket&nsp={}",
        namespace
    )));

    Ok(url)
}

/// Result of a successful WebSocket connection: Engine.IO handshake data
/// plus channels for sending/receiving packets.
pub(crate) struct WsTransportHandle {
    pub handshake: EioHandshake,
    pub tx: mpsc::Sender<EioPacket>,
    pub rx: mpsc::Receiver<EioPacket>,
}

/// Establishes a WebSocket connection, performs the Engine.IO handshake,
/// and spawns reader/writer background tasks.
pub(crate) async fn connect(server_url: &str, namespace: &str) -> Result<WsTransportHandle> {
    let url = build_ws_url(server_url, namespace)?;
    debug!("connecting to {url}");

    let (ws_stream, _response) = connect_async(url.as_str())
        .await
        .context("WebSocket connection failed")?;

    let (writer, mut reader) = ws_stream.split();

    let first_msg = reader
        .next()
        .await
        .context("connection closed before handshake")?
        .context("WebSocket error during handshake")?;

    let text = match first_msg {
        Message::Text(t) => t.to_string(),
        other => anyhow::bail!("expected text frame for handshake, got: {other:?}"),
    };

    let eio_pkt =
        EioPacket::decode(&text).context("failed to decode Engine.IO handshake packet")?;
    if eio_pkt.packet_type != EioPacketType::Open {
        anyhow::bail!(
            "expected Engine.IO Open packet, got {:?}",
            eio_pkt.packet_type
        );
    }

    let handshake: EioHandshake = serde_json::from_str(
        eio_pkt.data.as_deref().context("Open packet has no data")?,
    )
    .context("failed to parse Engine.IO handshake JSON")?;

    debug!(sid = %handshake.sid, "Engine.IO handshake complete");

    let (outgoing_tx, outgoing_rx) = mpsc::channel::<EioPacket>(64);
    let (incoming_tx, incoming_rx) = mpsc::channel::<EioPacket>(64);

    let ping_interval = handshake.ping_interval;

    tokio::spawn(writer_task(writer, outgoing_rx));
    tokio::spawn(reader_task(reader, incoming_tx, outgoing_tx.clone(), ping_interval));

    Ok(WsTransportHandle {
        handshake,
        tx: outgoing_tx,
        rx: incoming_rx,
    })
}

/// Background task: drains the outgoing channel and writes to the WebSocket.
async fn writer_task(mut writer: WsWriter, mut rx: mpsc::Receiver<EioPacket>) {
    while let Some(pkt) = rx.recv().await {
        let text = pkt.encode();
        if let Err(e) = writer.send(Message::Text(text.into())).await {
            error!("WebSocket write error: {e}");
            break;
        }
    }
    debug!("writer task exiting");
}

/// Background task: reads WebSocket frames, auto-responds to pings,
/// and forwards message packets to the incoming channel.
async fn reader_task(
    mut reader: futures_util::stream::SplitStream<WsStream>,
    incoming_tx: mpsc::Sender<EioPacket>,
    outgoing_tx: mpsc::Sender<EioPacket>,
    _ping_interval: u64,
) {
    while let Some(result) = reader.next().await {
        let msg = match result {
            Ok(m) => m,
            Err(e) => {
                warn!("WebSocket read error: {e}");
                break;
            }
        };

        let text = match msg {
            Message::Text(t) => t.to_string(),
            Message::Close(_) => {
                debug!("WebSocket close frame received");
                break;
            }
            Message::Ping(data) => {
                // tungstenite handles pong automatically, but just in case
                debug!("received WS ping ({} bytes)", data.len());
                continue;
            }
            _ => continue,
        };

        let Some(pkt) = EioPacket::decode(&text) else {
            warn!("failed to decode EIO packet: {text}");
            continue;
        };

        match pkt.packet_type {
            EioPacketType::Ping => {
                let pong = if let Some(ref d) = pkt.data {
                    EioPacket::new(EioPacketType::Pong, Some(d.clone()))
                } else {
                    EioPacket::new(EioPacketType::Pong, None)
                };
                if outgoing_tx.send(pong).await.is_err() {
                    break;
                }
            }
            EioPacketType::Close => {
                debug!("Engine.IO close received");
                break;
            }
            EioPacketType::Noop => {}
            _ => {
                if incoming_tx.send(pkt).await.is_err() {
                    break;
                }
            }
        }
    }

    debug!("reader task exiting");
}
