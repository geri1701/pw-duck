use anyhow::{Context, Result};
use pipewire as pw;
use pw::spa::param::audio::{AudioFormat, AudioInfoRaw};
use pw::spa::param::format::{MediaSubtype, MediaType};
use pw::spa::param::format_utils;
use pw::spa::pod::Pod;
use pw::{properties::properties, spa};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

#[derive(Debug)]
pub struct AtomicF32(AtomicU32);

impl AtomicF32 {
    pub const fn new(val: f32) -> Self {
        Self(AtomicU32::new(val.to_bits()))
    }

    pub fn load(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }

    fn store(&self, val: f32) {
        self.0.store(val.to_bits(), Ordering::Relaxed);
    }
}

#[derive(Debug)]
pub struct VadState {
    last_above: Option<Instant>,
    above_start: Option<Instant>,
    voice_active: bool,
}

impl VadState {
    pub const fn new() -> Self {
        Self {
            last_above: None,
            above_start: None,
            voice_active: false,
        }
    }

    pub fn step(&mut self, energy: f32, threshold: f32, attack: Duration, hold: Duration) -> bool {
        if is_disabled_threshold(threshold) {
            self.last_above = None;
            self.above_start = None;
            self.voice_active = false;
            return false;
        }

        let now = Instant::now();
        let start_threshold = start_threshold(threshold);
        let release_threshold = release_threshold(threshold);
        let above = if self.voice_active {
            energy > release_threshold
        } else {
            energy > start_threshold
        };

        if above {
            self.last_above = Some(now);
            if !self.voice_active {
                if attack.is_zero() {
                    self.voice_active = true;
                    self.above_start = None;
                } else {
                    match self.above_start {
                        None => self.above_start = Some(now),
                        Some(start) if now.duration_since(start) >= attack => {
                            self.voice_active = true;
                            self.above_start = None;
                        }
                        Some(_) => {}
                    }
                }
            }
        } else {
            self.above_start = None;
            if self.voice_active {
                if hold.is_zero() {
                    self.voice_active = false;
                    self.last_above = None;
                } else if let Some(last) = self.last_above {
                    if now.duration_since(last) >= hold {
                        self.voice_active = false;
                        self.last_above = None;
                    }
                }
            }
        }

        self.voice_active
    }
}

pub fn is_disabled_threshold(threshold: f32) -> bool {
    threshold >= 0.2
}

pub fn start_threshold(threshold: f32) -> f32 {
    if is_disabled_threshold(threshold) {
        return f32::INFINITY;
    }
    threshold * 2.0
}

pub fn release_threshold(threshold: f32) -> f32 {
    if is_disabled_threshold(threshold) {
        return f32::INFINITY;
    }
    threshold * 0.75
}

#[derive(Debug, Clone)]
pub struct VadOptions {
    pub threshold: f32,
    pub attack: Duration,
    pub hold: Duration,
}

impl Default for VadOptions {
    fn default() -> Self {
        Self {
            threshold: 0.01,
            attack: Duration::from_millis(40),
            hold: Duration::from_millis(700),
        }
    }
}

