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
pub const SUPPORTED_COMPONENTS: [DrumComponent; 8] = [
    DrumComponent::Crash,
    DrumComponent::Ride,
    DrumComponent::HiHat,
    DrumComponent::Snare,
    DrumComponent::Kick,
    DrumComponent::HiTom,
    DrumComponent::MidTom,
    DrumComponent::LowTom,
];

/// A physical drum component of a kit.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum DrumComponent {
    Crash,
    Ride,
    HiHat,
    Snare,
    Kick,
    HiTom,
    MidTom,
    LowTom,
}

impl DrumComponent {
    /// All supported components, in wizard order.
    pub const ALL: [DrumComponent; 8] = SUPPORTED_COMPONENTS;

    /// Short, all-caps label used in grids and prompts (e.g. `"HI-HAT"`).
    pub fn label(self) -> &'static str {
        match self {
            DrumComponent::Crash => "CRASH",
            DrumComponent::Ride => "RIDE",
            DrumComponent::HiHat => "HI-HAT",
            DrumComponent::Snare => "SNARE",
            DrumComponent::Kick => "KICK",
            DrumComponent::HiTom => "HI TOM",
            DrumComponent::MidTom => "MID TOM",
            DrumComponent::LowTom => "LOW TOM",
        }
    }

    /// Title-case name used when persisting the component (e.g. `"Low Tom"`).
    pub fn as_str(self) -> &'static str {
        match self {
            DrumComponent::Crash => "Crash",
            DrumComponent::Ride => "Ride",
            DrumComponent::HiHat => "Hi-Hat",
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
    /// MIDI note number 0..=127, learned from the device or typed manually.
    pub midi_note: u8,
    /// MIDI channel as displayed to the user, 1..=16.
    pub channel: u8,
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