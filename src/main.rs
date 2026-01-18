//! PipeWire voice ducking
#![cfg_attr(feature = "dev-tools", allow(dead_code))]

mod analysis;
mod capture;
mod ducking;
mod logging;
mod ui;

use anyhow::{anyhow, Result};
use clap::Parser;
use pipewire as pw;
use signal_hook::consts::signal::{SIGINT, SIGTERM};
use signal_hook::flag;
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{self, Write};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use crate::analysis::{auto_vad_step, AtomicF32, VadState};
use crate::capture::{probe_candidate_energy, setup_capture};
use crate::ducking::{is_voice_candidate, wpctl_get_volume, OutputStream, RestoreGuard};
use crate::logging::{elogln, logln};
use crate::ui::{
    enter_gui_mode, handle_gui_input, render_gui, select_voice_source_gui, GuiSelectResult,
};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum ControlMode {
    AutoVad,
    ManualDucked,
    ManualRestored,
}

impl ControlMode {
    #[cfg(feature = "dev-tools")]
    const fn as_str(self) -> &'static str {
        match self {
            Self::AutoVad => "AutoVad",
            Self::ManualDucked => "ManualDucked",
            Self::ManualRestored => "ManualRestored",
        }
    }
}

/// CLI opts
#[derive(Parser, Debug)]
#[command(
    name = "pw-duck",
    author,
    version,
    about = "Voice activity ducking for PipeWire (default: GUI)",
    long_about = None
)]
struct Opts {
    /// vad threshold
    #[arg(long, default_value_t = 0.02)]
    threshold: f32,
    /// attack ms
    #[arg(long, default_value_t = 0)]
    attack: u64,
    /// hold ms
    #[arg(long, default_value_t = 350)]
    hold: u64,
    /// duck factor
    #[arg(long, default_value_t = 0.45)]
    duck_factor: f32,
    /// debug
    #[arg(long)]
    debug: bool,
    /// force selection (gui)
    #[arg(long)]
    select: bool,
}