pub struct VoiceActivityMonitor {
    energy: Arc<AtomicF32>,
    audio_seen: Arc<AtomicBool>,
    frames: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl VoiceActivityMonitor {
    pub fn start(target_node_name: Option<String>, target_serial: Option<String>) -> Self {
        let energy = Arc::new(AtomicF32::new(0.0));
        let audio_seen = Arc::new(AtomicBool::new(false));
        let frames = Arc::new(AtomicU64::new(0));
        let stop = Arc::new(AtomicBool::new(false));

        let thread_energy = energy.clone();
        let thread_audio_seen = audio_seen.clone();
        let thread_frames = frames.clone();
        let thread_stop = stop.clone();

        let handle = thread::spawn(move || {
            if let Err(err) = run_capture_thread(
                target_node_name,
                target_serial,
                thread_energy,
                thread_audio_seen,
                thread_frames,
                thread_stop,
            ) {
                eprintln!("voice capture failed: {err:#}");
            }
        });

        Self {
            energy,
            audio_seen,
            frames,
            stop,
            handle: Some(handle),
        }
    }

    pub fn energy(&self) -> f32 {
        self.energy.load()
    }

    pub fn audio_seen(&self) -> bool {
        self.audio_seen.load(Ordering::Relaxed)
    }

    pub fn frames(&self) -> u64 {
        self.frames.load(Ordering::Relaxed)
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for VoiceActivityMonitor {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vad_activates_after_attack() {
        let mut vad = VadState::new();

        assert!(vad.step(0.5, 0.1, Duration::ZERO, Duration::from_secs(1)));
    }

    #[test]
    fn vad_releases_after_hold() {
        let mut vad = VadState::new();

        assert!(vad.step(0.5, 0.1, Duration::ZERO, Duration::ZERO));
        assert!(!vad.step(0.0, 0.1, Duration::ZERO, Duration::ZERO));
    }

    #[test]
    fn vad_does_not_activate_below_threshold() {
        let mut vad = VadState::new();

        assert!(!vad.step(0.05, 0.1, Duration::ZERO, Duration::ZERO));
    }

    #[test]
    fn vad_requires_energy_above_start_hysteresis() {
        let mut vad = VadState::new();

        assert!(!vad.step(0.015, 0.01, Duration::ZERO, Duration::ZERO));
        assert!(vad.step(0.021, 0.01, Duration::ZERO, Duration::ZERO));
    }

    #[test]
    fn vad_keeps_active_until_release_hysteresis() {
        let mut vad = VadState::new();

        assert!(vad.step(0.03, 0.01, Duration::ZERO, Duration::ZERO));
        assert!(vad.step(0.008, 0.01, Duration::ZERO, Duration::ZERO));
        assert!(!vad.step(0.007, 0.01, Duration::ZERO, Duration::ZERO));
    }

    #[test]
    fn vad_disabled_threshold_never_activates() {
        let mut vad = VadState::new();

        assert!(!vad.step(1.0, 0.2, Duration::ZERO, Duration::ZERO));
        assert!(vad.step(1.0, 0.199, Duration::ZERO, Duration::ZERO));
        assert!(!vad.step(1.0, 0.2, Duration::ZERO, Duration::ZERO));
    }
}

#[derive(Debug)]
struct CaptureData {
    format: AudioInfoRaw,
}

fn run_capture_thread(
    target_node_name: Option<String>,
    target_serial: Option<String>,
    energy: Arc<AtomicF32>,
    audio_seen: Arc<AtomicBool>,
    frames: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
) -> Result<()> {
    pw::init();

    let mainloop = pw::main_loop::MainLoopRc::new(None).context("create PipeWire mainloop")?;
    let context =
        pw::context::ContextRc::new(&mainloop, None).context("create PipeWire context")?;
    let core = context.connect_rc(None).context("connect PipeWire core")?;

    let mut props = properties! {
        *pw::keys::MEDIA_TYPE => "Audio",
        *pw::keys::MEDIA_CATEGORY => "Capture",
        *pw::keys::MEDIA_ROLE => "Communication",
        *pw::keys::MEDIA_CLASS => "Stream/Input/Audio",
    };

    if let Some(serial) = target_serial.filter(|value| !value.is_empty()) {
        props.insert("target.object", serial);
    } else if let Some(node_name) = target_node_name.filter(|value| !value.is_empty()) {
        props.insert("target.object", node_name);
    }
    props.insert(*pw::keys::STREAM_CAPTURE_SINK, "true");

    let stream = pw::stream::StreamBox::new(&core, "pw-duck voice capture", props)
        .context("create PipeWire voice capture stream")?;
    let user_data = CaptureData {
        format: Default::default(),
    };

    let process_energy = energy.clone();
    let process_audio_seen = audio_seen.clone();
    let process_frames = frames.clone();
    let _listener = stream
        .add_local_listener_with_user_data(user_data)
        .param_changed(|_, user_data, id, param| {
            let Some(param) = param else {
                return;
            };
            if id != pw::spa::param::ParamType::Format.as_raw() {
                return;
            }
            let Ok((media_type, media_subtype)) = format_utils::parse_format(param) else {
                return;
            };
            if media_type != MediaType::Audio || media_subtype != MediaSubtype::Raw {
                return;
            }
            let _ = user_data.format.parse(param);
        })
        .process(move |stream, user_data| {
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            let datas = buffer.datas_mut();
            if datas.is_empty() {
                return;
            }

            let channels = user_data.format.channels();
            if channels == 0 {
                return;
            }

            let data = &mut datas[0];
            let chunk = data.chunk();
            let offset = chunk.offset() as usize;
            let size = chunk.size() as usize;
            if size == 0 {
                return;
            }

            let Some(samples) = data.data() else {
                return;
            };
            if offset >= samples.len() {
                return;
            }
            let end = (offset + size).min(samples.len());
            if end <= offset {
                return;
            }

            let mut sum_sq = 0.0f32;
            let mut count = 0usize;
            let slice = &samples[offset..end];

            match user_data.format.format() {
                AudioFormat::F32LE => {
                    for chunk in slice.chunks_exact(std::mem::size_of::<f32>()) {
                        let sample = f32::from_le_bytes(chunk.try_into().unwrap());
                        sum_sq += sample * sample;
                        count += 1;
                    }
                }
                AudioFormat::F32BE => {
                    for chunk in slice.chunks_exact(std::mem::size_of::<f32>()) {
                        let sample = f32::from_be_bytes(chunk.try_into().unwrap());
                        sum_sq += sample * sample;
                        count += 1;
                    }
                }
                AudioFormat::S16LE => {
                    for chunk in slice.chunks_exact(std::mem::size_of::<i16>()) {
                        let sample =
                            i16::from_le_bytes(chunk.try_into().unwrap()) as f32 / i16::MAX as f32;
                        sum_sq += sample * sample;
                        count += 1;
                    }
                }
                AudioFormat::S16BE => {
                    for chunk in slice.chunks_exact(std::mem::size_of::<i16>()) {
                        let sample =
                            i16::from_be_bytes(chunk.try_into().unwrap()) as f32 / i16::MAX as f32;
                        sum_sq += sample * sample;
                        count += 1;
                    }
                }
                _ => process_energy.store(0.0),
            }

            if count > 0 {
                process_energy.store((sum_sq / count as f32).sqrt());
                process_audio_seen.store(true, Ordering::Relaxed);
                process_frames.fetch_add(1, Ordering::Relaxed);
            }
        })
        .register()
        .context("register PipeWire voice capture listener")?;

    let mut audio_info = AudioInfoRaw::new();
    audio_info.set_format(AudioFormat::F32LE);
    let obj = pw::spa::pod::Object {
        type_: pw::spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
        id: pw::spa::param::ParamType::EnumFormat.as_raw(),
        properties: audio_info.into(),
    };
    let serialized = pw::spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &pw::spa::pod::Value::Object(obj),
    )
    .unwrap()
    .0
    .into_inner();
    let mut params = [Pod::from_bytes(&serialized).unwrap()];

    stream
        .connect(
            spa::utils::Direction::Input,
            None,
            pw::stream::StreamFlags::AUTOCONNECT
                | pw::stream::StreamFlags::MAP_BUFFERS
                | pw::stream::StreamFlags::RT_PROCESS,
            &mut params,
        )
        .context("connect PipeWire voice capture stream")?;

    while !stop.load(Ordering::Relaxed) {
        mainloop.loop_().iterate(Duration::from_millis(50));
    }

    Ok(())
}
