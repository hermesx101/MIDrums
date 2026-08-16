//! DRUMS core library.
//!
//! Hardware-independent logic for the MIDI drum practice application. In
//! Phase 1 this crate contains only the MIDI foundation: a streaming parser,
//! event model and a thread-safe input backend. It deliberately has no GUI
//! or audio dependencies so it can be unit-tested quickly and reused by any
//! front-end.

pub mod mapping;
pub mod midi;
pub mod time;

pub use mapping::{
    DetectedHit, DrumComponent, DrumKitConfiguration, DrumMapping, MappingStore,
    SetupWizard, SetupWizardState, SUPPORTED_COMPONENTS,
};
pub use midi::{MidiBackend, MidiError, MidiEvent, MidiMessage, MidiParser, MessageKind, PortInfo};
pub use time::{Clock, SharedClock, SystemClock};