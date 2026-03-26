//! Engine.IO v4 protocol: packet types, encoding/decoding, session management, and heartbeat.

use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, Notify, RwLock};

/// Record separator used to concatenate packets in HTTP long-polling payloads.
pub(crate) const RECORD_SEPARATOR: char = '\x1e';

/// Engine.IO packet type identifiers (v4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EioPacketType {
    Open = 0,
    Close = 1,
    Ping = 2,
    Pong = 3,
    Message = 4,
    Upgrade = 5,
    Noop = 6,
}

impl EioPacketType {
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            b'0' => Some(Self::Open),
            b'1' => Some(Self::Close),
            b'2' => Some(Self::Ping),
            b'3' => Some(Self::Pong),
            b'4' => Some(Self::Message),
            b'5' => Some(Self::Upgrade),
            b'6' => Some(Self::Noop),
            _ => None,
        }
    }

    pub fn as_char(self) -> char {
        (b'0' + self as u8) as char
    }
}

/// A decoded Engine.IO packet.
#[derive(Debug, Clone)]
pub struct EioPacket {
    pub packet_type: EioPacketType,
    pub data: Option<String>,
}

impl EioPacket {
    pub fn new(packet_type: EioPacketType, data: Option<String>) -> Self {
        Self { packet_type, data }
    }

    /// Encodes a single packet to its wire format (e.g. `"4hello"`).
    pub fn encode(&self) -> String {
        let mut s = String::new();
        s.push(self.packet_type.as_char());
        if let Some(ref d) = self.data {
            s.push_str(d);
        }
        s
    }

    /// Decodes a single packet from its wire format.
    pub fn decode(input: &str) -> Option<Self> {
        let bytes = input.as_bytes();
        if bytes.is_empty() {
            return None;
        }
        let packet_type = EioPacketType::from_byte(bytes[0])?;
        let data = if bytes.len() > 1 {
            Some(input[1..].to_string())
        } else {
            None
        };
        Some(Self { packet_type, data })
    }

    /// Encodes multiple packets for HTTP long-polling (record-separator delimited).
    pub fn encode_polling_payload(packets: &[EioPacket]) -> String {
        packets
            .iter()
            .map(|p| p.encode())
            .collect::<Vec<_>>()
            .join(&RECORD_SEPARATOR.to_string())
    }

    /// Decodes an HTTP long-polling payload into individual packets.
    pub fn decode_polling_payload(input: &str) -> Vec<EioPacket> {
        input
            .split(RECORD_SEPARATOR)
            .filter_map(|s| EioPacket::decode(s))
            .collect()
    }
}

/// The JSON body sent in an Engine.IO `open` packet.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EioHandshake {
    pub sid: String,
    pub upgrades: Vec<String>,
    pub ping_interval: u64,
    pub ping_timeout: u64,
    pub max_payload: usize,
}

/// Active transport type for a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportType {
    Polling,
    WebSocket,
    Upgrading,
}

/// Engine.IO configuration values.
#[derive(Debug, Clone)]
pub struct EioConfig {
    pub ping_interval: Duration,
    pub ping_timeout: Duration,
    pub max_payload: usize,
    /// When `true`, the server rejects HTTP long-polling connections and only
    /// accepts WebSocket transport. This eliminates the need for sticky sessions
    /// in multi-instance deployments (Kubernetes, load balancers) because all
    /// communication happens over a single persistent WebSocket connection.
    ///
    /// Clients must connect with `{ transports: ["websocket"] }`.
    /// Defaults to `false` (both polling and WebSocket are accepted).
    pub websocket_only: bool,
}

impl Default for EioConfig {
    fn default() -> Self {
        Self {
            ping_interval: Duration::from_secs(25),
            ping_timeout: Duration::from_secs(20),
            max_payload: 1_000_000,
            websocket_only: false,
        }
    }
}

/// State for a single Engine.IO session.
pub(crate) struct EioSession {
    pub sid: String,
    pub transport: TransportType,
    pub created_at: Instant,
    pub last_pong: Instant,
    /// Channel for sending Engine.IO packets *to* the client.
    pub tx: mpsc::Sender<EioPacket>,
    /// Signals a polling GET that data is available.
    pub poll_notify: Arc<Notify>,
    /// Buffer of packets waiting to be delivered via polling GET.
    pub poll_buffer: Arc<RwLock<Vec<EioPacket>>>,
    /// Set to true when the session is closed.
    pub closed: bool,
}

