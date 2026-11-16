use encoding_rs::Decoder;
use llama_cpp_2::model::LlamaModel;
use llama_cpp_2::token::LlamaToken;

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

pub fn single_token_for(model: &LlamaModel, text: &str) -> Option<LlamaToken> {
    let tokens = model.vocab().tokenize(text.as_bytes(), false, true);
    match tokens.as_slice() {
        [token] => Some(*token),
        _ => None,
    }
}
