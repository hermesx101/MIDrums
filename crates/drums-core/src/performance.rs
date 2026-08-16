//! Derived performance statistics for one practice attempt.

use std::collections::BTreeMap;

use crate::mapping::DrumComponent;
use crate::practice::{PracticeResultKind, PracticeSession, TimingJudgment};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PerformanceConfig {
    pub on_time_dead_zone_ms: f64,
}

impl Default for PerformanceConfig {
    fn default() -> Self {
        Self {
            on_time_dead_zone_ms: 10.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TimingTendency {
    Early(f64),
    OnTime,
    Late(f64),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ComponentPerformance {
    pub total_expected: usize,
    pub finalized: usize,
    pub correct_hits: usize,
    pub missed: usize,
    pub wrong_attempts: usize,
    pub mean_absolute_error_ms: Option<f64>,
    timing_absolute_sum_ms: f64,
    timed_hits: usize,
}

impl ComponentPerformance {
    /// During practice the denominator contains finalized targets only. At song
    /// completion this is equivalent to correct / total expected.
    pub fn accuracy_percent(&self) -> f64 {
        percentage(self.correct_hits, self.finalized)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PracticePerformance {
    /// Known, independently evaluable expected drum events. Unknown notes are
    /// preserved by PracticeSession but excluded from performance accuracy.
    pub total_expected: usize,
    pub finalized_expected: usize,
    pub correct_hits: usize,
    pub missed: usize,
    pub wrong_drums: usize,
    pub ignored_extra_hits: usize,
    pub good_hits: usize,
    pub early_hits: usize,
    pub late_hits: usize,
    pub mean_absolute_error_ms: Option<f64>,
    pub mean_signed_error_ms: Option<f64>,
    pub tendency: TimingTendency,
    pub per_component: BTreeMap<DrumComponent, ComponentPerformance>,
}

impl PracticePerformance {
    pub fn from_session(session: &PracticeSession) -> Self {
        Self::from_session_with_config(session, PerformanceConfig::default())
    }

    pub fn from_session_with_config(session: &PracticeSession, config: PerformanceConfig) -> Self {
        let mut per_component = BTreeMap::new();
        for expected in session.expected() {
            if let Some(component) = expected.drum_component {
                per_component
                    .entry(component)
                    .or_insert_with(ComponentPerformance::default)
                    .total_expected += 1;
            }
        }

        let mut correct_hits = 0;
        let mut missed = 0;
        let mut wrong_drums = 0;
        let mut ignored_extra_hits = 0;
        let mut good_hits = 0;
        let mut early_hits = 0;
        let mut late_hits = 0;
        let mut signed_sum_ms = 0.0;
        let mut absolute_sum_ms = 0.0;
        let mut timed_hits = 0;

        for result in session.results() {
            match result.kind {
                PracticeResultKind::CorrectHit => {
                    correct_hits += 1;
                    let component = result
                        .expected
                        .as_ref()
                        .and_then(|note| note.drum_component);
                    if let Some(component) = component {
                        let stats = per_component.entry(component).or_default();
                        stats.correct_hits += 1;
                        stats.finalized += 1;
                    }
                    if let Some(judgment) = result.timing_judgment {
                        match judgment {
                            TimingJudgment::Good => good_hits += 1,
                            TimingJudgment::Early => early_hits += 1,
                            TimingJudgment::Late => late_hits += 1,
                        }
                    }
                    if let Some(difference) = result.timing_difference_micros {
                        timed_hits += 1;
                        let milliseconds = difference as f64 / 1_000.0;
                        signed_sum_ms += milliseconds;
                        absolute_sum_ms += milliseconds.abs();
                        if let Some(component) = component {
                            let stats = per_component.entry(component).or_default();
                            stats.timing_absolute_sum_ms += milliseconds.abs();
                            stats.timed_hits += 1;
                        }
                    }
                }
                PracticeResultKind::Missed => {
                    if let Some(component) = result
                        .expected
                        .as_ref()
                        .and_then(|note| note.drum_component)
                    {
                        missed += 1;
                        let stats = per_component.entry(component).or_default();
                        stats.missed += 1;
                        stats.finalized += 1;
                    }
                }
                PracticeResultKind::WrongDrum => {
                    wrong_drums += 1;
                    if let Some(component) = result
                        .expected
                        .as_ref()
                        .and_then(|note| note.drum_component)
                    {
                        let stats = per_component.entry(component).or_default();
                        stats.wrong_attempts += 1;
                        stats.finalized += 1;
                    }
                }
                PracticeResultKind::Ignored => {
                    if result.actual.is_some() {
                        ignored_extra_hits += 1;
                    }
                }
            }
        }

        for stats in per_component.values_mut() {
            // A loop iteration creates a new evaluable encounter without
            // duplicating the imported ExpectedNote. Reflect those repeated
            // finalized outcomes in session totals.
            stats.total_expected = stats.total_expected.max(stats.finalized);
            stats.mean_absolute_error_ms = (stats.timed_hits > 0)
                .then(|| stats.timing_absolute_sum_ms / stats.timed_hits as f64);
        }
        // Every finalized expected target has exactly one primary outcome.
        // Extra/ignored actual inputs are deliberately outside this denominator.
        let finalized_expected = correct_hits + wrong_drums + missed;
        let mean_absolute_error_ms = (timed_hits > 0).then(|| absolute_sum_ms / timed_hits as f64);
        let mean_signed_error_ms = (timed_hits > 0).then(|| signed_sum_ms / timed_hits as f64);
        let tendency = match mean_signed_error_ms {
            Some(value) if value < -config.on_time_dead_zone_ms => {
                TimingTendency::Early(value.abs())
            }
            Some(value) if value > config.on_time_dead_zone_ms => TimingTendency::Late(value),
            _ => TimingTendency::OnTime,
        };
        Self {
            total_expected: per_component
                .values()
                .map(|stats| stats.total_expected)
                .sum(),
            finalized_expected,
            correct_hits,
            missed,
            wrong_drums,
            ignored_extra_hits,
            good_hits,
            early_hits,
            late_hits,
            mean_absolute_error_ms,
            mean_signed_error_ms,
            tendency,
            per_component,
        }
    }

    /// Correct finalized targets / all finalized targets. Active/upcoming
    /// targets are excluded; at completion this is correct / total expected.
    pub fn accuracy_percent(&self) -> f64 {
        percentage(self.correct_hits, self.finalized_expected)
    }
    pub fn good_percent(&self) -> f64 {
        percentage(self.good_hits, self.correct_hits)
    }
}

fn percentage(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 * 100.0 / denominator as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ActualHit, ExpectedNote, MatchingConfig, MidiTiming, MusicalTransport, PracticeResultKind,
        PracticeSession,
    };

    fn transport() -> MusicalTransport {
        MusicalTransport::new(
            MidiTiming::Timecode {
                frames_per_second: 100,
                ticks_per_frame: 10,
            },
            &[],
            &[],
            20_000,
        )
    }
    fn note(component: DrumComponent, tick: u64) -> ExpectedNote {
        ExpectedNote {
            tick,
            timestamp_seconds: tick as f64 / 1_000.0,
            midi_note: 36,
            velocity: 90,
            channel: 9,
            track: 0,
            drum_component: Some(component),
            target_component: Some(component),
        }
    }
    fn hit(component: DrumComponent, tick: u64) -> ActualHit {
        ActualHit {
            received_midi_note: 24,
            drum_component: Some(component),
            velocity: 100,
            channel: 9,
            input_timestamp_micros: tick * 1_000,
            elapsed_seconds: tick as f64 / 1_000.0,
            transport_tick: tick,
        }
    }
    fn spaced(count: usize, component: DrumComponent) -> Vec<ExpectedNote> {
        (0..count)
            .map(|index| note(component, 1_000 + index as u64 * 500))
            .collect()
    }

    fn assert_completed_invariants(stats: &PracticePerformance) {
        assert_eq!(
            stats.correct_hits + stats.wrong_drums + stats.missed,
            stats.total_expected
        );
        for component in stats.per_component.values() {
            assert_eq!(
                component.correct_hits + component.wrong_attempts + component.missed,
                component.total_expected
            );
        }
    }

    #[test]
    fn ten_of_ten_and_nine_of_ten_accuracy() {
        let t = transport();
        let mut perfect =
            PracticeSession::new(spaced(10, DrumComponent::Kick), MatchingConfig::default());
        for index in 0..10 {
            perfect.record_hit(hit(DrumComponent::Kick, 1_000 + index * 500), &t);
        }
        assert_eq!(
            PracticePerformance::from_session(&perfect).accuracy_percent(),
            100.0
        );
        let mut nine =
            PracticeSession::new(spaced(10, DrumComponent::Kick), MatchingConfig::default());
        for index in 0..9 {
            nine.record_hit(hit(DrumComponent::Kick, 1_000 + index * 500), &t);
        }
        nine.finish();
        let stats = PracticePerformance::from_session(&nine);
        assert_eq!(stats.accuracy_percent(), 90.0);
        assert_eq!((stats.correct_hits, stats.missed), (9, 1));
    }
    #[test]
    fn wrong_drum_is_one_final_outcome_and_is_not_later_missed() {
        let t = transport();
        let mut session = PracticeSession::new(
            vec![note(DrumComponent::Kick, 1_000)],
            MatchingConfig::default(),
        );
        assert_eq!(
            session
                .record_hit(hit(DrumComponent::Snare, 1_000), &t)
                .kind,
            PracticeResultKind::WrongDrum
        );
        session.record_hit(hit(DrumComponent::Ride, 2_000), &t);
        session.finish();
        let stats = PracticePerformance::from_session(&session);
        assert_eq!(
            (
                stats.correct_hits,
                stats.missed,
                stats.wrong_drums,
                stats.ignored_extra_hits
            ),
            (0, 0, 1, 1)
        );
        assert_eq!(stats.finalized_expected, 1);
        assert_completed_invariants(&stats);
        assert_eq!(stats.accuracy_percent(), 0.0);
    }

    #[test]
    fn no_input_produces_one_missed_outcome() {
        let mut session = PracticeSession::new(
            vec![note(DrumComponent::Kick, 1_000)],
            MatchingConfig::default(),
        );
        session.finish();
        let stats = PracticePerformance::from_session(&session);
        assert_eq!(
            (stats.correct_hits, stats.wrong_drums, stats.missed),
            (0, 0, 1)
        );
        assert_completed_invariants(&stats);
    }

    #[test]
    fn unrelated_hit_outside_window_is_extra_and_target_is_missed() {
        let t = transport();
        let mut session = PracticeSession::new(
            vec![note(DrumComponent::Kick, 1_000)],
            MatchingConfig::default(),
        );
        assert_eq!(
            session.record_hit(hit(DrumComponent::Ride, 2_000), &t).kind,
            PracticeResultKind::Ignored
        );
        session.finish();
        let stats = PracticePerformance::from_session(&session);
        assert_eq!(
            (
                stats.correct_hits,
                stats.wrong_drums,
                stats.missed,
                stats.ignored_extra_hits
            ),
            (0, 0, 1, 1)
        );
        assert_completed_invariants(&stats);
    }
    #[test]
    fn timing_breakdown_mae_and_signed_average_are_correct() {
        let t = transport();
        let mut session = PracticeSession::new(
            vec![
                note(DrumComponent::Kick, 1_000),
                note(DrumComponent::Snare, 2_000),
                note(DrumComponent::HiHat, 3_000),
            ],
            MatchingConfig::default(),
        );
        session.record_hit(hit(DrumComponent::Kick, 940), &t);
        session.record_hit(hit(DrumComponent::Snare, 2_010), &t);
        session.record_hit(hit(DrumComponent::HiHat, 3_070), &t);
        let stats = PracticePerformance::from_session(&session);
        assert_eq!(
            (stats.early_hits, stats.good_hits, stats.late_hits),
            (1, 1, 1)
        );
        assert!((stats.mean_absolute_error_ms.unwrap() - 46.6667).abs() < 0.001);
        assert!((stats.mean_signed_error_ms.unwrap() - 6.6667).abs() < 0.001);
        assert_eq!(stats.tendency, TimingTendency::OnTime);
    }
    #[test]
    fn early_late_and_on_time_tendencies_use_dead_zone() {
        let t = transport();
        for (difference, expected_tendency) in [
            (-20i64, TimingTendency::Early(20.0)),
            (20, TimingTendency::Late(20.0)),
            (10, TimingTendency::OnTime),
        ] {
            let mut session = PracticeSession::new(
                vec![note(DrumComponent::Kick, 1_000)],
                MatchingConfig::default(),
            );
            session.record_hit(hit(DrumComponent::Kick, (1_000i64 + difference) as u64), &t);
            assert_eq!(
                PracticePerformance::from_session(&session).tendency,
                expected_tendency
            );
        }
    }
    #[test]
    fn per_drum_accuracy_and_timing_are_independent() {
        let t = transport();
        let mut session = PracticeSession::new(
            vec![
                note(DrumComponent::Kick, 1_000),
                note(DrumComponent::Kick, 2_000),
                note(DrumComponent::Snare, 3_000),
            ],
            MatchingConfig::default(),
        );
        session.record_hit(hit(DrumComponent::Kick, 1_020), &t);
        session.record_hit(hit(DrumComponent::Snare, 3_040), &t);
        session.finish();
        let stats = PracticePerformance::from_session(&session);
        let kick = &stats.per_component[&DrumComponent::Kick];
        let snare = &stats.per_component[&DrumComponent::Snare];
        assert_eq!(
            (kick.correct_hits, kick.missed, kick.accuracy_percent()),
            (1, 1, 50.0)
        );
        assert_eq!(
            (snare.correct_hits, snare.missed, snare.accuracy_percent()),
            (1, 0, 100.0)
        );
        assert_eq!(kick.mean_absolute_error_ms, Some(20.0));
        assert_eq!(snare.mean_absolute_error_ms, Some(40.0));
    }
    #[test]
    fn simultaneous_events_count_independently() {
        let t = transport();
        let mut both = PracticeSession::new(
            vec![
                note(DrumComponent::Kick, 1_000),
                note(DrumComponent::HiHat, 1_000),
            ],
            MatchingConfig::default(),
        );
        both.record_hit(hit(DrumComponent::Kick, 1_010), &t);
        both.record_hit(hit(DrumComponent::HiHat, 1_020), &t);
        assert_eq!(PracticePerformance::from_session(&both).correct_hits, 2);
        let mut one = PracticeSession::new(
            vec![
                note(DrumComponent::Kick, 1_000),
                note(DrumComponent::HiHat, 1_000),
            ],
            MatchingConfig::default(),
        );
        one.record_hit(hit(DrumComponent::Kick, 1_010), &t);
        one.finish();
        let stats = PracticePerformance::from_session(&one);
        assert_eq!((stats.correct_hits, stats.missed), (1, 1));
    }

    #[test]
    fn simultaneous_wrong_then_correct_resolves_only_one_target_per_input() {
        let t = transport();
        let mut session = PracticeSession::new(
            vec![
                note(DrumComponent::Kick, 1_000),
                note(DrumComponent::HiHat, 1_000),
            ],
            MatchingConfig::default(),
        );
        session.record_hit(hit(DrumComponent::Snare, 1_000), &t);
        session.record_hit(hit(DrumComponent::Kick, 1_010), &t);
        session.finish();
        let stats = PracticePerformance::from_session(&session);
        assert_eq!(
            (stats.correct_hits, stats.wrong_drums, stats.missed),
            (1, 1, 0)
        );
        assert_eq!(stats.per_component[&DrumComponent::Kick].correct_hits, 1);
        assert_eq!(stats.per_component[&DrumComponent::HiHat].wrong_attempts, 1);
        assert_completed_invariants(&stats);
    }
    #[test]
    fn reset_seek_pause_bpm_and_summary_snapshots_are_stable() {
        let mut t = transport();
        let mut session = PracticeSession::new(
            vec![note(DrumComponent::Kick, 1_000)],
            MatchingConfig::default(),
        );
        session.record_hit(hit(DrumComponent::Kick, 1_010), &t);
        let before = PracticePerformance::from_session(&session);
        t.pause();
        t.advance_by(std::time::Duration::from_secs(2));
        assert_eq!(PracticePerformance::from_session(&session), before);
        t.set_practice_bpm(60.0);
        assert_eq!(PracticePerformance::from_session(&session), before);
        session.finish();
        let final_one = PracticePerformance::from_session(&session);
        let final_two = PracticePerformance::from_session(&session);
        assert_eq!(final_one, final_two);
        session.on_seek_to_tick(0, &t);
        let reset = PracticePerformance::from_session(&session);
        assert_eq!((reset.correct_hits, reset.finalized_expected), (0, 0));
    }

    #[test]
    fn repeated_loop_encounters_expand_performance_denominator() {
        let t = transport();
        let mut session = PracticeSession::new(
            vec![note(DrumComponent::Kick, 1_000)],
            MatchingConfig::default(),
        );
        session.record_hit(hit(DrumComponent::Kick, 1_000), &t);
        session.on_loop_wrap(900, 1_100);
        session.record_hit(hit(DrumComponent::Kick, 1_000), &t);
        let stats = PracticePerformance::from_session(&session);
        assert_eq!(
            (
                stats.correct_hits,
                stats.finalized_expected,
                stats.total_expected
            ),
            (2, 2, 2)
        );
        assert_eq!(stats.per_component[&DrumComponent::Kick].total_expected, 2);
        assert_eq!(stats.accuracy_percent(), 100.0);
    }
}
