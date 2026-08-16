//! Persistence for drum-kit configurations.
//!
//! Each MIDI device gets its own TOML file, saved under a per-user mappings
//! directory. Files are named after (a sanitized version of) the device name
//! plus a short hash of the exact name, so distinct real-world device names
//! never collide on disk. Mappings are therefore associated with the device;
//! reconnecting the same device can restore its configuration automatically.
//!
//! Everything is plain local files — no database is required yet.
//!
//! The on-disk location is intentionally overridable via the `DRUMS_CONFIG_DIR`
//! environment variable (used by the integration tests) and otherwise follows
//! the conventional per-user config directory for the platform.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::DrumKitConfiguration;

/// Sub-directory under the platform config root that holds mapping files.
const CONFIG_SUBDIR: &str = "mappings";

/// Stable per-device config file store.
pub struct MappingStore {
    dir: PathBuf,
}

impl MappingStore {
    /// Creates a store rooted at a specific directory (used in tests).
    pub fn new(dir: impl AsRef<Path>) -> Self {
        Self {
            dir: dir.as_ref().to_path_buf(),
        }
    }

    /// Resolves the default mappings directory for the current user.
    pub fn default_dir() -> PathBuf {
        if let Ok(dir) = std::env::var("DRUMS_CONFIG_DIR") {
            return PathBuf::from(dir);
        }
        #[cfg(target_os = "windows")]
        {
            if let Ok(appdata) = std::env::var("APPDATA") {
                return PathBuf::from(appdata).join("drums").join(CONFIG_SUBDIR);
            }
            if let Ok(profile) = std::env::var("USERPROFILE") {
                return PathBuf::from(profile)
                    .join("AppData")
                    .join("Roaming")
                    .join("drums")
                    .join(CONFIG_SUBDIR);
            }
        }
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home)
                .join(".config")
                .join("drums")
                .join(CONFIG_SUBDIR);
        }
        // Last resort: a local directory next to where the app runs.
        PathBuf::from(".").join("drums-mappings")
    }

    /// Creates the default user store (per [`Self::default_dir`]).
    pub fn default() -> Self {
        Self::new(Self::default_dir())
    }

    /// The directory this store writes to.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Converts a device name into a unique, filesystem-safe file name.
    pub fn file_name_for(device_name: &str) -> String {
        let base = sanitize(device_name);
        let hash = fnv1a(device_name.as_bytes());
        format!("{base}-{hash:08x}.toml")
    }

    /// Full path for a device's config file.
    pub fn path_for(&self, device_name: &str) -> PathBuf {
        self.dir.join(Self::file_name_for(device_name))
    }

    /// Persists a configuration for its [`DrumKitConfiguration::device_name`].
    pub fn save(&self, config: &DrumKitConfiguration) -> io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        let text = toml::to_string_pretty(config)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let path = self.path_for(&config.device_name);
        fs::write(&path, text)
    }

    /// Loads the config saved for a device, or `None` if none exists.
    pub fn load(&self, device_name: &str) -> io::Result<Option<DrumKitConfiguration>> {
        let path = self.path_for(device_name);
        if !path.exists() {
            return Ok(None);
        }
        let text = fs::read_to_string(&path)?;
        let config: DrumKitConfiguration = toml::from_str(&text)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        Ok(Some(config))
    }

    /// Returns every saved configuration (one per device).
    pub fn load_all(&self) -> io::Result<Vec<DrumKitConfiguration>> {
        let mut out = Vec::new();
        if !self.dir.exists() {
            return Ok(out);
        }
        for entry in fs::read_dir(&self.dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("toml") {
                continue;
            }
            let text = fs::read_to_string(&path)?;
            if let Ok(config) = toml::from_str::<DrumKitConfiguration>(&text) {
                out.push(config);
            }
        }
        Ok(out)
    }

    /// Deletes the stored config for a device; `true` if it existed.
    pub fn remove(&self, device_name: &str) -> io::Result<bool> {
        let path = self.path_for(device_name);
        if !path.exists() {
            return Ok(false);
        }
        fs::remove_file(&path)?;
        Ok(true)
    }
}

/// Sanitizes a device name into a filesystem-safe identifier.
fn sanitize(device_name: &str) -> String {
    let mut out = String::with_capacity(device_name.len());
    let mut last_sep = false;
    for ch in device_name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            last_sep = false;
        } else if !last_sep && !out.is_empty() {
            out.push('-');
            last_sep = true;
        }
    }
    let mut out = out.trim_matches('-').to_string();
    if out.is_empty() {
        out = "device".to_string();
    }
    out.truncate(48);
    out
}

