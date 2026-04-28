use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "pw-duck", author, version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Clone, Subcommand)]
pub enum Command {
    /// Read-only: show current default sink and playback streams.
    Status,
    /// Print the config file path.
    ConfigPath,
    /// Create a default config if none exists yet.
    InitConfig,
    /// Read-only: list current playback streams as selectable voice-source candidates.
    Sources,
    /// Store one current playback stream as the configured voice source.
    SelectSource {
        /// pactl sink-input index shown by `sources`.
        sink_input_index: u32,
    },
    /// Open a small terminal tuner for sensitivity, ducking volume and hold time.
    Tune,
    /// Open a small graphical tuner for sensitivity, ducking volume and hold time (requires the `gui` feature).
    TuneGui,
    /// Start the StatusNotifier tray UI. Left click toggles ducking.
    Tray {
        /// Virtual monitor volume while voice is active.
        #[arg(long)]
        duck_percent: Option<u8>,
        /// Base RMS noise threshold. Voice activates above roughly 2x this value.
        #[arg(long)]
        vad_threshold: Option<f32>,
        /// How long to keep ducking active after voice falls below the release threshold.
        #[arg(long)]
        hold_ms: Option<u64>,
    },
    /// Route streams through a temporary virtual sink until Ctrl+C.
    Route {
        /// Virtual monitor volume while voice is active.
        #[arg(long)]
        duck_percent: Option<u8>,
        /// Base RMS noise threshold. Voice activates above roughly 2x this value.
        #[arg(long)]
        vad_threshold: Option<f32>,
        /// How long to keep ducking active after voice falls below the release threshold.
        #[arg(long)]
        hold_ms: Option<u64>,
        /// Required safety acknowledgement because this mutates the live audio graph.
        #[arg(long)]
        yes_really_route: bool,
    },
    /// Debug/PoC path: route streams through a temporary virtual sink for N seconds.
    RouteOnce {
        /// How long to keep the temporary route alive.
        #[arg(long, default_value_t = 5)]
        seconds: u64,
        /// Temporary virtual sink volume while ducked.
        #[arg(long)]
        duck_percent: Option<u8>,
        /// Required safety acknowledgement because this mutates the live audio graph.
        #[arg(long)]
        yes_really_route: bool,
    },
}

impl Default for Command {
    fn default() -> Self {
        Self::Tray {
            duck_percent: None,
            vad_threshold: None,
            hold_ms: None,
        }
    }
}
