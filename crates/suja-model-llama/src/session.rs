use std::num::NonZeroU32;

use llama_cpp_2::context::LlamaContext;
use llama_cpp_2::context::params::{KvCacheType, LlamaContextParams};
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::token::LlamaToken;
use tokio_util::sync::CancellationToken;

use crate::engine::Engine;
use crate::error::LlmError;
use suja_model::FinishReason;

use crate::stream::Delta;
use crate::toolcall::{Parsed, ToolCallParser};
use crate::util::tokens::PieceDecoder;

#[derive(Clone, Copy, Debug)]
pub struct SessionOptions {
    pub context_tokens: u32,
    pub batch_tokens: u32,
    pub threads: i32,
}

impl Default for SessionOptions {
    fn default() -> Self {
        Self {
            context_tokens: 8192,
            batch_tokens: 512,
            threads: 8,
        }
    }
}

pub struct Session<'engine> {
    engine: &'engine Engine,
    context: LlamaContext<'engine>,
    batch: LlamaBatch<'static>,
    position: i32,
    prefix_end: i32,
    options: SessionOptions,
}

impl<'engine> Session<'engine> {
    pub fn new(engine: &'engine Engine, options: SessionOptions) -> Result<Self, LlmError> {
        let params = LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(options.context_tokens))
            .with_n_batch(options.batch_tokens)
            .with_n_threads(options.threads)
            .with_n_threads_batch(options.threads)
            .with_type_k(KvCacheType::Q8_0)
            .with_type_v(KvCacheType::Q8_0)
            .with_offload_kqv(true);
        let context = engine
            .model()
            .new_context(engine.backend(), params)
            .map_err(|e| LlmError::Runtime(e.to_string()))?;
        let batch = LlamaBatch::new(options.batch_tokens as usize, 1);
        Ok(Self {
            engine,
            context,
            batch,
            position: 0,
            prefix_end: 0,
            options,
        })
    }

    pub fn prime(&mut self, prefix: &str) -> Result<(), LlmError> {
        self.reset_all();
        self.append_text(prefix, &CancellationToken::new())?;
        self.prefix_end = self.position;
        Ok(())
    }

    pub fn append_text(&mut self, text: &str, cancel: &CancellationToken) -> Result<(), LlmError> {
        let tokens = self
            .engine
            .model()
            .vocab()
            .tokenize(text.as_bytes(), false, true);
        self.append_tokens(&tokens, cancel)
    }

    pub fn tokens_in_context(&self) -> u32 {
        self.position.max(0) as u32
    }

    pub fn remaining_tokens(&self) -> u32 {
        self.options
            .context_tokens
            .saturating_sub(self.tokens_in_context())
    }

    pub fn reset_to_prefix(&mut self) {
        let _ = self
            .context
            .clear_kv_cache_seq(Some(0), Some(self.prefix_end.max(0) as u32), None);
        self.position = self.prefix_end;
    }

    fn reset_all(&mut self) {
        self.context.clear_kv_cache();
        self.position = 0;
        self.prefix_end = 0;
    }

    fn append_tokens(
        &mut self,
        tokens: &[LlamaToken],
        cancel: &CancellationToken,
    ) -> Result<(), LlmError> {
        if tokens.len() as u32 > self.remaining_tokens() {
            return Err(LlmError::ContextFull);
        }
        let chunk = self.options.batch_tokens as usize;
        let last_index = tokens.len().saturating_sub(1);
        for (chunk_start, chunk_tokens) in tokens
            .chunks(chunk)
            .enumerate()
            .map(|(i, c)| (i * chunk, c))
        {
            if cancel.is_cancelled() {
                return Err(LlmError::Cancelled);
            }
            self.batch.clear();
            for (offset, token) in chunk_tokens.iter().enumerate() {
                let is_last = chunk_start + offset == last_index;
                self.batch
                    .add(*token, self.position + offset as i32, &[0], is_last)
                    .map_err(|e| LlmError::Runtime(e.to_string()))?;
            }
            self.context
                .decode(&mut self.batch)
                .map_err(|e| LlmError::Runtime(e.to_string()))?;
            self.position += chunk_tokens.len() as i32;
        }
        Ok(())
    }

    pub fn generate(
        &mut self,
        sampler: &mut LlamaSampler,
        cancel: &CancellationToken,
        max_reply_tokens: usize,
        mut sink: impl FnMut(Delta),
    ) -> Result<FinishReason, LlmError> {
        let vocab = self.engine.model().vocab();
        let mut decoder = PieceDecoder::default();
        let mut parser = ToolCallParser::default();
        let mut produced = 0usize;
        loop {
            if cancel.is_cancelled() {
                return Ok(self.finish(FinishReason::Cancelled, &mut parser, &mut sink));
            }
            if produced >= max_reply_tokens || self.remaining_tokens() == 0 {
                return Ok(self.finish(FinishReason::LengthLimit, &mut parser, &mut sink));
            }
            let token = sampler.sample(&self.context, self.batch.n_tokens() - 1);
            if vocab.is_eog(token) {
                return Ok(self.finish(FinishReason::EndOfTurn, &mut parser, &mut sink));
            }
            let piece = decoder.decode(&vocab.token_to_piece(token, true, None));
            match parser.feed(&piece) {
                Parsed::Text(text) => sink(Delta::Text(text)),
                Parsed::Call(Ok(call)) => {
                    self.commit(token)?;
                    sink(Delta::ToolCall(call));
                    return Ok(FinishReason::ToolCall);
                }
                Parsed::Call(Err(error)) => {
                    return Err(LlmError::Runtime(format!("tool call json: {error}")));
                }
                Parsed::Nothing => {}
            }
            self.commit(token)?;
            produced += 1;
        }
    }

    fn commit(&mut self, token: LlamaToken) -> Result<(), LlmError> {
        self.batch.clear();
        self.batch
            .add(token, self.position, &[0], true)
            .map_err(|e| LlmError::Runtime(e.to_string()))?;
        self.context
            .decode(&mut self.batch)
            .map_err(|e| LlmError::Runtime(e.to_string()))?;
        self.position += 1;
        Ok(())
    }

    fn finish(
        &self,
        reason: FinishReason,
        parser: &mut ToolCallParser,
        sink: &mut impl FnMut(Delta),
    ) -> FinishReason {
        if let Some(rest) = parser.flush() {
            sink(Delta::Text(rest));
        }
        reason
    }
}
