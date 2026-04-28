use anyhow::{bail, Context, Result};
use ksni::blocking::{Handle, TrayMethods};
use ksni::menu::{CheckmarkItem, Disposition, StandardItem, SubMenu};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::config::Config;
use crate::duck::{self, DuckingEvent, DuckingOptions, DuckingSettings, SharedDuckingSettings};
use crate::icons;
use crate::identity::{AudioIdentity, ConfiguredSource};
use crate::pulse::{PulseCtl, SinkInput};
use crate::shell::SystemRunner;

#[derive(Debug, Copy, Clone)]
pub struct TrayOptions {
    pub settings: DuckingSettings,
}

#[derive(Debug, Clone)]
enum TrayCommand {
    Toggle,
    SelectSource(u32),
    OpenTuner,
    Quit,
}

#[derive(Debug, Clone)]
enum WorkerEvent {
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
    Stopped,
    Error(String),
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum TrayRunState {
    Idle,
    Waiting,
    Starting,
    Neutral,
    Ducked,
    Stopping,
    Error,
}

#[derive(Debug)]
struct DuckingWorker {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

#[derive(Debug)]
struct SingleInstanceGuard {
    path: PathBuf,
    pid: u32,
    _file: File,
}

impl SingleInstanceGuard {
    fn acquire() -> Result<Self> {
        let path = lock_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "create runtime directory for tray lock: {}",
                    parent.display()
                )
            })?;
        }

        loop {
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut file) => {
                    let pid = std::process::id();
                    writeln!(file, "{pid}")
                        .with_context(|| format!("Tray-Lock schreiben: {}", path.display()))?;
                    return Ok(Self {
                        path,
                        pid,
                        _file: file,
                    });
                }
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                    if let Some(pid) = read_lock_pid(&path) {
                        if pid_alive(pid) {
                            bail!(
                                "pw-duck is already running as process {pid}; not starting a second tray instance"
                            );
                        }
                    }
                    let _ = fs::remove_file(&path);
                }
                Err(err) => {
                    return Err(err)
                        .with_context(|| format!("Tray-Lock anlegen: {}", path.display()));
                }
            }
        }
    }
}

impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        if read_lock_pid(&self.path) == Some(self.pid) {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn lock_path() -> PathBuf {
    if let Some(runtime_dir) = std::env::var_os("XDG_RUNTIME_DIR") {
        return PathBuf::from(runtime_dir).join("pw-duck/tray.lock");
    }
    std::env::temp_dir().join(format!(
        "pw-duck-{}-tray.lock",
        std::env::var("USER").unwrap_or_else(|_| "user".into())
    ))
}

fn read_lock_pid(path: &PathBuf) -> Option<u32> {
    let mut text = String::new();
    File::open(path).ok()?.read_to_string(&mut text).ok()?;
    text.trim().parse().ok()
}

fn pid_alive(pid: u32) -> bool {
    PathBuf::from(format!("/proc/{pid}")).exists()
}

impl DuckingWorker {
    fn spawn(settings: SharedDuckingSettings, tx: Sender<WorkerEvent>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let handle = thread::spawn(move || {
            let runner = SystemRunner;
            let result = duck::run_until_stopped(
                &runner,
                &thread_stop,
                DuckingOptions {
                    settings,
                    reload_config: true,
                },
                |event| {
                    let worker_event = match event {
                        DuckingEvent::WaitingForSource => WorkerEvent::WaitingForSource,
                        DuckingEvent::Started {
                            label,
                            threshold,
                            start_threshold,
                            hold_ms,
                        } => WorkerEvent::Started {
                            label,
                            threshold,
                            start_threshold,
                            hold_ms,
                        },
                        DuckingEvent::VoiceActive { level, percent } => {
                            WorkerEvent::VoiceActive { level, percent }
                        }
                        DuckingEvent::VoiceInactive { level } => {
                            WorkerEvent::VoiceInactive { level }
                        }
                    };
                    let _ = tx.send(worker_event);
                },
            );

            match result {
                Ok(()) => {
                    let _ = tx.send(WorkerEvent::Stopped);
                }
                Err(err) => {
                    let _ = tx.send(WorkerEvent::Error(format!("{err:#}")));
                }
            }
        });

        Self {
            stop,
            handle: Some(handle),
        }
    }

    fn stop(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }

    fn request_stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    fn join_if_finished(&mut self) -> bool {
        if self
            .handle
            .as_ref()
            .is_some_and(|handle| handle.is_finished())
        {
            if let Some(handle) = self.handle.take() {
                let _ = handle.join();
            }
            true
        } else {
            false
        }
    }
}

#[derive(Clone)]
struct PwDuckTray {
    command_tx: Sender<TrayCommand>,
    settings: SharedDuckingSettings,
    state: TrayRunState,
    source_label: Option<String>,
    message: String,
    quitting: bool,
}

impl PwDuckTray {
    fn new(command_tx: Sender<TrayCommand>, settings: SharedDuckingSettings) -> Self {
        Self {
            command_tx,
            settings,
            state: TrayRunState::Idle,
            source_label: configured_source_label(),
            message: "Ready".to_string(),
            quitting: false,
        }
    }

    fn send(&self, command: TrayCommand) {
        let _ = self.command_tx.send(command);
    }

    fn set_pending_toggle_state(&mut self) {
        match self.state {
            TrayRunState::Idle | TrayRunState::Error => {
                self.state = TrayRunState::Starting;
                self.message = "Starting ducking …".to_string();
            }
            TrayRunState::Waiting
            | TrayRunState::Starting
            | TrayRunState::Neutral
            | TrayRunState::Ducked => {
                self.state = TrayRunState::Stopping;
                self.message = "Stopping ducking …".to_string();
            }
            TrayRunState::Stopping => {}
        }
    }

    fn source_menu(&self) -> Vec<ksni::MenuItem<Self>> {
        let runner = SystemRunner;
        let pulse = PulseCtl::new(&runner);
        let mut items = Vec::new();

        match pulse.sink_inputs() {
            Ok(inputs) => {
                for input in inputs
                    .into_iter()
                    .filter(|input| input.identity().is_playback_stream())
                {
                    let index = input.index;
                    let identity = input.identity();
                    let label = source_menu_label(&input, &identity);
                    let tx = self.command_tx.clone();
                    items.push(
                        StandardItem {
                            label,
                            activate: Box::new(move |this: &mut Self| {
                                this.message = format!("Saving source #{index} …");
                                let _ = tx.send(TrayCommand::SelectSource(index));
                            }),
                            ..Default::default()
                        }
                        .into(),
                    );
                }
            }
            Err(err) => items.push(
                StandardItem {
                    label: format!("Cannot read sources: {err}"),
                    enabled: false,
                    ..Default::default()
                }
                .into(),
            ),
        }

        if items.is_empty() {
            items.push(
                StandardItem {
                    label: "No playback streams visible".into(),
                    enabled: false,
                    ..Default::default()
                }
                .into(),
            );
        }

        items
    }

    fn controls_summary(&self) -> String {
        let settings = read_settings(&self.settings);
        format!(
            "Tuning: Duck {}%, Sens {:.4}, Hold {}ms",
            settings.duck_percent, settings.vad_threshold, settings.hold_ms
        )
    }

    fn visible_state_value(&self) -> &'static str {
        match self.state {
            TrayRunState::Waiting
            | TrayRunState::Starting
            | TrayRunState::Neutral
            | TrayRunState::Ducked => "ON",
            TrayRunState::Idle | TrayRunState::Stopping | TrayRunState::Error => "OFF",
        }
    }

    fn visible_state_label(&self) -> String {
        format!("Ducking {}", self.visible_state_value())
    }

    fn can_toggle(&self) -> bool {
        !matches!(self.state, TrayRunState::Starting | TrayRunState::Stopping) && !self.quitting
    }

    fn request_toggle(&mut self) {
        if !self.can_toggle() {
            return;
        }
        self.set_pending_toggle_state();
        self.send(TrayCommand::Toggle);
    }

    fn ducking_switch_checked(&self) -> bool {
        matches!(
            self.state,
            TrayRunState::Waiting
                | TrayRunState::Starting
                | TrayRunState::Neutral
                | TrayRunState::Ducked
        )
    }
}

