use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioIdentity {
    pub application_name: Option<String>,
    pub application_process_binary: Option<String>,
    pub media_name: Option<String>,
    pub media_role: Option<String>,
    pub media_class: Option<String>,
    pub node_name: Option<String>,
}

impl AudioIdentity {
    pub fn from_props(props: &BTreeMap<String, String>) -> Self {
        Self {
            application_name: get(props, "application.name"),
            application_process_binary: get(props, "application.process.binary"),
            media_name: get(props, "media.name"),
            media_role: get(props, "media.role"),
            media_class: get(props, "media.class"),
            node_name: get(props, "node.name"),
        }
    }

    pub fn matches_configured_source(&self, source: &ConfiguredSource) -> bool {
        self.matches_configured_source_exact(source)
            || self.matches_configured_voice_variant(source)
    }

    fn matches_configured_source_exact(&self, source: &ConfiguredSource) -> bool {
        field_matches(&source.application_name, &self.application_name)
            && field_matches(
                &source.application_process_binary,
                &self.application_process_binary,
            )
            && field_matches(&source.media_name, &self.media_name)
            && field_matches(&source.media_role, &self.media_role)
            && field_matches(&source.media_class, &self.media_class)
            && field_matches(&source.node_name, &self.node_name)
    }

    fn matches_configured_voice_variant(&self, source: &ConfiguredSource) -> bool {
        self.is_playback_stream()
            && field_matches(&source.media_class, &self.media_class)
            && same_present_value(
                &source.application_process_binary,
                &self.application_process_binary,
            )
            && (self.looks_like_voice_source() || source.looks_like_voice_source())
    }

    pub fn is_playback_stream(&self) -> bool {
        self.media_class.as_deref() == Some("Stream/Output/Audio")
    }

    fn looks_like_voice_source(&self) -> bool {
        looks_like_voice_text([
            self.application_name.as_deref(),
            self.application_process_binary.as_deref(),
            self.media_name.as_deref(),
            self.media_role.as_deref(),
            self.node_name.as_deref(),
        ])
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfiguredSource {
    pub label: Option<String>,
    pub application_name: Option<String>,
    pub application_process_binary: Option<String>,
    pub media_name: Option<String>,
    pub media_role: Option<String>,
    pub media_class: Option<String>,
    pub node_name: Option<String>,
}

impl ConfiguredSource {
    pub fn from_identity(label: String, identity: &AudioIdentity) -> Self {
        Self {
            label: Some(label),
            application_name: identity.application_name.clone(),
            application_process_binary: identity.application_process_binary.clone(),
            media_name: identity.media_name.clone(),
            media_role: identity.media_role.clone(),
            media_class: identity.media_class.clone(),
            node_name: identity.node_name.clone(),
        }
    }

    pub fn has_match_fields(&self) -> bool {
        self.application_name.is_some()
            || self.application_process_binary.is_some()
            || self.media_name.is_some()
            || self.media_role.is_some()
            || self.media_class.is_some()
            || self.node_name.is_some()
    }

    fn looks_like_voice_source(&self) -> bool {
        looks_like_voice_text([
            self.application_name.as_deref(),
            self.application_process_binary.as_deref(),
            self.media_name.as_deref(),
            self.media_role.as_deref(),
            self.node_name.as_deref(),
        ])
    }
}

fn get(props: &BTreeMap<String, String>, key: &str) -> Option<String> {
    props.get(key).filter(|v| !v.is_empty()).cloned()
}

fn field_matches(configured: &Option<String>, actual: &Option<String>) -> bool {
    match configured.as_deref() {
        None | Some("") => true,
        Some(expected) => actual.as_deref() == Some(expected),
    }
}

fn same_present_value(left: &Option<String>, right: &Option<String>) -> bool {
    matches!((left.as_deref(), right.as_deref()), (Some(left), Some(right)) if !left.is_empty() && left == right)
}

fn looks_like_voice_text<'a>(parts: impl IntoIterator<Item = Option<&'a str>>) -> bool {
    let text = parts
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();

    text.contains("voice")
        || text.contains("webrtc")
        || text.contains("discord")
        || text.contains("communication")
        || text.contains("playstream")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_config_fields_are_wildcards() {
        let source = ConfiguredSource {
            application_name: Some("WEBRTC VoiceEngine".into()),
            media_class: Some("Stream/Output/Audio".into()),
            ..ConfiguredSource::default()
        };
        let identity = AudioIdentity {
            application_name: Some("WEBRTC VoiceEngine".into()),
            media_class: Some("Stream/Output/Audio".into()),
            media_name: Some("playStream".into()),
            ..AudioIdentity::default()
        };

        assert!(identity.matches_configured_source(&source));
    }

    #[test]
    fn configured_fields_must_match_exactly() {
        let source = ConfiguredSource {
            application_process_binary: Some("discord".into()),
            ..ConfiguredSource::default()
        };
        let identity = AudioIdentity {
            application_process_binary: Some("chromium".into()),
            ..AudioIdentity::default()
        };

        assert!(!identity.matches_configured_source(&source));
    }

    #[test]
    fn discord_webrtc_variants_match_by_binary_and_voice_hint() {
        let source = ConfiguredSource {
            application_name: Some("Discord".into()),
            application_process_binary: Some(".Discord-wrapped".into()),
            media_class: Some("Stream/Output/Audio".into()),
            ..ConfiguredSource::default()
        };
        let identity = AudioIdentity {
            application_name: Some("WEBRTC VoiceEngine".into()),
            application_process_binary: Some(".Discord-wrapped".into()),
            media_name: Some("playStream".into()),
            media_class: Some("Stream/Output/Audio".into()),
            ..AudioIdentity::default()
        };

        assert!(identity.matches_configured_source(&source));
    }

    #[test]
    fn binary_match_without_voice_hint_is_not_enough() {
        let source = ConfiguredSource {
            application_name: Some("Other App".into()),
            application_process_binary: Some("chromium".into()),
            media_class: Some("Stream/Output/Audio".into()),
            ..ConfiguredSource::default()
        };
        let identity = AudioIdentity {
            application_name: Some("Chromium".into()),
            application_process_binary: Some("chromium".into()),
            media_name: Some("Playback".into()),
            media_class: Some("Stream/Output/Audio".into()),
            ..AudioIdentity::default()
        };

        assert!(!identity.matches_configured_source(&source));
    }
}
