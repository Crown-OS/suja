use sherpa_onnx::LinearResampler;

use crate::error::SpeechError;

pub struct Resampler {
    inner: LinearResampler,
}

impl Resampler {
    pub fn new(input_rate: i32, output_rate: i32) -> Result<Self, SpeechError> {
        LinearResampler::create(input_rate, output_rate)
            .map(|inner| Self { inner })
            .ok_or_else(|| SpeechError::Runtime(format!("resampler {input_rate} -> {output_rate}")))
    }

    pub fn process(&self, samples: &[f32]) -> Vec<f32> {
        self.inner.resample(samples, false)
    }

    pub fn flush(&self) -> Vec<f32> {
        self.inner.resample(&[], true)
    }

    pub fn reset(&self) {
        self.inner.reset();
    }
}
