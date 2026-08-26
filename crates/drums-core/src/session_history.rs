//! Persistent, analysis-ready records for completed practice sessions.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::{
    DrumComponent, MusicalTransport, PracticePerformance, PracticeResultKind, PracticeSession,
    Subdivision, TimingJudgment,
};

/// Durable representation of one finalized expected/actual outcome.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PracticeHitRecord {
    pub sequence: usize,
    pub input_timestamp_micros: Option<u64>,
    pub expected_drum: Option<DrumComponent>,
    pub actual_drum: Option<DrumComponent>,
    pub expected_time_seconds: Option<f64>,
    pub actual_time_seconds: Option<f64>,
    /// Actual minus expected: negative is early and positive is late.
    pub timing_offset_ms: Option<f64>,
    /// Raw MIDI Note On velocity. Missing events (for example, misses) use None.
    pub velocity: Option<u8>,
    pub result_kind: PracticeResultKind,
    pub timing_judgment: Option<TimingJudgment>,
}

/// Aggregate statistics for one logical drum within a completed session.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DrumSessionStatistics {
    pub finalized_count: usize,
    pub correct_count: usize,
    pub missed_count: usize,
    pub wrong_drum_count: usize,
    pub extra_count: usize,
    pub accuracy: f64,
    pub average_timing_offset_ms: Option<f64>,
    pub timing_spread_ms: Option<f64>,
    pub consistency: Option<f64>,
    pub average_velocity: Option<f64>,
    pub velocity_spread: Option<f64>,
    pub minimum_velocity: Option<u8>,
    pub maximum_velocity: Option<u8>,
}

/// Authoritative aggregate snapshot stored with a completed session.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SessionStatistics {
    pub accuracy: f64,
    pub consistency: Option<f64>,
    pub correct_count: usize,
    pub early_count: usize,
    pub late_count: usize,
    pub missed_count: usize,
    pub wrong_drum_count: usize,
    pub extra_count: usize,
    pub average_timing_offset_ms: Option<f64>,
    pub average_early_offset_ms: Option<f64>,
    pub average_late_offset_ms: Option<f64>,
    pub timing_spread_ms: Option<f64>,
    pub average_velocity: Option<f64>,
    pub velocity_spread: Option<f64>,
    pub minimum_velocity: Option<u8>,
    pub maximum_velocity: Option<u8>,
    pub per_drum: BTreeMap<DrumComponent, DrumSessionStatistics>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SessionBuildMetadata {
    pub id: String,
    pub track_id: String,
    pub started_at_unix_ms: u64,
    pub completed_at_unix_ms: u64,
    pub practice_bpm: f64,
    pub time_signature: String,
    pub subdivision: Subdivision,
}

/// Append-only persisted snapshot of one completed practice attempt.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CompletedPracticeSession {
    pub id: String,
    pub track_id: String,
    pub started_at_unix_ms: u64,
    pub completed_at_unix_ms: u64,
    pub duration_seconds: f64,
    pub practice_bpm: f64,
    pub time_signature: String,
    pub subdivision: Subdivision,
    pub statistics: SessionStatistics,
    pub hits: Vec<PracticeHitRecord>,
}

impl CompletedPracticeSession {
    /// Creates the single authoritative persisted result from practice-engine output.
    pub fn from_practice_session(
        session: &PracticeSession,
        transport: &MusicalTransport,
        metadata: SessionBuildMetadata,
    ) -> Self {
        let hits = session
            .results()
            .iter()
            .enumerate()
            .map(|(sequence, result)| {
                let expected_time_seconds = result
                    .expected
                    .as_ref()
                    .map(|note| transport.effective_seconds_at_tick(note.tick));
                PracticeHitRecord {
                    sequence,
                    input_timestamp_micros: result
                        .actual
                        .as_ref()
                        .map(|hit| hit.input_timestamp_micros),
                    expected_drum: result
                        .expected
                        .as_ref()
                        .and_then(|note| note.drum_component),
                    actual_drum: result.actual.as_ref().and_then(|hit| hit.drum_component),
                    expected_time_seconds,
                    actual_time_seconds: result.actual.as_ref().map(|hit| hit.elapsed_seconds),
                    timing_offset_ms: result
                        .timing_difference_micros
                        .map(|value| value as f64 / 1_000.0),
                    velocity: result.actual.as_ref().map(|hit| hit.velocity),
                    result_kind: result.kind,
                    timing_judgment: result.timing_judgment,
                }
            })
            .collect::<Vec<_>>();
        let performance = PracticePerformance::from_session(session);
        let statistics = SessionStatistics::calculate(&hits, &performance);
        Self {
            id: metadata.id,
            track_id: metadata.track_id,
            started_at_unix_ms: metadata.started_at_unix_ms,
            completed_at_unix_ms: metadata.completed_at_unix_ms,
            duration_seconds: metadata
                .completed_at_unix_ms
                .saturating_sub(metadata.started_at_unix_ms) as f64
                / 1_000.0,
            practice_bpm: metadata.practice_bpm,
            time_signature: metadata.time_signature,
            subdivision: metadata.subdivision,
            statistics,
            hits,
        }
    }
}

