mod cli;
mod config;
mod duck;
mod icons;
mod identity;
mod pipewire_sink;
mod pulse;
mod routing;
mod shell;
mod tray;
mod tune;
#[cfg(feature = "gui")]
mod tune_gui;
mod vad;

use anyhow::{bail, Result};
use clap::Parser;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use crate::cli::{Cli, Command};
use crate::config::Config;
use crate::duck::{DuckingEvent, DuckingOptions, DuckingSettings};
use crate::identity::{AudioIdentity, ConfiguredSource};
use crate::routing::{RoutingEngine, RoutingOptions};
use crate::shell::SystemRunner;

fn main() -> Result<()> {
    let cli = Cli::parse();
    let runner = SystemRunner;

    match cli.command.unwrap_or_default() {
        Command::Status => print_status(&runner),
        Command::ConfigPath => {
            println!("{}", Config::path()?.display());
            Ok(())
        }
        Command::InitConfig => {
            let path = Config::path()?;
            if path.exists() {
                println!("config exists: {}", path.display());
            } else {
                Config::default().save()?;
                println!("created config: {}", path.display());
            }
            Ok(())
        }
        Command::Sources => print_sources(&runner),
        Command::SelectSource { sink_input_index } => select_source(&runner, sink_input_index),
        Command::Tune => tune::run(),
        Command::TuneGui => run_tune_gui(),
        Command::Route {
            duck_percent,
            vad_threshold,
            hold_ms,
            yes_really_route,
        } => run_route_until_interrupted(
            &runner,
            resolve_settings(duck_percent, vad_threshold, hold_ms)?,
            yes_really_route,
        ),
        Command::Tray {
            duck_percent,
            vad_threshold,
            hold_ms,
        } => tray::run(tray::TrayOptions {
            settings: resolve_settings(duck_percent, vad_threshold, hold_ms)?,
        }),
        Command::RouteOnce {
            seconds,
            duck_percent,
            yes_really_route,
        } => run_route_once(
            &runner,
            seconds,
            resolve_duck_percent(duck_percent)?,
            yes_really_route,
        ),
    }
}

#[cfg(feature = "gui")]
fn run_tune_gui() -> Result<()> {
    tune_gui::run()
}

#[cfg(not(feature = "gui"))]
fn run_tune_gui() -> Result<()> {
    bail!("tune-gui ist in diesem Build nicht enthalten; baue mit `--features gui` oder nutze `pw-duck tune`")
}

fn resolve_settings(
    duck_percent: Option<u8>,
    vad_threshold: Option<f32>,
    hold_ms: Option<u64>,
) -> Result<DuckingSettings> {
    let config = Config::load_or_default()?;
    Ok(DuckingSettings {
        duck_percent: duck_percent.unwrap_or(config.duck_percent),
        vad_threshold: vad_threshold.unwrap_or(config.vad_threshold),
        hold_ms: hold_ms.unwrap_or(config.hold_ms),
    }
    .clamped())
}

fn resolve_duck_percent(duck_percent: Option<u8>) -> Result<u8> {
    let config = Config::load_or_default()?;
    Ok(duck_percent.unwrap_or(config.duck_percent).min(100))
}

