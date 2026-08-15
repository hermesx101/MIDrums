//! MIDI subsystem: parsing, events and the input backend.

pub mod backend;
pub mod event;
pub mod parser;

pub use backend::{MidiBackend, MidiError, PortInfo};
pub use event::{MidiEvent, MidiMessage, MessageKind};
pub use parser::MidiParser;