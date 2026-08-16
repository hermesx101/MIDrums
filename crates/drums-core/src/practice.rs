//! Deterministic, GUI-independent practice event matching.
//!
//! The musical transport remains the only timing authority. Live MIDI input is
//! sampled into the transport tick domain, then matched against imported
//! expected events using mapped drum components rather than raw note numbers.

use std::time::Duration;

use crate::mapping::{DrumComponent, DrumKitConfiguration};
use crate::midi::{ExpectedNote, MessageKind, MidiEvent, MusicalTransport};

#[derive(Clone, Debug, PartialEq)]
pub struct ActualHit {
    pub received_midi_note: u8,
    pub drum_component: Option<DrumComponent>,
    pub velocity: u8,
    /// Zero-based MIDI channel.
    pub channel: u8,
    /// Original monotonic input timestamp from the MIDI callback.
    pub input_timestamp_micros: u64,
    /// Effective elapsed song time sampled from the transport.
    pub elapsed_seconds: f64,
    pub transport_tick: u64,
}

impl ActualHit {
    pub fn from_midi_event(
        event: &MidiEvent,
        mapping: &DrumKitConfiguration,
        transport: &MusicalTransport,
    ) -> Option<Self> {
        Self::from_midi_event_observed_at(event, mapping, transport, event.timestamp_micros)
    }

