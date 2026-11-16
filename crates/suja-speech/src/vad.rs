use std::path::Path;

use sherpa_onnx::{SileroVadModelConfig, VadModelConfig, VoiceActivityDetector};

use crate::SPEECH_SAMPLE_RATE;
use crate::error::SpeechError;
use crate::models::require;

const WINDOW_SAMPLES: usize = 512;

#[derive(Clone, Copy, Debug)]
pub struct VadOptions {
    pub threshold: f32,
    pub min_silence_seconds: f32,
    pub min_speech_seconds: f32,
    pub max_speech_seconds: f32,
}

impl Default for VadOptions {
    fn default() -> Self {
        Self {
            threshold: 0.5,
            min_silence_seconds: 0.3,
            min_speech_seconds: 0.2,
            max_speech_seconds: 30.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum VadEvent {
    SpeechStart,
    SpeechEnd { samples: Vec<f32> },
}

pub struct Vad {
    detector: VoiceActivityDetector,
    window: Vec<f32>,
    speaking: bool,
}

impl Vad {
    pub fn new(model: &Path, options: VadOptions) -> Result<Self, SpeechError> {
        let config = VadModelConfig {
            silero_vad: SileroVadModelConfig {
                model: Some(require(model)?),
                threshold: options.threshold,
                min_silence_duration: options.min_silence_seconds,
                min_speech_duration: options.min_speech_seconds,
                max_speech_duration: options.max_speech_seconds,
                window_size: WINDOW_SAMPLES as i32,
            },
            sample_rate: SPEECH_SAMPLE_RATE,
            num_threads: 1,
            ..Default::default()
        };
        let detector = VoiceActivityDetector::create(&config, options.max_speech_seconds + 5.0)
            .ok_or_else(|| SpeechError::Runtime("silero vad failed to load".into()))?;
        Ok(Self {
            detector,
            window: Vec::with_capacity(WINDOW_SAMPLES),
            speaking: false,
        })
    }

    pub fn accept(&mut self, samples: &[f32]) -> Vec<VadEvent> {
        let mut events = Vec::new();
        for sample in samples {
            self.window.push(*sample);
            if self.window.len() == WINDOW_SAMPLES {
                self.detector.accept_waveform(&self.window);
                self.window.clear();
                self.collect(&mut events);
            }
        }
        events
    }

    pub fn flush(&mut self) -> Vec<VadEvent> {
        self.detector.flush();
        let mut events = Vec::new();
        self.collect(&mut events);
        events
    }

    pub fn reset(&mut self) {
        self.detector.reset();
        self.window.clear();
        self.speaking = false;
    }

    fn collect(&mut self, events: &mut Vec<VadEvent>) {
        let detected = self.detector.detected();
        if detected && !self.speaking {
            events.push(VadEvent::SpeechStart);
        }
        while let Some(segment) = self.detector.front() {
            events.push(VadEvent::SpeechEnd {
                samples: segment.samples().to_vec(),
            });
            self.detector.pop();
        }
        self.speaking = detected;
    }
}