#[allow(
    clippy::assigning_clones,
    clippy::explicit_iter_loop,
    clippy::if_not_else,
    clippy::manual_let_else,
    clippy::map_unwrap_or,
    clippy::option_if_let_else,
    clippy::redundant_clone,
    clippy::redundant_closure_for_method_calls,
    clippy::too_many_lines,
    clippy::uninlined_format_args
)]
fn main() -> Result<()> {
    // cli parse
    let opts = Opts::parse();
    let gui_enabled = !opts.debug;
    let force_select = opts.select && !opts.debug;
    let mut gui_mode_guard: Option<crate::ui::GuiModeGuard> = None;
    let duck_factor = if gui_enabled {
        opts.duck_factor
    } else {
        logln(gui_enabled, "default ducking enabled (duck_factor=0.0)");
        0.0
    };
    let duck_factor_live = Rc::new(RefCell::new(duck_factor));
    let threshold_live = Rc::new(RefCell::new(opts.threshold));
    let hold_live = Rc::new(RefCell::new(opts.hold));

    // pipewire init
    pw::init();

    if gui_enabled {
        gui_mode_guard = Some(enter_gui_mode()?);
    }

    // core setup
    let mainloop = pw::main_loop::MainLoopRc::new(None)?;
    let context = pw::context::ContextRc::new(&mainloop, None)?;
    let core = context.connect_rc(None)?;
    let registry = core.get_registry_rc()?;

    // shared state
    let outputs: Rc<RefCell<HashMap<u32, OutputStream>>> = Rc::new(RefCell::new(HashMap::new()));
    let voice_source_id: Rc<RefCell<Option<u32>>> = Rc::new(RefCell::new(None));
    let voice_source_node: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let voice_source_serial: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let voice_source_label: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));
    let voice_source_reason: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));
    let baselines: Rc<RefCell<HashMap<u32, f32>>> = Rc::new(RefCell::new(HashMap::new()));
    let gui_log: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let restore_guard: Rc<RefCell<Option<Arc<Mutex<RestoreGuard>>>>> = Rc::new(RefCell::new(None));
    let control_mode: Rc<RefCell<ControlMode>> = Rc::new(RefCell::new(ControlMode::AutoVad));

    // signals
    let quit_requested = Arc::new(AtomicBool::new(false));
    // signal handlers
    flag::register(SIGINT, Arc::clone(&quit_requested))?;
    flag::register(SIGTERM, Arc::clone(&quit_requested))?;

    // registry listener
    let _registry_listener = {
        // rc clones
        let outputs_g = outputs.clone();
        let outputs_r = outputs.clone();
        let voice_g = voice_source_id.clone();
        let voice_r = voice_source_id.clone();
        let voice_node_r = voice_source_node.clone();
        let voice_serial_r = voice_source_serial.clone();
        let baselines_g = baselines.clone();
        let baselines_r = baselines.clone();
        let guard_g = restore_guard.clone();
        let guard_r = restore_guard.clone();
        let duck_factor_live = duck_factor_live.clone();

        registry
            .add_listener_local()
            .global(move |global| {
                let props = match global.props.as_ref() {
                    Some(p) => p,
                    None => return,
                };

                // media props
                let media_class = props
                    .get("media.class")
                    .map(|v| v.to_string())
                    .unwrap_or_default();
                let app_name = props
                    .get("application.name")
                    .map(|v| v.to_string())
                    .unwrap_or_default();
                // skip inputs
                if media_class == "Stream/Input/Audio" {
                    return;
                }
                // output filter
                if media_class != "Stream/Output/Audio" && app_name != "WEBRTC VoiceEngine" {
                    return;
                }

                let info = OutputStream {
                    id: global.id,
                    serial: props
                        .get("object.serial")
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "-".into()),
                    app: props
                        .get("application.name")
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "unknown-app".into()),
                    bin: props
                        .get("application.process.binary")
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "-".into()),
                    pid: props
                        .get("application.process.id")
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "-".into()),
                    role: props
                        .get("media.role")
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "-".into()),
                    media: props
                        .get("media.name")
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "unknown-media".into()),
                    media_class: media_class.clone(),
                    node: props
                        .get("node.name")
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "unknown-node".into()),
                    client: props
                        .get("client.id")
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "-".into()),
                };

                outputs_g.borrow_mut().insert(info.id, info.clone());

                if voice_g.borrow().is_none() {
                    if !gui_enabled {
                        logln(gui_enabled, format!("+ output stream added: {:?}", info));
                    }
                    return;
                }
                if !gui_enabled {
                    logln(gui_enabled, format!("+ output stream added: {:?}", info));
                }
                // baseline+duck
                let voice_id_opt = *voice_g.borrow();
                if let Some(voice) = voice_id_opt {
                    // non-voice baseline
                    if info.id != voice {
                        if let Some(v) = wpctl_get_volume(info.id) {
                            baselines_g.borrow_mut().insert(info.id, v);
                            if !gui_enabled {
                                logln(
                                    gui_enabled,
                                    format!("baseline captured: id={} -> {}", info.id, v),
                                );
                            }
                            if let Some(guard) = guard_g.borrow().as_ref() {
                                let mut guard = guard.lock().unwrap();
                                guard.add_stream(info.id, v);
                                if guard.ducked {
                                    let factor = if gui_enabled {
                                        *duck_factor_live.borrow()
                                    } else {
                                        duck_factor
                                    };
                                    guard.apply_duck(factor);
                                }
                            }
                        } else if !gui_enabled {
                            logln(
                                gui_enabled,
                                format!("baseline capture failed: id={}", info.id),
                            );
                        }
                    }
                }
            })
            .global_remove(move |id| {
                if let Some(info) = outputs_r.borrow_mut().remove(&id) {
                    // voice removed
                    if Some(id) == *voice_r.borrow() {
                        if !gui_enabled {
                            logln(
                                gui_enabled,
                                format!("! voice source disappeared: {:?}", info),
                            );
                        }
                        *voice_r.borrow_mut() = None;
                        *voice_node_r.borrow_mut() = None;
                        *voice_serial_r.borrow_mut() = None;
                        // stop ducking
                        if let Some(guard) = guard_r.borrow().as_ref() {
                            let mut guard = guard.lock().unwrap();
                            if guard.ducked {
                                guard.restore();
                            }
                        }
                    } else {
                        if !gui_enabled {
                            logln(gui_enabled, format!("- output stream removed: {:?}", info));
                        }
                        baselines_r.borrow_mut().remove(&id);
                        if let Some(guard) = guard_r.borrow().as_ref() {
                            guard.lock().unwrap().remove_stream(id);
                        }
                    }
                }
            })
            .register()
    };

    let run_mainloop_for = |duration: Duration| -> Result<()> {
        let quit = mainloop.clone();
        let timer = mainloop.loop_().add_timer(move |_| quit.quit());
        timer.update_timer(Some(duration), None).into_result()?;
        mainloop.run();
        Ok(())
    };

    // phase A list
    run_mainloop_for(Duration::from_millis(250))?;

    // voice selection
    {
        let score_voice_candidate = |s: &OutputStream| -> i32 {
            let mut score = 0;
            if s.app == "WEBRTC VoiceEngine" {
                score += 100;
            }
            if s.node == "WEBRTC VoiceEngine" {
                score += 30;
            }
            if s.node != "unknown-node" {
                score += 5;
            }
            if s.client != "-" {
                score += 3;
            }
            if s.media != "unknown-media" {
                score += 1;
            }
            if s.role != "-" {
                score += 1;
            }
            score
        };
        let build_list = || {
            let mut list: Vec<OutputStream> = outputs.borrow().values().cloned().collect();
            list.sort_by_key(|s| s.id);
            list.dedup_by_key(|s| s.id);
            list
        };
        let mut list = build_list();
        let mut selected: Option<(OutputStream, String)> = None;

        if !force_select {
            let candidates: Vec<OutputStream> = list
                .iter()
                .filter(|s| s.app == "WEBRTC VoiceEngine" && s.media_class == "Stream/Output/Audio")
                .cloned()
                .collect();
            if !gui_enabled {
                logln(
                    gui_enabled,
                    format!(
                        "WEBRTC VoiceEngine candidates (Stream/Output/Audio): {}",
                        candidates.len()
                    ),
                );
                for s in candidates.iter() {
                    logln(
                        gui_enabled,
                        format!(
                            "  id={} node=\"{}\" serial={} pid={} media=\"{}\" role=\"{}\"",
                            s.id, s.node, s.serial, s.pid, s.media, s.role
                        ),
                    );
                }
            }

            if candidates.len() == 1 {
                selected = Some((candidates[0].clone(), "single candidate".into()));
            } else if candidates.len() > 1 {
                let mut best_score = 0.0_f32;
                let mut best = None;
                for cand in candidates.iter() {
                    let score = probe_candidate_energy(
                        &mainloop,
                        &core,
                        Some(cand.node.clone()),
                        Some(cand.serial.clone()),
                        Duration::from_millis(700),
                    )
                    .unwrap_or(0.0);
                    if score > best_score {
                        best_score = score;
                        best = Some(cand.clone());
                    }
                }
                if best_score > 0.001 {
                    if let Some(best) = best {
                        selected = Some((best, format!("probe rms={:.4}", best_score)));
                    }
                } else if !gui_enabled {
                    logln(
                        gui_enabled,
                        "no usable WEBRTC output signal found (probe below floor)",
                    );
                }
            } else if !gui_enabled {
                logln(gui_enabled, "no WEBRTC VoiceEngine output candidates found");
            }
        }

        if selected.is_none() {
            // fallback scoring
            list.sort_by_key(|s| -(score_voice_candidate(s)));
            // default candidate
            let default_candidate_index = list
                .iter()
                .enumerate()
                .filter(|(_, s)| s.app == "WEBRTC VoiceEngine")
                .max_by_key(|(_, s)| score_voice_candidate(s))
                .map(|(idx, _)| idx);
            if gui_enabled {
                loop {
                    let default_candidate_index = list
                        .iter()
                        .enumerate()
                        .filter(|(_, s)| s.app == "WEBRTC VoiceEngine")
                        .max_by_key(|(_, s)| score_voice_candidate(s))
                        .map(|(idx, _)| idx);
                    let default_index = default_candidate_index.unwrap_or(0);
                    match select_voice_source_gui(&list, default_index)? {
                        GuiSelectResult::Selected(idx) => {
                            selected = Some((list[idx].clone(), "gui selection".into()));
                            break;
                        }
                        GuiSelectResult::Refresh => {
                            run_mainloop_for(Duration::from_millis(250))?;
                            list = build_list();
                            continue;
                        }
                        GuiSelectResult::Quit => return Ok(()),
                    }
                }
            } else {
                if list.is_empty() {
                    logln(gui_enabled, "Keine aktiven Ausgabeströme gefunden.");
                    return Ok(());
                }
                logln(gui_enabled, "Aktive Ausgabeströme (Stream/Output/Audio):");
                logln(
                    gui_enabled,
                    "  [*] = wahrscheinlicher Remote‑Voice‑Kandidat (nur Hinweis)\n",
                );
                for (i, s) in list.iter().enumerate() {
                    let mark = if is_voice_candidate(s) { "[*]" } else { "[ ]" };
                    logln(
                        gui_enabled,
                        format!(
                            "  {} [{:02}] id={}  app=\"{}\"  role=\"{}\"  media=\"{}\"  node=\"{}\"  bin=\"{}\"  pid={}  client={}  serial={}",
                            mark,
                            i + 1,
                            s.id,
                            s.app,
                            s.role,
                            s.media,
                            s.node,
                            s.bin,
                            s.pid,
                            s.client,
                            s.serial
                        ),
                    );
                }
                // prompt
                if let Some(idx) = default_candidate_index {
                    logln(
                        gui_enabled,
                        format!(
                            "\nNummer der VOICE SOURCE wählen (1-{}), Enter für bevorzugten Kandidaten [{}]: ",
                            list.len(),
                            idx + 1
                        ),
                    );
                } else {
                    logln(
                        gui_enabled,
                        format!("\nNummer der VOICE SOURCE wählen (1-{}): ", list.len()),
                    );
                }
                io::stdout().flush().ok();
                let mut line = String::new();
                io::stdin().read_line(&mut line)?;
                let trimmed = line.trim();
                let sel: usize = if trimmed.is_empty() {
                    // empty -> default
                    match default_candidate_index {
                        Some(idx) => idx + 1,
                        None => 1,
                    }
                } else {
                    // parse selection
                    trimmed.parse().map_err(|_| anyhow!("Ungültige Zahl"))?
                };
                if sel == 0 || sel > list.len() {
                    return Err(anyhow!("Auswahl außerhalb des gültigen Bereichs"));
                }
                selected = Some((list[sel - 1].clone(), "manual selection".into()));
            }
        }

        let (chosen, reason) = selected.expect("voice selection missing");
        *voice_source_id.borrow_mut() = Some(chosen.id);
        *voice_source_node.borrow_mut() = Some(chosen.node.clone());
        *voice_source_serial.borrow_mut() = Some(chosen.serial.clone());
        *voice_source_label.borrow_mut() = chosen.app.clone();
        *voice_source_reason.borrow_mut() = reason.clone();
        if !gui_enabled {
            logln(
                gui_enabled,
                format!(
                    "\nVoice Source ausgewählt: id={} app=\"{}\" role=\"{}\" media=\"{}\" node=\"{}\" serial={} ({})",
                    chosen.id, chosen.app, chosen.role, chosen.media, chosen.node, chosen.serial, reason
                ),
            );
        }
        // capture baselines
        {
            let voice = chosen.id;
            let mut b = baselines.borrow_mut();
            for (id, _) in outputs.borrow().iter() {
                if *id == voice {
                    continue;
                }
                if let Some(v) = wpctl_get_volume(*id) {
                    b.insert(*id, v);
                    logln(
                        gui_enabled,
                        format!("baseline captured: id={} -> {}", id, v),
                    );
                } else {
                    logln(gui_enabled, format!("baseline capture failed: id={}", id));
                }
            }
        }
        {
            let guard = Arc::new(Mutex::new(RestoreGuard::new(
                &baselines.borrow(),
                Some(chosen.id),
                gui_enabled,
            )));
            *restore_guard.borrow_mut() = Some(guard.clone());
            let weak_guard: Weak<Mutex<RestoreGuard>> = Arc::downgrade(&guard);
            std::panic::set_hook(Box::new(move |_| {
                elogln(gui_enabled, "panic: restoring volumes");
                if let Some(guard) = weak_guard.upgrade() {
                    let mut guard = guard.lock().unwrap();
                    let _ = guard.restore();
                }
            }));
            if !gui_enabled {
                *control_mode.borrow_mut() = ControlMode::ManualDucked;
                let mut guard = guard.lock().unwrap();
                guard.apply_duck_logged(duck_factor, "duck init", true);
            } else {
                *control_mode.borrow_mut() = ControlMode::ManualRestored;
            }
        }
    }

    // energy init
    let energy_atomic = Arc::new(AtomicF32::new(0.0));
    let vad_state: Rc<RefCell<VadState>> = Rc::new(RefCell::new(VadState::new(!gui_enabled)));

    // capture setup
    let audio_seen = Arc::new(AtomicBool::new(false));
    let capture_frames = Arc::new(AtomicU64::new(0));
    let voice_id_opt = *voice_source_id.borrow();
    let voice_node_opt = voice_source_node.borrow().clone();
    let voice_serial_opt = voice_source_serial.borrow().clone();
    let _capture = setup_capture(
        &core,
        voice_id_opt,
        voice_node_opt,
        voice_serial_opt,
        energy_atomic.clone(),
        audio_seen.clone(),
        capture_frames.clone(),
        gui_enabled,
    )?;

    // VAD timer
    let vad_timer = {
        let voice_label_t = voice_source_label.clone();
        let voice_reason_t = voice_source_reason.clone();
        let vad_t = vad_state.clone();
        let guard_t = restore_guard.clone();
        let mode_t = control_mode.clone();
        let threshold = opts.threshold;
        let attack_ms = opts.attack;
        let hold_ms = opts.hold;
        let energy_t = energy_atomic.clone();
        let quit_flag_t = quit_requested.clone();
        let gui_log_t = gui_log.clone();
        let duck_factor_live = duck_factor_live.clone();
        let threshold_live = threshold_live.clone();
        let hold_live = hold_live.clone();
        // heartbeat source
        let audio_seen_t = audio_seen.clone();
        let capture_frames_t = capture_frames.clone();
        let audio_logged = Arc::new(AtomicBool::new(false));
        let audio_logged_t = audio_logged.clone();
        let idle_warned = Arc::new(AtomicBool::new(false));
        let idle_warned_t = idle_warned.clone();
        let last_log = Rc::new(RefCell::new(Instant::now()));
        let last_log_t = last_log.clone();
        let start_time = Rc::new(Instant::now());
        let start_time_t = start_time.clone();
        let timer = mainloop.loop_().add_timer(move |_| {
            if gui_enabled {
                handle_gui_input(
                    &guard_t,
                    &mode_t,
                    &vad_t,
                    &gui_log_t,
                    &quit_flag_t,
                    &threshold_live,
                    &duck_factor_live,
                    &hold_live,
                    gui_enabled,
                );
            }
            if audio_seen_t.load(Ordering::Relaxed) && !audio_logged_t.swap(true, Ordering::Relaxed)
            {
                logln(gui_enabled, "Audio-Frames empfangen (Capture aktiv).");
            }
            let now = Instant::now();
            if now.duration_since(*last_log_t.borrow()) >= Duration::from_secs(1) {
                let seen = capture_frames_t.load(Ordering::Relaxed);
                logln(gui_enabled, format!("capture frames seen = {}", seen));
                *last_log_t.borrow_mut() = now;
            }
            if now.duration_since(*start_time_t) >= Duration::from_secs(3)
                && capture_frames_t.load(Ordering::Relaxed) == 0
                && !idle_warned_t.swap(true, Ordering::Relaxed)
                && !gui_enabled
            {
                logln(gui_enabled, "CAPTURE IDLE (no frames) -> likely not linked");
            }

            let energy = energy_t.load();
            let now = Instant::now();
            let mode = *mode_t.borrow();
            let threshold = if gui_enabled {
                *threshold_live.borrow()
            } else {
                threshold
            };
            let hold_ms = if gui_enabled {
                *hold_live.borrow()
            } else {
                hold_ms
            };
            let hold_ms_effective = if hold_ms < 300 { 300 } else { hold_ms };
            let duck_factor_now = if gui_enabled {
                *duck_factor_live.borrow()
            } else {
                duck_factor
            };

            let snapshot = if let Some(guard_ref) = guard_t.borrow().as_ref() {
                let mut guard = guard_ref.lock().unwrap();
                let mut vad = vad_t.borrow_mut();
                let mut log_fn = |msg: String| {
                    if gui_enabled {
                        gui_log_t.borrow_mut().push(msg);
                    } else {
                        logln(gui_enabled, msg);
                    }
                };
                auto_vad_step(
                    mode,
                    energy,
                    threshold,
                    now,
                    &mut vad,
                    &mut guard,
                    duck_factor_now,
                    &mut log_fn,
                    attack_ms,
                    hold_ms_effective,
                )
            } else {
                return;
            };

            if gui_enabled {
                let duck_factor_live = *duck_factor_live.borrow();
                let threshold_live = *threshold_live.borrow();
                let label = voice_label_t.borrow().clone();
                let reason = voice_reason_t.borrow().clone();
                let log = gui_log_t.borrow();
                render_gui(
                    label,
                    reason,
                    mode,
                    &snapshot,
                    energy,
                    threshold_live,
                    duck_factor_live,
                    hold_ms,
                    &log,
                );
            }
        });
        timer
            .update_timer(
                Some(Duration::from_millis(50)),
                Some(Duration::from_millis(50)),
            )
            .into_result()?;
        timer
    };
    let _vad_timer = vad_timer;

    // heartbeat
    let heartbeat_timer = {
        let mainloop_t = mainloop.clone();
        let quit_flag_t = quit_requested.clone();
        let start_time = Instant::now();
        let pid = std::process::id();
        let frames_t = capture_frames.clone();
        let timer = mainloop.loop_().add_timer(move |_| {
            if !gui_enabled {
                let elapsed = start_time.elapsed().as_secs();
                let frames = frames_t.load(Ordering::Relaxed);
                elogln(
                    gui_enabled,
                    format!("HEARTBEAT pid={} t={} frames={}", pid, elapsed, frames),
                );
            }
            if quit_flag_t.load(Ordering::Relaxed) {
                elogln(gui_enabled, "quit requested");
                mainloop_t.quit();
            }
        });
        timer
            .update_timer(Some(Duration::from_secs(1)), Some(Duration::from_secs(1)))
            .into_result()?;
        timer
    };
    let _heartbeat_timer = heartbeat_timer;

    let _gui_mode_guard = if gui_enabled { gui_mode_guard } else { None };

    logln(gui_enabled, "\nLive‑Betrieb … (Ctrl+C zum Beenden)\n");
    mainloop.run();
    elogln(gui_enabled, "mainloop exited");
    Ok(())
}
