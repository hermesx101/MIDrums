use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LibraryEntry {
    pub path: String,
    pub display_name: String,
    pub bpm: Option<f64>,
    pub time_signature: Option<String>,
    pub duration_seconds: Option<f64>,
    pub note_count: usize,
    pub likely_drums: bool,
    pub added_at: u64,
    pub last_practiced_at: Option<u64>,
    pub last_accuracy: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LibrarySort {
    RecentlyPlayed,
    RecentlyAdded,
    Name,
    Bpm,
    Accuracy,
}

impl LibrarySort {
    pub fn label(self) -> &'static str {
        match self {
            Self::RecentlyPlayed => "Recently Played",
            Self::RecentlyAdded => "Recently Added",
            Self::Name => "Name",
            Self::Bpm => "BPM",
            Self::Accuracy => "Accuracy",
        }
    }
}

#[derive(Default, Serialize, Deserialize)]
struct LibraryFile {
    entries: Vec<LibraryEntry>,
}

pub fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn storage_path() -> PathBuf {
    if let Ok(value) = std::env::var("APPDATA") {
        return PathBuf::from(value)
            .join("drums")
            .join("track-library.toml");
    }
    if let Ok(value) = std::env::var("USERPROFILE") {
        return PathBuf::from(value)
            .join("AppData")
            .join("Roaming")
            .join("drums")
            .join("track-library.toml");
    }
    PathBuf::from("track-library.toml")
}

pub fn load() -> Vec<LibraryEntry> {
    fs::read_to_string(storage_path())
        .ok()
        .and_then(|text| toml::from_str::<LibraryFile>(&text).ok())
        .map(|file| file.entries)
        .unwrap_or_default()
}

pub fn save(entries: &[LibraryEntry]) -> std::io::Result<()> {
    let path = storage_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = toml::to_string_pretty(&LibraryFile {
        entries: entries.to_vec(),
    })
    .map_err(std::io::Error::other)?;
    fs::write(path, text)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn library_file_round_trip_preserves_display_identity() {
        let file = LibraryFile {
            entries: vec![LibraryEntry {
                path: "original.mid".into(),
                display_name: "My Lesson".into(),
                bpm: Some(60.0),
                time_signature: Some("4/4".into()),
                duration_seconds: Some(12.0),
                note_count: 20,
                likely_drums: true,
                added_at: 1,
                last_practiced_at: None,
                last_accuracy: None,
            }],
        };
        let encoded = toml::to_string(&file).unwrap();
        let decoded: LibraryFile = toml::from_str(&encoded).unwrap();
        assert_eq!(decoded.entries[0].display_name, "My Lesson");
        assert_eq!(decoded.entries[0].path, "original.mid");
    }
}
