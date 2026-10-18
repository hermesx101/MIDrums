//! Hardware-independent domain logic for MI Drums.
//!
//! MIDI input/parsing, drum mapping, transport, scoring, notation, session
//! history, performance, and progress live here. The crate deliberately has no
//! GUI dependency, keeping behavior testable and reusable by the desktop app.

pub mod audio_sync;
pub mod mapping;
pub mod midi;
pub mod notation;
pub mod performance;
pub mod practice;
pub mod progress;
pub mod session_history;
pub mod time;

pub use audio_sync::{audio_position, compensate_midi_input, looped_audio_position};
pub use mapping::{
    CcRange, DetectedHit, DrumComponent, DrumKitConfiguration, DrumMapping, HiHatPedalMode,
    MappingArticulation, MappingConflict, MappingEventKind, MappingInputIdentity, MappingStore,
    MidiInputResolver, ObservedMidiInput, SetupWizard, SetupWizardState, SUPPORTED_COMPONENTS,
};
pub use midi::{
    count_tokens, general_midi_component, suggest_subdivision, ClickKind, CountToken, ExpectedNote,
    MessageKind, Metronome, MetronomeEvent, MidiBackend, MidiError, MidiEvent, MidiFile,
    MidiFileError, MidiMessage, MidiParser, MidiTiming, MusicalPosition, MusicalPositionMap,
    MusicalTransport, PortInfo, RhythmGrid, RhythmGridSlot, Subdivision, SubdivisionSuggestion,
    SuggestionConfidence, TempoMap, Timeline, TimelineEvent, TransportState,
};
pub use notation::{
    DrumNotationScore, NotationDrumNote, NotationMeasure, NotationSlot, NotationTempo,
    RhythmicValue,
};
pub use performance::{
    ComponentPerformance, PerformanceConfig, PracticePerformance, TimingTendency,
};
pub use practice::{
    ActualHit, HiHatPedalState, MatchingConfig, PracticeResult, PracticeResultKind,
    PracticeSession, PracticeTarget, TargetState, TimingConfig, TimingJudgment,
};
pub use progress::{DrumProgressPoint, SessionProgressPoint, TrackProgress, WeakestDrum};
pub use session_history::{
    new_session_id, CompletedPracticeSession, DrumSessionStatistics, PracticeHitRecord,
    PracticeSessionStore, SessionBuildMetadata, SessionStatistics,
};
pub use time::{Clock, SharedClock, SystemClock};
