//! Socket.IO v5 protocol: packet types, encoding/decoding.

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
    ///
    /// Format: `<type>[<attachments>-][<namespace>,][<id>][JSON data]`
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

        // Parse binary attachment count (e.g. "51-" -> attachments=1)
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
                pos += 1; // skip '-'
            }
        }

        // Parse namespace (starts with '/')
        let namespace = if pos < chars.len() && chars[pos] == '/' {
            let start = pos;
            while pos < chars.len() && chars[pos] != ',' {
                pos += 1;
            }
            let ns = input[start..pos].to_string();
            if pos < chars.len() && chars[pos] == ',' {
                pos += 1; // skip ','
            }
            ns
        } else {
            "/".to_string()
        };

        // Parse ack id (digits before JSON)
        let id = if pos < chars.len() && chars[pos].is_ascii_digit() {
            let start = pos;
            while pos < chars.len() && chars[pos].is_ascii_digit() {
                pos += 1;
            }
            // Only treat as ack id if what follows is '[' or '{' or end of string
            if pos >= chars.len()
                || chars[pos] == '['
                || chars[pos] == '{'
            {
                Some(input[start..pos].parse::<u64>().unwrap_or(0))
            } else {
                // Not an ack id, rewind
                pos = start;
                None
            }
        } else {
            None
        };

        // Remaining is JSON data
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

    /// Creates a CONNECT response with the socket ID.
    pub fn connect_ok(namespace: &str, sid: &str) -> Self {
        Self {
            packet_type: SioPacketType::Connect,
            namespace: namespace.to_string(),
            data: Some(serde_json::json!({ "sid": sid })),
            id: None,
            attachment_count: 0,
        }
    }

    /// Creates a CONNECT_ERROR packet.
    pub fn connect_error(namespace: &str, message: &str) -> Self {
        Self {
            packet_type: SioPacketType::ConnectError,
            namespace: namespace.to_string(),
            data: Some(serde_json::json!({ "message": message })),
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

    /// Creates an ACK packet.
    pub fn ack(namespace: &str, id: u64, data: &Value) -> Self {
        let payload = if data.is_array() {
            data.clone()
        } else {
            Value::Array(vec![data.clone()])
        };
        Self {
            packet_type: SioPacketType::Ack,
            namespace: namespace.to_string(),
            data: Some(payload),
            id: Some(id),
            attachment_count: 0,
        }
    }

    /// Extracts the event name from an EVENT packet's data array.
    pub fn event_name(&self) -> Option<&str> {
        self.data
            .as_ref()?
            .as_array()?
            .first()?
            .as_str()
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
        assert!(decoded.data.is_none());
    }

    #[test]
    fn test_connect_custom_namespace() {
        let pkt = SioPacket::connect_ok("/admin", "oSO0OpakMV_3jnilAAAA");
        let encoded = pkt.encode();
        assert_eq!(encoded, "0/admin,{\"sid\":\"oSO0OpakMV_3jnilAAAA\"}");

        let decoded = SioPacket::decode(&encoded).unwrap();
        assert_eq!(decoded.packet_type, SioPacketType::Connect);
        assert_eq!(decoded.namespace, "/admin");
        assert!(decoded.data.is_some());
    }

    #[test]
    fn test_connect_error() {
        let pkt = SioPacket::connect_error("/", "Not authorized");
        let encoded = pkt.encode();
        assert_eq!(encoded, "4{\"message\":\"Not authorized\"}");

        let decoded = SioPacket::decode(&encoded).unwrap();
        assert_eq!(decoded.packet_type, SioPacketType::ConnectError);
    }

    #[test]
    fn test_event_main_namespace() {
        let pkt = SioPacket::event("/", "foo", &Value::Null, None);
        let encoded = pkt.encode();
        assert_eq!(encoded, "2[\"foo\",null]");

        let decoded = SioPacket::decode(&encoded).unwrap();
        assert_eq!(decoded.packet_type, SioPacketType::Event);
        assert_eq!(decoded.event_name(), Some("foo"));
    }

    #[test]
    fn test_event_custom_namespace() {
        let pkt = SioPacket::event("/admin", "bar", &serde_json::json!("baz"), None);
        let encoded = pkt.encode();
        assert_eq!(encoded, "2/admin,[\"bar\",\"baz\"]");

        let decoded = SioPacket::decode(&encoded).unwrap();
        assert_eq!(decoded.namespace, "/admin");
        assert_eq!(decoded.event_name(), Some("bar"));
    }

    #[test]
    fn test_event_with_ack() {
        let pkt = SioPacket::event("/", "foo", &serde_json::json!("bar"), Some(12));
        let encoded = pkt.encode();
        assert_eq!(encoded, "212[\"foo\",\"bar\"]");

        let decoded = SioPacket::decode(&encoded).unwrap();
        assert_eq!(decoded.id, Some(12));
        assert_eq!(decoded.event_name(), Some("foo"));
    }

    #[test]
    fn test_ack() {
        let pkt = SioPacket::ack("/admin", 13, &serde_json::json!(["bar"]));
        let encoded = pkt.encode();
        assert_eq!(encoded, "3/admin,13[\"bar\"]");

        let decoded = SioPacket::decode(&encoded).unwrap();
        assert_eq!(decoded.packet_type, SioPacketType::Ack);
        assert_eq!(decoded.namespace, "/admin");
        assert_eq!(decoded.id, Some(13));
    }

    #[test]
    fn test_disconnect() {
        let pkt = SioPacket::disconnect("/");
        assert_eq!(pkt.encode(), "1");

        let pkt2 = SioPacket::disconnect("/admin");
        assert_eq!(pkt2.encode(), "1/admin,");
    }

    #[test]
    fn test_binary_event_decode() {
        let decoded = SioPacket::decode("51-[\"baz\",{\"_placeholder\":true,\"num\":0}]").unwrap();
        assert_eq!(decoded.packet_type, SioPacketType::BinaryEvent);
        assert_eq!(decoded.attachment_count, 1);
        assert_eq!(decoded.event_name(), Some("baz"));
    }

    #[test]
    fn test_event_data_extraction() {
        let decoded = SioPacket::decode("2[\"hello\",\"world\",42]").unwrap();
        let args = decoded.event_data().unwrap();
        assert_eq!(args.len(), 2);
        assert_eq!(args[0], Value::String("world".into()));
        assert_eq!(args[1], serde_json::json!(42));
    }
}
