use std::io::Cursor;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::{Context, Result};
use pipewire as pw;
use pw::properties::properties;
use pw::spa::param::ParamType;
use pw::spa::param::audio::{AudioFormat, AudioInfoRaw};
use pw::spa::pod::serialize::PodSerializer;
use pw::spa::pod::{Object, Pod, Value};
use pw::spa::utils::{Direction, SpaTypes};
use pw::stream::{StreamFlags, StreamRc};
use rtrb::{Consumer, Producer, RingBuffer};

const STREAM_NAME: &str = "suja";
const SAMPLE_BYTES: usize = size_of::<f32>();
const RING_SECONDS: usize = 4;

pub struct Player {
    producer: Producer<f32>,
    shared: Arc<Shared>,
    commands: pw::channel::Sender<StreamCommand>,
    thread: Option<JoinHandle<()>>,
}

#[derive(Clone)]
pub struct PlaybackControl {
    shared: Arc<Shared>,
}

impl PlaybackControl {
    pub fn flush(&self) {
        self.shared.flush_requested.store(true, Ordering::Release);
    }

    pub fn drained(&self) -> bool {
        self.shared.queued.load(Ordering::Acquire) == 0
    }
}

#[derive(Default)]
struct Shared {
    flush_requested: AtomicBool,
    queued: AtomicUsize,
}

struct RealtimeState {
    samples: Consumer<f32>,
    shared: Arc<Shared>,
}

#[derive(Clone, Copy)]
enum StreamCommand {
    Quit,
}

impl Player {
    pub fn start(sample_rate: u32) -> Result<Self> {
        let (producer, consumer) = RingBuffer::new(sample_rate as usize * RING_SECONDS);
        let shared = Arc::new(Shared::default());
        let (commands, command_receiver) = pw::channel::channel();
        let (ready_sender, ready) = std::sync::mpsc::sync_channel(1);
        let state = RealtimeState {
            samples: consumer,
            shared: shared.clone(),
        };
        let thread = thread::Builder::new()
            .name("suja-playback".into())
            .spawn(move || {
                if let Err(error) = run_loop(state, sample_rate, command_receiver, &ready_sender) {
                    let _ = ready_sender.send(Err(error));
                }
            })?;
        ready
            .recv()
            .context("playback thread exited")?
            .context("open pipewire playback")?;
        Ok(Self {
            producer,
            shared,
            commands,
            thread: Some(thread),
        })
    }

    pub fn enqueue(&mut self, samples: &[f32]) {
        let mut rest = samples;
        while !rest.is_empty() {
            let free = self.producer.slots();
            if free == 0 {
                thread::sleep(Duration::from_millis(5));
                continue;
            }
            let count = free.min(rest.len());
            if let Ok(chunk) = self.producer.write_chunk_uninit(count) {
                chunk.fill_from_iter(rest[..count].iter().copied());
                self.shared.queued.fetch_add(count, Ordering::AcqRel);
                rest = &rest[count..];
            }
        }
    }

    pub fn control(&self) -> PlaybackControl {
        PlaybackControl {
            shared: self.shared.clone(),
        }
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        let _ = self.commands.send(StreamCommand::Quit);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run_loop(
    state: RealtimeState,
    sample_rate: u32,
    commands: pw::channel::Receiver<StreamCommand>,
    ready: &std::sync::mpsc::SyncSender<Result<(), pw::Error>>,
) -> Result<(), pw::Error> {
    pw::init();
    let main_loop = pw::main_loop::MainLoopRc::new(None)?;
    let context = pw::context::ContextRc::new(&main_loop, None)?;
    let core = context.connect_rc(None)?;
    let stream = StreamRc::new(
        core,
        STREAM_NAME,
        properties! {
            *pw::keys::MEDIA_TYPE => "Audio",
            *pw::keys::MEDIA_CATEGORY => "Playback",
            *pw::keys::MEDIA_ROLE => "Communication",
            *pw::keys::NODE_NAME => STREAM_NAME,
        },
    )?;
    let _listener = stream
        .add_local_listener_with_user_data(state)
        .process(fill_buffer)
        .register()?;
    let format = format_pod(sample_rate)?;
    let mut parameters = [Pod::from_bytes(&format).ok_or(pw::Error::CreationFailed)?];
    stream.connect(
        Direction::Output,
        None,
        StreamFlags::AUTOCONNECT | StreamFlags::MAP_BUFFERS | StreamFlags::RT_PROCESS,
        &mut parameters,
    )?;
    let _commands = commands.attach(main_loop.loop_(), {
        let main_loop = main_loop.clone();
        move |StreamCommand::Quit| main_loop.quit()
    });
    let _ = ready.send(Ok(()));
    main_loop.run();
    Ok(())
}

fn fill_buffer(stream: &pw::stream::Stream, state: &mut RealtimeState) {
    let Some(mut buffer) = stream.dequeue_buffer() else {
        return;
    };
    let requested = usize::try_from(buffer.requested()).unwrap_or_default();
    let Some(data) = buffer.datas_mut().first_mut() else {
        return;
    };
    let frames = data.data().map_or(0, |bytes| {
        let (slots, _) = bytes.as_chunks_mut::<SAMPLE_BYTES>();
        let frames = match requested {
            0 => slots.len(),
            requested => requested.min(slots.len()),
        };
        state.fill(&mut slots[..frames]);
        frames
    });
    let chunk = data.chunk_mut();
    *chunk.offset_mut() = 0;
    *chunk.stride_mut() = SAMPLE_BYTES as i32;
    *chunk.size_mut() = u32::try_from(frames * SAMPLE_BYTES).unwrap_or_default();
}

impl RealtimeState {
    fn fill(&mut self, output: &mut [[u8; SAMPLE_BYTES]]) {
        if self.shared.flush_requested.swap(false, Ordering::AcqRel) {
            let pending = self.samples.slots();
            if let Ok(chunk) = self.samples.read_chunk(pending) {
                chunk.commit_all();
            }
            self.shared.queued.fetch_sub(pending, Ordering::AcqRel);
        }
        let available = self.samples.slots().min(output.len());
        let mut written = 0;
        if let Ok(chunk) = self.samples.read_chunk(available) {
            for (slot, sample) in output.iter_mut().zip(chunk) {
                *slot = sample.to_le_bytes();
                written += 1;
            }
        }
        self.shared.queued.fetch_sub(written, Ordering::AcqRel);
        output[written..].fill([0; SAMPLE_BYTES]);
    }
}

fn format_pod(sample_rate: u32) -> Result<Vec<u8>, pw::Error> {
    let mut info = AudioInfoRaw::new();
    info.set_format(AudioFormat::F32LE);
    info.set_rate(sample_rate);
    info.set_channels(1);
    let mut position = [0; pw::spa::sys::SPA_AUDIO_MAX_CHANNELS as usize];
    position[0] = pw::spa::sys::SPA_AUDIO_CHANNEL_MONO;
    info.set_position(position);
    let object = Object {
        type_: SpaTypes::ObjectParamFormat.as_raw(),
        id: ParamType::EnumFormat.as_raw(),
        properties: info.into(),
    };
    PodSerializer::serialize(Cursor::new(Vec::new()), &Value::Object(object))
        .map(|(cursor, _)| cursor.into_inner())
        .map_err(|_| pw::Error::CreationFailed)
}
