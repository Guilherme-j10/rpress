//! Socket.IO v5 packet types, encoding/decoding.

use serde_json::Value;

/// Socket.IO packet type identifiers (protocol v5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SioPacketType {
    Connect = 0,
    Disconnect = 1,
    Event = 2,
    Ack = 3,
    ConnectError = 4,
    BinaryEvent = 5,
    BinaryAck = 6,
}

impl SioPacketType {
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            b'0' => Some(Self::Connect),
            b'1' => Some(Self::Disconnect),
            b'2' => Some(Self::Event),
            b'3' => Some(Self::Ack),
            b'4' => Some(Self::ConnectError),
            b'5' => Some(Self::BinaryEvent),
            b'6' => Some(Self::BinaryAck),
            _ => None,
        }
    }

    pub fn as_char(self) -> char {
        (b'0' + self as u8) as char
    }
}

/// A decoded Socket.IO packet.
#[derive(Debug, Clone)]
pub struct SioPacket {
    pub packet_type: SioPacketType,
    pub namespace: String,
    pub data: Option<Value>,
    pub id: Option<u64>,
    pub attachment_count: usize,
}

impl SioPacket {
    /// Encodes a Socket.IO packet to its wire format.
    pub fn encode(&self) -> String {
        let mut s = String::new();
        s.push(self.packet_type.as_char());

        if self.attachment_count > 0 {
            s.push_str(&self.attachment_count.to_string());
            s.push('-');
        }

        if self.namespace != "/" {
            s.push_str(&self.namespace);
            s.push(',');
        }

        if let Some(id) = self.id {
            s.push_str(&id.to_string());
        }

        if let Some(ref data) = self.data {
            s.push_str(&data.to_string());
        }

        s
    }

    /// Decodes a Socket.IO packet from its wire format.
    pub fn decode(input: &str) -> Option<Self> {
        let bytes = input.as_bytes();
        if bytes.is_empty() {
            return None;
        }

        let packet_type = SioPacketType::from_byte(bytes[0])?;
        let mut pos = 1;
        let chars: Vec<char> = input.chars().collect();

        let mut attachment_count = 0;
        if matches!(
            packet_type,
            SioPacketType::BinaryEvent | SioPacketType::BinaryAck
        ) {
            let start = pos;
            while pos < chars.len() && chars[pos].is_ascii_digit() {
                pos += 1;
            }
            if pos < chars.len() && chars[pos] == '-' {
                attachment_count = input[start..pos].parse().unwrap_or(0);
                pos += 1;
            }
        }

        let namespace = if pos < chars.len() && chars[pos] == '/' {
            let start = pos;
            while pos < chars.len() && chars[pos] != ',' {
                pos += 1;
            }
            let ns = input[start..pos].to_string();
            if pos < chars.len() && chars[pos] == ',' {
                pos += 1;
            }
            ns
        } else {
            "/".to_string()
        };

        let id = if pos < chars.len() && chars[pos].is_ascii_digit() {
            let start = pos;
            while pos < chars.len() && chars[pos].is_ascii_digit() {
                pos += 1;
            }
            if pos >= chars.len() || chars[pos] == '[' || chars[pos] == '{' {
                Some(input[start..pos].parse::<u64>().unwrap_or(0))
            } else {
                pos = start;
                None
            }
        } else {
            None
        };

        let data = if pos < chars.len() {
            serde_json::from_str(&input[pos..]).ok()
        } else {
            None
        };

        Some(Self {
            packet_type,
            namespace,
            data,
            id,
            attachment_count,
        })
    }

    /// Creates a CONNECT packet for a namespace.
    pub fn connect(namespace: &str, data: Option<Value>) -> Self {
        Self {
            packet_type: SioPacketType::Connect,
            namespace: namespace.to_string(),
            data,
            id: None,
            attachment_count: 0,
        }
    }

    /// Creates a DISCONNECT packet.
    pub fn disconnect(namespace: &str) -> Self {
        Self {
            packet_type: SioPacketType::Disconnect,
            namespace: namespace.to_string(),
            data: None,
            id: None,
            attachment_count: 0,
        }
    }

    /// Creates an EVENT packet.
    pub fn event(namespace: &str, event: &str, data: &Value, ack_id: Option<u64>) -> Self {
        let mut arr = vec![Value::String(event.to_string())];
        if data.is_array() {
            if let Some(items) = data.as_array() {
                arr.extend(items.iter().cloned());
            }
        } else {
            arr.push(data.clone());
        }
        Self {
            packet_type: SioPacketType::Event,
            namespace: namespace.to_string(),
            data: Some(Value::Array(arr)),
            id: ack_id,
            attachment_count: 0,
        }
    }

    /// Extracts the event name from an EVENT packet's data array.
    pub fn event_name(&self) -> Option<&str> {
        self.data.as_ref()?.as_array()?.first()?.as_str()
    }

    /// Extracts the event arguments (everything after the event name).
    pub fn event_data(&self) -> Option<Vec<Value>> {
        let arr = self.data.as_ref()?.as_array()?;
        if arr.len() > 1 {
            Some(arr[1..].to_vec())
        } else {
            Some(vec![])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connect_main_namespace() {
        let pkt = SioPacket::connect("/", None);
        assert_eq!(pkt.encode(), "0");

        let decoded = SioPacket::decode("0").unwrap();
        assert_eq!(decoded.packet_type, SioPacketType::Connect);
        assert_eq!(decoded.namespace, "/");
    }

    #[test]
    fn test_event_encode_decode() {
        let pkt = SioPacket::event("/", "hello", &serde_json::json!("world"), None);
        let encoded = pkt.encode();
        assert_eq!(encoded, "2[\"hello\",\"world\"]");

        let decoded = SioPacket::decode(&encoded).unwrap();
        assert_eq!(decoded.event_name(), Some("hello"));
    }

    #[test]
    fn test_event_with_ack() {
        let pkt = SioPacket::event("/", "greet", &serde_json::json!("Rpress"), Some(7));
        let encoded = pkt.encode();
        assert_eq!(encoded, "27[\"greet\",\"Rpress\"]");

        let decoded = SioPacket::decode(&encoded).unwrap();
        assert_eq!(decoded.id, Some(7));
        assert_eq!(decoded.event_name(), Some("greet"));
    }

    #[test]
    fn test_connect_response_decode() {
        let decoded = SioPacket::decode("0{\"sid\":\"abc123\"}").unwrap();
        assert_eq!(decoded.packet_type, SioPacketType::Connect);
        assert_eq!(decoded.namespace, "/");
        let binding = decoded.data.unwrap();
        let sid = binding["sid"].as_str().unwrap();
        assert_eq!(sid, "abc123");
    }

    #[test]
    fn test_disconnect() {
        let pkt = SioPacket::disconnect("/");
        assert_eq!(pkt.encode(), "1");

        let pkt2 = SioPacket::disconnect("/admin");
        assert_eq!(pkt2.encode(), "1/admin,");
    }

    #[test]
    fn test_ack_decode() {
        let decoded = SioPacket::decode("37[\"hello, Rpress!\"]").unwrap();
        assert_eq!(decoded.packet_type, SioPacketType::Ack);
        assert_eq!(decoded.id, Some(7));
    }
}
