//! Transport-synchronised metronome event generation.

use super::{MusicalPosition, MusicalPositionMap, MusicalTransport, TransportState};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Subdivision { Quarter, Eighth, Sixteenth }

impl Subdivision {
    pub fn label(self) -> &'static str { match self { Self::Quarter => "Quarter", Self::Eighth => "Eighth", Self::Sixteenth => "Sixteenth" } }
    pub fn units_per_beat(self) -> u64 { match self { Self::Quarter => 1, Self::Eighth => 2, Self::Sixteenth => 4 } }
}

/// A display- and audio-independent description of the rhythmic grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RhythmGrid { subdivision: Subdivision }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RhythmGridSlot {
    pub beat: u32,
    pub subdivision_index: u8,
    pub ticks_per_subdivision: u64,
    pub is_main_beat: bool,
    pub is_accent: bool,
    pub label: String,
    pub active: bool,
}

impl RhythmGrid {
    pub fn new(subdivision: Subdivision) -> Self { Self { subdivision } }
    pub fn subdivision(self) -> Subdivision { self.subdivision }
    pub fn ticks_per_subdivision(self, position: MusicalPosition) -> u64 { (position.ticks_per_beat / self.subdivision.units_per_beat()).max(1) }
    pub fn slots_at_tick(self, position_map: &MusicalPositionMap, tick: u64) -> Vec<RhythmGridSlot> {
        let position = position_map.position_at_tick(tick);
        let beats = position_map.beats_per_measure_at_tick(tick);
        let units = self.subdivision.units_per_beat();
        let ticks_per_subdivision = self.ticks_per_subdivision(position);
        let active_subdivision = (position.ticks_into_beat / ticks_per_subdivision) as u8;
        (0..beats as u64).flat_map(|beat| (0..units).map(move |unit| RhythmGridSlot {
            beat: beat as u32 + 1,
            subdivision_index: unit as u8,
            ticks_per_subdivision,
            is_main_beat: unit == 0,
            is_accent: beat == 0 && unit == 0,
            label: if unit == 0 { (beat + 1).to_string() } else { offbeat_label(self.subdivision, unit as u8).to_string() },
            active: beat + 1 == position.beat as u64 && unit as u8 == active_subdivision,
        })).collect()
    }
}

