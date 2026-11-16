use std::path::PathBuf;

use anyhow::{Context, Result};
use suja_speech::{Recognizer, RecognizerOptions, Resampler, SPEECH_SAMPLE_RATE, SttEvent};

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let model_dir = PathBuf::from(
        args.next()
            .context("usage: transcribe <stt model dir> <wav>")?,
    );
    let wav = args.next().context("wav path")?;
    let mut reader = hound::WavReader::open(&wav)?;
    let spec = reader.spec();
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => reader
            .samples::<i16>()
            .map(|s| s.map(|v| f32::from(v) / f32::from(i16::MAX)))
            .collect::<Result<_, _>>()?,
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>()?,
    };
    let resampler = Resampler::new(spec.sample_rate as i32, SPEECH_SAMPLE_RATE)?;
    let mut audio = resampler.process(&samples);
    audio.extend(resampler.flush());
    let recognizer = Recognizer::new(&model_dir, &RecognizerOptions::default())?;
    let mut transcription = recognizer.transcription();
    for pass in 1..=2 {
        let started = std::time::Instant::now();
        let mut partials = 0usize;
        for chunk in audio.chunks(160) {
            for event in transcription.feed(chunk) {
                if let SttEvent::Partial(text) = event {
                    partials += 1;
                    eprint!("\r{text:<80}");
                }
            }
        }
        let text = transcription.finish();
        eprintln!(
            "\npass {pass}: {text:?} ({partials} partials, {:?} for {:.2}s of audio)",
            started.elapsed(),
            audio.len() as f32 / SPEECH_SAMPLE_RATE as f32
        );
    }
    Ok(())
}
