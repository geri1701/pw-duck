use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::thread;
use std::time::{Duration, Instant};

use crate::identity::AudioIdentity;
use crate::shell::CommandRunner;

pub struct PulseCtl<'a, R: CommandRunner> {
    runner: &'a R,
}

impl<'a, R: CommandRunner> PulseCtl<'a, R> {
    pub fn new(runner: &'a R) -> Self {
        Self { runner }
    }

    pub fn default_sink_name(&self) -> Result<String> {
        Ok(self
            .runner
            .output("pactl", &["get-default-sink"])?
            .trim()
            .to_string())
    }

    pub fn set_default_sink(&self, sink_name: &str) -> Result<()> {
        self.runner
            .status("pactl", &["set-default-sink", sink_name])
    }

    pub fn sinks(&self) -> Result<Vec<Sink>> {
        let raw = self
            .runner
            .output("pactl", &["--format=json", "list", "sinks"])?;
        serde_json::from_str(&raw).context("failed to parse pactl sinks JSON")
    }

    pub fn sink_by_name(&self, name: &str) -> Result<Option<Sink>> {
        Ok(self.sinks()?.into_iter().find(|sink| sink.name == name))
    }

    pub fn sink_by_index(&self, index: u32) -> Result<Option<Sink>> {
        Ok(self.sinks()?.into_iter().find(|sink| sink.index == index))
    }

    pub fn wait_for_sink(&self, name: &str, timeout: Duration) -> Result<Sink> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(sink) = self.sink_by_name(name)? {
                return Ok(sink);
            }
            if Instant::now() >= deadline {
                anyhow::bail!("sink {name} did not appear within {:?}", timeout);
            }
            thread::sleep(Duration::from_millis(50));
        }
    }

    pub fn sink_inputs(&self) -> Result<Vec<SinkInput>> {
        let raw = self
            .runner
            .output("pactl", &["--format=json", "list", "sink-inputs"])?;
        if raw.trim().is_empty() {
            return Ok(Vec::new());
        }
        serde_json::from_str(&raw).context("failed to parse pactl sink-inputs JSON")
    }

    pub fn sink_inputs_on_sink(&self, sink_index: u32) -> Result<Vec<SinkInput>> {
        Ok(self
            .sink_inputs()?
            .into_iter()
            .filter(|input| input.sink == sink_index)
            .collect())
    }

    pub fn move_sink_input(&self, input_index: u32, sink_name: &str) -> Result<()> {
        self.runner.status(
            "pactl",
            &["move-sink-input", &input_index.to_string(), sink_name],
        )
    }

    pub fn move_sink_input_to_sink_index(&self, input_index: u32, sink_index: u32) -> Result<()> {
        self.runner.status(
            "pactl",
            &[
                "move-sink-input",
                &input_index.to_string(),
                &sink_index.to_string(),
            ],
        )
    }

    pub fn set_source_volume_percent(&self, source_name: &str, percent: u8) -> Result<()> {
        let clamped = percent.min(150);
        self.runner.status(
            "pactl",
            &["set-source-volume", source_name, &format!("{clamped}%")],
        )
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Sink {
    pub index: u32,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SinkInput {
    pub index: u32,
    pub sink: u32,
    #[serde(default)]
    pub properties: BTreeMap<String, String>,
}

impl SinkInput {
    pub fn identity(&self) -> AudioIdentity {
        AudioIdentity::from_props(&self.properties)
    }
}
