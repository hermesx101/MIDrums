//! Musical transport, timing conversion, and GUI-independent timeline data.

use std::time::Duration;

use super::{ExpectedNote, MidiFile, MidiTiming, TempoEvent, TimeSignatureEvent};

const DEFAULT_MICROS_PER_QUARTER: u32 = 500_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportState { Stopped, Playing, Paused }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MusicalPosition {
    /// One-based bar number.
    pub measure: u64,
    /// One-based beat number within the bar.
    pub beat: u32,
    pub ticks_into_beat: u64,
    pub ticks_per_beat: u64,
}

#[derive(Clone, Debug)]
pub struct TempoMap {
    timing: MidiTiming,
    events: Vec<TempoEvent>,
}

impl TempoMap {
    pub fn new(timing: MidiTiming, events: &[TempoEvent]) -> Self {
        let mut events = events.to_vec();
        events.sort_by_key(|event| event.tick);
        // A later event at the same tick is the effective tempo.
        events.dedup_by(|previous, next| {
            if previous.tick == next.tick { *next = previous.clone(); true } else { false }
        });
        Self { timing, events }
    }

    pub fn seconds_at_tick(&self, target: f64) -> f64 {
        match self.timing {
            MidiTiming::Timecode { frames_per_second, ticks_per_frame } => target / (frames_per_second as f64 * ticks_per_frame as f64),
            MidiTiming::TicksPerQuarter(tpq) => {
                let mut seconds = 0.0;
                let mut previous_tick = 0.0;
                let mut micros_per_quarter = DEFAULT_MICROS_PER_QUARTER;
                for event in self.events.iter().filter(|event| event.tick as f64 <= target) {
                    seconds += (event.tick as f64 - previous_tick) * micros_per_quarter as f64 / (tpq as f64 * 1_000_000.0);
                    previous_tick = event.tick as f64;
                    micros_per_quarter = event.micros_per_quarter;
                }
                seconds + (target - previous_tick) * micros_per_quarter as f64 / (tpq as f64 * 1_000_000.0)
            }
        }
    }

    pub fn tick_at_seconds(&self, target: f64) -> f64 {
        let target = target.max(0.0);
        match self.timing {
            MidiTiming::Timecode { frames_per_second, ticks_per_frame } => target * frames_per_second as f64 * ticks_per_frame as f64,
            MidiTiming::TicksPerQuarter(tpq) => {
                let mut elapsed = 0.0;
                let mut previous_tick = 0.0;
                let mut micros_per_quarter = DEFAULT_MICROS_PER_QUARTER;
                for event in &self.events {
                    let seconds_to_event = (event.tick as f64 - previous_tick) * micros_per_quarter as f64 / (tpq as f64 * 1_000_000.0);
                    if elapsed + seconds_to_event >= target {
                        return previous_tick + (target - elapsed) * tpq as f64 * 1_000_000.0 / micros_per_quarter as f64;
                    }
                    elapsed += seconds_to_event;
                    previous_tick = event.tick as f64;
                    micros_per_quarter = event.micros_per_quarter;
                }
                previous_tick + (target - elapsed) * tpq as f64 * 1_000_000.0 / micros_per_quarter as f64
            }
        }
    }

    pub fn bpm_at_tick(&self, tick: u64) -> f64 {
        let micros = self.events.iter().filter(|event| event.tick <= tick).last().map(|event| event.micros_per_quarter).unwrap_or(DEFAULT_MICROS_PER_QUARTER);
        60_000_000.0 / micros as f64
    }
}

#[derive(Clone, Debug)]
pub struct MusicalPositionMap {
    timing: MidiTiming,
    signatures: Vec<TimeSignatureEvent>,
}

impl MusicalPositionMap {
    pub fn new(timing: MidiTiming, signatures: &[TimeSignatureEvent]) -> Self {
        let mut signatures = signatures.to_vec();
        signatures.sort_by_key(|event| event.tick);
        Self { timing, signatures }
    }

    pub fn position_at_tick(&self, tick: u64) -> MusicalPosition {
        let MidiTiming::TicksPerQuarter(tpq) = self.timing else {
            return MusicalPosition { measure: 1, beat: 1, ticks_into_beat: tick, ticks_per_beat: 1 };
        };
        let mut measure = 1;
        let mut section_start = 0;
        let mut numerator = 4u8;
        let mut denominator_exponent = 2u8;
        for signature in self.signatures.iter().filter(|signature| signature.tick <= tick) {
            if signature.tick > section_start {
                let ticks_per_measure = ticks_per_beat(tpq, denominator_exponent) * numerator as u64;
                measure += (signature.tick - section_start) / ticks_per_measure;
                section_start = signature.tick;
            }
            numerator = signature.numerator;
            denominator_exponent = signature.denominator_exponent;
        }
        let per_beat = ticks_per_beat(tpq, denominator_exponent);
        let elapsed = tick - section_start;
        let ticks_per_measure = per_beat * numerator as u64;
        measure += elapsed / ticks_per_measure;
        let within_measure = elapsed % ticks_per_measure;
        MusicalPosition { measure, beat: (within_measure / per_beat) as u32 + 1, ticks_into_beat: within_measure % per_beat, ticks_per_beat: per_beat }
    }