fn offbeat_label(subdivision: Subdivision, subdivision_index: u8) -> &'static str {
    match (subdivision, subdivision_index) {
        (Subdivision::Eighth, 1) => "&",
        (Subdivision::Sixteenth, 1) => "e",
        (Subdivision::Sixteenth, 2) => "&",
        (Subdivision::Sixteenth, 3) => "a",
        _ => "",
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClickKind { Accent, Beat, Subdivision }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetronomeEvent {
    pub tick: u64,
    pub position: MusicalPosition,
    /// Zero-based position within the selected subdivision of the beat.
    pub subdivision: u8,
    pub kind: ClickKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CountToken { pub label: String, pub active: bool }

/// Generates boundaries crossed by a [`MusicalTransport`]. It owns no clock:
/// callers provide the current transport position through [`Self::observe`].
#[derive(Clone, Debug)]
pub struct Metronome { grid: RhythmGrid, last_tick: Option<u64> }

impl Metronome {
    pub fn new(subdivision: Subdivision) -> Self { Self { grid: RhythmGrid::new(subdivision), last_tick: None } }
    pub fn subdivision(&self) -> Subdivision { self.grid.subdivision() }
    pub fn set_subdivision(&mut self, subdivision: Subdivision) { self.grid = RhythmGrid::new(subdivision); self.last_tick = None; }
    /// Clears boundary history; the next playing observation emits a click at
    /// its current boundary (used for a new transport start, never for seek).
    pub fn reset(&mut self) { self.last_tick = None; }
    /// Aligns after seek/pause without creating an artificial click.
    pub fn sync_to_transport(&mut self, transport: &MusicalTransport) { self.last_tick = Some(transport.current_tick()); }
    pub fn observe(&mut self, transport: &MusicalTransport) -> Vec<MetronomeEvent> {
        let current = transport.current_tick();
        if transport.state() != TransportState::Playing { self.last_tick = Some(current); return Vec::new(); }
        let start = self.last_tick.map(|tick| tick.saturating_add(1)).unwrap_or(current);
        let events = boundaries_between(transport.position_map(), start, current, self.grid);
        self.last_tick = Some(current);
        events
    }
}

pub fn count_tokens(position_map: &MusicalPositionMap, tick: u64, subdivision: Subdivision) -> Vec<CountToken> {
    RhythmGrid::new(subdivision).slots_at_tick(position_map, tick).into_iter().map(|slot| CountToken { label: slot.label, active: slot.active }).collect()
}

fn boundaries_between(map: &MusicalPositionMap, start: u64, end: u64, grid: RhythmGrid) -> Vec<MetronomeEvent> {
    if start > end { return Vec::new(); }
    let mut out = Vec::new();
    let mut cursor = start;
    while cursor <= end {
        let position = map.position_at_tick(cursor);
        let unit = grid.ticks_per_subdivision(position);
        let offset = position.ticks_into_beat % unit;
        let boundary = cursor + (unit - offset) % unit;
        if boundary > end { break; }
        let position = map.position_at_tick(boundary);
        let subdivision_index = (position.ticks_into_beat / unit) as u8;
        let kind = if position.beat == 1 && subdivision_index == 0 { ClickKind::Accent } else if subdivision_index == 0 { ClickKind::Beat } else { ClickKind::Subdivision };
        out.push(MetronomeEvent { tick: boundary, position, subdivision: subdivision_index, kind });
        cursor = boundary.saturating_add(1);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::midi::{MidiTiming, TempoEvent, TimeSignatureEvent};
    use std::time::Duration;
    fn transport(signature: Option<TimeSignatureEvent>) -> MusicalTransport { MusicalTransport::new(MidiTiming::TicksPerQuarter(480), &[TempoEvent { tick: 0, micros_per_quarter: 1_000_000 }], &signature.into_iter().collect::<Vec<_>>(), 4_000) }
    #[test] fn quarter_and_eighth_boundaries_are_correct() { let mut t = transport(None); let mut quarter = Metronome::new(Subdivision::Quarter); let mut eighth = Metronome::new(Subdivision::Eighth); t.play(); assert_eq!(quarter.observe(&t).iter().map(|e| e.tick).collect::<Vec<_>>(), vec![0]); assert_eq!(eighth.observe(&t).iter().map(|e| e.tick).collect::<Vec<_>>(), vec![0]); t.advance_by(Duration::from_secs(1)); assert_eq!(quarter.observe(&t).iter().map(|e| e.tick).collect::<Vec<_>>(), vec![480]); eighth.sync_to_transport(&t); t.advance_by(Duration::from_millis(500)); assert_eq!(eighth.observe(&t).iter().map(|e| e.tick).collect::<Vec<_>>(), vec![720]); }
    #[test] fn accents_and_counts_follow_time_signatures() { let four = transport(None); assert_eq!(count_tokens(four.position_map(), 480, Subdivision::Eighth).iter().filter(|t| t.active).map(|t| t.label.as_str()).collect::<Vec<_>>(), vec!["2"]); let three = transport(Some(TimeSignatureEvent { tick: 0, numerator: 3, denominator_exponent: 2, clocks_per_click: 24, thirty_seconds_per_quarter: 8 })); assert_eq!(count_tokens(three.position_map(), 0, Subdivision::Eighth).len(), 6); let six = transport(Some(TimeSignatureEvent { tick: 0, numerator: 6, denominator_exponent: 3, clocks_per_click: 36, thirty_seconds_per_quarter: 8 })); assert_eq!(count_tokens(six.position_map(), 0, Subdivision::Eighth).len(), 12); let mut met = Metronome::new(Subdivision::Eighth); let mut t = transport(None); t.play(); assert_eq!(met.observe(&t)[0].kind, ClickKind::Accent); t.advance_by(Duration::from_secs(1)); assert_eq!(met.observe(&t)[1].kind, ClickKind::Beat); }
    #[test] fn pause_seek_stop_and_duplicate_positions_do_not_click() { let mut t = transport(None); let mut met = Metronome::new(Subdivision::Quarter); t.play(); assert_eq!(met.observe(&t).len(), 1); assert!(met.observe(&t).is_empty()); t.pause(); assert!(met.observe(&t).is_empty()); t.seek(960); met.sync_to_transport(&t); t.play(); assert!(met.observe(&t).is_empty()); t.stop(); met.reset(); assert!(met.observe(&t).is_empty()); }
    #[test] fn rhythm_grid_labels_and_ticks_cover_quarter_eighth_and_sixteenth() {
        let four = transport(None);
        let quarter = RhythmGrid::new(Subdivision::Quarter).slots_at_tick(four.position_map(), 480);
        assert_eq!(quarter.iter().map(|slot| slot.label.as_str()).collect::<Vec<_>>(), vec!["1", "2", "3", "4"]);
        assert_eq!(quarter[1].ticks_per_subdivision, 480);
        assert!(quarter[1].active && quarter[1].is_main_beat && !quarter[1].is_accent);
        let eighth = RhythmGrid::new(Subdivision::Eighth).slots_at_tick(four.position_map(), 720);
        assert_eq!(eighth.iter().map(|slot| slot.label.as_str()).collect::<Vec<_>>(), vec!["1", "&", "2", "&", "3", "&", "4", "&"]);
        assert_eq!(eighth[0].ticks_per_subdivision, 240);
        assert!(eighth[3].active && !eighth[3].is_main_beat);
        let sixteenth = RhythmGrid::new(Subdivision::Sixteenth).slots_at_tick(four.position_map(), 600);
        assert_eq!(sixteenth.len(), 16);
        assert_eq!(sixteenth.iter().map(|slot| slot.label.as_str()).collect::<Vec<_>>(), vec!["1", "e", "&", "a", "2", "e", "&", "a", "3", "e", "&", "a", "4", "e", "&", "a"]);
        assert_eq!(sixteenth[0].ticks_per_subdivision, 120);
        assert!(sixteenth[5].active && !sixteenth[5].is_main_beat);
    }
    #[test] fn rhythm_grid_respects_three_four_and_six_eight() {
        let three = transport(Some(TimeSignatureEvent { tick: 0, numerator: 3, denominator_exponent: 2, clocks_per_click: 24, thirty_seconds_per_quarter: 8 }));
        assert_eq!(RhythmGrid::new(Subdivision::Quarter).slots_at_tick(three.position_map(), 0).len(), 3);
        assert_eq!(RhythmGrid::new(Subdivision::Eighth).slots_at_tick(three.position_map(), 0).len(), 6);
        assert_eq!(RhythmGrid::new(Subdivision::Sixteenth).slots_at_tick(three.position_map(), 0).len(), 12);
        let six = transport(Some(TimeSignatureEvent { tick: 0, numerator: 6, denominator_exponent: 3, clocks_per_click: 36, thirty_seconds_per_quarter: 8 }));
        let grid = RhythmGrid::new(Subdivision::Sixteenth).slots_at_tick(six.position_map(), 60);
        assert_eq!(grid.len(), 24);
        assert_eq!(grid[0].ticks_per_subdivision, 60);
        assert!(grid[1].active);
    }
}
