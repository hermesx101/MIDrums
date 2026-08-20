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
    DrumComponent::HiHatOpen,
    DrumComponent::HiHatPedal,
    DrumComponent::HiHatPedalPosition,
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
            | DrumComponent::HiHatPedalPosition => "HI-HAT",
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
        DrumComponent::from_str(&s)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown drum component {s:?}")))
    }
}

/// A single drum → MIDI mapping entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DrumMapping {
    pub component: DrumComponent,
    #[serde(default)]
    pub event_kind: MappingEventKind,
    /// MIDI Note number or CC controller number (0..=127), depending on kind.
    /// The legacy field name is retained for on-disk compatibility.
    pub midi_note: u8,
    /// MIDI channel as displayed to the user, 1..=16.
    pub channel: u8,
    /// Optional zone/articulation metadata. It is descriptive only; all aliases
    /// still resolve to the mapping's logical [`DrumComponent`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub articulation: Option<MappingArticulation>,
    /// Observed range for a continuous Control Change mapping.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cc_range: Option<CcRange>,
}

/// Optional descriptive articulation for devices that expose distinct zones.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MappingArticulation {
    Primary,
    Bow,
    Edge,
    Bell,
    Closed,
    Open,
    Pedal,
    Continuous,
}

impl MappingArticulation {
    pub fn label(self) -> &'static str {
        match self {
            Self::Primary => "Primary",
            Self::Bow => "Bow",
            Self::Edge => "Edge",
            Self::Bell => "Bell",
            Self::Closed => "Closed",
            Self::Open => "Open",
            Self::Pedal => "Pedal",
            Self::Continuous => "Continuous",
        }
    }
}

/// Observed value range for a continuous MIDI controller.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CcRange {
    pub min: u8,
    pub max: u8,
}

impl CcRange {
    pub fn new(a: u8, b: u8) -> Self {
        Self {
            min: a.min(b),
            max: a.max(b),
        }
    }

    pub fn observe(&mut self, value: u8) {
        self.min = self.min.min(value);
        self.max = self.max.max(value);
    }
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

/// Stable identity of one physical MIDI input. Velocity/controller value is
/// intentionally excluded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MappingInputIdentity {
    pub event_kind: MappingEventKind,
    pub number: u8,
    pub channel: u8,
}

impl DrumMapping {
    pub fn identity(self) -> MappingInputIdentity {
        MappingInputIdentity {
            event_kind: self.event_kind,
            number: self.midi_note,
            channel: self.channel,
        }
    }

