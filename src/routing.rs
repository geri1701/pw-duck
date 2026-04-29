use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use crate::identity::ConfiguredSource;
use crate::pipewire_sink::VirtualSink;
use crate::pulse::{PulseCtl, SinkInput};
use crate::shell::CommandRunner;

const PW_LINK_COMMAND_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone)]
pub struct RoutingOptions {
    pub virtual_sink_prefix: String,
    pub sink_appear_timeout: Duration,
}

impl Default for RoutingOptions {
    fn default() -> Self {
        Self {
            virtual_sink_prefix: "pw-duck".to_string(),
            sink_appear_timeout: Duration::from_secs(2),
        }
    }
}

pub struct RoutingEngine<'a, R: CommandRunner> {
    runner: &'a R,
    voice_source: ConfiguredSource,
}

impl<'a, R: CommandRunner> RoutingEngine<'a, R> {
    pub fn new(runner: &'a R, voice_source: ConfiguredSource) -> Self {
        Self {
            runner,
            voice_source,
        }
    }

    pub fn start(&mut self, options: RoutingOptions) -> Result<RouteSession<'a, R>> {
        if !self.voice_source.has_match_fields() {
            anyhow::bail!("configured voice source contains no stable match fields");
        }

        let pulse = PulseCtl::new(self.runner);
        let original_default_sink_name = pulse.default_sink_name()?;
        let real_sink = self.real_sink_for_session(&pulse)?;
        let real_sink_name = real_sink.name.clone();

        let virtual_sink = VirtualSink::create(&options.virtual_sink_prefix)?;
        let virtual_sink_name = virtual_sink.name().to_string();
        let virtual_pulse_sink =
            pulse.wait_for_sink(&virtual_sink_name, options.sink_appear_timeout)?;
        pulse
            .set_default_sink(&original_default_sink_name)
            .with_context(|| format!("restore default sink after creating {virtual_sink_name}"))?;

        let links = PipeWireLinks::new(self.runner);
        let monitor_links =
            links.link_stereo_monitor_to_sink(&virtual_sink_name, &real_sink_name)?;

        let mut session = RouteSession {
            runner: self.runner,
            virtual_sink,
            virtual_sink_name,
            virtual_sink_index: virtual_pulse_sink.index,
            original_default_sink_name,
            real_sink_name,
            real_sink_index: real_sink.index,
            moved_inputs: BTreeMap::new(),
            monitor_links,
            stopped: false,
        };

        session.route_existing_outputs(&self.voice_source)?;
        session.set_neutral()?;
        Ok(session)
    }

    fn real_sink_for_session(&self, pulse: &PulseCtl<'a, R>) -> Result<crate::pulse::Sink> {
        if let Some(voice_input) = pulse.sink_inputs()?.into_iter().find(|input| {
            input
                .identity()
                .matches_configured_source(&self.voice_source)
        }) {
            return pulse
                .sink_by_index(voice_input.sink)?
                .with_context(|| format!("voice source sink #{} not found", voice_input.sink));
        }

        let default_sink_name = pulse.default_sink_name()?;
        pulse
            .sink_by_name(&default_sink_name)?
            .with_context(|| format!("default sink {default_sink_name} not found"))
    }
}

pub struct RouteSession<'a, R: CommandRunner> {
    runner: &'a R,
    virtual_sink: VirtualSink,
    virtual_sink_name: String,
    virtual_sink_index: u32,
    original_default_sink_name: String,
    real_sink_name: String,
    real_sink_index: u32,
    moved_inputs: BTreeMap<u32, u32>,
    monitor_links: Vec<PortLink>,
    stopped: bool,
}

impl<'a, R: CommandRunner> RouteSession<'a, R> {
    pub fn set_duck_percent(&self, percent: u8) -> Result<()> {
        PulseCtl::new(self.runner).set_source_volume_percent(&self.virtual_monitor_name(), percent)
    }

    pub fn set_neutral(&self) -> Result<()> {
        PulseCtl::new(self.runner).set_source_volume_percent(&self.virtual_monitor_name(), 100)
    }

    fn virtual_monitor_name(&self) -> String {
        format!("{}.monitor", self.virtual_sink_name)
    }

    pub fn route_existing_outputs(&mut self, voice_source: &ConfiguredSource) -> Result<usize> {
        let pulse = PulseCtl::new(self.runner);
        let inputs = pulse.sink_inputs()?;
        let mut moved = 0;

        for input in inputs {
            if is_routed_target(&input, voice_source, self.virtual_sink_index) {
                self.moved_inputs
                    .entry(input.index)
                    .or_insert(self.real_sink_index);
                continue;
            }

            if !should_route(&input, voice_source, self.real_sink_index) {
                continue;
            }

            self.moved_inputs.entry(input.index).or_insert(input.sink);
            pulse.move_sink_input(input.index, &self.virtual_sink_name)?;
            moved += 1;
        }

        Ok(moved)
    }

