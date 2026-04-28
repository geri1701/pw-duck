use anyhow::{Context, Result};
use pipewire as pw;
use std::cell::Cell;
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct VirtualSink {
    mainloop: pw::main_loop::MainLoopRc,
    core: pw::core::CoreRc,
    _context: pw::context::ContextRc,
    node: Option<pw::node::Node>,
    name: String,
}

impl VirtualSink {
    pub fn create(prefix: &str) -> Result<Self> {
        pw::init();

        let mainloop = pw::main_loop::MainLoopRc::new(None).context("create PipeWire mainloop")?;
        let context =
            pw::context::ContextRc::new(&mainloop, None).context("create PipeWire context")?;
        let core = context.connect_rc(None).context("connect PipeWire core")?;
        let name = unique_node_name(prefix);

        let node = core
            .create_object::<pw::node::Node>(
                "adapter",
                &pw::properties::properties! {
                    "factory.name" => "support.null-audio-sink",
                    "node.name" => name.as_str(),
                    "node.description" => "pw-duck virtual ducking sink",
                    "media.class" => "Audio/Sink",
                    "audio.position" => "FL,FR",
                    "node.passive" => "true",
                    "priority.driver" => "1",
                    "priority.session" => "1"
                },
            )
            .context("create PipeWire virtual sink node")?;

        let sink = Self {
            mainloop,
            core,
            _context: context,
            node: Some(node),
            name,
        };
        sink.roundtrip()?;
        Ok(sink)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn destroy(&mut self) -> Result<()> {
        if let Some(node) = self.node.take() {
            self.core
                .destroy_object(node)
                .context("destroy PipeWire virtual sink node")?;
            self.roundtrip()?;
        }
        Ok(())
    }

    /// Keep the remote node alive instead of explicitly destroying it on drop.
    ///
    /// This is a last-resort fail-safe for teardown: if a playback stream cannot
    /// be moved away from the virtual sink, destroying the sink can terminate the
    /// client stream. In that case it is safer to leak the temporary routing node
    /// than to kill user audio.
    pub fn abandon(&mut self) {
        self.node.take();
    }

    fn roundtrip(&self) -> Result<()> {
        let done = Rc::new(Cell::new(false));
        let done_clone = done.clone();
        let loop_clone = self.mainloop.clone();
        let pending = self.core.sync(0).context("PipeWire core sync")?;

        let _listener = self
            .core
            .add_listener_local()
            .done(move |id, seq| {
                if id == pw::core::PW_ID_CORE && seq == pending {
                    done_clone.set(true);
                    loop_clone.quit();
                }
            })
            .register();

        while !done.get() {
            self.mainloop.run();
        }
        Ok(())
    }
}

impl Drop for VirtualSink {
    fn drop(&mut self) {
        let _ = self.destroy();
    }
}

fn unique_node_name(prefix: &str) -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    format!("{prefix}-{millis}")
}
