use serde::{Deserialize, Serialize};
use std::{fs, io, path::Path, path::PathBuf};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemePreference {
    System,
    #[default]
    Dark,
    Light,
}

impl ThemePreference {
    pub fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Dark => "Dark",
            Self::Light => "Light",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccentColour {
    #[default]
    Blue,
    Teal,
    Purple,
    Amber,
}

impl AccentColour {
    pub fn label(self) -> &'static str {
        match self {
            Self::Blue => "Blue",
            Self::Teal => "Teal",
            Self::Purple => "Purple",
            Self::Amber => "Amber",
        }
    }

    pub fn rgb(self) -> (u8, u8, u8) {
        match self {
            Self::Blue => (90, 175, 230),
            Self::Teal => (64, 184, 166),
            Self::Purple => (148, 112, 210),
            Self::Amber => (210, 156, 66),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiScale {
    Small,
    #[default]
    Normal,
    Large,
}

impl UiScale {
    pub fn label(self) -> &'static str {
        match self {
            Self::Small => "Small",
            Self::Normal => "Normal",
            Self::Large => "Large",
        }
    }

    pub fn zoom_factor(self) -> f32 {
        match self {
            Self::Small => 0.9,
            Self::Normal => 1.0,
            Self::Large => 1.15,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PracticeSettings {
    pub midi_input_latency_ms: f64,
    pub audio_sync_offset_ms: f64,
    pub start_audio_with_practice: bool,
    pub practice_view: String,
    pub expected_drums_only: bool,
    pub audio_output_device: Option<String>,
    pub output_volume: f32,
    pub output_muted: bool,
    pub theme: ThemePreference,
    pub accent_colour: AccentColour,
    pub ui_scale: UiScale,
}
impl Default for PracticeSettings {
    fn default() -> Self {
        Self {
            midi_input_latency_ms: 0.0,
            audio_sync_offset_ms: 0.0,
            start_audio_with_practice: true,
            practice_view: "timeline".into(),
            expected_drums_only: false,
            audio_output_device: None,
            // Unity gain preserves the application's existing audio levels.
            output_volume: 1.0,
            output_muted: false,
            theme: ThemePreference::Dark,
            accent_colour: AccentColour::Blue,
            ui_scale: UiScale::Normal,
        }
    }
}
pub(crate) fn path() -> PathBuf {
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
    load_from(&path())
}

pub(crate) fn load_from(path: &Path) -> PracticeSettings {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| toml::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(value: &PracticeSettings) -> std::io::Result<()> {
    save_to(&path(), value)
}

fn save_to(path: &Path, value: &PracticeSettings) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(
        path,
        toml::to_string_pretty(value).map_err(std::io::Error::other)?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "drums-preferences-{tag}-{}-{}.toml",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn defaults_keep_audio_optional_and_offsets_zero() {
        let s = PracticeSettings::default();
        assert_eq!(s.midi_input_latency_ms, 0.0);
        assert_eq!(s.audio_sync_offset_ms, 0.0);
        assert!(s.start_audio_with_practice);
        assert_eq!(s.practice_view, "timeline");
        assert!(!s.expected_drums_only);
        assert_eq!(s.audio_output_device, None);
        assert_eq!(s.output_volume, 1.0);
        assert!(!s.output_muted);
        assert_eq!(s.theme, ThemePreference::Dark);
        assert_eq!(s.accent_colour, AccentColour::Blue);
        assert_eq!(s.ui_scale, UiScale::Normal);
    }
    #[test]
    fn legacy_empty_settings_deserialize() {
        let s: PracticeSettings = toml::from_str("").unwrap();
        assert_eq!(s.audio_sync_offset_ms, 0.0);
        assert_eq!(s.practice_view, "timeline");
        assert!(!s.expected_drums_only);
        assert_eq!(s.audio_output_device, None);
        assert_eq!(s.output_volume, 1.0);
        assert!(!s.output_muted);
        assert_eq!(s.theme, ThemePreference::Dark);
        assert_eq!(s.accent_colour, AccentColour::Blue);
        assert_eq!(s.ui_scale, UiScale::Normal);
    }

    #[test]
    fn appearance_preferences_round_trip_through_existing_settings_file() {
        let path = temp_file("round-trip");
        let value = PracticeSettings {
            expected_drums_only: true,
            audio_output_device: Some("USB Headphones".into()),
            output_volume: 0.35,
            output_muted: true,
            theme: ThemePreference::Light,
            accent_colour: AccentColour::Purple,
            ui_scale: UiScale::Large,
            ..PracticeSettings::default()
        };
        save_to(&path, &value).unwrap();
        let loaded = load_from(&path);
        assert!(loaded.expected_drums_only);
        assert_eq!(
            loaded.audio_output_device.as_deref(),
            Some("USB Headphones")
        );
        assert_eq!(loaded.output_volume, 0.35);
        assert!(loaded.output_muted);
        assert_eq!(loaded.theme, ThemePreference::Light);
        assert_eq!(loaded.accent_colour, AccentColour::Purple);
        assert_eq!(loaded.ui_scale, UiScale::Large);
        fs::remove_file(path).unwrap();
    }
}
