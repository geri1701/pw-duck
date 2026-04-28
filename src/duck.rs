use anyhow::{bail, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::config::Config;
use crate::identity::ConfiguredSource;
use crate::pulse;
use crate::routing::{RoutingEngine, RoutingOptions};
use crate::shell::CommandRunner;
use crate::vad;

const WAITING_POLL_INTERVAL: Duration = Duration::from_millis(500);
const ACTIVE_VAD_INTERVAL: Duration = Duration::from_millis(50);
const SOURCE_CHECK_INTERVAL: Duration = Duration::from_millis(500);
const ROUTE_REFRESH_INTERVAL: Duration = Duration::from_millis(500);
const CONFIG_RELOAD_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Debug, Copy, Clone)]
pub struct DuckingSettings {
    pub duck_percent: u8,
    pub vad_threshold: f32,
    pub hold_ms: u64,
}

impl DuckingSettings {
    pub fn clamped(self) -> Self {
        Self {
            duck_percent: self.duck_percent.min(100),
            vad_threshold: self.vad_threshold.clamp(0.0025, 0.2),
            hold_ms: self.hold_ms.min(4_000),
        }
    }
}

pub type SharedDuckingSettings = Arc<Mutex<DuckingSettings>>;

#[derive(Debug, Clone)]
pub struct DuckingOptions {
    pub settings: SharedDuckingSettings,
    pub reload_config: bool,
}

#[derive(Debug, Clone)]
pub enum DuckingEvent {
    WaitingForSource,
    Started {
        label: String,
        threshold: f32,
        start_threshold: f32,
        hold_ms: u64,
    },
    VoiceActive {
        level: f32,
        percent: u8,
    },
    VoiceInactive {
        level: f32,
    },
}

