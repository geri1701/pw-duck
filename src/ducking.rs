use crate::logging::logln;
use regex::Regex;
use std::collections::HashMap;
use std::process::Command;

/// output stream info
#[derive(Debug, Clone)]
pub struct OutputStream {
    pub id: u32,
    pub serial: String,
    pub app: String,
    pub bin: String,
    pub pid: String,
    pub role: String,
    pub media: String,
    pub media_class: String,
    pub node: String,
    pub client: String,
}

/// case-insensitive contains
pub fn contains_ci(haystack: &str, needle: &str) -> bool {
    haystack
        .to_ascii_lowercase()
        .contains(&needle.to_ascii_lowercase())
}

/// voice heuristic
pub fn is_voice_candidate(s: &OutputStream) -> bool {
    // keywords
    s.app == "WEBRTC VoiceEngine"
        || contains_ci(&s.app, "voiceengine")
        || contains_ci(&s.node, "voiceengine")
        || contains_ci(&s.media, "playstream")
        || contains_ci(&s.bin, "discord")
        || contains_ci(&s.role, "communication")
}

/// get volume
pub fn wpctl_get_volume(id: u32) -> Option<f32> {
    let out = Command::new("wpctl")
        .args(["get-volume", &id.to_string()])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout);
    // first float
    let re = Regex::new(r"([0-9]+(?:\.[0-9]+)?)").ok()?;
    let cap = re.captures(&s)?;
    cap.get(1)?.as_str().parse::<f32>().ok()
}

/// set volume
pub fn wpctl_set_volume(id: u32, vol: f32) -> bool {
    let v = vol.clamp(0.0, 1.5).to_string();
    Command::new("wpctl")
        .args(["set-volume", &id.to_string(), &v])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[derive(Debug)]
pub struct RestoreGuard {
    baselines: HashMap<u32, f32>,
    ids: Vec<u32>,
    voice_id: Option<u32>,
    pub ducked: bool,
    gui_enabled: bool,
}

impl RestoreGuard {
    pub fn new(baselines: &HashMap<u32, f32>, voice_id: Option<u32>, gui_enabled: bool) -> Self {
        let mut ids: Vec<u32> = baselines
            .keys()
            .copied()
            .filter(|id| Some(*id) != voice_id)
            .collect();
        ids.sort_unstable();
        Self {
            baselines: baselines.clone(),
            ids,
            voice_id,
            ducked: false,
            gui_enabled,
        }
    }

    pub fn add_stream(&mut self, id: u32, baseline: f32) {
        if Some(id) == self.voice_id {
            return;
        }
        self.baselines.insert(id, baseline);
        if !self.ids.contains(&id) {
            self.ids.push(id);
        }
    }

    pub fn remove_stream(&mut self, id: u32) {
        self.baselines.remove(&id);
        self.ids.retain(|v| *v != id);
    }

    pub fn apply_duck(&mut self, factor: f32) -> usize {
        let failures = self.apply_factor(factor, None, false, true);
        self.ducked = factor < 0.999;
        failures
    }

    pub fn restore(&mut self) -> usize {
        let failures = self.apply_factor(1.0, None, false, false);
        self.ducked = false;
        failures
    }

    pub fn apply_duck_logged(&mut self, factor: f32, prefix: &str, log_per_stream: bool) -> usize {
        let failures = self.apply_factor(factor, Some(prefix), log_per_stream, true);
        self.ducked = factor < 0.999;
        failures
    }

    #[cfg(feature = "dev-tools")]
    pub fn restore_logged(&mut self, prefix: &str, log_per_stream: bool) -> usize {
        let failures = self.apply_factor(1.0, Some(prefix), log_per_stream, true);
        self.ducked = false;
        failures
    }

    fn apply_factor(
        &self,
        factor: f32,
        prefix: Option<&str>,
        log_per_stream: bool,
        warn_summary: bool,
    ) -> usize {
        let mut failures = 0;
        for id in self.ids.iter().copied() {
            let Some(base) = self.baselines.get(&id) else {
                continue;
            };
            let new_vol = (*base * factor).clamp(0.0, 1.5);
            let ok = wpctl_set_volume(id, new_vol);
            if log_per_stream {
                logln(
                    self.gui_enabled,
                    format!(
                        "{}: id={} base={} -> {} {}",
                        prefix.unwrap_or(""),
                        id,
                        base,
                        new_vol,
                        if ok { "ok" } else { "FAIL" }
                    ),
                );
            }
            if !ok {
                failures += 1;
            }
        }
        if warn_summary && failures > 0 {
            logln(
                self.gui_enabled,
                format!("warning: wpctl_set_volume failed for {failures} streams"),
            );
        }
        failures
    }
}

impl Drop for RestoreGuard {
    fn drop(&mut self) {
        if self.ducked {
            let failures = self.restore();
            if failures > 0 {
                logln(
                    self.gui_enabled,
                    format!("restore: failed for {failures} streams"),
                );
            } else {
                logln(self.gui_enabled, "restore: ok");
            }
        }
    }
}
