//! Reusable MIDI-to-drum-notation intermediate representation.

use crate::{DrumComponent, ExpectedNote, MidiFile, MidiTiming, MusicalPositionMap};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RhythmicValue {
    Whole,
    DottedHalf,
    Half,
    DottedQuarter,
    Quarter,
    DottedEighth,
    Eighth,
    Sixteenth,
    ThirtySecond,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotationDrumNote {
    pub component: DrumComponent,
    pub velocity: u8,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NotationSlot {
    pub tick: u64,
    pub offset_ticks: u64,
    pub value: RhythmicValue,
    pub notes: Vec<NotationDrumNote>,
}
impl NotationSlot {
    pub fn is_rest(&self) -> bool {
        self.notes.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct NotationMeasure {
    pub number: u64,
    pub start_tick: u64,
    pub end_tick: u64,
    pub numerator: u8,
    pub denominator: u32,
    pub slots: Vec<NotationSlot>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NotationTempo {
    pub tick: u64,
    pub bpm: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DrumNotationScore {
    pub title: String,
    pub ticks_per_quarter: u16,
    pub grid_ticks: u64,
    pub measures: Vec<NotationMeasure>,
    pub tempos: Vec<NotationTempo>,
}

impl DrumNotationScore {
    pub fn from_midi(file: &MidiFile, track_index: usize) -> Option<Self> {
        let MidiTiming::TicksPerQuarter(_) = file.timing else {
            return None;
        };
        let track = file.tracks.get(track_index)?;
        let expected = track
            .notes
            .iter()
            .filter_map(|note| {
                notation_component(note.midi_note).map(|component| ExpectedNote {
                    tick: note.tick,
                    timestamp_seconds: note.timestamp_seconds,
                    midi_note: note.midi_note,
                    velocity: note.velocity,
                    channel: note.channel,
                    track: note.track,
                    drum_component: Some(component),
                    target_component: Some(component),
                })
            })
            .collect::<Vec<_>>();
        Self::from_expected_notes(file, &expected)
    }
    /// Builds notation from the same normalized events consumed by Practice.
    /// Hardware MIDI mapping and source parsing remain outside the notation layer.
    pub fn from_expected_notes(file: &MidiFile, expected: &[ExpectedNote]) -> Option<Self> {
        let MidiTiming::TicksPerQuarter(tpq) = file.timing else {
            return None;
        };
        let mut notes = expected
            .iter()
            .filter_map(|note| {
                note.drum_component.map(|component| {
                    (
                        note.tick,
                        NotationDrumNote {
                            component,
                            velocity: note.velocity,
                        },
                    )
                })
            })
            .collect::<Vec<_>>();
        notes.sort_by_key(|(tick, _)| *tick);
        let grid_ticks = choose_grid(
            tpq as u64,
            &notes.iter().map(|(tick, _)| *tick).collect::<Vec<_>>(),
        );
        let position_map = MusicalPositionMap::new(file.timing, &file.time_signature_events);
        let final_measure = position_map
            .position_at_tick(file.duration_ticks.saturating_sub(1))
            .measure
            .max(1);
        let mut measures = Vec::new();
        for number in 1..=final_measure {
            let start = position_map.tick_at_measure(number, file.duration_ticks);
            let mut end = position_map.tick_at_measure(number + 1, file.duration_ticks);
            if end <= start {
                end = file.duration_ticks.max(start + grid_ticks);
            }
            let (numerator, denominator) = position_map.time_signature_at_tick(start);
            let mut slots = Vec::new();
            let mut tick = start;
            while tick < end {
                let next = (tick + grid_ticks).min(end);
                let center = tick;
                let mut chord = notes
                    .iter()
                    .filter(|(raw, _)| quantize(*raw, grid_ticks) == center)
                    .map(|(_, note)| note.clone())
                    .collect::<Vec<_>>();
                chord.sort_by_key(|note| note.component);
                chord.dedup_by_key(|note| note.component);
                slots.push(NotationSlot {
                    tick,
                    offset_ticks: tick - start,
                    value: value_for_ticks(next - tick, tpq as u64),
                    notes: chord,
                });
                tick = next;
            }
            let slots = compress_rests(slots, tpq as u64, end);
            measures.push(NotationMeasure {
                number,
                start_tick: start,
                end_tick: end,
                numerator,
                denominator,
                slots,
            });
        }
        Some(Self {
            title: file.filename.clone(),
            ticks_per_quarter: tpq,
            grid_ticks,
            measures,
            tempos: file
                .tempo_events
                .iter()
                .map(|t| NotationTempo {
                    tick: t.tick,
                    bpm: t.bpm(),
                })
                .collect(),
        })
    }
    pub fn initial_bpm(&self) -> f64 {
        self.tempos.first().map(|t| t.bpm).unwrap_or(120.0)
    }
}

fn notation_component(note: u8) -> Option<DrumComponent> {
    match note {
        35 | 36 => Some(DrumComponent::Kick),
        37 | 38 | 40 => Some(DrumComponent::Snare),
        41 | 43 => Some(DrumComponent::LowTom),
        45 | 47 => Some(DrumComponent::MidTom),
        48 | 50 => Some(DrumComponent::HiTom),
        42 => Some(DrumComponent::HiHat),
        44 => Some(DrumComponent::HiHatPedal),
        46 => Some(DrumComponent::HiHatOpen),
        49 | 52 | 55 | 57 => Some(DrumComponent::Crash),
        51 | 53 | 56 | 59 => Some(DrumComponent::Ride),
        _ => None,
    }
}
fn quantize(tick: u64, grid: u64) -> u64 {
    ((tick as f64 / grid as f64).round() as u64) * grid
}
fn choose_grid(tpq: u64, ticks: &[u64]) -> u64 {
    for division in [1, 2, 4, 8] {
        let grid = (tpq / division).max(1);
        let tolerance = (grid / 8).max((tpq / 48).max(1));
        if ticks
            .iter()
            .all(|tick| tick.abs_diff(quantize(*tick, grid)) <= tolerance)
        {
            return grid;
        }
    }
    (tpq / 8).max(1)
}
fn value_for_ticks(ticks: u64, tpq: u64) -> RhythmicValue {
    let ratio = ticks as f64 / tpq as f64;
    let values = [
        (4.0, RhythmicValue::Whole),
        (3.0, RhythmicValue::DottedHalf),
        (2.0, RhythmicValue::Half),
        (1.5, RhythmicValue::DottedQuarter),
        (1.0, RhythmicValue::Quarter),
        (0.75, RhythmicValue::DottedEighth),
        (0.5, RhythmicValue::Eighth),
        (0.25, RhythmicValue::Sixteenth),
        (0.125, RhythmicValue::ThirtySecond),
    ];
    values
        .into_iter()
        .min_by(|a, b| (a.0 - ratio).abs().total_cmp(&(b.0 - ratio).abs()))
        .unwrap()
        .1
}
fn compress_rests(slots: Vec<NotationSlot>, tpq: u64, measure_end: u64) -> Vec<NotationSlot> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < slots.len() {
        if !slots[i].is_rest() {
            out.push(slots[i].clone());
            i += 1;
            continue;
        }
        let start = i;
        while i < slots.len() && slots[i].is_rest() {
            i += 1;
        }
        let run_start = slots[start].tick;
        let run_end = if i < slots.len() {
            slots[i].tick
        } else {
            measure_end
        };
        let mut tick = run_start;
        while tick < run_end {
            let remaining = run_end - tick;
            let choices = [
                tpq * 4,
                tpq * 3,
                tpq * 2,
                tpq * 3 / 2,
                tpq,
                tpq * 3 / 4,
                tpq / 2,
                tpq / 4,
                (tpq / 8).max(1),
            ];
            let duration = choices
                .into_iter()
                .find(|v| *v > 0 && *v <= remaining)
                .unwrap_or(remaining);
            out.push(NotationSlot {
                tick,
                offset_ticks: slots[start].offset_ticks + (tick - run_start),
                value: value_for_ticks(duration, tpq),
                notes: Vec::new(),
            });
            tick += duration;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::midi::{MidiTrack, ParsedNote, SmfFormat, TempoEvent, TimeSignatureEvent};
    fn file(notes: &[(u64, u8)]) -> MidiFile {
        MidiFile {
            filename: "lesson.mid".into(),
            format: SmfFormat::Format1,
            timing: MidiTiming::TicksPerQuarter(480),
            tracks: vec![MidiTrack {
                index: 0,
                name: Some("Drums".into()),
                event_count: notes.len(),
                note_event_count: notes.len(),
                channels: vec![9],
                likely_drum: true,
                drum_score: 8,
                notes: notes
                    .iter()
                    .map(|(tick, note)| ParsedNote {
                        tick: *tick,
                        timestamp_seconds: 0.0,
                        midi_note: *note,
                        velocity: 90,
                        channel: 9,
                        track: 0,
                    })
                    .collect(),
            }],
            tempo_events: vec![TempoEvent {
                tick: 0,
                micros_per_quarter: 1_000_000,
            }],
            time_signature_events: vec![TimeSignatureEvent {
                tick: 0,
                numerator: 4,
                denominator_exponent: 2,
                clocks_per_click: 24,
                thirty_seconds_per_quarter: 8,
            }],
            duration_ticks: 1920,
            duration_seconds: Some(4.0),
        }
    }
    #[test]
    fn simultaneous_drums_form_one_chord() {
        let s = DrumNotationScore::from_midi(&file(&[(0, 36), (0, 42), (480, 38)]), 0).unwrap();
        assert_eq!(s.measures[0].slots[0].notes.len(), 2);
        assert_eq!(
            s.measures[0].slots[1].notes[0].component,
            DrumComponent::Snare
        );
    }
    #[test]
    fn open_hat_pedal_crash_and_toms_are_distinct() {
        let s =
            DrumNotationScore::from_midi(&file(&[(0, 46), (480, 44), (960, 49), (1440, 45)]), 0)
                .unwrap();
        let c = s.measures[0]
            .slots
            .iter()
            .flat_map(|x| x.notes.iter().map(|n| n.component))
            .collect::<Vec<_>>();
        assert_eq!(
            c,
            [
                DrumComponent::HiHatOpen,
                DrumComponent::HiHatPedal,
                DrumComponent::Crash,
                DrumComponent::MidTom
            ]
        );
    }
    #[test]
    fn humanized_eighths_quantize_without_changing_source() {
        let f = file(&[(3, 42), (237, 42), (482, 42)]);
        let before = f.tracks[0].notes.clone();
        let s = DrumNotationScore::from_midi(&f, 0).unwrap();
        assert_eq!(s.grid_ticks, 240);
        assert_eq!(f.tracks[0].notes, before);
    }
    #[test]
    fn empty_and_unrecognized_tracks_are_safe() {
        let s = DrumNotationScore::from_midi(&file(&[(0, 1)]), 0).unwrap();
        assert!(s
            .measures
            .iter()
            .all(|m| m.slots.iter().all(NotationSlot::is_rest)));
    }
    #[test]
    fn tempo_and_signature_are_preserved() {
        let s = DrumNotationScore::from_midi(&file(&[(0, 36)]), 0).unwrap();
        assert_eq!(s.initial_bpm(), 60.0);
        assert_eq!((s.measures[0].numerator, s.measures[0].denominator), (4, 4));
    }
    #[test]
    fn sixteenth_note_patterns_choose_a_sixteenth_grid() {
        let s = DrumNotationScore::from_midi(&file(&[(0, 42), (120, 42), (240, 42), (360, 42)]), 0)
            .unwrap();
        assert_eq!(s.grid_ticks, 120);
    }
    #[test]
    fn non_four_four_measures_are_respected() {
        let mut f = file(&[(0, 36), (480, 38), (960, 42)]);
        f.time_signature_events[0].numerator = 3;
        f.duration_ticks = 1440;
        let s = DrumNotationScore::from_midi(&f, 0).unwrap();
        assert_eq!(s.measures.len(), 1);
        assert_eq!((s.measures[0].numerator, s.measures[0].denominator), (3, 4));
    }
    #[test]
    fn practice_notation_uses_normalized_component_not_raw_note_number() {
        let f = file(&[(0, 42)]);
        let mut expected = f.expected_notes_for_track(0, None);
        expected[0].drum_component = Some(DrumComponent::Kick);
        let score = DrumNotationScore::from_expected_notes(&f, &expected).unwrap();
        assert_eq!(
            score.measures[0].slots[0].notes[0].component,
            DrumComponent::Kick
        );
    }
}
