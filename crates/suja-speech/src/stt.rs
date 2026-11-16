use std::path::Path;

use sherpa_onnx::{OnlineRecognizer, OnlineRecognizerConfig, OnlineStream};

use crate::SPEECH_SAMPLE_RATE;
use crate::error::SpeechError;
use crate::models::require;

const TAIL_PADDING_SAMPLES: usize = (SPEECH_SAMPLE_RATE as usize) * 4 / 5;
const HEAD_PADDING_SAMPLES: usize = (SPEECH_SAMPLE_RATE as usize) * 3 / 10;

#[derive(Clone, Debug)]
pub struct RecognizerOptions {
    pub threads: i32,
    pub quantized: bool,
    pub endpoint_trailing_silence_seconds: f32,
    pub endpoint_trailing_silence_after_text_seconds: f32,
    pub endpoint_max_utterance_seconds: f32,
}

impl Default for RecognizerOptions {
    fn default() -> Self {
        Self {
            threads: 2,
            quantized: true,
            endpoint_trailing_silence_seconds: 2.4,
            endpoint_trailing_silence_after_text_seconds: 0.6,
            endpoint_max_utterance_seconds: 20.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SttEvent {
    Partial(String),
    Endpoint(String),
}

pub struct Recognizer {
    inner: OnlineRecognizer,
}

impl Recognizer {
    pub fn new(model_dir: &Path, options: &RecognizerOptions) -> Result<Self, SpeechError> {
        let mut config = OnlineRecognizerConfig::default();
        config.model_config.transducer.encoder =
            Some(transducer_file(model_dir, "encoder", options.quantized)?);
        config.model_config.transducer.decoder =
            Some(transducer_file(model_dir, "decoder", options.quantized)?);
        config.model_config.transducer.joiner =
            Some(transducer_file(model_dir, "joiner", options.quantized)?);
        config.model_config.tokens = Some(require(&model_dir.join("tokens.txt"))?);
        config.model_config.num_threads = options.threads;
        config.model_config.provider = Some("cpu".into());
        config.decoding_method = Some("greedy_search".into());
        config.enable_endpoint = true;
        config.rule1_min_trailing_silence = options.endpoint_trailing_silence_seconds;
        config.rule2_min_trailing_silence = options.endpoint_trailing_silence_after_text_seconds;
        config.rule3_min_utterance_length = options.endpoint_max_utterance_seconds;
        let inner = OnlineRecognizer::create(&config)
            .ok_or_else(|| SpeechError::Runtime("streaming recognizer failed to load".into()))?;
        Ok(Self { inner })
    }

    fn padded_stream(&self) -> OnlineStream {
        let stream = self.inner.create_stream();
        stream.accept_waveform(SPEECH_SAMPLE_RATE, &vec![0.0f32; HEAD_PADDING_SAMPLES]);
        stream
    }

    pub fn transcription(&self) -> Transcription<'_> {
        Transcription {
            recognizer: self,
            stream: self.padded_stream(),
            committed: String::new(),
            last_text: String::new(),
        }
    }
}

pub struct Transcription<'r> {
    recognizer: &'r Recognizer,
    stream: OnlineStream,
    committed: String,
    last_text: String,
}

impl Transcription<'_> {
    pub fn feed(&mut self, samples: &[f32]) -> Vec<SttEvent> {
        self.stream.accept_waveform(SPEECH_SAMPLE_RATE, samples);
        self.drain(false)
    }

    pub fn finish(&mut self) -> String {
        let tail = vec![0.0f32; TAIL_PADDING_SAMPLES];
        self.stream.accept_waveform(SPEECH_SAMPLE_RATE, &tail);
        self.stream.input_finished();
        let events = self.drain(true);
        let text = events
            .into_iter()
            .next_back()
            .map(|event| match event {
                SttEvent::Endpoint(text) | SttEvent::Partial(text) => text,
            })
            .unwrap_or_default();
        self.stream = self.recognizer.padded_stream();
        self.committed.clear();
        self.last_text.clear();
        text
    }

    pub fn reset(&mut self) {
        self.stream = self.recognizer.padded_stream();
        self.committed.clear();
        self.last_text.clear();
    }

    fn drain(&mut self, force_endpoint: bool) -> Vec<SttEvent> {
        let recognizer = &self.recognizer.inner;
        let mut events = Vec::new();
        while recognizer.is_ready(&self.stream) {
            recognizer.decode(&self.stream);
        }
        let current = recognizer
            .get_result(&self.stream)
            .map(|result| result.text.trim().to_owned())
            .unwrap_or_default();
        let text = join_words(&self.committed, &current);
        if force_endpoint {
            if !text.is_empty() {
                events.push(SttEvent::Endpoint(text));
            }
            return events;
        }
        if recognizer.is_endpoint(&self.stream) {
            self.committed = text.clone();
            recognizer.reset(&self.stream);
            if text != self.last_text {
                events.push(SttEvent::Partial(text.clone()));
                self.last_text = text;
            }
        } else if text != self.last_text {
            events.push(SttEvent::Partial(text.clone()));
            self.last_text = text;
        }
        events
    }
}

fn transducer_file(model_dir: &Path, part: &str, quantized: bool) -> Result<String, SpeechError> {
    let wanted_suffix = if quantized { ".int8.onnx" } else { ".onnx" };
    let entries = std::fs::read_dir(model_dir)
        .map_err(|e| SpeechError::MissingModel(format!("{}: {e}", model_dir.display())))?;
    let mut candidates: Vec<String> = entries
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.starts_with(part) && name.ends_with(wanted_suffix))
        .filter(|name| quantized || !name.ends_with(".int8.onnx"))
        .collect();
    candidates.sort();
    let name = candidates.into_iter().next().ok_or_else(|| {
        SpeechError::MissingModel(format!("{part}*{wanted_suffix} in {}", model_dir.display()))
    })?;
    require(&model_dir.join(name))
}

fn join_words(committed: &str, current: &str) -> String {
    match (committed.is_empty(), current.is_empty()) {
        (true, _) => current.to_owned(),
        (false, true) => committed.to_owned(),
        (false, false) => format!("{committed} {current}"),
    }
}
