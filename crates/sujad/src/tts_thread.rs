use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc as std_mpsc;
use std::thread;

use anyhow::{Context, Result};
use suja_speech::Synthesizer;
use suja_speech::tts::SherpaSynthesizer;
use tokio::sync::mpsc;

use crate::audio::playback::{PlaybackControl, Player};

pub struct TtsJob {
    pub turn_id: u64,
    pub text: String,
}

#[derive(Debug)]
pub enum TtsEvent {
    FirstAudio { turn_id: u64 },
    Finished { turn_id: u64 },
    PlaybackDrained { turn_id: u64 },
    Failed { turn_id: u64, message: String },
}

#[derive(Clone)]
pub struct TtsHandle {
    jobs: std_mpsc::Sender<TtsJob>,
    current_turn: Arc<AtomicU64>,
    playback: PlaybackControl,
}

impl TtsHandle {
    pub fn speak(&self, turn_id: u64, text: String) {
        let _ = self.jobs.send(TtsJob { turn_id, text });
    }

    pub fn set_current_turn(&self, turn_id: u64) {
        self.current_turn.store(turn_id, Ordering::Release);
    }

    pub fn flush_playback(&self) {
        self.playback.flush();
    }

    pub fn playback_drained(&self) -> bool {
        self.playback.drained()
    }
}

pub fn spawn(
    synthesizer: SherpaSynthesizer,
    mut player: Player,
    events: mpsc::UnboundedSender<TtsEvent>,
) -> Result<TtsHandle> {
    let (jobs, inbox) = std_mpsc::channel::<TtsJob>();
    let current_turn = Arc::new(AtomicU64::new(0));
    let current = current_turn.clone();
    let playback = player.control();
    thread::Builder::new()
        .name("suja-tts".into())
        .spawn(move || {
            let (chunks_tx, chunks_rx) = std_mpsc::channel::<Vec<f32>>();
            let mut spoken_turn = None;
            if let Err(error) = synthesizer.synthesize("Okay.", Box::new(|_: &[f32]| false)) {
                tracing::warn!(%error, "synthesizer warm-up failed");
            }
            while let Ok(job) = inbox.recv() {
                if current.load(Ordering::Acquire) != job.turn_id {
                    continue;
                }
                let sender = chunks_tx.clone();
                let turn_id = job.turn_id;
                let gate = current.clone();
                let result = synthesizer.synthesize(
                    &job.text,
                    Box::new(move |samples: &[f32]| {
                        let live = gate.load(Ordering::Acquire) == turn_id;
                        if live {
                            let _ = sender.send(samples.to_vec());
                        }
                        live
                    }),
                );
                while let Ok(chunk) = chunks_rx.try_recv() {
                    if current.load(Ordering::Acquire) != turn_id {
                        break;
                    }
                    if spoken_turn != Some(turn_id) {
                        spoken_turn = Some(turn_id);
                        let _ = events.send(TtsEvent::FirstAudio { turn_id });
                    }
                    player.enqueue(&chunk);
                }
                match result {
                    Ok(_) => {
                        let _ = events.send(TtsEvent::Finished { turn_id });
                    }
                    Err(error) => {
                        let _ = events.send(TtsEvent::Failed {
                            turn_id,
                            message: error.to_string(),
                        });
                    }
                }
            }
        })
        .context("spawn tts thread")?;
    Ok(TtsHandle {
        jobs,
        current_turn,
        playback,
    })
}