    /// Converts callback time to the transport domain by subtracting only the
    /// queue/drain delay measured on the same monotonic application clock.
    pub fn from_midi_event_observed_at(
        event: &MidiEvent,
        mapping: &DrumKitConfiguration,
        transport: &MusicalTransport,
        observed_at_micros: u64,
    ) -> Option<Self> {
        if event.message.kind != MessageKind::NoteOn || event.message.velocity().unwrap_or(0) == 0 {
            return None;
        }
        let note = event.message.note()?;
        let channel = event.message.channel?;
        let component = mapping
            .mappings
            .iter()
            .find(|entry| entry.midi_note == note && entry.channel == channel + 1)
            .map(|entry| entry.component);
        let queue_delay_seconds =
            observed_at_micros.saturating_sub(event.timestamp_micros) as f64 / 1_000_000.0;
        let elapsed_seconds = (transport.elapsed_seconds() - queue_delay_seconds).max(0.0);
        Some(Self {
            received_midi_note: note,
            drum_component: component,
            velocity: event.message.velocity().unwrap_or(0),
            channel,
            input_timestamp_micros: event.timestamp_micros,
            elapsed_seconds,
            transport_tick: transport.effective_tick_at_seconds(elapsed_seconds),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PracticeResultKind {
    CorrectHit,
    WrongDrum,
    Missed,
    Ignored,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimingJudgment {
    Early,
    Good,
    Late,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimingConfig {
    pub good_window: Duration,
    pub match_window: Duration,
}

impl Default for TimingConfig {
    fn default() -> Self {
        Self {
            good_window: Duration::from_millis(50),
            match_window: Duration::from_millis(100),
        }
    }
}

impl TimingJudgment {
    /// Classifies `actual - expected`: negative is early and positive is late.
    /// The Good window and outer match window are inclusive.
    pub fn from_difference_micros(difference_micros: i64, config: TimingConfig) -> Option<Self> {
        let absolute = difference_micros.unsigned_abs() as u128;
        if absolute > config.match_window.as_micros() {
            None
        } else if absolute <= config.good_window.as_micros() {
            Some(Self::Good)
        } else if difference_micros < 0 {
            Some(Self::Early)
        } else {
            Some(Self::Late)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetState {
    Upcoming,
    Hittable,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PracticeTarget {
    pub expected_indices: Vec<usize>,
    pub notes: Vec<ExpectedNote>,
    pub tick: u64,
    /// Target minus current transport time. Positive is upcoming.
    pub time_until_seconds: f64,
    pub state: TargetState,
    pub ordinal_start: usize,
    pub ordinal_end: usize,
    pub total_matchable: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PracticeResult {
    pub kind: PracticeResultKind,
    pub expected: Option<ExpectedNote>,
    pub actual: Option<ActualHit>,
    /// Actual minus expected. Negative is early; positive is late.
    pub timing_difference_ticks: Option<i64>,
    /// Actual minus expected in the effective practice-tempo time domain.
    pub timing_difference_micros: Option<i64>,
    pub timing_judgment: Option<TimingJudgment>,
}

pub type MatchingConfig = TimingConfig;

#[derive(Clone, Debug)]
pub struct PracticeSession {
    expected: Vec<ExpectedNote>,
    consumed: Vec<bool>,
    results: Vec<PracticeResult>,
    config: TimingConfig,
}

impl PracticeSession {
    pub fn new(mut expected: Vec<ExpectedNote>, config: TimingConfig) -> Self {
        expected.sort_by_key(|note| note.tick);
        let consumed = vec![false; expected.len()];
        Self {
            expected,
            consumed,
            results: Vec::new(),
            config,
        }
    }

    pub fn expected(&self) -> &[ExpectedNote] {
        &self.expected
    }
    pub fn results(&self) -> &[PracticeResult] {
        &self.results
    }
    pub fn pending_count(&self) -> usize {
        self.consumed.iter().filter(|value| !**value).count()
    }
    pub fn matchable_count(&self) -> usize {
        self.expected
            .iter()
            .filter(|note| note.drum_component.is_some())
            .count()
    }
    pub fn finalized_matchable_count(&self) -> usize {
        self.expected
            .iter()
            .zip(&self.consumed)
            .filter(|(note, consumed)| note.drum_component.is_some() && **consumed)
            .count()
    }
    pub fn matching_window(&self) -> Duration {
        self.config.match_window
    }
    pub fn next_pending(&self, tick: u64) -> Option<(usize, &ExpectedNote)> {
        self.expected
            .iter()
            .enumerate()
            .find(|(index, note)| !self.consumed[*index] && note.tick >= tick)
    }
    pub fn next_known_pending(&self, tick: u64) -> Option<(usize, &ExpectedNote)> {
        self.expected.iter().enumerate().find(|(index, note)| {
            !self.consumed[*index] && note.tick >= tick && note.drum_component.is_some()
        })
    }
    pub fn next_known_after(&self, expected_index: usize) -> Option<(usize, &ExpectedNote)> {
        self.expected
            .iter()
            .enumerate()
            .skip(expected_index.saturating_add(1))
            .find(|(index, note)| !self.consumed[*index] && note.drum_component.is_some())
    }
    pub fn current_target(
        &self,
        tick: u64,
        transport: &MusicalTransport,
    ) -> Option<PracticeTarget> {
        let (first_index, first) = self
            .expected
            .iter()
            .enumerate()
            .find(|(index, note)| !self.consumed[*index] && note.drum_component.is_some())?;
        self.target_from(first_index, first, tick, transport)
    }
    pub fn next_target_after(
        &self,
        expected_index: usize,
        tick: u64,
        transport: &MusicalTransport,
    ) -> Option<PracticeTarget> {
        let (first_index, first) = self
            .expected
            .iter()
            .enumerate()
            .skip(expected_index.saturating_add(1))
            .find(|(index, note)| !self.consumed[*index] && note.drum_component.is_some())?;
        self.target_from(first_index, first, tick, transport)
    }
    fn target_from(
        &self,
        first_index: usize,
        first: &ExpectedNote,
        tick: u64,
        transport: &MusicalTransport,
    ) -> Option<PracticeTarget> {
        let target_tick = first.tick;
        let grouped: Vec<(usize, &ExpectedNote)> = self
            .expected
            .iter()
            .enumerate()
            .filter(|(index, note)| {
                !self.consumed[*index] && note.drum_component.is_some() && note.tick == target_tick
            })
            .collect();
        let time_until_seconds = transport.effective_seconds_at_tick(target_tick)
            - transport.effective_seconds_at_tick(tick);
        let ordinal_start = self.expected[..first_index]
            .iter()
            .filter(|note| note.drum_component.is_some())
            .count()
            + 1;
        let group_len = grouped.len();
        Some(PracticeTarget {
            expected_indices: grouped.iter().map(|(index, _)| *index).collect(),
            notes: grouped.into_iter().map(|(_, note)| note.clone()).collect(),
            tick: target_tick,
            time_until_seconds,
            state: if time_until_seconds.abs() <= self.config.match_window.as_secs_f64() {
                TargetState::Hittable
            } else {
                TargetState::Upcoming
            },
            ordinal_start,
            ordinal_end: ordinal_start + group_len - 1,
            total_matchable: self.matchable_count(),
        })
    }

    /// Consumes one actual hit. Candidates must be within the inclusive time
    /// window. Same-component candidates are preferred, then absolute timing
    /// distance, expected tick, and original stable order break ties.
    pub fn record_hit(
        &mut self,
        actual: ActualHit,
        transport: &MusicalTransport,
    ) -> &PracticeResult {
        let window_micros = self.config.match_window.as_micros() as i64;
        let mut candidates: Vec<(usize, bool, i64, u64)> = self
            .expected
            .iter()
            .enumerate()
            .filter(|(index, expected)| !self.consumed[*index] && expected.drum_component.is_some())
            .filter_map(|(index, expected)| {
                let difference =
                    timing_difference_micros(expected.tick, actual.transport_tick, transport);
                (difference.abs() <= window_micros).then_some((
                    index,
                    expected.drum_component != actual.drum_component,
                    difference.abs(),
                    expected.tick,
                ))
            })
            .collect();
        candidates.sort_by(|left, right| {
            let (left_index, left_wrong, left_distance, left_tick) = *left;
            let (right_index, right_wrong, right_distance, right_tick) = *right;
            (left_wrong, left_distance, left_tick)
                .cmp(&(right_wrong, right_distance, right_tick))
                // For an all-wrong simultaneous group, resolve the later stable
                // target first. This preserves an earlier same-component target
                // for a following valid hit while still consuming only one event.
                .then_with(|| {
                    if left_wrong {
                        right_index.cmp(&left_index)
                    } else {
                        left_index.cmp(&right_index)
                    }
                })
        });

        let result = if let Some((index, _, _, _)) = candidates.first().copied() {
            let expected = self.expected[index].clone();
            let kind = if expected.drum_component == actual.drum_component {
                PracticeResultKind::CorrectHit
            } else {
                PracticeResultKind::WrongDrum
            };
            // CorrectHit and WrongDrum are both final outcomes for exactly one
            // expected event. Leaving a wrong target pending would later emit a
            // second, contradictory Missed result for the same event.
            self.consumed[index] = true;
            let timing_difference =
                timing_difference_micros(expected.tick, actual.transport_tick, transport);
            PracticeResult {
                kind,
                timing_difference_ticks: Some(signed_tick_difference(
                    actual.transport_tick,
                    expected.tick,
                )),
                timing_difference_micros: Some(timing_difference),
                timing_judgment: if kind == PracticeResultKind::CorrectHit {
                    TimingJudgment::from_difference_micros(timing_difference, self.config)
                } else {
                    None
                },
                expected: Some(expected),
                actual: Some(actual),
            }
        } else {
            PracticeResult {
                kind: PracticeResultKind::Ignored,
                expected: None,
                actual: Some(actual),
                timing_difference_ticks: None,
                timing_difference_micros: None,
                timing_judgment: None,
            }
        };
        self.results.push(result);
        self.results.last().unwrap()
    }

    /// Finalizes events only after the late edge of their matching window.
    /// Unknown expected notes are preserved as `Ignored`, since Phase 4A has
    /// no safe component with which to match them.
    pub fn advance_to_tick(&mut self, tick: u64, transport: &MusicalTransport) {
        let now = transport.effective_seconds_at_tick(tick);
        let window = self.config.match_window.as_secs_f64();
        for index in 0..self.expected.len() {
            if self.consumed[index] {
                continue;
            }
            let expected_time = transport.effective_seconds_at_tick(self.expected[index].tick);
            if now > expected_time + window {
                self.consumed[index] = true;
                self.results.push(PracticeResult {
                    kind: if self.expected[index].drum_component.is_some() {
                        PracticeResultKind::Missed
                    } else {
                        PracticeResultKind::Ignored
                    },
                    expected: Some(self.expected[index].clone()),
                    actual: None,
                    timing_difference_ticks: None,
                    timing_difference_micros: None,
                    timing_judgment: None,
                });
            }
        }
    }

    pub fn finish(&mut self) {
        for index in 0..self.expected.len() {
            if self.consumed[index] {
                continue;
            }
            self.consumed[index] = true;
            self.results.push(PracticeResult {
                kind: if self.expected[index].drum_component.is_some() {
                    PracticeResultKind::Missed
                } else {
                    PracticeResultKind::Ignored
                },
                expected: Some(self.expected[index].clone()),
                actual: None,
                timing_difference_ticks: None,
                timing_difference_micros: None,
                timing_judgment: None,
            });
        }
    }

    /// Used for stop-to-beginning and arbitrary seek. Phase 4A intentionally
    /// invalidates all matching history rather than preserving partial scoring.
    pub fn reset(&mut self) {
        self.consumed.fill(false);
        self.results.clear();
    }
    pub fn on_seek(&mut self) {
        self.reset();
    }
    pub fn on_seek_to_tick(&mut self, tick: u64, transport: &MusicalTransport) {
        self.reset();
        let now = transport.effective_seconds_at_tick(tick);
        let window = self.config.match_window.as_secs_f64();
        for (index, expected) in self.expected.iter().enumerate() {
            if now > transport.effective_seconds_at_tick(expected.tick) + window {
                self.consumed[index] = true;
            }
        }
    }
    pub fn on_stop(&mut self) {
        self.reset();
    }
}

fn signed_tick_difference(actual: u64, expected: u64) -> i64 {
    (actual as i128 - expected as i128).clamp(i64::MIN as i128, i64::MAX as i128) as i64
}

fn timing_difference_micros(expected: u64, actual: u64, transport: &MusicalTransport) -> i64 {
    ((transport.effective_seconds_at_tick(actual) - transport.effective_seconds_at_tick(expected))
        * 1_000_000.0)
        .round() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mapping::DrumMapping;
    use crate::midi::{MidiMessage, MidiTiming, TempoEvent};

    fn transport() -> MusicalTransport {
        MusicalTransport::new(
            MidiTiming::TicksPerQuarter(480),
            &[TempoEvent {
                tick: 0,
                micros_per_quarter: 500_000,
            }],
            &[],
            10_000,
        )
    }
    fn expected(component: Option<DrumComponent>, tick: u64) -> ExpectedNote {
        ExpectedNote {
            tick,
            timestamp_seconds: tick as f64 / 960.0,
            midi_note: component
                .map(|value| match value {
                    DrumComponent::Kick => 36,
                    DrumComponent::Snare => 38,
                    DrumComponent::HiHat => 42,
                    _ => 49,
                })
                .unwrap_or(69),
            velocity: 90,
            channel: 9,
            track: 1,
            drum_component: component,
            target_component: component,
        }
    }
    fn actual(component: Option<DrumComponent>, tick: u64) -> ActualHit {
        ActualHit {
            received_midi_note: component
                .map(|value| match value {
                    DrumComponent::Kick => 24,
                    DrumComponent::Snare => 25,
                    DrumComponent::HiHat => 26,
                    _ => 27,
                })
                .unwrap_or(99),
            drum_component: component,
            velocity: 100,
            channel: 9,
            input_timestamp_micros: 123,
            elapsed_seconds: tick as f64 / 960.0,
            transport_tick: tick,
        }
    }
    fn session(notes: Vec<ExpectedNote>) -> PracticeSession {
        PracticeSession::new(notes, MatchingConfig::default())
    }

    #[test]
    fn correct_kick_and_snare_hits() {
        let t = transport();
        let mut s = session(vec![
            expected(Some(DrumComponent::Kick), 960),
            expected(Some(DrumComponent::Snare), 1440),
        ]);
        assert_eq!(
            s.record_hit(actual(Some(DrumComponent::Kick), 965), &t)
                .kind,
            PracticeResultKind::CorrectHit
        );
        assert_eq!(
            s.record_hit(actual(Some(DrumComponent::Snare), 1430), &t)
                .kind,
            PracticeResultKind::CorrectHit
        );
    }
    #[test]
    fn wrong_drum_finalizes_target_and_cannot_later_be_missed() {
        let t = transport();
        let mut s = session(vec![expected(Some(DrumComponent::Kick), 960)]);
        let result = s.record_hit(actual(Some(DrumComponent::Snare), 965), &t);
        assert_eq!(result.kind, PracticeResultKind::WrongDrum);
        assert_eq!(
            result.expected.as_ref().unwrap().drum_component,
            Some(DrumComponent::Kick)
        );
        assert_eq!(
            result.actual.as_ref().unwrap().drum_component,
            Some(DrumComponent::Snare)
        );
        assert_eq!(s.pending_count(), 0);
        s.finish();
        assert_eq!(s.results().len(), 1);
        assert_eq!(
            s.record_hit(actual(Some(DrumComponent::Kick), 970), &t)
                .kind,
            PracticeResultKind::Ignored
        );
        assert_eq!(s.pending_count(), 0);
    }
    #[test]
    fn missed_is_not_finalized_prematurely() {
        let t = transport();
        let mut s = session(vec![expected(Some(DrumComponent::Kick), 960)]);
        s.advance_to_tick(1056, &t);
        assert!(s.results().is_empty());
        s.advance_to_tick(1057, &t);
        assert_eq!(s.results()[0].kind, PracticeResultKind::Missed);
    }
    #[test]
    fn extra_hit_is_preserved_as_ignored() {
        let t = transport();
        let mut s = session(vec![expected(Some(DrumComponent::Kick), 960)]);
        let result = s.record_hit(actual(Some(DrumComponent::Crash), 500), &t);
        assert_eq!(result.kind, PracticeResultKind::Ignored);
        assert!(result.actual.is_some());
    }
    #[test]
    fn early_is_negative_and_late_is_positive() {
        let t = transport();
        let mut early = session(vec![expected(Some(DrumComponent::Kick), 960)]);
        assert!(
            early
                .record_hit(actual(Some(DrumComponent::Kick), 950), &t)
                .timing_difference_micros
                .unwrap()
                < 0
        );
        let mut late = session(vec![expected(Some(DrumComponent::Kick), 960)]);
        assert!(
            late.record_hit(actual(Some(DrumComponent::Kick), 970), &t)
                .timing_difference_micros
                .unwrap()
                > 0
        );
    }
    #[test]
    fn one_actual_cannot_match_two_expected_events() {
        let t = transport();
        let mut s = session(vec![
            expected(Some(DrumComponent::Kick), 960),
            expected(Some(DrumComponent::Kick), 1000),
        ]);
        s.record_hit(actual(Some(DrumComponent::Kick), 980), &t);
        assert_eq!(s.pending_count(), 1);
        assert_eq!(s.results().len(), 1);
        assert_eq!(s.results()[0].expected.as_ref().unwrap().tick, 960);
    }
    #[test]
    fn simultaneous_components_both_match() {
        let t = transport();
        let mut s = session(vec![
            expected(Some(DrumComponent::Kick), 960),
            expected(Some(DrumComponent::HiHat), 960),
        ]);
        s.record_hit(actual(Some(DrumComponent::HiHat), 968), &t);
        s.record_hit(actual(Some(DrumComponent::Kick), 965), &t);
        assert!(
            s.results()
                .iter()
                .all(|result| result.kind == PracticeResultKind::CorrectHit)
        );
        assert_eq!(s.pending_count(), 0);
    }
    #[test]
    fn repeated_kicks_choose_closest_with_earlier_tie_break() {
        let t = transport();
        let mut s = session(vec![
            expected(Some(DrumComponent::Kick), 960),
            expected(Some(DrumComponent::Kick), 1056),
            expected(Some(DrumComponent::Kick), 1152),
        ]);
        assert_eq!(
            s.record_hit(actual(Some(DrumComponent::Kick), 1050), &t)
                .expected
                .as_ref()
                .unwrap()
                .tick,
            1056
        );
        let mut tie = session(vec![
            expected(Some(DrumComponent::Kick), 960),
            expected(Some(DrumComponent::Kick), 1000),
        ]);
        assert_eq!(
            tie.record_hit(actual(Some(DrumComponent::Kick), 980), &t)
                .expected
                .as_ref()
                .unwrap()
                .tick,
            960
        );
    }
    #[test]
    fn unknown_expected_is_never_a_correct_hit_and_is_preserved() {
        let t = transport();
        let mut s = session(vec![expected(None, 960)]);
        assert_eq!(
            s.record_hit(actual(Some(DrumComponent::Kick), 960), &t)
                .kind,
            PracticeResultKind::Ignored
        );
        s.advance_to_tick(1057, &t);
        assert_eq!(s.results()[1].kind, PracticeResultKind::Ignored);
        assert!(
            s.results()[1]
                .expected
                .as_ref()
                .unwrap()
                .drum_component
                .is_none()
        );
    }
    #[test]
    fn empty_expected_and_empty_actual_are_safe() {
        let t = transport();
        let mut empty = session(Vec::new());
        assert_eq!(
            empty
                .record_hit(actual(Some(DrumComponent::Kick), 0), &t)
                .kind,
            PracticeResultKind::Ignored
        );
        let mut no_actual = session(vec![expected(Some(DrumComponent::Kick), 0)]);
        no_actual.advance_to_tick(97, &t);
        assert_eq!(no_actual.results()[0].kind, PracticeResultKind::Missed);
    }
    #[test]
    fn matching_window_is_inclusive_and_just_outside_is_ignored() {
        let t = transport();
        let mut boundary = session(vec![expected(Some(DrumComponent::Kick), 960)]);
        assert_eq!(
            boundary
                .record_hit(actual(Some(DrumComponent::Kick), 1056), &t)
                .kind,
            PracticeResultKind::CorrectHit
        );
        let mut outside = session(vec![expected(Some(DrumComponent::Kick), 960)]);
        assert_eq!(
            outside
                .record_hit(actual(Some(DrumComponent::Kick), 1057), &t)
                .kind,
            PracticeResultKind::Ignored
        );
    }
    #[test]
    fn stop_and_seek_clear_matching_state() {
        let t = transport();
        let mut s = session(vec![expected(Some(DrumComponent::Kick), 960)]);
        s.record_hit(actual(Some(DrumComponent::Kick), 960), &t);
        s.on_stop();
        assert!(s.results().is_empty());
        assert_eq!(s.pending_count(), 1);
        s.record_hit(actual(Some(DrumComponent::Kick), 960), &t);
        s.on_seek();
        assert!(s.results().is_empty());
        assert_eq!(s.pending_count(), 1);
    }
    #[test]
    fn practice_tempo_changes_time_not_ticks() {
        let mut t = transport();
        t.set_practice_bpm(60.0);
        let mut s = session(vec![expected(Some(DrumComponent::Kick), 960)]);
        let result = s.record_hit(actual(Some(DrumComponent::Kick), 970), &t);
        assert_eq!(result.timing_difference_ticks, Some(10));
        assert_eq!(result.timing_difference_micros, Some(20_833));
        assert_eq!(result.expected.as_ref().unwrap().tick, 960);
    }
    #[test]
    fn tempo_changes_are_used_for_timing_difference() {
        let t = MusicalTransport::new(
            MidiTiming::TicksPerQuarter(480),
            &[
                TempoEvent {
                    tick: 0,
                    micros_per_quarter: 500_000,
                },
                TempoEvent {
                    tick: 480,
                    micros_per_quarter: 600_000,
                },
            ],
            &[],
            2_000,
        );
        let mut s = PracticeSession::new(
            vec![expected(Some(DrumComponent::Kick), 480)],
            TimingConfig {
                good_window: Duration::from_millis(50),
                match_window: Duration::from_millis(200),
            },
        );
        let result = s.record_hit(actual(Some(DrumComponent::Kick), 528), &t);
        assert_eq!(result.timing_difference_micros, Some(60_000));
        assert_eq!(result.timing_judgment, Some(TimingJudgment::Late));
    }
    #[test]
    fn nux_mapping_not_raw_note_equality_determines_component() {
        let t = transport();
        let mut mapping = DrumKitConfiguration::new("NUX DM-1");
        mapping.set(DrumMapping {
            component: DrumComponent::Kick,
            midi_note: 24,
            channel: 10,
        });
        let event = MidiEvent {
            message: MidiMessage {
                kind: MessageKind::NoteOn,
                channel: Some(9),
                data1: Some(24),
                data2: Some(100),
                system_exclusive: None,
            },
            timestamp_micros: 5,
        };
        let hit = ActualHit::from_midi_event(&event, &mapping, &t).unwrap();
        assert_eq!(hit.received_midi_note, 24);
        assert_eq!(hit.drum_component, Some(DrumComponent::Kick));
        let mut s = session(vec![expected(Some(DrumComponent::Kick), 0)]);
        assert_eq!(s.record_hit(hit, &t).kind, PracticeResultKind::CorrectHit);
    }
    #[test]
    fn unmapped_and_non_hit_midi_are_handled_safely() {
        let t = transport();
        let mapping = DrumKitConfiguration::new("NUX");
        let unmapped = MidiEvent {
            message: MidiMessage {
                kind: MessageKind::NoteOn,
                channel: Some(9),
                data1: Some(77),
                data2: Some(100),
                system_exclusive: None,
            },
            timestamp_micros: 1,
        };
        assert!(
            ActualHit::from_midi_event(&unmapped, &mapping, &t)
                .unwrap()
                .drum_component
                .is_none()
        );
        let off = MidiEvent {
            message: MidiMessage {
                kind: MessageKind::NoteOff,
                channel: Some(9),
                data1: Some(24),
                data2: Some(0),
                system_exclusive: None,
            },
            timestamp_micros: 2,
        };
        assert!(ActualHit::from_midi_event(&off, &mapping, &t).is_none());
    }
    #[test]
    fn callback_timestamp_removes_gui_queue_delay_from_actual_tick() {
        let mut t = transport();
        t.play();
        t.advance_by(Duration::from_secs(1));
        let mut mapping = DrumKitConfiguration::new("NUX");
        mapping.set(DrumMapping {
            component: DrumComponent::Kick,
            midi_note: 24,
            channel: 10,
        });
        let event = MidiEvent {
            message: MidiMessage {
                kind: MessageKind::NoteOn,
                channel: Some(9),
                data1: Some(24),
                data2: Some(100),
                system_exclusive: None,
            },
            timestamp_micros: 900_000,
        };
        let hit = ActualHit::from_midi_event_observed_at(&event, &mapping, &t, 1_000_000).unwrap();
        assert_eq!(hit.transport_tick, 864);
        assert!((hit.elapsed_seconds - 0.9).abs() < 0.0001);
    }
    #[test]
    fn unknown_pending_does_not_hide_later_known_event() {
        let s = session(vec![
            expected(None, 960),
            expected(Some(DrumComponent::Kick), 1200),
        ]);
        assert_eq!(s.next_pending(900).unwrap().1.drum_component, None);
        assert_eq!(
            s.next_known_pending(900).unwrap().1.drum_component,
            Some(DrumComponent::Kick)
        );
    }
    #[test]
    fn current_and_next_known_targets_are_independent_of_last_result() {
        let t = transport();
        let mut s = session(vec![
            expected(None, 800),
            expected(Some(DrumComponent::Kick), 960),
            expected(Some(DrumComponent::Snare), 1200),
        ]);
        s.record_hit(actual(Some(DrumComponent::Ride), 100), &t);
        let (index, current) = s.next_known_pending(100).unwrap();
        assert_eq!(current.drum_component, Some(DrumComponent::Kick));
        assert_eq!(
            s.next_known_after(index).unwrap().1.drum_component,
            Some(DrumComponent::Snare)
        );
        assert_eq!(
            s.results().last().unwrap().kind,
            PracticeResultKind::Ignored
        );
        assert!(s.results().last().unwrap().expected.is_none());
    }
    #[test]
    fn target_moves_from_upcoming_to_hittable_and_advances_after_hit() {
        let t = transport();
        let mut s = session(vec![
            expected(Some(DrumComponent::Kick), 960),
            expected(Some(DrumComponent::Snare), 1200),
        ]);
        let upcoming = s.current_target(700, &t).unwrap();
        assert_eq!(upcoming.state, TargetState::Upcoming);
        assert!(upcoming.time_until_seconds > 0.1);
        assert_eq!(
            s.current_target(900, &t).unwrap().state,
            TargetState::Hittable
        );
        s.record_hit(actual(Some(DrumComponent::Kick), 960), &t);
        let next = s.current_target(960, &t).unwrap();
        assert_eq!(next.notes[0].drum_component, Some(DrumComponent::Snare));
        assert_eq!(s.pending_count(), 1);
    }
    #[test]
    fn target_misses_and_advances_without_actual_input() {
        let t = transport();
        let mut s = session(vec![
            expected(Some(DrumComponent::Kick), 960),
            expected(Some(DrumComponent::Snare), 1200),
        ]);
        s.advance_to_tick(1057, &t);
        assert_eq!(s.results().last().unwrap().kind, PracticeResultKind::Missed);
        assert_eq!(
            s.current_target(1057, &t).unwrap().notes[0].drum_component,
            Some(DrumComponent::Snare)
        );
        assert_eq!(s.pending_count(), 1);
    }
    #[test]
    fn simultaneous_notes_form_one_target_group_and_one_hit_consumes_only_one() {
        let t = transport();
        let mut s = session(vec![
            expected(Some(DrumComponent::Kick), 960),
            expected(Some(DrumComponent::HiHat), 960),
            expected(Some(DrumComponent::Snare), 1200),
        ]);
        let target = s.current_target(900, &t).unwrap();
        assert_eq!(target.notes.len(), 2);
        assert_eq!(
            (
                target.ordinal_start,
                target.ordinal_end,
                target.total_matchable
            ),
            (1, 2, 3)
        );
        assert_eq!(
            s.next_target_after(1, 900, &t).unwrap().notes[0].drum_component,
            Some(DrumComponent::Snare)
        );
        s.record_hit(actual(Some(DrumComponent::Kick), 960), &t);
        let remaining = s.current_target(960, &t).unwrap();
        assert_eq!(remaining.notes.len(), 1);
        assert_eq!(
            remaining.notes[0].drum_component,
            Some(DrumComponent::HiHat)
        );
        assert_eq!(s.finalized_matchable_count(), 1);
    }
    #[test]
    fn seek_resynchronizes_immediately_to_destination() {
        let t = transport();
        let mut s = session(vec![
            expected(Some(DrumComponent::Kick), 480),
            expected(Some(DrumComponent::Snare), 1200),
        ]);
        s.on_seek_to_tick(1000, &t);
        assert_eq!(
            s.current_target(1000, &t).unwrap().notes[0].drum_component,
            Some(DrumComponent::Snare)
        );
        assert!(s.results().is_empty());
        assert_eq!(s.pending_count(), 1);
    }
    #[test]
    fn paused_transport_freezes_countdown_resume_and_bpm_change_update_it() {
        let mut t = transport();
        let s = session(vec![expected(Some(DrumComponent::Kick), 960)]);
        let before = s
            .current_target(t.current_tick(), &t)
            .unwrap()
            .time_until_seconds;
        t.pause();
        t.advance_by(Duration::from_secs(1));
        assert_eq!(
            s.current_target(t.current_tick(), &t)
                .unwrap()
                .time_until_seconds,
            before
        );
        t.play();
        t.advance_by(Duration::from_millis(250));
        assert!(
            s.current_target(t.current_tick(), &t)
                .unwrap()
                .time_until_seconds
                < before
        );
        t.stop();
        t.set_practice_bpm(60.0);
        assert_eq!(s.current_target(0, &t).unwrap().time_until_seconds, 2.0);
    }
    #[test]
    fn timing_judgment_default_boundaries_are_symmetric_and_inclusive() {
        let config = TimingConfig::default();
        for difference in [0, -10_000, 10_000, -50_000, 50_000] {
            assert_eq!(
                TimingJudgment::from_difference_micros(difference, config),
                Some(TimingJudgment::Good)
            );
        }
        assert_eq!(
            TimingJudgment::from_difference_micros(-50_001, config),
            Some(TimingJudgment::Early)
        );
        assert_eq!(
            TimingJudgment::from_difference_micros(50_001, config),
            Some(TimingJudgment::Late)
        );
        assert_eq!(
            TimingJudgment::from_difference_micros(-100_000, config),
            Some(TimingJudgment::Early)
        );
        assert_eq!(
            TimingJudgment::from_difference_micros(100_000, config),
            Some(TimingJudgment::Late)
        );
        assert_eq!(
            TimingJudgment::from_difference_micros(-100_001, config),
            None
        );
        assert_eq!(
            TimingJudgment::from_difference_micros(100_001, config),
            None
        );
    }
    #[test]
    fn correct_hits_receive_early_good_and_late_judgments() {
        let t = transport();
        for (actual_tick, judgment) in [
            (900, TimingJudgment::Early),
            (970, TimingJudgment::Good),
            (1020, TimingJudgment::Late),
        ] {
            let mut s = session(vec![expected(Some(DrumComponent::Kick), 960)]);
            let result = s.record_hit(actual(Some(DrumComponent::Kick), actual_tick), &t);
            assert_eq!(result.kind, PracticeResultKind::CorrectHit);
            assert_eq!(result.timing_judgment, Some(judgment));
        }
    }
    #[test]
    fn wrong_missed_and_ignored_never_receive_timing_judgment() {
        let t = transport();
        let mut wrong = session(vec![expected(Some(DrumComponent::Kick), 960)]);
        assert_eq!(
            wrong
                .record_hit(actual(Some(DrumComponent::Snare), 970), &t)
                .timing_judgment,
            None
        );
        let mut missed = session(vec![expected(Some(DrumComponent::Kick), 960)]);
        missed.advance_to_tick(1057, &t);
        assert_eq!(missed.results()[0].timing_judgment, None);
        let mut ignored = session(Vec::new());
        assert_eq!(
            ignored
                .record_hit(actual(Some(DrumComponent::Ride), 960), &t)
                .timing_judgment,
            None
        );
    }
    #[test]
    fn simultaneous_and_rapid_notes_keep_independent_judgments() {
        let t = transport();
        let mut simultaneous = session(vec![
            expected(Some(DrumComponent::Kick), 960),
            expected(Some(DrumComponent::HiHat), 960),
        ]);
        simultaneous.record_hit(actual(Some(DrumComponent::Kick), 900), &t);
        simultaneous.record_hit(actual(Some(DrumComponent::HiHat), 970), &t);
        assert_eq!(
            simultaneous
                .results()
                .iter()
                .map(|result| result.timing_judgment)
                .collect::<Vec<_>>(),
            vec![Some(TimingJudgment::Early), Some(TimingJudgment::Good)]
        );
        let mut rapid = session(vec![
            expected(Some(DrumComponent::Kick), 960),
            expected(Some(DrumComponent::Snare), 1080),
            expected(Some(DrumComponent::Kick), 1200),
        ]);
        rapid.record_hit(actual(Some(DrumComponent::Kick), 970), &t);
        rapid.record_hit(actual(Some(DrumComponent::Snare), 1140), &t);
        rapid.record_hit(actual(Some(DrumComponent::Kick), 1140), &t);
        assert_eq!(
            rapid
                .results()
                .iter()
                .map(|result| result.timing_judgment)
                .collect::<Vec<_>>(),
            vec![
                Some(TimingJudgment::Good),
                Some(TimingJudgment::Late),
                Some(TimingJudgment::Early)
            ]
        );
    }
}