impl SessionStatistics {
    fn calculate(hits: &[PracticeHitRecord], performance: &PracticePerformance) -> Self {
        let timing = hits
            .iter()
            .filter_map(|hit| hit.timing_offset_ms)
            .collect::<Vec<_>>();
        let early = timing
            .iter()
            .copied()
            .filter(|value| *value < 0.0)
            .collect::<Vec<_>>();
        let late = timing
            .iter()
            .copied()
            .filter(|value| *value > 0.0)
            .collect::<Vec<_>>();
        let velocities = hits
            .iter()
            .filter_map(|hit| hit.velocity)
            .collect::<Vec<_>>();
        let timing_spread_ms = spread(&timing);
        let mut components = BTreeMap::<DrumComponent, Vec<&PracticeHitRecord>>::new();
        for hit in hits {
            if let Some(component) = hit.expected_drum.or(hit.actual_drum) {
                components.entry(component).or_default().push(hit);
            }
        }
        let per_drum = components
            .into_iter()
            .map(|(component, hits)| (component, drum_statistics(&hits)))
            .collect();
        Self {
            accuracy: performance.accuracy_percent(),
            consistency: timing_spread_ms.map(consistency_from_spread),
            correct_count: performance.correct_hits,
            early_count: performance.early_hits,
            late_count: performance.late_hits,
            missed_count: performance.missed,
            wrong_drum_count: performance.wrong_drums,
            extra_count: performance.ignored_extra_hits,
            average_timing_offset_ms: mean(&timing),
            average_early_offset_ms: mean(&early),
            average_late_offset_ms: mean(&late),
            timing_spread_ms,
            average_velocity: mean_u8(&velocities),
            velocity_spread: spread_u8(&velocities),
            minimum_velocity: velocities.iter().min().copied(),
            maximum_velocity: velocities.iter().max().copied(),
            per_drum,
        }
    }
}

fn drum_statistics(hits: &[&PracticeHitRecord]) -> DrumSessionStatistics {
    let correct_count = hits
        .iter()
        .filter(|hit| hit.result_kind == PracticeResultKind::CorrectHit)
        .count();
    let missed_count = hits
        .iter()
        .filter(|hit| hit.result_kind == PracticeResultKind::Missed)
        .count();
    let wrong_drum_count = hits
        .iter()
        .filter(|hit| hit.result_kind == PracticeResultKind::WrongDrum)
        .count();
    let extra_count = hits
        .iter()
        .filter(|hit| hit.result_kind == PracticeResultKind::Ignored && hit.actual_drum.is_some())
        .count();
    let finalized_count = correct_count + missed_count + wrong_drum_count;
    let timing = hits
        .iter()
        .filter_map(|hit| hit.timing_offset_ms)
        .collect::<Vec<_>>();
    let velocities = hits
        .iter()
        .filter_map(|hit| hit.velocity)
        .collect::<Vec<_>>();
    let timing_spread_ms = spread(&timing);
    DrumSessionStatistics {
        finalized_count,
        correct_count,
        missed_count,
        wrong_drum_count,
        extra_count,
        accuracy: percentage(correct_count, finalized_count),
        average_timing_offset_ms: mean(&timing),
        timing_spread_ms,
        consistency: timing_spread_ms.map(consistency_from_spread),
        average_velocity: mean_u8(&velocities),
        velocity_spread: spread_u8(&velocities),
        minimum_velocity: velocities.iter().min().copied(),
        maximum_velocity: velocities.iter().max().copied(),
    }
}

