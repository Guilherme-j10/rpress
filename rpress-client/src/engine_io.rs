//! Engine.IO v4 packet types, encoding/decoding, and handshake data.

use serde::Deserialize;

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
}

/// The JSON body received in an Engine.IO `open` packet.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct EioHandshake {
    pub sid: String,
    pub upgrades: Vec<String>,
    pub ping_interval: u64,
    pub ping_timeout: u64,
    pub max_payload: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_decode_message() {
        let pkt = EioPacket::new(EioPacketType::Message, Some("hello".into()));
        assert_eq!(pkt.encode(), "4hello");

        let decoded = EioPacket::decode("4hello").unwrap();
        assert_eq!(decoded.packet_type, EioPacketType::Message);
        assert_eq!(decoded.data.as_deref(), Some("hello"));
    }

    #[test]
    fn test_encode_decode_ping() {
        let pkt = EioPacket::new(EioPacketType::Ping, None);
        assert_eq!(pkt.encode(), "2");

        let decoded = EioPacket::decode("2").unwrap();
        assert_eq!(decoded.packet_type, EioPacketType::Ping);
        assert!(decoded.data.is_none());
    }

    #[test]
    fn test_handshake_deserialize() {
        let json = r#"{"sid":"abc","upgrades":["websocket"],"pingInterval":25000,"pingTimeout":20000,"maxPayload":1000000}"#;
        let hs: EioHandshake = serde_json::from_str(json).unwrap();
        assert_eq!(hs.sid, "abc");
        assert_eq!(hs.ping_interval, 25000);
    }
}