    pub fn beats_per_measure_at_tick(&self, tick: u64) -> u8 {
        self.signatures.iter().filter(|signature| signature.tick <= tick).last().map(|signature| signature.numerator).unwrap_or(4)
    }
}

fn ticks_per_beat(ticks_per_quarter: u16, denominator_exponent: u8) -> u64 {
    let denominator = 2u64.pow(denominator_exponent as u32);
    (ticks_per_quarter as u64 * 4) / denominator.max(1)
}

#[derive(Clone, Debug)]
pub struct TimelineEvent {
    pub expected: ExpectedNote,
    pub position: MusicalPosition,
}

#[derive(Clone, Debug)]
pub struct Timeline { pub events: Vec<TimelineEvent>, pub final_tick: u64 }

impl Timeline {
    pub fn from_expected_notes(notes: Vec<ExpectedNote>, position_map: &MusicalPositionMap, final_tick: u64) -> Self {
        let events = notes.into_iter().map(|expected| TimelineEvent { position: position_map.position_at_tick(expected.tick), expected }).collect();
        Self { events, final_tick }
    }
}

#[derive(Clone, Debug)]
pub struct MusicalTransport {
    state: TransportState,
    current_tick: f64,
    final_tick: u64,
    tempo_map: TempoMap,
    position_map: MusicalPositionMap,
}

