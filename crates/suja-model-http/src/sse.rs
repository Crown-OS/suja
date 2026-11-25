/// One server-sent event: its `event:` name, if any, and its joined `data:` lines.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ServerSentEvent {
    pub event: Option<String>,
    pub data: String,
}

/// Splits a byte stream into events. Bytes may break anywhere, also inside a UTF-8 character.
#[derive(Default)]
pub struct SseParser {
    pending: Vec<u8>,
    event: ServerSentEvent,
    has_data: bool,
}

impl SseParser {
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<ServerSentEvent> {
        self.pending.extend_from_slice(bytes);
        let mut events = Vec::new();
        while let Some(end) = self.pending.iter().position(|&byte| byte == b'\n') {
            let line: Vec<u8> = self.pending.drain(..=end).collect();
            let line = String::from_utf8_lossy(&line);
            let line = line.trim_end_matches(['\n', '\r']);
            if line.is_empty() {
                if self.has_data {
                    events.push(std::mem::take(&mut self.event));
                    self.has_data = false;
                }
                continue;
            }
            let (field, value) = line.split_once(':').unwrap_or((line, ""));
            let value = value.strip_prefix(' ').unwrap_or(value);
            match field {
                "event" => self.event.event = Some(value.to_owned()),
                "data" => {
                    if self.has_data {
                        self.event.data.push('\n');
                    }
                    self.event.data.push_str(value);
                    self.has_data = true;
                }
                _ => {}
            }
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_split_across_chunks_are_joined() {
        let mut parser = SseParser::default();
        assert!(parser.feed(b"event: ping\nda").is_empty());
        let events = parser.feed(b"ta: {\"a\":1}\r\n\r\ndata: [DONE]\n\n");
        assert_eq!(
            events,
            vec![
                ServerSentEvent {
                    event: Some("ping".into()),
                    data: "{\"a\":1}".into()
                },
                ServerSentEvent {
                    event: None,
                    data: "[DONE]".into()
                },
            ]
        );
    }

    #[test]
    fn a_character_split_between_chunks_stays_whole() {
        let mut parser = SseParser::default();
        let text = "data: é\n\n".as_bytes();
        assert!(parser.feed(&text[..7]).is_empty());
        assert_eq!(parser.feed(&text[7..])[0].data, "é");
    }

    #[test]
    fn comments_are_ignored() {
        let mut parser = SseParser::default();
        assert!(parser.feed(b": keep-alive\n\n").is_empty());
    }
}
