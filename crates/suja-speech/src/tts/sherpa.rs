use std::collections::HashMap;

use sherpa_onnx::{
    GenerationConfig, OfflineTts, OfflineTtsConfig, OfflineTtsKokoroModelConfig,
    OfflineTtsModelConfig, OfflineTtsPocketModelConfig, Wave,
};

use crate::error::SpeechError;
use crate::models::require;
use crate::tts::{ChunkSink, Pcm, Synthesizer, TtsBackend, TtsOptions};

pub struct SherpaSynthesizer {
    tts: OfflineTts,
    speed: f32,
    speaker_id: i32,
    reference: Option<(Vec<f32>, i32)>,
}

impl SherpaSynthesizer {
    pub fn new(options: &TtsOptions) -> Result<Self, SpeechError> {
        let dir = &options.model_dir;
        let path = |name: &str| require(&dir.join(name));
        let mut model = OfflineTtsModelConfig {
            num_threads: options.threads,
            provider: Some("cpu".into()),
            ..Default::default()
        };
        let mut speaker_id = 0;
        let mut reference = None;
        match &options.backend {
            TtsBackend::Pocket { reference_voice } => {
                let quantized = dir.join("lm_main.int8.onnx").exists();
                let variant = |stem: &str| {
                    if quantized {
                        format!("{stem}.int8.onnx")
                    } else {
                        format!("{stem}.onnx")
                    }
                };
                model.pocket = OfflineTtsPocketModelConfig {
                    lm_flow: Some(path(&variant("lm_flow"))?),
                    lm_main: Some(path(&variant("lm_main"))?),
                    encoder: Some(path("encoder.onnx")?),
                    decoder: Some(path(&variant("decoder"))?),
                    text_conditioner: Some(path("text_conditioner.onnx")?),
                    vocab_json: Some(path("vocab.json")?),
                    token_scores_json: Some(path("token_scores.json")?),
                    voice_embedding_cache_capacity: 8,
                };
                let wave = Wave::read(&require(reference_voice)?)
                    .ok_or_else(|| SpeechError::Runtime("reference voice wav unreadable".into()))?;
                reference = Some((wave.samples().to_vec(), wave.sample_rate()));
            }
            TtsBackend::Kokoro { speaker_id: id } => {
                speaker_id = *id;
                model.kokoro = OfflineTtsKokoroModelConfig {
                    model: Some(path("model.onnx")?),
                    voices: Some(path("voices.bin")?),
                    tokens: Some(path("tokens.txt")?),
                    data_dir: Some(path("espeak-ng-data")?),
                    ..Default::default()
                };
            }
        }
        let config = OfflineTtsConfig {
            model,
            max_num_sentences: 1,
            ..Default::default()
        };
        let tts = OfflineTts::create(&config)
            .ok_or_else(|| SpeechError::Runtime("text to speech failed to load".into()))?;
        Ok(Self {
            tts,
            speed: options.speed,
            speaker_id,
            reference,
        })
    }
}

impl Synthesizer for SherpaSynthesizer {
    fn sample_rate(&self) -> i32 {
        self.tts.sample_rate()
    }

    fn synthesize(&self, text: &str, mut on_chunk: ChunkSink) -> Result<Pcm, SpeechError> {
        let mut extra = HashMap::new();
        extra.insert("seed".to_owned(), serde_json::json!(7));
        extra.insert(
            "max_reference_audio_len".to_owned(),
            serde_json::json!(10.0),
        );
        let config = GenerationConfig {
            sid: self.speaker_id,
            speed: self.speed,
            reference_audio: self.reference.as_ref().map(|(samples, _)| samples.clone()),
            reference_sample_rate: self.reference.as_ref().map_or(0, |(_, rate)| *rate),
            extra: Some(extra),
            ..Default::default()
        };
        let audio = self
            .tts
            .generate_with_config(
                text,
                &config,
                Some(move |samples: &[f32], _progress: f32| on_chunk(samples)),
            )
            .ok_or_else(|| SpeechError::Runtime("synthesis failed".into()))?;
        Ok(Pcm {
            samples: audio.samples().to_vec(),
            sample_rate: audio.sample_rate(),
        })
    }
}
