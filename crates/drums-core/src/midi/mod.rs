//! MIDI subsystem: parsing, events and the input backend.

pub mod backend;
pub mod event;
pub mod file;
pub mod metronome;
pub mod parser;
pub mod transport;

pub use backend::{MidiBackend, MidiError, PortInfo};
pub use event::{MessageKind, MidiEvent, MidiMessage};
pub use file::{
    general_midi_component, ExpectedNote, MidiFile, MidiFileError, MidiTiming, MidiTrack,
    ParsedNote, SmfFormat, TempoEvent, TimeSignatureEvent,
};
pub use metronome::{
    count_tokens, suggest_subdivision, ClickKind, CountToken, Metronome, MetronomeEvent,
    RhythmGrid, RhythmGridSlot, Subdivision, SubdivisionSuggestion, SuggestionConfidence,
};
pub use parser::MidiParser;
pub use transport::{
    MusicalPosition, MusicalPositionMap, MusicalTransport, TempoMap, Timeline, TimelineEvent,
    TransportState,
};