    pub fn matches(self, identity: MappingInputIdentity) -> bool {
        self.identity() == identity
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MappingConflict {
    pub assigned_to: DrumComponent,
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

    /// Returns the first mapping for a component, if one exists.
    pub fn get(&self, component: DrumComponent) -> Option<&DrumMapping> {
        self.mappings.iter().find(|m| m.component == component)
    }

    /// Returns every physical MIDI alias assigned to a logical component.
    pub fn mappings_for(&self, component: DrumComponent) -> impl Iterator<Item = &DrumMapping> {
        self.mappings
            .iter()
            .filter(move |mapping| mapping.component == component)
    }

    /// Returns the MIDI note mapped to a component, if any.
    pub fn note_for(&self, component: DrumComponent) -> Option<u8> {
        self.get(component).map(|m| m.midi_note)
    }

    /// Returns the MIDI channel mapped to a component, if any.
    pub fn channel_for(&self, component: DrumComponent) -> Option<u8> {
        self.get(component).map(|m| m.channel)
    }

    /// Adds an input alias without replacing other inputs for the component.
    /// Inputs already owned by another component are rejected.
    pub fn add_input(&mut self, mapping: DrumMapping) -> Result<bool, MappingConflict> {
        if let Some(existing) = self
            .mappings
            .iter_mut()
            .find(|existing| existing.identity() == mapping.identity())
        {
            if existing.component != mapping.component {
                return Err(MappingConflict {
                    assigned_to: existing.component,
                });
            }
            *existing = mapping;
            return Ok(false);
        }
        self.mappings.push(mapping);
        Ok(true)
    }

    /// Compatibility helper. Interactive callers should prefer
    /// [`Self::add_input`] so conflicts can be shown to the user.
    pub fn set(&mut self, mapping: DrumMapping) {
        let _ = self.add_input(mapping);
    }

    /// Removes the mapping for a component, returning whether it was present.
    pub fn remove(&mut self, component: DrumComponent) -> bool {
        let before = self.mappings.len();
        self.mappings.retain(|m| m.component != component);
        before != self.mappings.len()
    }

    /// Removes one physical input while retaining the component's other aliases.
    pub fn remove_input(&mut self, identity: MappingInputIdentity) -> bool {
        let before = self.mappings.len();
        self.mappings
            .retain(|mapping| mapping.identity() != identity);
        before != self.mappings.len()
    }

    /// Explicitly transfers an input between logical components.
    pub fn reassign_input(&mut self, mapping: DrumMapping) {
        self.remove_input(mapping.identity());
        self.mappings.push(mapping);
    }

    /// Resolves a physical input to the logical identity consumed by scoring.
    pub fn resolve(&self, identity: MappingInputIdentity) -> Option<DrumComponent> {
        self.mappings
            .iter()
            .find(|mapping| mapping.matches(identity))
            .map(|mapping| mapping.component)
    }

    /// Number of logical drums represented, independent of alias count.
    pub fn mapped_component_count(&self) -> usize {
        let mut components = std::collections::BTreeSet::new();
        components.extend(self.mappings.iter().map(|mapping| mapping.component));
        components.len()
    }

    /// Continuous-controller calibration alone cannot drive practice hits.
    pub fn is_usable(&self) -> bool {
        self.mappings
            .iter()
            .any(|mapping| mapping.event_kind == MappingEventKind::Note)
    }

    /// Checks persisted/input invariants without modifying the configuration.
    pub fn validate(&self) -> Result<(), String> {
        for (index, mapping) in self.mappings.iter().enumerate() {
            if !(1..=16).contains(&mapping.channel) {
                return Err(format!("mapping {} has invalid MIDI channel", index + 1));
            }
            if mapping.event_kind == MappingEventKind::Note && mapping.cc_range.is_some() {
                return Err(format!("Note mapping {} contains a CC range", index + 1));
            }
            if let Some(range) = mapping.cc_range {
                if range.min > range.max {
                    return Err(format!("mapping {} has an inverted CC range", index + 1));
                }
            }
            if let Some(other) = self.mappings[..index]
                .iter()
                .find(|other| other.identity() == mapping.identity())
            {
                return Err(format!(
                    "{} {} on channel {} is assigned to both {} and {}",
                    mapping.event_kind.label(),
                    mapping.midi_note,
                    mapping.channel,
                    other.component.label(),
                    mapping.component.label()
                ));
            }
        }
        Ok(())
    }

    /// Number of configured physical MIDI inputs (including aliases).
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
                articulation: None,
                cc_range: None,
            });
        }
        config.set(DrumMapping {
            component: DrumComponent::HiHatPedalPosition,
            event_kind: MappingEventKind::ControlChange,
            midi_note: 4,
            channel: 10,
            articulation: Some(MappingArticulation::Continuous),
            cc_range: Some(CcRange::new(0, 127)),
        });
        assert_eq!(config.note_for(DrumComponent::HiHat), Some(42));
        assert_eq!(config.note_for(DrumComponent::HiHatOpen), Some(46));
        assert_eq!(config.note_for(DrumComponent::HiHatPedal), Some(44));
        assert_eq!(
            config
                .get(DrumComponent::HiHatPedalPosition)
                .unwrap()
                .event_kind,
            MappingEventKind::ControlChange
        );
        let encoded = toml::to_string(&config).unwrap();
        let decoded: DrumKitConfiguration = toml::from_str(&encoded).unwrap();
        assert_eq!(decoded, config);
        assert_eq!(
            DrumComponent::from_str("Hi-Hat"),
            Some(DrumComponent::HiHat)
        );
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
        assert_eq!(config.mappings[0].articulation, None);
        assert_eq!(config.mappings[0].cc_range, None);
    }

    fn note(component: DrumComponent, number: u8) -> DrumMapping {
        DrumMapping {
            component,
            event_kind: MappingEventKind::Note,
            midi_note: number,
            channel: 10,
            articulation: None,
            cc_range: None,
        }
    }

    #[test]
    fn multiple_inputs_are_first_class_aliases_for_one_logical_drum() {
        let mut config = DrumKitConfiguration::new("multi-zone kit");
        assert_eq!(config.add_input(note(DrumComponent::Ride, 51)), Ok(true));
        assert_eq!(config.add_input(note(DrumComponent::Ride, 53)), Ok(true));
        assert_eq!(config.add_input(note(DrumComponent::Ride, 59)), Ok(true));

        assert_eq!(config.mappings_for(DrumComponent::Ride).count(), 3);
        assert_eq!(config.mapped_component_count(), 1);
        for number in [51, 53, 59] {
            assert_eq!(
                config.resolve(MappingInputIdentity {
                    event_kind: MappingEventKind::Note,
                    number,
                    channel: 10,
                }),
                Some(DrumComponent::Ride)
            );
        }
    }

    #[test]
    fn input_conflict_requires_explicit_reassignment() {
        let mut config = DrumKitConfiguration::new("kit");
        config.add_input(note(DrumComponent::Snare, 38)).unwrap();
        let error = config
            .add_input(note(DrumComponent::Kick, 38))
            .expect_err("the same physical input must not be silently moved");
        assert_eq!(error.assigned_to, DrumComponent::Snare);
        assert_eq!(config.get(DrumComponent::Kick), None);

        config.reassign_input(note(DrumComponent::Kick, 38));
        assert_eq!(config.get(DrumComponent::Snare), None);
        assert!(config.get(DrumComponent::Kick).is_some());
    }

    #[test]
    fn continuous_cc_calibration_round_trips() {
        let mut config = DrumKitConfiguration::new("kit");
        config
            .add_input(DrumMapping {
                component: DrumComponent::HiHatPedalPosition,
                event_kind: MappingEventKind::ControlChange,
                midi_note: 17,
                channel: 2,
                articulation: Some(MappingArticulation::Continuous),
                cc_range: Some(CcRange::new(11, 118)),
            })
            .unwrap();
        let encoded = toml::to_string(&config).unwrap();
        let decoded: DrumKitConfiguration = toml::from_str(&encoded).unwrap();
        assert_eq!(decoded, config);
        assert!(!decoded.is_usable());
    }
}
