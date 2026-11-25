use encoding_rs::Decoder;

pub struct PieceDecoder {
    decoder: Decoder,
}

impl Default for PieceDecoder {
    fn default() -> Self {
        Self {
            decoder: encoding_rs::UTF_8.new_decoder(),
        }
    }
}

impl PieceDecoder {
    pub fn decode(&mut self, bytes: &[u8]) -> String {
        let capacity = self
            .decoder
            .max_utf8_buffer_length(bytes.len())
            .unwrap_or(bytes.len());
        let mut text = String::with_capacity(capacity);
        let _ = self.decoder.decode_to_string(bytes, &mut text, false);
        text
    }
}
