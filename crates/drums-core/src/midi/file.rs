//! Standard MIDI File parsing and inspection data.
//!
//! This module intentionally performs no GUI work.  It preserves the musical
//! tick domain alongside derived seconds, so later transport code can choose
//! the correct timing model without reparsing an imported file.

use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::path::Path;

use midly::{Format, MetaMessage, MidiMessage as SmfMidiMessage, Smf, Timing, TrackEventKind};

use crate::mapping::{DrumComponent, DrumKitConfiguration};

const DEFAULT_MICROS_PER_QUARTER: u32 = 500_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MidiFileError {
    UnsupportedExtension,
    ReadError(String),
    InvalidFile(String),
    EmptyFile,
}

impl fmt::Display for MidiFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedExtension => write!(f, "Please select a .mid or .midi file."),
            Self::ReadError(error) => write!(f, "Could not read MIDI file: {error}"),
            Self::InvalidFile(error) => write!(f, "Invalid MIDI file: {error}"),
            Self::EmptyFile => write!(f, "The MIDI file is empty."),
        }
    }
}

impl std::error::Error for MidiFileError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SmfFormat {
    Format0,
    Format1,
    Format2,
}

impl SmfFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Format0 => "SMF format 0",
            Self::Format1 => "SMF format 1",
            Self::Format2 => "SMF format 2",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MidiTiming {
    TicksPerQuarter(u16),
    Timecode { frames_per_second: u8, ticks_per_frame: u8 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TempoEvent {
    pub tick: u64,
    pub micros_per_quarter: u32,
}

impl TempoEvent {
    pub fn bpm(&self) -> f64 {
        60_000_000.0 / self.micros_per_quarter as f64
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimeSignatureEvent {
    pub tick: u64,
    pub numerator: u8,
    /// Original SMF denominator exponent (denominator = 2^exponent).
    pub denominator_exponent: u8,
    pub clocks_per_click: u8,
    pub thirty_seconds_per_quarter: u8,
}

impl TimeSignatureEvent {
    pub fn denominator(&self) -> u32 {
        2u32.pow(self.denominator_exponent as u32)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParsedNote {
    pub tick: u64,
    pub timestamp_seconds: f64,
    pub midi_note: u8,
    pub velocity: u8,
    /// Zero-based MIDI channel, matching raw MIDI conventions.
    pub channel: u8,
    pub track: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExpectedNote {
    pub tick: u64,
    pub timestamp_seconds: f64,
    pub midi_note: u8,
    pub velocity: u8,
    pub channel: u8,
    pub track: usize,
    /// General MIDI interpretation of the imported note. `None` means the
    /// song note is not one of this application's supported drum roles.
    pub drum_component: Option<DrumComponent>,
    /// The corresponding component on the user's configured kit. This is
    /// separate from `drum_component` so an imported GM role is still known
    /// even when the connected kit has not been configured for that role.
    pub target_component: Option<DrumComponent>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MidiTrack {
    pub index: usize,
    pub name: Option<String>,
    pub event_count: usize,
    pub note_event_count: usize,
    pub channels: Vec<u8>,
    pub likely_drum: bool,
    pub drum_score: u8,
    pub notes: Vec<ParsedNote>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MidiFile {
    pub filename: String,
    pub format: SmfFormat,
    pub timing: MidiTiming,
    pub tracks: Vec<MidiTrack>,
    pub tempo_events: Vec<TempoEvent>,
    pub time_signature_events: Vec<TimeSignatureEvent>,
    pub duration_ticks: u64,
    pub duration_seconds: Option<f64>,
}

impl MidiFile {
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, MidiFileError> {
        let path = path.as_ref();
        let extension = path.extension().and_then(|value| value.to_str());
        if !extension.is_some_and(|value| value.eq_ignore_ascii_case("mid") || value.eq_ignore_ascii_case("midi")) {
            return Err(MidiFileError::UnsupportedExtension);
        }
        let bytes = fs::read(path).map_err(|error| MidiFileError::ReadError(error.to_string()))?;
        let filename = path.file_name().and_then(|value| value.to_str()).unwrap_or("MIDI file");
        Self::parse(filename, &bytes)
    }

    pub fn parse(filename: impl Into<String>, bytes: &[u8]) -> Result<Self, MidiFileError> {
        if bytes.is_empty() {
            return Err(MidiFileError::EmptyFile);
        }
        let smf = Smf::parse(bytes).map_err(|error| MidiFileError::InvalidFile(error.to_string()))?;
        if smf.tracks.is_empty() {
            return Err(MidiFileError::InvalidFile("file contains no tracks".to_string()));
        }

        let format = match smf.header.format {
            Format::SingleTrack => SmfFormat::Format0,
            Format::Parallel => SmfFormat::Format1,
            Format::Sequential => SmfFormat::Format2,
        };
        let timing = match smf.header.timing {
            Timing::Metrical(ticks) => MidiTiming::TicksPerQuarter(ticks.as_int()),
            Timing::Timecode(fps, ticks) => MidiTiming::Timecode {
                frames_per_second: fps.as_int() as u8,
                ticks_per_frame: ticks,
            },
        };

        let mut raw_tracks = Vec::with_capacity(smf.tracks.len());
        let mut tempos = Vec::new();
        let mut signatures = Vec::new();
        let mut duration_ticks = 0;

        for (index, track) in smf.tracks.iter().enumerate() {
            let mut tick = 0u64;
            let mut name = None;
            let mut channels = BTreeSet::new();
            let mut notes = Vec::new();
            let mut note_event_count = 0;
            for event in track {
                tick += event.delta.as_int() as u64;
                duration_ticks = duration_ticks.max(tick);
                match event.kind {
                    TrackEventKind::Midi { channel, message } => {
                        let channel = channel.as_int();
                        channels.insert(channel);
                        if let SmfMidiMessage::NoteOn { key, vel } = message {
                            // SMF Note On velocity zero is semantically Note Off.
                            if vel.as_int() > 0 {
                                note_event_count += 1;
                                notes.push(ParsedNote { tick, timestamp_seconds: 0.0, midi_note: key.as_int(), velocity: vel.as_int(), channel, track: index });
                            }
                        }
                    }
                    TrackEventKind::Meta(MetaMessage::TrackName(value)) => {
                        if name.is_none() {
                            let text = String::from_utf8_lossy(value).trim().to_string();
                            if !text.is_empty() { name = Some(text); }
                        }
                    }
                    TrackEventKind::Meta(MetaMessage::Tempo(value)) => tempos.push(TempoEvent { tick, micros_per_quarter: value.as_int() }),
                    TrackEventKind::Meta(MetaMessage::TimeSignature(numerator, denominator_exponent, clocks, thirty_seconds)) => signatures.push(TimeSignatureEvent { tick, numerator, denominator_exponent, clocks_per_click: clocks, thirty_seconds_per_quarter: thirty_seconds }),
                    _ => {}
                }
            }
            raw_tracks.push((MidiTrack { index, name, event_count: track.len(), note_event_count, channels: channels.into_iter().collect(), likely_drum: false, drum_score: 0, notes }, tick));
        }

        tempos.sort_by_key(|event| event.tick);
        signatures.sort_by_key(|event| event.tick);
        deduplicate_tempos(&mut tempos);
        deduplicate_signatures(&mut signatures);
        let tempo_map = TempoMap::new(&timing, &tempos);

        let tracks = raw_tracks.into_iter().map(|(mut track, _)| {
            for note in &mut track.notes { note.timestamp_seconds = tempo_map.seconds_at(note.tick); }
            track.drum_score = drum_score(&track);
            track.likely_drum = track.drum_score >= 3;
            track
        }).collect();
        let duration_seconds = tempo_map.duration_seconds(duration_ticks);

        Ok(Self { filename: filename.into(), format, timing, tracks, tempo_events: tempos, time_signature_events: signatures, duration_ticks, duration_seconds })
    }

    pub fn suggested_drum_track(&self) -> Option<usize> {
        self.tracks.iter().filter(|track| track.likely_drum).max_by_key(|track| track.drum_score).map(|track| track.index)
    }

    pub fn expected_notes_for_track(&self, track_index: usize, mapping: Option<&DrumKitConfiguration>) -> Vec<ExpectedNote> {
        let Some(track) = self.tracks.get(track_index) else { return Vec::new(); };
        track.notes.iter().map(|note| ExpectedNote {
            tick: note.tick, timestamp_seconds: note.timestamp_seconds, midi_note: note.midi_note, velocity: note.velocity, channel: note.channel, track: note.track,
            drum_component: general_midi_component(note.midi_note),
            target_component: general_midi_component(note.midi_note)
                .filter(|component| mapping.is_some_and(|config| config.get(*component).is_some())),
        }).collect()
    }
}

/// Interprets the common General MIDI percussion notes supported by DRUMS.
///
/// This intentionally maps musical roles, not a user's hardware note
/// assignments. The latter remain in [`DrumKitConfiguration`].
pub fn general_midi_component(midi_note: u8) -> Option<DrumComponent> {
    match midi_note {
        35 | 36 => Some(DrumComponent::Kick),
        37 | 38 | 40 => Some(DrumComponent::Snare),
        41 | 43 => Some(DrumComponent::LowTom),
        42 | 44 | 46 => Some(DrumComponent::HiHat),
        45 | 47 => Some(DrumComponent::MidTom),
        48 | 50 => Some(DrumComponent::HiTom),
        49 | 52 | 55 | 57 => Some(DrumComponent::Crash),
        51 | 53 | 56 | 59 => Some(DrumComponent::Ride),
        _ => None,
    }
}

fn drum_score(track: &MidiTrack) -> u8 {
    let mut score = 0;
    if track.channels.contains(&9) { score += 4; }
    if track.name.as_deref().is_some_and(|name| { let name = name.to_ascii_lowercase(); name.contains("drum") || name.contains("percussion") || name.contains("perc") }) { score += 3; }
    if !track.notes.is_empty() {
        let percussion = track.notes.iter().filter(|note| (35..=81).contains(&note.midi_note)).count();
        if percussion * 2 >= track.notes.len() { score += 2; }
    }
    score
}

fn deduplicate_tempos(events: &mut Vec<TempoEvent>) { events.dedup_by(|a, b| a.tick == b.tick && { *b = a.clone(); true }); }
fn deduplicate_signatures(events: &mut Vec<TimeSignatureEvent>) { events.dedup_by(|a, b| a.tick == b.tick && { *b = a.clone(); true }); }

struct TempoMap<'a> { timing: &'a MidiTiming, events: &'a [TempoEvent] }
impl<'a> TempoMap<'a> {
    fn new(timing: &'a MidiTiming, events: &'a [TempoEvent]) -> Self { Self { timing, events } }
    fn seconds_at(&self, target: u64) -> f64 {
        match self.timing {
            MidiTiming::Timecode { frames_per_second, ticks_per_frame } => target as f64 / (*frames_per_second as f64 * *ticks_per_frame as f64),
            MidiTiming::TicksPerQuarter(tpq) => {
                let mut seconds = 0.0; let mut previous_tick = 0; let mut tempo = DEFAULT_MICROS_PER_QUARTER;
                for event in self.events.iter().filter(|event| event.tick <= target) {
                    seconds += (event.tick - previous_tick) as f64 * tempo as f64 / (*tpq as f64 * 1_000_000.0);
                    previous_tick = event.tick; tempo = event.micros_per_quarter;
                }
                seconds + (target - previous_tick) as f64 * tempo as f64 / (*tpq as f64 * 1_000_000.0)
            }
        }
    }
    fn duration_seconds(&self, ticks: u64) -> Option<f64> { Some(self.seconds_at(ticks)) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mapping::DrumMapping;

    fn midi(track_chunks: &[Vec<u8>], format: u16) -> Vec<u8> {
        let mut out = b"MThd\0\0\0\x06".to_vec(); out.extend_from_slice(&format.to_be_bytes()); out.extend_from_slice(&(track_chunks.len() as u16).to_be_bytes()); out.extend_from_slice(&480u16.to_be_bytes());
        for chunk in track_chunks { out.extend_from_slice(b"MTrk"); out.extend_from_slice(&(chunk.len() as u32).to_be_bytes()); out.extend_from_slice(chunk); } out
    }
    fn end() -> Vec<u8> { vec![0, 0xff, 0x2f, 0] }
    #[test]
    fn parses_notes_metadata_tracks_timing_and_mapping() {
        let mut conductor = vec![0, 0xff, 0x51, 3, 0x07, 0xa1, 0x20, 0, 0xff, 0x58, 4, 4, 2, 24, 8]; conductor.extend(end());
        let mut drums = vec![0, 0xff, 0x03, 5, b'D', b'r', b'u', b'm', b's', 0, 0x99, 36, 100, 0, 0x99, 36, 0, 0x83, 0x60, 0x99, 38, 110]; drums.extend(end());
        let file = MidiFile::parse("fixture.mid", &midi(&[conductor, drums], 1)).unwrap();
        assert_eq!(file.format, SmfFormat::Format1); assert_eq!(file.timing, MidiTiming::TicksPerQuarter(480)); assert_eq!(file.tracks.len(), 2); assert_eq!(file.tracks[1].name.as_deref(), Some("Drums")); assert_eq!(file.tracks[1].note_event_count, 2); assert_eq!(file.tracks[1].notes[1].tick, 480); assert_eq!(file.tracks[1].notes[1].velocity, 110); assert_eq!(file.tempo_events[0].bpm(), 120.0); assert_eq!(file.time_signature_events[0].denominator(), 4); assert_eq!(file.suggested_drum_track(), Some(1));
        let mut mapping = DrumKitConfiguration::new("kit"); mapping.set(DrumMapping { component: DrumComponent::Kick, midi_note: 24, channel: 10 });
        let expected = file.expected_notes_for_track(1, Some(&mapping)); assert_eq!(expected[0].drum_component, Some(DrumComponent::Kick)); assert_eq!(expected[0].target_component, Some(DrumComponent::Kick)); assert_eq!(expected[1].drum_component, Some(DrumComponent::Snare)); assert_eq!(expected[1].target_component, None);
    }
    #[test]
    fn preserves_multiple_tempo_changes_and_note_offs_are_not_hits() {
        let mut track = vec![0, 0xff, 0x51, 3, 0x07, 0xa1, 0x20, 0x83, 0x60, 0xff, 0x51, 3, 0x09, 0x27, 0xc0, 0, 0x89, 40, 64, 0, 0x99, 42, 0, 0x83, 0x60, 0x99, 42, 90]; track.extend(end());
        let file = MidiFile::parse("tempo.mid", &midi(&[track], 0)).unwrap(); assert_eq!(file.tempo_events.len(), 2); assert_eq!(file.tracks[0].notes.len(), 1); assert_eq!(file.tracks[0].notes[0].tick, 960); assert!((file.tracks[0].notes[0].timestamp_seconds - 1.1).abs() < 0.0001);
    }
    #[test]
    fn invalid_and_empty_files_are_errors() { assert_eq!(MidiFile::parse("x.mid", &[]).unwrap_err(), MidiFileError::EmptyFile); assert!(matches!(MidiFile::parse("x.mid", b"not midi"), Err(MidiFileError::InvalidFile(_)))); }
    #[test]
    fn general_midi_notes_map_to_supported_drum_components() {
        assert_eq!(general_midi_component(36), Some(DrumComponent::Kick));
        assert_eq!(general_midi_component(38), Some(DrumComponent::Snare));
        assert_eq!(general_midi_component(42), Some(DrumComponent::HiHat));
        assert_eq!(general_midi_component(49), Some(DrumComponent::Crash));
        assert_eq!(general_midi_component(51), Some(DrumComponent::Ride));
        assert_eq!(general_midi_component(48), Some(DrumComponent::HiTom));
        assert_eq!(general_midi_component(12), None);
    }
    #[test]
    fn imports_from_a_midi_path_and_reports_path_errors() {
        let path = std::env::temp_dir().join(format!("drums_phase3a_{}.mid", std::process::id()));
        let mut track = vec![0, 0x99, 36, 100]; track.extend(end());
        std::fs::write(&path, midi(&[track], 0)).unwrap();
        let imported = MidiFile::from_path(&path).unwrap();
        assert_eq!(imported.tracks[0].notes[0].midi_note, 36);
        std::fs::remove_file(&path).unwrap();
        assert_eq!(MidiFile::from_path("not-midi.txt").unwrap_err(), MidiFileError::UnsupportedExtension);
        assert!(matches!(MidiFile::from_path("missing.mid"), Err(MidiFileError::ReadError(_))));
    }
}