impl MusicalTransport {
    pub fn new(timing: MidiTiming, tempos: &[TempoEvent], signatures: &[TimeSignatureEvent], final_tick: u64) -> Self {
        Self { state: TransportState::Stopped, current_tick: 0.0, final_tick, tempo_map: TempoMap::new(timing, tempos), position_map: MusicalPositionMap::new(timing, signatures) }
    }
    pub fn from_midi_file(file: &MidiFile) -> Self { Self::new(file.timing, &file.tempo_events, &file.time_signature_events, file.duration_ticks) }
    pub fn state(&self) -> TransportState { self.state }
    pub fn is_playing(&self) -> bool { self.state == TransportState::Playing }
    pub fn current_tick(&self) -> u64 { self.current_tick.round() as u64 }
    pub fn final_tick(&self) -> u64 { self.final_tick }
    pub fn elapsed_seconds(&self) -> f64 { self.tempo_map.seconds_at_tick(self.current_tick) }
    pub fn total_seconds(&self) -> f64 { self.tempo_map.seconds_at_tick(self.final_tick as f64) }
    pub fn current_bpm(&self) -> f64 { self.tempo_map.bpm_at_tick(self.current_tick()) }
    pub fn musical_position(&self) -> MusicalPosition { self.position_map.position_at_tick(self.current_tick()) }
    pub fn position_map(&self) -> &MusicalPositionMap { &self.position_map }
    pub fn play(&mut self) { if self.current_tick < self.final_tick as f64 { self.state = TransportState::Playing; } }
    pub fn pause(&mut self) { if self.state == TransportState::Playing { self.state = TransportState::Paused; } }
    pub fn stop(&mut self) { self.state = TransportState::Stopped; self.current_tick = 0.0; }
    pub fn seek(&mut self, tick: i64) { self.current_tick = tick.clamp(0, self.final_tick.min(i64::MAX as u64) as i64) as f64; }
    pub fn seek_to_end(&mut self) { self.current_tick = self.final_tick as f64; self.state = TransportState::Stopped; }
    pub fn advance_by(&mut self, elapsed: Duration) {
        if self.state != TransportState::Playing { return; }
        let next = self.tempo_map.tick_at_seconds(self.elapsed_seconds() + elapsed.as_secs_f64());
        if next >= self.final_tick as f64 { self.current_tick = self.final_tick as f64; self.state = TransportState::Stopped; } else { self.current_tick = next; }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn transport(bpm: u32, end: u64) -> MusicalTransport { MusicalTransport::new(MidiTiming::TicksPerQuarter(480), &[TempoEvent { tick: 0, micros_per_quarter: 60_000_000 / bpm }], &[], end) }
    #[test] fn stopped_starts_at_zero_and_stop_resets() { let mut t = transport(60, 2000); assert_eq!(t.state(), TransportState::Stopped); assert_eq!(t.current_tick(), 0); t.play(); t.advance_by(Duration::from_secs(1)); t.stop(); assert_eq!(t.current_tick(), 0); assert_eq!(t.state(), TransportState::Stopped); }
    #[test] fn play_pause_and_resume_are_deterministic() { let mut t = transport(60, 2000); t.play(); t.advance_by(Duration::from_secs(1)); assert_eq!(t.current_tick(), 480); t.pause(); t.advance_by(Duration::from_secs(2)); assert_eq!(t.current_tick(), 480); t.play(); t.advance_by(Duration::from_secs(1)); assert_eq!(t.current_tick(), 960); }
    #[test] fn seek_clamps_and_repositions() { let mut t = transport(60, 1000); t.seek(-2); assert_eq!(t.current_tick(), 0); t.seek(500); assert_eq!(t.current_tick(), 500); t.seek(5000); assert_eq!(t.current_tick(), 1000); }
    #[test] fn bpm_60_and_120_conversions_are_correct() { let sixty = transport(60, 2000); assert_eq!(sixty.elapsed_seconds(), 0.0); assert_eq!(sixty.tempo_map.seconds_at_tick(480.0), 1.0); assert_eq!(sixty.tempo_map.seconds_at_tick(1440.0), 3.0); let fast = transport(120, 2000); assert_eq!(fast.tempo_map.seconds_at_tick(480.0), 0.5); assert_eq!(fast.tempo_map.seconds_at_tick(960.0), 1.0); }
    #[test] fn tempo_changes_convert_both_directions() { let map = TempoMap::new(MidiTiming::TicksPerQuarter(480), &[TempoEvent { tick: 0, micros_per_quarter: 500_000 }, TempoEvent { tick: 480, micros_per_quarter: 1_000_000 }]); assert_eq!(map.seconds_at_tick(480.0), 0.5); assert_eq!(map.seconds_at_tick(960.0), 1.5); assert_eq!(map.tick_at_seconds(1.5), 960.0); }
    #[test] fn musical_positions_support_common_signatures() { let four = MusicalPositionMap::new(MidiTiming::TicksPerQuarter(480), &[]); assert_eq!(four.position_at_tick(1920), MusicalPosition { measure: 2, beat: 1, ticks_into_beat: 0, ticks_per_beat: 480 }); let three = MusicalPositionMap::new(MidiTiming::TicksPerQuarter(480), &[TimeSignatureEvent { tick: 0, numerator: 3, denominator_exponent: 2, clocks_per_click: 24, thirty_seconds_per_quarter: 8 }]); assert_eq!(three.position_at_tick(1440), MusicalPosition { measure: 2, beat: 1, ticks_into_beat: 0, ticks_per_beat: 480 }); let six_eight = MusicalPositionMap::new(MidiTiming::TicksPerQuarter(480), &[TimeSignatureEvent { tick: 0, numerator: 6, denominator_exponent: 3, clocks_per_click: 36, thirty_seconds_per_quarter: 8 }]); assert_eq!(six_eight.position_at_tick(1440), MusicalPosition { measure: 2, beat: 1, ticks_into_beat: 0, ticks_per_beat: 240 }); }
    #[test] fn end_of_song_clamps_and_stops() { let mut t = transport(60, 480); t.play(); t.advance_by(Duration::from_secs(2)); assert_eq!(t.current_tick(), 480); assert_eq!(t.state(), TransportState::Stopped); }
    #[test] fn advancement_is_independent_of_gui_frame_rate() { let mut one_frame = transport(60, 2_000); let mut ten_frames = transport(60, 2_000); one_frame.play(); ten_frames.play(); one_frame.advance_by(Duration::from_secs(1)); for _ in 0..10 { ten_frames.advance_by(Duration::from_millis(100)); } assert_eq!(one_frame.current_tick(), ten_frames.current_tick()); }
    #[test] fn timeline_reuses_expected_notes_and_assigns_musical_positions() { let map = MusicalPositionMap::new(MidiTiming::TicksPerQuarter(480), &[]); let note = ExpectedNote { tick: 480, timestamp_seconds: 1.0, midi_note: 36, velocity: 100, channel: 9, track: 0, drum_component: None, target_component: None }; let timeline = Timeline::from_expected_notes(vec![note], &map, 960); assert_eq!(timeline.events.len(), 1); assert_eq!(timeline.events[0].expected.midi_note, 36); assert_eq!(timeline.events[0].position.beat, 2); }
}