/// Manages all active Engine.IO sessions.
pub(crate) struct EioSessionStore {
    pub sessions: DashMap<String, EioSession>,
    pub config: EioConfig,
}

impl EioSessionStore {
    pub fn new(config: EioConfig) -> Self {
        Self {
            sessions: DashMap::new(),
            config,
        }
    }

    /// Creates a new session and returns `(sid, rx)` where `rx` receives packets
    /// destined for the WebSocket writer or polling buffer.
    pub fn create_session(&self) -> (String, mpsc::Receiver<EioPacket>) {
        let sid = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = mpsc::channel(64);
        let poll_notify = Arc::new(Notify::new());
        let poll_buffer = Arc::new(RwLock::new(Vec::new()));

        let session = EioSession {
            sid: sid.clone(),
            transport: TransportType::Polling,
            created_at: Instant::now(),
            last_pong: Instant::now(),
            tx,
            poll_notify,
            poll_buffer,
            closed: false,
        };
        self.sessions.insert(sid.clone(), session);
        (sid, rx)
    }

    pub fn get_handshake_data(&self, sid: &str) -> Option<EioHandshake> {
        self.sessions.get(sid).map(|_| EioHandshake {
            sid: sid.to_string(),
            upgrades: if self.config.websocket_only {
                vec![]
            } else {
                vec!["websocket".to_string()]
            },
            ping_interval: self.config.ping_interval.as_millis() as u64,
            ping_timeout: self.config.ping_timeout.as_millis() as u64,
            max_payload: self.config.max_payload,
        })
    }

    pub fn remove_session(&self, sid: &str) -> Option<EioSession> {
        self.sessions.remove(sid).map(|(_, s)| s)
    }

    pub fn mark_pong(&self, sid: &str) {
        if let Some(mut s) = self.sessions.get_mut(sid) {
            s.last_pong = Instant::now();
        }
    }

    pub fn set_transport(&self, sid: &str, transport: TransportType) {
        if let Some(mut s) = self.sessions.get_mut(sid) {
            s.transport = transport;
        }
    }

    pub fn is_valid(&self, sid: &str) -> bool {
        self.sessions.contains_key(sid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_packet_encode_decode() {
        let pkt = EioPacket::new(EioPacketType::Message, Some("hello".into()));
        assert_eq!(pkt.encode(), "4hello");

        let decoded = EioPacket::decode("4hello").unwrap();
        assert_eq!(decoded.packet_type, EioPacketType::Message);
        assert_eq!(decoded.data.as_deref(), Some("hello"));
    }

    #[test]
    fn test_packet_no_data() {
        let pkt = EioPacket::new(EioPacketType::Ping, None);
        assert_eq!(pkt.encode(), "2");

        let decoded = EioPacket::decode("2").unwrap();
        assert_eq!(decoded.packet_type, EioPacketType::Ping);
        assert!(decoded.data.is_none());
    }

    #[test]
    fn test_polling_payload() {
        let packets = vec![
            EioPacket::new(EioPacketType::Message, Some("hello".into())),
            EioPacket::new(EioPacketType::Ping, None),
            EioPacket::new(EioPacketType::Message, Some("world".into())),
        ];
        let encoded = EioPacket::encode_polling_payload(&packets);
        assert_eq!(encoded, "4hello\x1e2\x1e4world");

        let decoded = EioPacket::decode_polling_payload(&encoded);
        assert_eq!(decoded.len(), 3);
        assert_eq!(decoded[0].packet_type, EioPacketType::Message);
        assert_eq!(decoded[0].data.as_deref(), Some("hello"));
        assert_eq!(decoded[1].packet_type, EioPacketType::Ping);
        assert_eq!(decoded[2].data.as_deref(), Some("world"));
    }

    #[test]
    fn test_handshake_json() {
        let hs = EioHandshake {
            sid: "abc123".into(),
            upgrades: vec!["websocket".into()],
            ping_interval: 25000,
            ping_timeout: 20000,
            max_payload: 1000000,
        };
        let json = serde_json::to_string(&hs).unwrap();
        assert!(json.contains("\"sid\":\"abc123\""));
        assert!(json.contains("\"pingInterval\":25000"));
    }
}