fn mean(values: &[f64]) -> Option<f64> {
    (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
}

fn mean_u8(values: &[u8]) -> Option<f64> {
    (!values.is_empty())
        .then(|| values.iter().map(|value| *value as f64).sum::<f64>() / values.len() as f64)
}

fn spread(values: &[f64]) -> Option<f64> {
    let average = mean(values)?;
    Some(
        (values
            .iter()
            .map(|value| (value - average).powi(2))
            .sum::<f64>()
            / values.len() as f64)
            .sqrt(),
    )
}

fn spread_u8(values: &[u8]) -> Option<f64> {
    spread(&values.iter().map(|value| *value as f64).collect::<Vec<_>>())
}

/// 50 ms standard deviation maps to 50%; zero variation maps to 100%.
fn consistency_from_spread(spread_ms: f64) -> f64 {
    100.0 / (1.0 + spread_ms / 50.0)
}

fn percentage(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 * 100.0 / denominator as f64
    }
}

static SESSION_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub fn new_session_id(track_id: &str, timestamp_ms: u64) -> String {
    let sequence = SESSION_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in track_id.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{timestamp_ms}-{hash:016x}-{sequence:08x}")
}

#[derive(Default, Serialize, Deserialize)]
struct SessionFile {
    sessions: Vec<CompletedPracticeSession>,
}

/// TOML-backed local store for completed practice-session history.
///
/// Appends preserve earlier sessions. Progress and Performance Analysis query
/// this store rather than maintaining independent result histories.
pub struct PracticeSessionStore {
    path: PathBuf,
}

impl PracticeSessionStore {
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }

    pub fn default_path() -> PathBuf {
        if let Ok(path) = std::env::var("DRUMS_HISTORY_PATH") {
            return PathBuf::from(path);
        }
        #[cfg(target_os = "windows")]
        if let Ok(appdata) = std::env::var("APPDATA") {
            return PathBuf::from(appdata)
                .join("drums")
                .join("practice-sessions.toml");
        }
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home)
                .join(".config")
                .join("drums")
                .join("practice-sessions.toml");
        }
        PathBuf::from("practice-sessions.toml")
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn append(&self, session: CompletedPracticeSession) -> io::Result<()> {
        let mut file = self.load_file()?;
        if file
            .sessions
            .iter()
            .any(|existing| existing.id == session.id)
        {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "practice session id already exists",
            ));
        }
        file.sessions.push(session);
        self.write_file(&file)
    }

    pub fn load_all(&self) -> io::Result<Vec<CompletedPracticeSession>> {
        Ok(self.load_file()?.sessions)
    }

    pub fn load_for_track(&self, track_id: &str) -> io::Result<Vec<CompletedPracticeSession>> {
        let mut sessions = self
            .load_all()?
            .into_iter()
            .filter(|session| session.track_id == track_id)
            .collect::<Vec<_>>();
        sessions.sort_by_key(|session| std::cmp::Reverse(session.completed_at_unix_ms));
        Ok(sessions)
    }

    /// Returns the newest session for a track from the authoritative history file.
    pub fn latest_for_track(&self, track_id: &str) -> io::Result<Option<CompletedPracticeSession>> {
        Ok(self.load_for_track(track_id)?.into_iter().next())
    }

    /// Returns the highest-accuracy session (newest wins a tie).
    pub fn best_for_track(&self, track_id: &str) -> io::Result<Option<CompletedPracticeSession>> {
        Ok(self.load_for_track(track_id)?.into_iter().max_by(|a, b| {
            a.statistics
                .accuracy
                .total_cmp(&b.statistics.accuracy)
                .then_with(|| a.completed_at_unix_ms.cmp(&b.completed_at_unix_ms))
        }))
    }

    fn load_file(&self) -> io::Result<SessionFile> {
        if !self.path.exists() {
            return Ok(SessionFile::default());
        }
        let text = fs::read_to_string(&self.path)?;
        toml::from_str(&text).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }

    fn write_file(&self, file: &SessionFile) -> io::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = toml::to_string_pretty(file)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        fs::write(&self.path, text)
    }
}

impl Default for PracticeSessionStore {
    fn default() -> Self {
        Self::new(Self::default_path())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ActualHit, ExpectedNote, MatchingConfig, MidiTiming};

    fn transport() -> MusicalTransport {
        MusicalTransport::new(
            MidiTiming::Timecode {
                frames_per_second: 100,
                ticks_per_frame: 10,
            },
            &[],
            &[],
            4_000,
        )
    }

    fn expected(component: DrumComponent, tick: u64) -> ExpectedNote {
        ExpectedNote {
            tick,
            timestamp_seconds: tick as f64 / 1_000.0,
            midi_note: 36,
            velocity: 80,
            channel: 9,
            track: 0,
            drum_component: Some(component),
            target_component: Some(component),
        }
    }

