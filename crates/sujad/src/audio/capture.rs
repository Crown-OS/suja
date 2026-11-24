use std::sync::mpsc as std_mpsc;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use crownui_audio::{FrameConsumer, Microphone, MicrophoneOptions, MicrophoneStream};
use suja_speech::util::pcm::rms_level;
use suja_speech::{
    ModelPaths, Recognizer, RecognizerOptions, Resampler, SPEECH_SAMPLE_RATE, SttEvent,
};
use tokio::sync::mpsc;

const CAPTURE_RATE: i32 = 48_000;
const LEVEL_EVERY_FRAMES: usize = 5;
const RELEASE_TAIL: Duration = Duration::from_millis(300);

#[derive(Debug)]
pub enum SpeechEvent {
    Level(f32),
    Partial(String),
    Final(String),
    Failed(String),
}

pub enum ListenCommand {
    Start,
    Stop,
    Discard,
}

#[derive(Clone)]
pub struct ListenHandle {
    commands: std_mpsc::Sender<ListenCommand>,
}

impl ListenHandle {
    pub fn start(&self) {
        let _ = self.commands.send(ListenCommand::Start);
    }

    pub fn stop(&self) {
        let _ = self.commands.send(ListenCommand::Stop);
    }

    /// Closes the microphone without a transcript.
    pub fn discard(&self) {
        let _ = self.commands.send(ListenCommand::Discard);
    }
}

pub fn spawn(
    paths: &ModelPaths,
    events: mpsc::UnboundedSender<SpeechEvent>,
) -> Result<ListenHandle> {
    let recognizer = Recognizer::new(&paths.stt_dir, &RecognizerOptions::default())
        .context("load recognizer")?;
    let (commands, inbox) = std_mpsc::channel();
    thread::Builder::new()
        .name("suja-speech".into())
        .spawn(move || run(recognizer, inbox, events))
        .context("spawn speech thread")?;
    Ok(ListenHandle { commands })
}

struct Listening {
    _stream: MicrophoneStream,
    frames: FrameConsumer,
    resampler: Resampler,
    frame_count: usize,
}

fn open_microphone() -> Result<Listening> {
    let mut stream = Microphone::open(MicrophoneOptions::default()).context("open microphone")?;
    let frames = stream
        .take_frames()
        .context("microphone frames already taken")?;
    Ok(Listening {
        _stream: stream,
        frames,
        resampler: Resampler::new(CAPTURE_RATE, SPEECH_SAMPLE_RATE).context("resampler")?,
        frame_count: 0,
    })
}

fn run(
    recognizer: Recognizer,
    inbox: std_mpsc::Receiver<ListenCommand>,
    events: mpsc::UnboundedSender<SpeechEvent>,
) {
    let mut transcription = recognizer.transcription();
    let mut listening: Option<Listening> = None;
    loop {
        let command = match &listening {
            Some(_) => inbox.try_recv().ok(),
            None => match inbox.recv() {
                Ok(command) => Some(command),
                Err(_) => return,
            },
        };
        match command {
            Some(ListenCommand::Start) if listening.is_none() => match open_microphone() {
                Ok(mic) => {
                    transcription.reset();
                    listening = Some(mic);
                }
                Err(error) => {
                    let _ = events.send(SpeechEvent::Failed(error.to_string()));
                }
            },
            Some(ListenCommand::Stop) => {
                if let Some(mut mic) = listening.take() {
                    thread::sleep(RELEASE_TAIL);
                    drain(&mut mic, &mut transcription, &events);
                    let tail = mic.resampler.flush();
                    transcription.feed(&tail);
                    let text = transcription.finish();
                    let _ = events.send(SpeechEvent::Final(text));
                }
            }
            Some(ListenCommand::Discard) => listening = None,
            _ => {}
        }
        if let Some(mic) = listening.as_mut()
            && !drain(mic, &mut transcription, &events)
        {
            thread::sleep(Duration::from_millis(2));
        }
    }
}

fn drain(
    mic: &mut Listening,
    transcription: &mut suja_speech::Transcription<'_>,
    events: &mpsc::UnboundedSender<SpeechEvent>,
) -> bool {
    let mut any = false;
    while let Ok(frame) = mic.frames.pop() {
        any = true;
        mic.frame_count += 1;
        if mic.frame_count.is_multiple_of(LEVEL_EVERY_FRAMES) {
            let _ = events.send(SpeechEvent::Level(rms_level(&frame.samples)));
        }
        let samples = mic.resampler.process(&frame.samples);
        for event in transcription.feed(&samples) {
            let _ = events.send(match event {
                SttEvent::Partial(text) => SpeechEvent::Partial(text),
                SttEvent::Endpoint(text) => SpeechEvent::Partial(text),
            });
        }
    }
    any
}
