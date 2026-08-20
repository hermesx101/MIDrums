use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct PracticeSettings {
    pub midi_input_latency_ms: f64,
    pub audio_sync_offset_ms: f64,
    pub start_audio_with_practice: bool,
    pub practice_view: String,
}
impl Default for PracticeSettings {
    fn default() -> Self {
        Self {
            midi_input_latency_ms: 0.0,
            audio_sync_offset_ms: 0.0,
            start_audio_with_practice: true,
            practice_view: "timeline".into(),
        }
    }
}
fn path() -> PathBuf {
    if let Ok(v) = std::env::var("APPDATA") {
        return PathBuf::from(v)
            .join("drums")
            .join("practice-settings.toml");
    }
    if let Ok(v) = std::env::var("HOME") {
        return PathBuf::from(v)
            .join(".config")
            .join("drums")
            .join("practice-settings.toml");
    }
    PathBuf::from("practice-settings.toml")
}
pub fn load() -> PracticeSettings {
    fs::read_to_string(path())
        .ok()
        .and_then(|s| toml::from_str(&s).ok())
        .unwrap_or_default()
}
pub fn save(value: &PracticeSettings) -> std::io::Result<()> {
    let p = path();
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(
        p,
        toml::to_string_pretty(value).map_err(std::io::Error::other)?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_keep_audio_optional_and_offsets_zero() {
        let s = PracticeSettings::default();
        assert_eq!(s.midi_input_latency_ms, 0.0);
        assert_eq!(s.audio_sync_offset_ms, 0.0);
        assert!(s.start_audio_with_practice);
        assert_eq!(s.practice_view, "timeline");
    }
    #[test]
    fn legacy_empty_settings_deserialize() {
        let s: PracticeSettings = toml::from_str("").unwrap();
        assert_eq!(s.audio_sync_offset_ms, 0.0);
        assert_eq!(s.practice_view, "timeline");
    }
}
