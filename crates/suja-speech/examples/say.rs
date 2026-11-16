use std::path::PathBuf;

use anyhow::{Context, Result};
use suja_speech::tts::SherpaSynthesizer;
use suja_speech::{Synthesizer, TtsBackend, TtsOptions};

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let backend = args.next().unwrap_or_else(|| "pocket".into());
    let model_dir = PathBuf::from(
        args.next()
            .context("usage: say <pocket|kokoro> <model dir> [text] [threads]")?,
    );
    let text = args
        .next()
        .unwrap_or_else(|| "Hello, this is Suja speaking.".into());
    let threads = args.next().and_then(|t| t.parse().ok()).unwrap_or(4);
    let backend = match backend.as_str() {
        "kokoro" => TtsBackend::Kokoro { speaker_id: 0 },
        _ => TtsBackend::Pocket {
            reference_voice: model_dir.join("test_wavs/bria.wav"),
        },
    };
    let synthesizer = SherpaSynthesizer::new(&TtsOptions {
        model_dir,
        backend,
        threads,
        speed: 1.0,
    })?;
    let started = std::time::Instant::now();
    let mut first = None;
    let pcm = synthesizer.synthesize(
        &text,
        Box::new(move |chunk: &[f32]| {
            if first.is_none() {
                first = Some(started.elapsed());
                eprintln!(
                    "first chunk of {} samples after {:?}",
                    chunk.len(),
                    started.elapsed()
                );
            }
            true
        }),
    )?;
    let seconds = pcm.samples.len() as f32 / pcm.sample_rate as f32;
    eprintln!(
        "{seconds:.2}s of audio in {:?} with {threads} threads",
        started.elapsed()
    );
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: pcm.sample_rate as u32,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create("/tmp/suja-say.wav", spec)?;
    for sample in &pcm.samples {
        writer.write_sample((sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16)?;
    }
    writer.finalize()?;
    Ok(())
}
