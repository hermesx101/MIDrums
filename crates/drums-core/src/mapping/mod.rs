//! Drum kit mapping: the physical components, a per-device configuration and
//! the interactive setup wizard.
//!
//! Everything in this module is pure logic — it has no GUI and no MIDI backend
//! dependency — so the state machine in [`wizard`] and the persistence in
//! [`store`] can be unit-tested independently of any front-end. It deliberately
//! makes *no* assumption that a particular MIDI note maps to a particular drum:
//! the values are learned from the connected device during the setup wizard, or
//! typed by hand in the manual mapping view.

mod store;
mod wizard;

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub use store::MappingStore;
pub use wizard::{DetectedHit, SetupWizard, SetupWizardState};

/// The drum components supported by the initial setup wizard, in the order the
/// wizard walks through them.
pub const SUPPORTED_COMPONENTS: [DrumComponent; 11] = [
    DrumComponent::Crash,
    DrumComponent::Ride,
    DrumComponent::HiHat,
    DrumComponent::Snare,
    DrumComponent::Kick,
    DrumComponent::HiTom,
    DrumComponent::MidTom,
    DrumComponent::LowTom,
    DrumComponent::HiHatOpen,
    DrumComponent::HiHatPedal,
    DrumComponent::HiHatPedalPosition,
];

/// A physical drum component of a kit.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum DrumComponent {
    Crash,
    Ride,
    HiHat,
    HiHatOpen,
    HiHatPedal,
    HiHatPedalPosition,
    Snare,
    Kick,
    HiTom,
    MidTom,
    LowTom,
}

impl DrumComponent {
    /// All supported components, in wizard order.
    pub const ALL: [DrumComponent; 10] = [
        DrumComponent::Crash,
        DrumComponent::Ride,
        DrumComponent::HiHat,
        DrumComponent::HiHatOpen,
        DrumComponent::HiHatPedal,
        DrumComponent::Snare,
        DrumComponent::Kick,
        DrumComponent::HiTom,
        DrumComponent::MidTom,
        DrumComponent::LowTom,
    ];

    /// Short, all-caps label used in grids and prompts (e.g. `"HI-HAT"`).
    pub fn label(self) -> &'static str {
        match self {
            DrumComponent::Crash => "CRASH",
            DrumComponent::Ride => "RIDE",
            DrumComponent::HiHat => "HI-HAT CLOSED",
            DrumComponent::HiHatOpen => "HI-HAT OPEN",
            DrumComponent::HiHatPedal => "HI-HAT PEDAL",
            DrumComponent::HiHatPedalPosition => "HI-HAT PEDAL POSITION",
            DrumComponent::Snare => "SNARE",
            DrumComponent::Kick => "KICK",
            DrumComponent::HiTom => "HI TOM",
            DrumComponent::MidTom => "MID TOM",
            DrumComponent::LowTom => "LOW TOM",
        }
    }

    /// Physical instrument name, shared by playable hi-hat states.
    pub fn instrument_label(self) -> &'static str {
        match self {
            DrumComponent::HiHat
            | DrumComponent::HiHatOpen
            | DrumComponent::HiHatPedal
            | DrumComponent::HiHatPedalPosition => {
                "HI-HAT"
            }
            _ => self.label(),
        }
    }

    /// Optional playable state within a physical instrument.
    pub fn state_label(self) -> Option<&'static str> {
        match self {
            DrumComponent::HiHat => Some("Closed"),
            DrumComponent::HiHatOpen => Some("Open"),
            DrumComponent::HiHatPedal => Some("Pedal"),
            DrumComponent::HiHatPedalPosition => Some("Pedal Position"),
            _ => None,
        }
    }

    /// Title-case name used when persisting the component (e.g. `"Low Tom"`).
    pub fn as_str(self) -> &'static str {
        match self {
            DrumComponent::Crash => "Crash",
            DrumComponent::Ride => "Ride",
            DrumComponent::HiHat => "Hi-Hat",
            DrumComponent::HiHatOpen => "Hi-Hat Open",
            DrumComponent::HiHatPedal => "Hi-Hat Pedal",
            DrumComponent::HiHatPedalPosition => "Hi-Hat Pedal Position",
            DrumComponent::Snare => "Snare",
            DrumComponent::Kick => "Kick",
            DrumComponent::HiTom => "Hi Tom",
            DrumComponent::MidTom => "Mid Tom",
            DrumComponent::LowTom => "Low Tom",
        }
    }

    /// Resolves a component from its persisted [`Self::as_str`] form.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "Crash" => Some(DrumComponent::Crash),
            "Ride" => Some(DrumComponent::Ride),
            "Hi-Hat" => Some(DrumComponent::HiHat),
            "Hi-Hat Open" => Some(DrumComponent::HiHatOpen),
            "Hi-Hat Pedal" => Some(DrumComponent::HiHatPedal),
            "Hi-Hat Pedal Position" => Some(DrumComponent::HiHatPedalPosition),
            "Snare" => Some(DrumComponent::Snare),
            "Kick" => Some(DrumComponent::Kick),
            "Hi Tom" => Some(DrumComponent::HiTom),
            "Mid Tom" => Some(DrumComponent::MidTom),
            "Low Tom" => Some(DrumComponent::LowTom),
            _ => None,
        }
    }
}