/// FNV-1a 64-bit hash, used to make device file-names unique.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::mapping::{DrumComponent, DrumMapping};

    /// Creates a unique temp directory for a test store.
    fn temp_dir(tag: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "drums_store_{}_{}_{}",
            tag,
            std::process::id(),
            unique
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample_config(device: &str) -> DrumKitConfiguration {
        let mut config = DrumKitConfiguration::new(device);
        config.set(DrumMapping {
            component: DrumComponent::Crash,
            midi_note: 49,
            channel: 10,
        });
        config.set(DrumMapping {
            component: DrumComponent::Snare,
            midi_note: 38,
            channel: 10,
        });
        config
    }

    #[test]
    fn save_load_roundtrip() {
        let dir = temp_dir("roundtrip");
        let store = MappingStore::new(&dir);
        let config = sample_config("NUX DM-1");

        store.save(&config).expect("save must succeed");
        let loaded = store
            .load("NUX DM-1")
            .expect("load must succeed")
            .expect("config must exist");
        assert_eq!(loaded, config);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_device_loads_none() {
        let dir = temp_dir("missing");
        let store = MappingStore::new(&dir);
        assert!(store.load("No Such Device").unwrap().is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn mappings_are_device_specific() {
        let dir = temp_dir("per_device");
        let store = MappingStore::new(&dir);

        let mut kit_a = sample_config("Kit A");
        kit_a.set(DrumMapping {
            component: DrumComponent::Kick,
            midi_note: 36,
            channel: 10,
        });
        let mut kit_b = sample_config("Kit B");
        kit_b.set(DrumMapping {
            component: DrumComponent::Kick,
            midi_note: 22, // different layout
            channel: 1,
        });

        store.save(&kit_a).unwrap();
        store.save(&kit_b).unwrap();

        // Distinct files exist for distinct devices.
        assert_ne!(
            MappingStore::file_name_for("Kit A"),
            MappingStore::file_name_for("Kit B")
        );

        let loaded_a = store.load("Kit A").unwrap().unwrap();
        let loaded_b = store.load("Kit B").unwrap().unwrap();
        assert_eq!(loaded_a, kit_a);
        assert_eq!(loaded_b, kit_b);
        assert_ne!(loaded_a, loaded_b);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_all_returns_every_device() {
        let dir = temp_dir("load_all");
        let store = MappingStore::new(&dir);
        store.save(&sample_config("First Kit")).unwrap();
        store.save(&sample_config("Second Kit")).unwrap();

        let all = store.load_all().unwrap();
        assert_eq!(all.len(), 2);
        let names: Vec<&str> = all.iter().map(|c| c.device_name.as_str()).collect();
        assert!(names.contains(&"First Kit"));
        assert!(names.contains(&"Second Kit"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn device_names_with_special_characters_are_safe() {
        let dir = temp_dir("special");
        let store = MappingStore::new(&dir);
        let device = "USB MIDI/Device (Port 1)";
        let config = sample_config(device);
        store.save(&config).unwrap();

        // Sanitized name has no path-hostile characters and is unique.
        let name = MappingStore::file_name_for(device);
        assert!(!name.contains('/') && !name.contains('\\'));
        assert!(name.ends_with(".toml"));

        let loaded = store.load(device).unwrap().unwrap();
        assert_eq!(loaded, config);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn remove_deletes_just_that_device() {
        let dir = temp_dir("remove");
        let store = MappingStore::new(&dir);
        store.save(&sample_config("Keep Me")).unwrap();
        store.save(&sample_config("Drop Me")).unwrap();

        assert!(store.remove("Drop Me").unwrap());
        assert!(!store.remove("Drop Me").unwrap()); // second call: nothing to remove
        assert!(store.load("Keep Me").unwrap().is_some());
        assert!(store.load("Drop Me").unwrap().is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupted_file_reports_error_not_panic() {
        let dir = temp_dir("corrupt");
        let store = MappingStore::new(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = store.path_for("Broken");
        fs::write(&path, "not valid toml [[").unwrap();
        let result = store.load("Broken");
        assert!(result.is_err());
        let _ = fs::remove_dir_all(&dir);
    }
}