    pub fn stop(&mut self) -> Result<()> {
        if self.stopped {
            return Ok(());
        }

        let pulse = PulseCtl::new(self.runner);
        let _ = self.set_neutral();
        let _ = pulse.set_default_sink(&self.original_default_sink_name);

        self.record_inputs_still_on_virtual_sink(&pulse)?;

        for (input_index, original_sink_index) in self.moved_inputs.clone() {
            if let Err(err) = pulse.move_sink_input_to_sink_index(input_index, original_sink_index)
            {
                eprintln!(
                    "restore sink-input #{input_index} to sink #{original_sink_index} failed: {err:#}; trying {}",
                    self.real_sink_name
                );
                let _ = pulse.move_sink_input(input_index, &self.real_sink_name);
            }
        }

        if let Err(err) = self.wait_until_virtual_sink_is_empty(&pulse, Duration::from_secs(3)) {
            eprintln!(
                "teardown warning: {err:#}; leaving virtual sink {} alive to avoid terminating audio streams",
                self.virtual_sink_name
            );
            self.virtual_sink.abandon();
            let _ = pulse.set_default_sink(&self.original_default_sink_name);
            self.stopped = true;
            return Ok(());
        }

        let links = PipeWireLinks::new(self.runner);
        for link in self.monitor_links.drain(..) {
            let _ = links.unlink(&link);
        }

        self.virtual_sink.destroy()?;
        let _ = pulse.set_default_sink(&self.original_default_sink_name);
        self.moved_inputs.clear();
        self.stopped = true;
        Ok(())
    }

    fn record_inputs_still_on_virtual_sink(&mut self, pulse: &PulseCtl<'a, R>) -> Result<()> {
        for input in pulse.sink_inputs_on_sink(self.virtual_sink_index)? {
            self.moved_inputs
                .entry(input.index)
                .or_insert(self.real_sink_index);
        }
        Ok(())
    }

    fn wait_until_virtual_sink_is_empty(
        &mut self,
        pulse: &PulseCtl<'a, R>,
        timeout: Duration,
    ) -> Result<()> {
        let deadline = Instant::now() + timeout;
        loop {
            let inputs = pulse.sink_inputs_on_sink(self.virtual_sink_index)?;
            if inputs.is_empty() {
                return Ok(());
            }

            for input in inputs {
                self.moved_inputs
                    .entry(input.index)
                    .or_insert(self.real_sink_index);
                let _ = pulse.move_sink_input(input.index, &self.real_sink_name);
            }

            if Instant::now() >= deadline {
                let remaining = pulse
                    .sink_inputs_on_sink(self.virtual_sink_index)?
                    .into_iter()
                    .map(|input| format!("#{}", input.index))
                    .collect::<Vec<_>>()
                    .join(", ");
                anyhow::bail!(
                    "refusing to destroy virtual sink {}; still has sink-input(s): {remaining}",
                    self.virtual_sink_name
                );
            }

            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl<R: CommandRunner> Drop for RouteSession<'_, R> {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

fn should_route(input: &SinkInput, voice_source: &ConfiguredSource, real_sink_index: u32) -> bool {
    let identity = input.identity();
    identity.is_playback_stream()
        && input.sink == real_sink_index
        && !identity.matches_configured_source(voice_source)
}

fn is_routed_target(
    input: &SinkInput,
    voice_source: &ConfiguredSource,
    virtual_sink_index: u32,
) -> bool {
    let identity = input.identity();
    identity.is_playback_stream()
        && input.sink == virtual_sink_index
        && !identity.matches_configured_source(voice_source)
}

#[derive(Debug, Clone)]
struct PortLink {
    output: String,
    input: String,
}

struct PipeWireLinks<'a, R: CommandRunner> {
    runner: &'a R,
}

impl<'a, R: CommandRunner> PipeWireLinks<'a, R> {
    fn new(runner: &'a R) -> Self {
        Self { runner }
    }

    fn link_stereo_monitor_to_sink(
        &self,
        virtual_sink: &str,
        real_sink: &str,
    ) -> Result<Vec<PortLink>> {
        let pairs = [
            PortLink {
                output: format!("{virtual_sink}:monitor_FL"),
                input: format!("{real_sink}:playback_FL"),
            },
            PortLink {
                output: format!("{virtual_sink}:monitor_FR"),
                input: format!("{real_sink}:playback_FR"),
            },
        ];

        let mut created = Vec::new();
        for link in pairs {
            self.runner
                .status_with_timeout(
                    "pw-link",
                    &["-w", &link.output, &link.input],
                    PW_LINK_COMMAND_TIMEOUT,
                )
                .with_context(|| format!("link {} -> {}", link.output, link.input))?;
            created.push(link);
        }
        Ok(created)
    }

    fn unlink(&self, link: &PortLink) -> Result<()> {
        self.runner
            .status_with_timeout(
                "pw-link",
                &["-d", &link.output, &link.input],
                PW_LINK_COMMAND_TIMEOUT,
            )
            .with_context(|| format!("unlink {} -> {}", link.output, link.input))
    }
}