impl fmt::Display for DrumComponent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.label())
    }
}

impl Serialize for DrumComponent {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for DrumComponent {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        DrumComponent::from_str(&s).ok_or_else(|| {
            serde::de::Error::custom(format!("unknown drum component {s:?}"))
        })
    }
}

/// A single drum → MIDI mapping entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DrumMapping {
    pub component: DrumComponent,
    #[serde(default)]
    pub event_kind: MappingEventKind,
    /// MIDI note number 0..=127, learned from the device or typed manually.
    pub midi_note: u8,
    /// MIDI channel as displayed to the user, 1..=16.
    pub channel: u8,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MappingEventKind {
    #[default]
    Note,
    ControlChange,
}

impl MappingEventKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Note => "Note",
            Self::ControlChange => "CC",
        }
    }
}

/// An ordered set of drum-mapping entries for one MIDI device.
///
/// The configuration is associated with a device via [`Self::device_name`], so
/// that reconnecting the same device can restore its mapping.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DrumKitConfiguration {
    pub device_name: String,
    pub mappings: Vec<DrumMapping>,
}

impl DrumKitConfiguration {
    /// Creates an empty configuration for the named device.
    pub fn new(device_name: impl Into<String>) -> Self {
        Self {
            device_name: device_name.into(),
            mappings: Vec::new(),
        }
    }

    /// Returns the mapping for a component, if one exists.
    pub fn get(&self, component: DrumComponent) -> Option<&DrumMapping> {
        self.mappings
            .iter()
            .find(|m| m.component == component)
    }

    /// Returns the MIDI note mapped to a component, if any.
    pub fn note_for(&self, component: DrumComponent) -> Option<u8> {
        self.get(component).map(|m| m.midi_note)
    }

    /// Returns the MIDI channel mapped to a component, if any.
    pub fn channel_for(&self, component: DrumComponent) -> Option<u8> {
        self.get(component).map(|m| m.channel)
    }

    /// Inserts or replaces the mapping for a component.
    pub fn set(&mut self, mapping: DrumMapping) {
        if let Some(existing) = self
            .mappings
            .iter_mut()
            .find(|m| m.component == mapping.component)
        {
            *existing = mapping;
        } else {
            self.mappings.push(mapping);
        }
    }

    /// Removes the mapping for a component, returning whether it was present.
    pub fn remove(&mut self, component: DrumComponent) -> bool {
        let before = self.mappings.len();
        self.mappings.retain(|m| m.component != component);
        before != self.mappings.len()
    }

    /// Number of configured drums.
    pub fn mapped_count(&self) -> usize {
        self.mappings.len()
    }

    /// Returns `true` when no drums are configured.
    pub fn is_empty(&self) -> bool {
        self.mappings.is_empty()
    }
}

#[cfg(test)]
mod phase5e_tests {
    use super::*;

    #[test]
    fn closed_open_and_pedal_hi_hat_mappings_coexist_and_remain_compatible() {
        let mut config = DrumKitConfiguration::new("kit");
        for (component, midi_note) in [
            (DrumComponent::HiHat, 42),
            (DrumComponent::HiHatOpen, 46),
            (DrumComponent::HiHatPedal, 44),
        ] {
            config.set(DrumMapping {
                component,
                event_kind: MappingEventKind::Note,
                midi_note,
                channel: 10,
            });
        }
        config.set(DrumMapping {
            component: DrumComponent::HiHatPedalPosition,
            event_kind: MappingEventKind::ControlChange,
            midi_note: 4,
            channel: 10,
        });
        assert_eq!(config.note_for(DrumComponent::HiHat), Some(42));
        assert_eq!(config.note_for(DrumComponent::HiHatOpen), Some(46));
        assert_eq!(config.note_for(DrumComponent::HiHatPedal), Some(44));
        assert_eq!(
            config.get(DrumComponent::HiHatPedalPosition).unwrap().event_kind,
            MappingEventKind::ControlChange
        );
        let encoded = toml::to_string(&config).unwrap();
        let decoded: DrumKitConfiguration = toml::from_str(&encoded).unwrap();
        assert_eq!(decoded, config);
        assert_eq!(DrumComponent::from_str("Hi-Hat"), Some(DrumComponent::HiHat));
    }

    #[test]
    fn legacy_mapping_without_event_kind_defaults_to_note() {
        let legacy = r#"
device_name = "NUX"
[[mappings]]
component = "Snare"
midi_note = 38
channel = 10
"#;
        let config: DrumKitConfiguration = toml::from_str(legacy).unwrap();
        assert_eq!(config.mappings[0].event_kind, MappingEventKind::Note);
    }
}