impl ksni::Tray for PwDuckTray {
    fn id(&self) -> String {
        "pw-duck".into()
    }

    fn category(&self) -> ksni::Category {
        ksni::Category::ApplicationStatus
    }

    fn title(&self) -> String {
        self.visible_state_label()
    }

    fn status(&self) -> ksni::Status {
        if self.quitting {
            ksni::Status::Passive
        } else if self.state == TrayRunState::Error {
            ksni::Status::NeedsAttention
        } else {
            ksni::Status::Active
        }
    }

    fn icon_theme_path(&self) -> String {
        icons::icon_theme_path()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    fn icon_name(&self) -> String {
        String::new()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        icons::tray_icon_pixmap()
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            icon_pixmap: icons::tray_icon_pixmap(),
            title: self.title(),
            description: format!(
                "{}\nDetails: {}\nSource: {}",
                self.visible_state_label(),
                self.message,
                self.source_label.as_deref().unwrap_or("not selected")
            ),
            ..Default::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.request_toggle();
    }

    fn secondary_activate(&mut self, _x: i32, _y: i32) {
        self.request_toggle();
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        let tx_quit = self.command_tx.clone();

        vec![
            StandardItem {
                label: "Info:".into(),
                enabled: false,
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: format!("Ducking: {}", self.visible_state_value()),
                enabled: false,
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: format!("Details: {}", self.message),
                enabled: false,
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: format!(
                    "Source: {}",
                    self.source_label.as_deref().unwrap_or("not selected")
                ),
                enabled: false,
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: self.controls_summary(),
                enabled: false,
                ..Default::default()
            }
            .into(),
            ksni::MenuItem::Separator,
            StandardItem {
                label: "Controls:".into(),
                enabled: false,
                ..Default::default()
            }
            .into(),
            CheckmarkItem {
                label: "Ducking".into(),
                enabled: self.can_toggle(),
                checked: self.ducking_switch_checked(),
                activate: Box::new(move |this: &mut Self| {
                    this.request_toggle();
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: tuner_menu_label().into(),
                enabled: tuner_menu_enabled(),
                activate: Box::new(move |this: &mut Self| {
                    this.message = "Opening tuner …".into();
                    let _ = this.command_tx.send(TrayCommand::OpenTuner);
                }),
                ..Default::default()
            }
            .into(),
            SubMenu {
                label: "Source: choose".into(),
                submenu: self.source_menu(),
                ..Default::default()
            }
            .into(),
            ksni::MenuItem::Separator,
            StandardItem {
                label: "Quit".into(),
                disposition: Disposition::Alert,
                activate: Box::new(move |this: &mut Self| {
                    this.state = TrayRunState::Stopping;
                    this.message = "Quitting …".into();
                    let _ = tx_quit.send(TrayCommand::Quit);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

fn tuner_menu_label() -> &'static str {
    if cfg!(feature = "gui") {
        "Tuner: open"
    } else {
        "Tuner: GUI build only"
    }
}

fn tuner_menu_enabled() -> bool {
    cfg!(feature = "gui")
}

fn read_settings(settings: &SharedDuckingSettings) -> DuckingSettings {
    if let Ok(config) = Config::load_or_default() {
        return DuckingSettings {
            duck_percent: config.duck_percent,
            vad_threshold: config.vad_threshold,
            hold_ms: config.hold_ms,
        }
        .clamped();
    }

    settings
        .lock()
        .map(|settings| (*settings).clamped())
        .unwrap_or(DuckingSettings {
            duck_percent: 25,
            vad_threshold: 0.01,
            hold_ms: 700,
        })
}

fn persist_settings(settings: DuckingSettings) -> Result<()> {
    let mut config = Config::load_or_default()?;
    let settings = settings.clamped();
    config.duck_percent = settings.duck_percent;
    config.vad_threshold = settings.vad_threshold;
    config.hold_ms = settings.hold_ms;
    config.save()
}

fn open_tuner(handle: &Handle<PwDuckTray>) {
    let result = spawn_tuner();
    handle.update(|tray: &mut PwDuckTray| match result {
        Ok(()) => {
            tray.message = "Tuner opened".into();
            if tray.state == TrayRunState::Error {
                tray.state = TrayRunState::Idle;
            }
        }
        Err(err) => {
            tray.state = TrayRunState::Error;
            tray.message = format!("Could not open tuner: {err:#}");
        }
    });
}

fn spawn_tuner() -> Result<()> {
    #[cfg(feature = "gui")]
    {
        let exe = std::env::current_exe().context("get current executable path")?;
        let mut child = std::process::Command::new(exe)
            .arg("tune-gui")
            .spawn()
            .context("start tuner GUI")?;
        thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(())
    }

    #[cfg(not(feature = "gui"))]
    {
        bail!("the tuner GUI is not included in this build; use `pw-duck tune` in a terminal or build with `--features gui`")
    }
}

pub fn run(options: TrayOptions) -> Result<()> {
    let _single_instance = SingleInstanceGuard::acquire()?;
    let (command_tx, command_rx) = mpsc::channel();
    let (worker_tx, worker_rx) = mpsc::channel();
    let settings = duck::shared_settings(options.settings);
    persist_settings(options.settings)?;
    let tray = PwDuckTray::new(command_tx.clone(), settings.clone());
    let handle = tray
        .disable_dbus_name(use_unique_name_sni())
        .assume_sni_available(true)
        .spawn()
        .context("start StatusNotifier tray")?;

    {
        let tx = command_tx.clone();
        ctrlc::set_handler(move || {
            let _ = tx.send(TrayCommand::Quit);
        })?;
    }

    let mut worker: Option<DuckingWorker> = None;
    let mut quit = false;

    while !quit {
        drain_worker_events(&handle, &worker_rx, &mut worker);

        match command_rx.recv_timeout(Duration::from_millis(200)) {
            Ok(TrayCommand::Toggle) => {
                if worker.is_some() {
                    stop_worker(&handle, &mut worker);
                } else {
                    start_worker(&handle, &mut worker, settings.clone(), worker_tx.clone());
                }
            }
            Ok(TrayCommand::SelectSource(index)) => {
                select_source(index, &handle);
            }
            Ok(TrayCommand::OpenTuner) => {
                open_tuner(&handle);
            }
            Ok(TrayCommand::Quit) => {
                quit = true;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }

        if worker
            .as_mut()
            .is_some_and(|worker| worker.join_if_finished())
        {
            worker = None;
        }
    }

    if let Some(worker) = worker.take() {
        handle.update(|tray: &mut PwDuckTray| {
            tray.state = TrayRunState::Stopping;
            tray.message = "Stopping ducking …".into();
        });
        worker.stop();
    }

    prepare_tray_shutdown(&handle);
    handle.shutdown().wait();
    std::thread::sleep(Duration::from_millis(300));
    Ok(())
}

fn use_unique_name_sni() -> bool {
    if let Some(value) = std::env::var_os("PW_DUCK_SNI_UNIQUE_NAME") {
        return value != "0" && value != "false" && value != "no";
    }

    status_notifier_watcher_is_ashell()
}

fn status_notifier_watcher_is_ashell() -> bool {
    let Ok(output) = Command::new("busctl")
        .args([
            "--user",
            "--no-pager",
            "status",
            "org.kde.StatusNotifierWatcher",
        ])
        .output()
    else {
        return false;
    };
    if !output.status.success() {
        return false;
    }

    let text = String::from_utf8_lossy(&output.stdout);
    text.lines().any(|line| {
        line == "Comm=ashell"
            || line == "CommandLine=ashell"
            || line.starts_with("CommandLine=ashell ")
    })
}

fn prepare_tray_shutdown(handle: &Handle<PwDuckTray>) {
    handle.update(|tray: &mut PwDuckTray| {
        tray.quitting = true;
        tray.state = TrayRunState::Idle;
        tray.message = "Quit".into();
    });
    std::thread::sleep(Duration::from_millis(150));
}

fn start_worker(
    handle: &Handle<PwDuckTray>,
    worker: &mut Option<DuckingWorker>,
    settings: SharedDuckingSettings,
    worker_tx: Sender<WorkerEvent>,
) {
    handle.update(|tray: &mut PwDuckTray| {
        tray.state = TrayRunState::Starting;
        tray.message = "Starting ducking …".into();
    });
    *worker = Some(DuckingWorker::spawn(settings, worker_tx));
}

fn stop_worker(handle: &Handle<PwDuckTray>, worker: &mut Option<DuckingWorker>) {
    if let Some(worker) = worker.take() {
        handle.update(|tray: &mut PwDuckTray| {
            tray.state = TrayRunState::Stopping;
            tray.message = "Stopping ducking …".into();
        });
        worker.request_stop();
        worker.stop();
        handle.update(|tray: &mut PwDuckTray| {
            tray.state = TrayRunState::Idle;
            tray.message = "Off".into();
        });
    }
}

fn drain_worker_events(
    handle: &Handle<PwDuckTray>,
    worker_rx: &Receiver<WorkerEvent>,
    worker: &mut Option<DuckingWorker>,
) {
    while let Ok(event) = worker_rx.try_recv() {
        match event {
            WorkerEvent::WaitingForSource => {
                handle.update(|tray: &mut PwDuckTray| {
                    tray.state = TrayRunState::Waiting;
                    tray.message = "Waiting for configured voice source".into();
                });
            }
            WorkerEvent::Started {
                label,
                threshold,
                start_threshold,
                hold_ms,
            } => {
                handle.update(|tray: &mut PwDuckTray| {
                    tray.state = TrayRunState::Neutral;
                    tray.message = format!(
                        "Ready: {label}, threshold={threshold:.4}, start={start_threshold:.4}, hold={hold_ms}ms"
                    );
                });
            }
            WorkerEvent::VoiceActive { level, percent } => {
                handle.update(|tray: &mut PwDuckTray| {
                    tray.state = TrayRunState::Ducked;
                    tray.message = format!("Voice active: level={level:.4}, ducking {percent}%");
                });
            }
            WorkerEvent::VoiceInactive { level } => {
                handle.update(|tray: &mut PwDuckTray| {
                    tray.state = TrayRunState::Neutral;
                    tray.message = format!("Voice inactive: level={level:.4}");
                });
            }
            WorkerEvent::Stopped => {
                if let Some(running_worker) = worker.take() {
                    running_worker.stop();
                }
                handle.update(|tray: &mut PwDuckTray| {
                    tray.state = TrayRunState::Idle;
                    tray.message = "Off".into();
                });
            }
            WorkerEvent::Error(err) => {
                if let Some(running_worker) = worker.take() {
                    running_worker.stop();
                }
                handle.update(|tray: &mut PwDuckTray| {
                    tray.state = TrayRunState::Error;
                    tray.message = err;
                });
            }
        }
    }
}

fn select_source(index: u32, handle: &Handle<PwDuckTray>) {
    let runner = SystemRunner;
    let pulse = PulseCtl::new(&runner);
    let result = (|| -> Result<String> {
        let input = pulse
            .sink_inputs()?
            .into_iter()
            .find(|input| input.index == index)
            .ok_or_else(|| anyhow::anyhow!("sink-input #{index} not found"))?;
        let identity = input.identity();
        if !identity.is_playback_stream() {
            anyhow::bail!("sink-input #{index} is not a playback stream");
        }
        let label = source_label(&identity);
        let mut config = Config::load_or_default()?;
        config.voice_source = Some(ConfiguredSource::from_identity(label.clone(), &identity));
        config.save()?;
        Ok(label)
    })();

    handle.update(|tray: &mut PwDuckTray| match result {
        Ok(label) => {
            tray.source_label = Some(label.clone());
            tray.message = format!("Source saved: {label}");
            if tray.state == TrayRunState::Error {
                tray.state = TrayRunState::Idle;
            }
        }
        Err(err) => {
            tray.state = TrayRunState::Error;
            tray.message = format!("Could not save source: {err:#}");
        }
    });
}

fn configured_source_label() -> Option<String> {
    Config::load_or_default()
        .ok()
        .and_then(|config| config.voice_source.and_then(|source| source.label))
}

fn source_menu_label(input: &SinkInput, identity: &AudioIdentity) -> String {
    let hint = if looks_like_voice_source(identity) {
        " voice?"
    } else {
        ""
    };
    format!(
        "#{}{} {} / {} / {}",
        input.index,
        hint,
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