    fn actual(component: DrumComponent, tick: u64, velocity: u8) -> ActualHit {
        ActualHit {
            received_midi_note: 36,
            drum_component: Some(component),
            velocity,
            channel: 9,
            input_timestamp_micros: tick * 1_000,
            elapsed_seconds: tick as f64 / 1_000.0,
            transport_tick: tick,
        }
    }

    fn completed() -> CompletedPracticeSession {
        let transport = transport();
        let mut live = PracticeSession::new(
            vec![
                expected(DrumComponent::Kick, 1_000),
                expected(DrumComponent::Snare, 2_000),
                expected(DrumComponent::HiHat, 3_000),
            ],
            MatchingConfig::default(),
        );
        live.record_hit(actual(DrumComponent::Kick, 940, 60), &transport);
        live.record_hit(actual(DrumComponent::Snare, 2_070, 100), &transport);
        live.finish();
        CompletedPracticeSession::from_practice_session(
            &live,
            &transport,
            SessionBuildMetadata {
                id: "session-1".into(),
                track_id: "lesson.mid".into(),
                started_at_unix_ms: 1_000,
                completed_at_unix_ms: 5_000,
                practice_bpm: 95.0,
                time_signature: "4/4".into(),
                subdivision: Subdivision::Eighth,
            },
        )
    }

    #[test]
    fn raw_timing_velocity_and_authoritative_counts_are_preserved() {
        let session = completed();
        assert_eq!(session.duration_seconds, 4.0);
        assert_eq!(session.hits.len(), 3);
        assert_eq!(session.hits[0].timing_offset_ms, Some(-60.0));
        assert_eq!(session.hits[1].timing_offset_ms, Some(70.0));
        assert_eq!(session.hits[2].velocity, None);
        assert_eq!(session.statistics.correct_count, 2);
        assert_eq!(session.statistics.early_count, 1);
        assert_eq!(session.statistics.late_count, 1);
        assert_eq!(session.statistics.missed_count, 1);
        assert!((session.statistics.accuracy - 66.666_666).abs() < 0.001);
        assert_eq!(session.statistics.average_timing_offset_ms, Some(5.0));
        assert_eq!(session.statistics.timing_spread_ms, Some(65.0));
        assert_eq!(session.statistics.average_velocity, Some(80.0));
        assert_eq!(session.statistics.minimum_velocity, Some(60));
        assert_eq!(session.statistics.maximum_velocity, Some(100));
    }

    #[test]
    fn consistency_is_distinct_from_accuracy() {
        assert_eq!(consistency_from_spread(0.0), 100.0);
        assert_eq!(consistency_from_spread(50.0), 50.0);
        assert!(consistency_from_spread(10.0) > consistency_from_spread(40.0));
    }

    #[test]
    fn store_appends_round_trips_and_filters_without_overwriting() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("drums_history_{}_{}", std::process::id(), unique));
        let path = dir.join("sessions.toml");
        let store = PracticeSessionStore::new(&path);
        let first = completed();
        let mut second = first.clone();
        second.id = "session-2".into();
        second.track_id = "other.mid".into();
        store.append(first.clone()).unwrap();
        store.append(second.clone()).unwrap();
        let reopened = PracticeSessionStore::new(&path);
        assert_eq!(reopened.load_all().unwrap(), vec![first.clone(), second]);
        assert_eq!(
            reopened.load_for_track("lesson.mid").unwrap(),
            vec![first.clone()]
        );
        assert_eq!(
            store.append(first).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn latest_and_best_are_independent_history_queries() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "drums_history_summary_{}_{}",
            std::process::id(),
            unique
        ));
        let store = PracticeSessionStore::new(dir.join("sessions.toml"));
        let mut best = completed();
        best.id = "best".into();
        best.completed_at_unix_ms = 10;
        best.statistics.accuracy = 95.0;
        let mut latest = completed();
        latest.id = "latest".into();
        latest.completed_at_unix_ms = 20;
        latest.statistics.accuracy = 90.0;
        store.append(best.clone()).unwrap();
        store.append(latest.clone()).unwrap();
        assert_eq!(
            store.latest_for_track("lesson.mid").unwrap().unwrap().id,
            "latest"
        );
        assert_eq!(
            store.best_for_track("lesson.mid").unwrap().unwrap().id,
            "best"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn corrupt_history_is_reported_not_discarded() {
        let path =
            std::env::temp_dir().join(format!("drums_bad_history_{}.toml", std::process::id()));
        fs::write(&path, "invalid [[").unwrap();
        assert_eq!(
            PracticeSessionStore::new(&path)
                .load_all()
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        let _ = fs::remove_file(path);
    }
}