pub fn run_until_stopped<R, F>(
    runner: &R,
    stop: &AtomicBool,
    options: DuckingOptions,
    mut on_event: F,
) -> Result<()>
where
    R: CommandRunner,
    F: FnMut(DuckingEvent),
{
    let mut waiting_reported = false;

    while !stop.load(Ordering::SeqCst) {
        if options.reload_config {
            sync_settings_from_config(&options.settings);
        }

        let voice_source = match configured_voice_source() {
            Ok(source) => source,
            Err(_) => {
                if !waiting_reported {
                    on_event(DuckingEvent::WaitingForSource);
                    waiting_reported = true;
                }
                std::thread::sleep(WAITING_POLL_INTERVAL);
                continue;
            }
        };

        let capture_target = match voice_capture_target(runner, &voice_source) {
            Ok(target) => {
                waiting_reported = false;
                target
            }
            Err(_) => {
                if !waiting_reported {
                    on_event(DuckingEvent::WaitingForSource);
                    waiting_reported = true;
                }
                std::thread::sleep(WAITING_POLL_INTERVAL);
                continue;
            }
        };

        match run_active_session(
            runner,
            stop,
            &options,
            &voice_source,
            capture_target,
            &mut on_event,
        )? {
            ActiveSessionEnd::StopRequested => break,
            ActiveSessionEnd::SourceUnavailable => {
                if !waiting_reported {
                    on_event(DuckingEvent::WaitingForSource);
                    waiting_reported = true;
                }
            }
        }
    }

    Ok(())
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum ActiveSessionEnd {
    StopRequested,
    SourceUnavailable,
}

fn run_active_session<R, F>(
    runner: &R,
    stop: &AtomicBool,
    options: &DuckingOptions,
    voice_source: &ConfiguredSource,
    capture_target: VoiceCaptureTarget,
    on_event: &mut F,
) -> Result<ActiveSessionEnd>
where
    R: CommandRunner,
    F: FnMut(DuckingEvent),
{
    let mut engine = RoutingEngine::new(runner, voice_source.clone());
    let mut session = engine.start(RoutingOptions::default())?;
    session.set_neutral()?;

    let mut voice_monitor = vad::VoiceActivityMonitor::start(
        capture_target.node_name.clone(),
        capture_target.object_serial.clone(),
    );
    let initial_settings = current_settings(&options);
    let vad_defaults = vad::VadOptions::default();
    let mut vad_state = vad::VadState::new();
    let mut ducked = false;
    let mut applied_duck_percent: Option<u8> = None;
    let mut last_capture_frames = 0u64;
    let mut next_source_check = Instant::now();
    let mut next_route_refresh = Instant::now();
    let mut next_config_reload = Instant::now();

    on_event(DuckingEvent::Started {
        label: capture_target.label.clone(),
        threshold: initial_settings.vad_threshold,
        start_threshold: vad::start_threshold(initial_settings.vad_threshold),
        hold_ms: initial_settings.hold_ms,
    });

    let mut end = ActiveSessionEnd::StopRequested;
    while !stop.load(Ordering::SeqCst) {
        let now = Instant::now();

        if now >= next_source_check {
            if options.reload_config {
                match configured_voice_source() {
                    Ok(current_source) if current_source != *voice_source => {
                        end = ActiveSessionEnd::SourceUnavailable;
                        break;
                    }
                    Ok(_) => {}
                    Err(_) => {
                        end = ActiveSessionEnd::SourceUnavailable;
                        break;
                    }
                }
            }

            let current_target = voice_capture_target(runner, voice_source);
            if !matches!(current_target, Ok(ref target) if target.same_stream_as(&capture_target)) {
                end = ActiveSessionEnd::SourceUnavailable;
                break;
            }
            next_source_check = now + SOURCE_CHECK_INTERVAL;
        }

        if now >= next_route_refresh {
            session.route_existing_outputs(voice_source)?;
            next_route_refresh = now + ROUTE_REFRESH_INTERVAL;
        }

        if !voice_monitor.audio_seen() {
            std::thread::sleep(ACTIVE_VAD_INTERVAL);
            continue;
        }

        if options.reload_config && now >= next_config_reload {
            sync_settings_from_config(&options.settings);
            next_config_reload = now + CONFIG_RELOAD_INTERVAL;
        }
        let capture_frames = voice_monitor.frames();
        let energy = if capture_frames == last_capture_frames {
            0.0
        } else {
            voice_monitor.energy()
        };
        last_capture_frames = capture_frames;
        let settings = current_settings(&options);
        let voice_active = vad_state.step(
            energy,
            settings.vad_threshold,
            vad_defaults.attack,
            Duration::from_millis(settings.hold_ms),
        );
        if voice_active != ducked {
            if voice_active {
                session.set_duck_percent(settings.duck_percent)?;
                applied_duck_percent = Some(settings.duck_percent);
                on_event(DuckingEvent::VoiceActive {
                    level: energy,
                    percent: settings.duck_percent,
                });
            } else {
                session.set_neutral()?;
                applied_duck_percent = None;
                on_event(DuckingEvent::VoiceInactive { level: energy });
            }
            ducked = voice_active;
        } else if ducked && applied_duck_percent != Some(settings.duck_percent) {
            session.set_duck_percent(settings.duck_percent)?;
            applied_duck_percent = Some(settings.duck_percent);
            on_event(DuckingEvent::VoiceActive {
                level: energy,
                percent: settings.duck_percent,
            });
        }

        std::thread::sleep(ACTIVE_VAD_INTERVAL);
    }

    session.set_neutral()?;
    session.stop()?;
    voice_monitor.stop();
    Ok(end)
}

pub fn shared_settings(settings: DuckingSettings) -> SharedDuckingSettings {
    Arc::new(Mutex::new(settings.clamped()))
}

pub fn current_settings(options: &DuckingOptions) -> DuckingSettings {
    options
        .settings
        .lock()
        .map(|settings| (*settings).clamped())
        .unwrap_or_else(|_| fallback_settings())
}

fn sync_settings_from_config(settings: &SharedDuckingSettings) {
    let Ok(config) = Config::load_or_default() else {
        return;
    };
    let Ok(mut guard) = settings.lock() else {
        return;
    };
    *guard = DuckingSettings {
        duck_percent: config.duck_percent,
        vad_threshold: config.vad_threshold,
        hold_ms: config.hold_ms,
    }
    .clamped();
}

fn fallback_settings() -> DuckingSettings {
    let vad_defaults = vad::VadOptions::default();
    DuckingSettings {
        duck_percent: 25,
        vad_threshold: vad_defaults.threshold,
        hold_ms: vad_defaults.hold.as_millis() as u64,
    }
}

pub fn configured_voice_source() -> Result<ConfiguredSource> {
    let config = Config::load_or_default()?;
    config.voice_source.ok_or_else(|| {
        anyhow::anyhow!("no voice source configured; create config.toml and choose a source first")
    })
}

#[derive(Debug)]
struct VoiceCaptureTarget {
    label: String,
    node_name: Option<String>,
    object_serial: Option<String>,
}

impl VoiceCaptureTarget {
    fn same_stream_as(&self, other: &Self) -> bool {
        match (
            self.object_serial.as_deref(),
            other.object_serial.as_deref(),
        ) {
            (Some(left), Some(right)) => left == right,
            _ => self.node_name == other.node_name,
        }
    }
}

fn voice_capture_target<R: CommandRunner>(
    runner: &R,
    voice_source: &ConfiguredSource,
) -> Result<VoiceCaptureTarget> {
    let pulse = pulse::PulseCtl::new(runner);
    let input = pulse
        .sink_inputs()?
        .into_iter()
        .find(|input| input.identity().matches_configured_source(voice_source))
        .ok_or_else(|| anyhow::anyhow!("configured voice source is not visible right now"))?;
    let identity = input.identity();
    let object_serial = input.properties.get("object.serial").cloned();
    let node_name = identity.node_name.clone();
    let label = format!(
        "#{} {} / {} / {}",
        input.index,
        identity
            .application_name
            .as_deref()
            .unwrap_or("unknown-app"),
        identity
            .application_process_binary
            .as_deref()
            .unwrap_or("unknown-bin"),
        identity.media_name.as_deref().unwrap_or("unknown-media")
    );

    Ok(VoiceCaptureTarget {
        label,
        node_name,
        object_serial,
    })
}

pub fn ensure_route_acknowledged(command: &str, yes_really_route: bool) -> Result<()> {
    if yes_really_route {
        Ok(())
    } else {
        bail!("{command} changes the live PipeWire graph; use --yes-really-route")
    }
}