fn run_route_once(
    runner: &SystemRunner,
    seconds: u64,
    duck_percent: u8,
    yes_really_route: bool,
) -> Result<()> {
    duck::ensure_route_acknowledged("route-once", yes_really_route)?;

    let voice_source = duck::configured_voice_source()?;
    let mut engine = RoutingEngine::new(runner, voice_source.clone());
    let mut session = engine.start(RoutingOptions::default())?;
    session.set_duck_percent(duck_percent)?;

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(seconds);
    while std::time::Instant::now() < deadline {
        session.route_existing_outputs(&voice_source)?;
        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    session.set_neutral()?;
    session.stop()?;
    Ok(())
}

fn run_route_until_interrupted(
    runner: &SystemRunner,
    settings: DuckingSettings,
    yes_really_route: bool,
) -> Result<()> {
    duck::ensure_route_acknowledged("route", yes_really_route)?;
    let stop = Arc::new(AtomicBool::new(false));
    let stop_for_handler = stop.clone();
    ctrlc::set_handler(move || {
        stop_for_handler.store(true, Ordering::SeqCst);
    })?;

    duck::run_until_stopped(
        runner,
        &stop,
        DuckingOptions {
            settings: duck::shared_settings(settings),
            reload_config: false,
        },
        |event| {
            match event {
            DuckingEvent::WaitingForSource => {
                println!("Warte auf konfigurierte Voice-Quelle …")
            }
            DuckingEvent::Started {
                label,
                threshold,
                start_threshold,
                hold_ms,
            } => println!(
                "Routing aktiv. VAD target={label} threshold={threshold:.4} start={start_threshold:.4} hold={hold_ms}ms. Beenden mit Ctrl+C."
            ),
            DuckingEvent::VoiceActive { level, percent } => {
                println!("VOICE ACTIVE level={level:.4} → Ducking {percent}%")
            }
            DuckingEvent::VoiceInactive { level } => {
                println!("VOICE INACTIVE level={level:.4} → Ducking aus")
            }
        }
        },
    )?;
    println!("Beende Routing und stelle Ursprungszustand wieder her …");
    Ok(())
}

fn print_status(runner: &SystemRunner) -> Result<()> {
    let pulse = pulse::PulseCtl::new(runner);
    let default_sink = pulse.default_sink_name()?;
    let default_sink_info = pulse.sink_by_name(&default_sink)?;
    let inputs = pulse.sink_inputs()?;

    if let Some(sink) = default_sink_info {
        println!(
            "Default-Sink: {} #{} ({})",
            sink.name,
            sink.index,
            sink.description.as_deref().unwrap_or("-")
        );
    } else {
        println!("Default-Sink: {default_sink}");
    }
    println!("Aktive Playback-Streams: {}", inputs.len());
    print_inputs(inputs);

    Ok(())
}

fn print_sources(runner: &SystemRunner) -> Result<()> {
    let pulse = pulse::PulseCtl::new(runner);
    let inputs = pulse.sink_inputs()?;
    println!("Aktuelle Playback-Streams:");
    print_inputs(inputs);
    println!();
    println!("Quelle speichern: pw-duck select-source <sink-input-index>");
    Ok(())
}

fn select_source(runner: &SystemRunner, sink_input_index: u32) -> Result<()> {
    let pulse = pulse::PulseCtl::new(runner);
    let input = pulse
        .sink_inputs()?
        .into_iter()
        .find(|input| input.index == sink_input_index)
        .ok_or_else(|| anyhow::anyhow!("sink-input #{sink_input_index} not found"))?;

    let identity = input.identity();
    if !identity.is_playback_stream() {
        bail!("sink-input #{sink_input_index} is not a playback stream");
    }

    let label = source_label(&identity);
    let mut config = Config::load_or_default()?;
    config.voice_source = Some(ConfiguredSource::from_identity(label.clone(), &identity));
    config.save()?;

    println!("gespeichert: {label}");
    println!("config: {}", Config::path()?.display());
    Ok(())
}

fn print_inputs(inputs: Vec<pulse::SinkInput>) {
    for input in inputs {
        let id = input.identity();
        let hint = if looks_like_voice_source(&id) {
            " voice?"
        } else {
            ""
        };
        println!(
            "- #{} sink={}{} app={} bin={} media={} role={} class={}",
            input.index,
            input.sink,
            hint,
            id.application_name.as_deref().unwrap_or("-"),
            id.application_process_binary.as_deref().unwrap_or("-"),
            id.media_name.as_deref().unwrap_or("-"),
            id.media_role.as_deref().unwrap_or("-"),
            id.media_class.as_deref().unwrap_or("-"),
        );
    }
}

fn source_label(identity: &AudioIdentity) -> String {
    format!(
        "{} / {} / {}",
        identity
            .application_name
            .as_deref()
            .unwrap_or("unknown-app"),
        identity
            .application_process_binary
            .as_deref()
            .unwrap_or("unknown-bin"),
        identity.media_name.as_deref().unwrap_or("unknown-media")
    )
}

fn looks_like_voice_source(identity: &AudioIdentity) -> bool {
    let text = [
        identity.application_name.as_deref(),
        identity.application_process_binary.as_deref(),
        identity.media_name.as_deref(),
        identity.media_role.as_deref(),
        identity.node_name.as_deref(),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" ")
    .to_ascii_lowercase();

    text.contains("voice")
        || text.contains("webrtc")
        || text.contains("discord")
        || text.contains("communication")
}
