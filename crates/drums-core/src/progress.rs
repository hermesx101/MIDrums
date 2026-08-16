//! Derived, read-only progress views over persisted practice sessions.

use std::collections::BTreeMap;

use crate::{CompletedPracticeSession, DrumComponent, DrumSessionStatistics};

#[derive(Clone, Debug, PartialEq)]
pub struct SessionProgressPoint {
    pub session_id: String,
    pub completed_at_unix_ms: u64,
    pub bpm: f64,
    pub accuracy: f64,
    pub consistency: Option<f64>,
    pub average_timing_offset_ms: Option<f64>,
    pub average_velocity: Option<f64>,
    pub velocity_spread: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DrumProgressPoint {
    pub session_id: String,
    pub accuracy: f64,
    pub average_timing_offset_ms: Option<f64>,
    pub consistency: Option<f64>,
    pub average_velocity: Option<f64>,
    pub finalized_count: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WeakestDrum {
    pub component: DrumComponent,
    pub average_accuracy: f64,
    pub sample_count: usize,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrackProgress {
    /// Session points ordered oldest to newest.
    pub sessions: Vec<SessionProgressPoint>,
    pub best_session_id: Option<String>,
    pub average_accuracy: Option<f64>,
    pub average_consistency: Option<f64>,
    pub timing_offsets_ms: Vec<f64>,
    pub per_drum: BTreeMap<DrumComponent, Vec<DrumProgressPoint>>,
    pub weakest_drum: Option<WeakestDrum>,
    pub insights: Vec<String>,
}

impl TrackProgress {
    pub fn from_sessions(source: &[CompletedPracticeSession]) -> Self {
        let mut ordered = source.iter().collect::<Vec<_>>();
        ordered.sort_by_key(|session| session.completed_at_unix_ms);
        let sessions = ordered.iter().map(|session| SessionProgressPoint {
            session_id: session.id.clone(),
            completed_at_unix_ms: session.completed_at_unix_ms,
            bpm: session.practice_bpm,
            accuracy: session.statistics.accuracy,
            consistency: session.statistics.consistency,
            average_timing_offset_ms: session.statistics.average_timing_offset_ms,
            average_velocity: session.statistics.average_velocity,
            velocity_spread: session.statistics.velocity_spread,
        }).collect::<Vec<_>>();
        let average_accuracy = mean(sessions.iter().map(|point| point.accuracy));
        let average_consistency = mean(sessions.iter().filter_map(|point| point.consistency));
        let best_session_id = ordered.iter().max_by(|a, b| {
            a.statistics.accuracy.total_cmp(&b.statistics.accuracy)
                .then_with(|| a.completed_at_unix_ms.cmp(&b.completed_at_unix_ms))
        }).map(|session| session.id.clone());
        let timing_offsets_ms = ordered.iter().flat_map(|session| session.hits.iter())
            .filter_map(|hit| hit.timing_offset_ms).collect::<Vec<_>>();
        let mut per_drum = BTreeMap::<DrumComponent, Vec<DrumProgressPoint>>::new();
        for session in &ordered {
            for (component, stats) in &session.statistics.per_drum {
                per_drum.entry(*component).or_default().push(drum_point(session, stats));
            }
        }
        let weakest_drum = weakest(&per_drum);
        let mut result = Self { sessions, best_session_id, average_accuracy, average_consistency, timing_offsets_ms, per_drum, weakest_drum, insights: Vec::new() };
        result.insights = build_insights(&result);
        result
    }

    pub fn latest(&self) -> Option<&SessionProgressPoint> { self.sessions.last() }
    pub fn best(&self) -> Option<&SessionProgressPoint> {
        let id = self.best_session_id.as_ref()?;
        self.sessions.iter().find(|point| &point.session_id == id)
    }
    pub fn improvement(&self) -> Option<f64> {
        (self.sessions.len() >= 2).then(|| self.sessions.last().unwrap().accuracy - self.sessions.first().unwrap().accuracy)
    }
}

fn drum_point(session: &CompletedPracticeSession, stats: &DrumSessionStatistics) -> DrumProgressPoint {
    DrumProgressPoint { session_id: session.id.clone(), accuracy: stats.accuracy,
        average_timing_offset_ms: stats.average_timing_offset_ms, consistency: stats.consistency,
        average_velocity: stats.average_velocity, finalized_count: stats.finalized_count }
}

fn weakest(per_drum: &BTreeMap<DrumComponent, Vec<DrumProgressPoint>>) -> Option<WeakestDrum> {
    per_drum.iter().filter_map(|(component, points)| {
        let sample_count = points.iter().map(|point| point.finalized_count).sum::<usize>();
        (sample_count >= 3).then(|| WeakestDrum { component: *component,
            average_accuracy: points.iter().map(|point| point.accuracy * point.finalized_count as f64).sum::<f64>() / sample_count as f64,
            sample_count })
    }).min_by(|a, b| a.average_accuracy.total_cmp(&b.average_accuracy))
}

fn build_insights(progress: &TrackProgress) -> Vec<String> {
    let mut insights = Vec::new();
    if progress.sessions.len() >= 3 {
        let recent = &progress.sessions[progress.sessions.len().saturating_sub(5)..];
        let change = recent.last().unwrap().accuracy - recent.first().unwrap().accuracy;
        if change.abs() >= 2.0 {
            insights.push(format!("Accuracy has {} by {:.1} percentage points over the last {} sessions.",
                if change > 0.0 { "improved" } else { "decreased" }, change.abs(), recent.len()));
        }
        let first_consistency = recent.iter().find_map(|point| point.consistency);
        let last_consistency = recent.iter().rev().find_map(|point| point.consistency);
        if let (Some(first), Some(last)) = (first_consistency, last_consistency) {
            if (last - first).abs() >= 3.0 { insights.push(format!("Timing consistency has {} by {:.1} percentage points across recent sessions.", if last > first { "improved" } else { "decreased" }, (last - first).abs())); }
        }
        let mut low = Vec::new(); let mut high = Vec::new();
        let midpoint = progress.sessions.iter().map(|point| point.bpm).sum::<f64>() / progress.sessions.len() as f64;
        for point in &progress.sessions { if point.bpm <= midpoint { low.push(point.accuracy); } else { high.push(point.accuracy); } }
        if low.len() >= 2 && high.len() >= 2 {
            let difference = mean(low.into_iter()).unwrap() - mean(high.into_iter()).unwrap();
            if difference >= 5.0 { insights.push(format!("Accuracy averages {difference:.1} points lower in sessions above {midpoint:.0} BPM.")); }
        }
    }
    if progress.timing_offsets_ms.len() >= 10 {
        let average = mean(progress.timing_offsets_ms.iter().copied()).unwrap();
        if average.abs() >= 10.0 { insights.push(format!("Timing is centered {:.1} ms {} across recorded hits.", average.abs(), if average < 0.0 { "early" } else { "late" })); }
    }
    if let Some(weakest) = &progress.weakest_drum {
        if progress.per_drum.len() >= 2 { insights.push(format!("{} currently has the lowest historical accuracy at {:.1}%.", weakest.component.instrument_label(), weakest.average_accuracy)); }
    }
    insights
}

fn mean(values: impl Iterator<Item = f64>) -> Option<f64> {
    let values = values.collect::<Vec<_>>();
    (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PracticeHitRecord, PracticeResultKind, SessionStatistics, Subdivision};

    fn session(id: &str, time: u64, bpm: f64, accuracy: f64, consistency: Option<f64>) -> CompletedPracticeSession {
        CompletedPracticeSession { id: id.into(), track_id: "track".into(), started_at_unix_ms: time - 1,
            completed_at_unix_ms: time, duration_seconds: 1.0, practice_bpm: bpm, time_signature: "4/4".into(),
            subdivision: Subdivision::Eighth, statistics: SessionStatistics { accuracy, consistency, ..Default::default() }, hits: vec![] }
    }
    #[test] fn histories_are_ordered_and_session_values_stay_associated() {
        let p = TrackProgress::from_sessions(&[session("new", 20, 100.0, 80.0, Some(70.0)), session("old", 10, 80.0, 90.0, Some(60.0))]);
        assert_eq!(p.sessions.iter().map(|x| x.session_id.as_str()).collect::<Vec<_>>(), ["old", "new"]);
        assert_eq!((p.sessions[1].bpm, p.sessions[1].accuracy, p.sessions[1].consistency), (100.0, 80.0, Some(70.0)));
        assert_eq!(p.best().unwrap().session_id, "old"); assert_eq!(p.latest().unwrap().session_id, "new");
    }
    #[test] fn averages_and_empty_history_are_safe() {
        let p = TrackProgress::from_sessions(&[session("a", 1, 80.0, 80.0, Some(60.0)), session("b", 2, 90.0, 100.0, Some(80.0))]);
        assert_eq!(p.average_accuracy, Some(90.0)); assert_eq!(p.average_consistency, Some(70.0));
        let empty = TrackProgress::from_sessions(&[]); assert_eq!(empty.average_accuracy, None); assert_eq!(empty.improvement(), None);
    }
    #[test] fn one_session_never_produces_a_trend() { assert_eq!(TrackProgress::from_sessions(&[session("a", 1, 80.0, 80.0, None)]).improvement(), None); }
    #[test] fn timing_sign_is_preserved() {
        let mut early = session("early", 1, 80.0, 80.0, None); early.hits.push(hit(-12.0));
        let mut late = session("late", 2, 80.0, 80.0, None); late.hits.push(hit(18.0));
        assert_eq!(TrackProgress::from_sessions(&[late, early]).timing_offsets_ms, [-12.0, 18.0]);
    }
    #[test] fn per_drum_and_weakest_are_independent() {
        let mut a = session("a", 1, 80.0, 80.0, None);
        a.statistics.per_drum.insert(DrumComponent::Kick, drum(90.0, 5));
        a.statistics.per_drum.insert(DrumComponent::Snare, drum(70.0, 5));
        let p = TrackProgress::from_sessions(&[a]);
        assert_eq!(p.per_drum[&DrumComponent::Kick][0].accuracy, 90.0);
        assert_eq!(p.weakest_drum.unwrap().component, DrumComponent::Snare);
    }
    #[test] fn missing_velocity_is_not_fabricated() {
        let mut a = session("a", 1, 80.0, 80.0, None); a.statistics.average_velocity = Some(88.0);
        let p = TrackProgress::from_sessions(&[a, session("b", 2, 80.0, 80.0, None)]);
        assert_eq!(p.sessions[0].average_velocity, Some(88.0)); assert_eq!(p.sessions[1].average_velocity, None);
    }
    #[test] fn building_progress_and_appending_never_mutates_history() {
        let original = session("a", 1, 80.0, 80.0, None); let mut history = vec![original.clone()];
        let _ = TrackProgress::from_sessions(&history); history.push(session("b", 2, 90.0, 90.0, None));
        assert_eq!(history[0], original);
    }
    #[test] fn measurable_accuracy_change_requires_three_sessions() {
        let two = TrackProgress::from_sessions(&[session("a", 1, 80.0, 70.0, None), session("b", 2, 80.0, 90.0, None)]);
        assert!(two.insights.iter().all(|text| !text.contains("Accuracy has")));
        let three = TrackProgress::from_sessions(&[session("a", 1, 80.0, 70.0, None), session("b", 2, 80.0, 80.0, None), session("c", 3, 80.0, 90.0, None)]);
        assert!(three.insights.iter().any(|text| text.contains("improved by 20.0")));
    }
    #[test] fn bpm_observation_requires_two_samples_in_each_group() {
        let p = TrackProgress::from_sessions(&[
            session("a", 1, 80.0, 95.0, None), session("b", 2, 80.0, 93.0, None),
            session("c", 3, 120.0, 70.0, None), session("d", 4, 120.0, 72.0, None),
        ]);
        assert!(p.insights.iter().any(|text| text.contains("lower in sessions above")));
    }
    fn hit(offset: f64) -> PracticeHitRecord { PracticeHitRecord { sequence: 0, input_timestamp_micros: None, expected_drum: None, actual_drum: None, expected_time_seconds: None, actual_time_seconds: None, timing_offset_ms: Some(offset), velocity: None, result_kind: PracticeResultKind::CorrectHit, timing_judgment: None } }
    fn drum(accuracy: f64, count: usize) -> DrumSessionStatistics { DrumSessionStatistics { accuracy, finalized_count: count, correct_count: (accuracy * count as f64 / 100.0) as usize, ..Default::default() } }
}
