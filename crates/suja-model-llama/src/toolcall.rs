use crate::grammar::{TOOL_CALL_CLOSE, TOOL_CALL_OPEN};
use crate::stream::ParsedToolCall;

#[derive(Default)]
pub struct ToolCallParser {
    pending: String,
    inside_call: bool,
}

pub enum Parsed {
    Text(String),
    Call(Result<ParsedToolCall, serde_json::Error>),
    Nothing,
}

impl ToolCallParser {
    pub fn feed(&mut self, piece: &str) -> Parsed {
        self.pending.push_str(piece);
        if self.inside_call {
            return self.try_close();
        }
        if let Some(open) = self.pending.find(TOOL_CALL_OPEN) {
            let before = self.pending[..open].to_owned();
            self.pending.drain(..open + TOOL_CALL_OPEN.len());
            self.inside_call = true;
            return if before.is_empty() {
                self.try_close()
            } else {
                Parsed::Text(before)
            };
        }
        let keep = longest_prefix_tail(&self.pending, TOOL_CALL_OPEN);
        if keep == self.pending.len() {
            return Parsed::Nothing;
        }
        let emit: String = self.pending.drain(..self.pending.len() - keep).collect();
        Parsed::Text(emit)
    }

    pub fn flush(&mut self) -> Option<String> {
        if self.inside_call || self.pending.is_empty() {
            return None;
        }
        Some(std::mem::take(&mut self.pending))
    }

    fn try_close(&mut self) -> Parsed {
        let Some(close) = self.pending.find(TOOL_CALL_CLOSE) else {
            return Parsed::Nothing;
        };
        let body = self.pending[..close].trim().to_owned();
        self.pending.drain(..close + TOOL_CALL_CLOSE.len());
        self.inside_call = false;
        Parsed::Call(serde_json::from_str(&body))
    }
}

fn longest_prefix_tail(text: &str, marker: &str) -> usize {
    (1..marker.len().min(text.len()))
        .rev()
        .find(|&len| {
            text.is_char_boundary(text.len() - len) && marker.starts_with(&text[text.len() - len..])
        })
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect(pieces: &[&str]) -> (String, Option<ParsedToolCall>) {
        let mut parser = ToolCallParser::default();
        let mut text = String::new();
        let mut call = None;
        for piece in pieces {
            match parser.feed(piece) {
                Parsed::Text(t) => text.push_str(&t),
                Parsed::Call(Ok(c)) => call = Some(c),
                Parsed::Call(Err(e)) => panic!("bad json: {e}"),
                Parsed::Nothing => {}
            }
        }
        if let Some(rest) = parser.flush() {
            text.push_str(&rest);
        }
        (text, call)
    }

    #[test]
    fn plain_text_streams_through() {
        let (text, call) = collect(&["Hello", ", world", "."]);
        assert_eq!(text, "Hello, world.");
        assert!(call.is_none());
    }

    #[test]
    fn tool_call_split_across_pieces_is_parsed() {
        let (text, call) = collect(&[
            "Let me check. <tool",
            "_call>\n{\"name\": \"clock\", \"arguments\": {}}\n</tool_call>",
        ]);
        assert_eq!(text, "Let me check. ");
        assert_eq!(call.map(|c| c.name), Some("clock".to_owned()));
    }

    #[test]
    fn angle_bracket_that_is_not_a_tag_is_emitted() {
        let (text, _) = collect(&["a < b", " and c"]);
        assert_eq!(text, "a < b and c");
    }
}
