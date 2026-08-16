//! MIDI subsystem: parsing, events and the input backend.

pub mod backend;
pub mod event;
pub mod file;
pub mod parser;

pub use backend::{MidiBackend, MidiError, PortInfo};
pub use event::{MidiEvent, MidiMessage, MessageKind};
pub use file::{general_midi_component, ExpectedNote, MidiFile, MidiFileError, MidiTiming, MidiTrack, ParsedNote, SmfFormat, TempoEvent, TimeSignatureEvent};
pub use parser::MidiParser;
