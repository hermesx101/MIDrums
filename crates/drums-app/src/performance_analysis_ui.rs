use crate::{DrumApp, View};
use drums_core::{CompletedPracticeSession, DrumSessionStatistics};
use eframe::egui;

pub fn show(app: &mut DrumApp, ui: &mut egui::Ui) {
    let Some(track_id) = app.analysis_track_path.clone() else {
        ui.heading("PERFORMANCE ANALYSIS");
        ui.label("Select a track from the Track Library first.");
        if ui.button("Back to Track Library").clicked() { app.view = View::TrackLibrary; }
        return;
    };
    let name = app.track_library.iter().find(|entry| entry.path == track_id)
        .map(|entry| entry.display_name.clone()).unwrap_or_else(|| track_id.clone());
    let sessions = match app.session_store.load_for_track(&track_id) {
        Ok(value) => value,
        Err(error) => {
            ui.heading("PERFORMANCE ANALYSIS");
            ui.colored_label(egui::Color32::from_rgb(230, 120, 120), format!("Could not load practice history: {error}"));
            return;
        }
    };

    ui.horizontal_wrapped(|ui| {
        if ui.button("Back to Track Library").clicked() { app.view = View::TrackLibrary; }
        if app.current_library_path.as_deref() == Some(track_id.as_str()) && ui.button("Back to Practice").clicked() {
            app.view = View::Practice;
        }
    });
    ui.add_space(8.0);
    ui.heading("PERFORMANCE ANALYSIS");
    ui.label(egui::RichText::new(name).strong().size(22.0));
    ui.add_space(12.0);

    if sessions.is_empty() {
        ui.heading("No practice sessions yet.");
        ui.label("Practice this track to start building your performance history.");
        return;
    }
    let selected = app.analysis_session_id.as_ref()
        .and_then(|id| sessions.iter().find(|session| &session.id == id))
        .unwrap_or(&sessions[0]);
    let best = sessions.iter().max_by(|a, b| a.statistics.accuracy.total_cmp(&b.statistics.accuracy)).unwrap();

    ui.horizontal_wrapped(|ui| {
        metric_card(ui, "SELECTED SESSION", &format!("{:.1}%", selected.statistics.accuracy), "ACCURACY");
        metric_card(ui, "BEST SCORE", &format!("{:.1}%", best.statistics.accuracy), &format!("{} at {:.0} BPM", date_text(best.completed_at_unix_ms), best.practice_bpm));
        metric_card(ui, "PRACTICE", &sessions.len().to_string(), "SESSIONS");
    });
    ui.add_space(12.0);
    session_details(ui, selected);
    ui.add_space(14.0);

    ui.heading("PRACTICE HISTORY");
    egui::Grid::new("practice_history").striped(true).min_col_width(105.0).show(ui, |ui| {
        ui.strong("Date"); ui.strong("BPM"); ui.strong("Accuracy"); ui.strong("Consistency"); ui.end_row();
        for session in &sessions {
            let active = session.id == selected.id;
            if ui.selectable_label(active, date_text(session.completed_at_unix_ms)).clicked() {
                app.analysis_session_id = Some(session.id.clone());
            }
            ui.label(format!("{:.0}", session.practice_bpm));
            ui.label(format!("{:.1}%", session.statistics.accuracy));
            ui.label(percent_or_unavailable(session.statistics.consistency));
            ui.end_row();
        }
    });
}

fn metric_card(ui: &mut egui::Ui, title: &str, value: &str, detail: &str) {
    egui::Frame::group(ui.style()).inner_margin(14.0).show(ui, |ui| {
        ui.strong(title);
        ui.label(egui::RichText::new(value).strong().size(30.0));
        ui.weak(detail);
    });
}

fn session_details(ui: &mut egui::Ui, session: &CompletedPracticeSession) {
    ui.heading("SESSION DETAILS");
    ui.label(format!("{}  |  {:.0} BPM  |  {}  |  {:.0}m {:02.0}s", date_time_text(session.completed_at_unix_ms), session.practice_bpm, session.time_signature, session.duration_seconds / 60.0, session.duration_seconds % 60.0));
    ui.horizontal_wrapped(|ui| {
        for (label, value) in [
            ("Correct", session.statistics.correct_count), ("Early", session.statistics.early_count),
            ("Late", session.statistics.late_count), ("Missed", session.statistics.missed_count),
            ("Wrong drum", session.statistics.wrong_drum_count), ("Extra", session.statistics.extra_count),
        ] { metric_card(ui, label, &value.to_string(), "HITS"); }
    });
    ui.add_space(10.0);
    ui.columns(2, |columns| {
        columns[0].heading("TIMING");
        let timing_count = session.hits.iter().filter(|hit| hit.timing_offset_ms.is_some()).count();
        if timing_count < 3 {
            columns[0].label("Not enough data yet.");
        } else if let Some(average) = session.statistics.average_timing_offset_ms {
            columns[0].label(format!("Average timing: {average:+.1} ms"));
            columns[0].label(format!("Tendency: {}", tendency(average)));
            columns[0].label(format!("Early: {}  Late: {}", session.statistics.early_count, session.statistics.late_count));
            if let Some(spread) = session.statistics.timing_spread_ms { columns[0].label(format!("Timing spread: {spread:.1} ms")); }
            columns[0].label(format!("Consistency: {}", percent_or_unavailable(session.statistics.consistency)));
        }
        columns[1].heading("VELOCITY");
        if let Some(average) = session.statistics.average_velocity {
            columns[1].label(format!("Average velocity: {average:.1}"));
            if let (Some(min), Some(max)) = (session.statistics.minimum_velocity, session.statistics.maximum_velocity) { columns[1].label(format!("Range: {min}-{max}")); }
            if let Some(spread) = session.statistics.velocity_spread { columns[1].label(format!("Velocity spread: {spread:.1}")); }
        } else { columns[1].label("Velocity data unavailable"); }
    });
    ui.add_space(10.0);
    ui.heading("PERFORMANCE SCORECARD");
    ui.horizontal_wrapped(|ui| {
        metric_card(ui, "Accuracy", &format!("{:.1}%", session.statistics.accuracy), "SCORE");
        metric_card(ui, "Consistency", &percent_or_unavailable(session.statistics.consistency), "TIMING");
        let timing = session.statistics.average_timing_offset_ms.map(|v| format!("{v:+.1} ms")).unwrap_or_else(|| "Not enough data".into());
        metric_card(ui, "Timing", &timing, "AVERAGE");
        let dynamics = session.statistics.average_velocity.map(|v| format!("{v:.1}")).unwrap_or_else(|| "Not enough data".into());
        metric_card(ui, "Dynamics", &dynamics, "AVG VELOCITY");
    });
    if !session.statistics.per_drum.is_empty() {
        ui.add_space(10.0); ui.heading("DRUM BREAKDOWN");
        for (component, stats) in &session.statistics.per_drum {
            drum_row(ui, component.label(), stats);
        }
    }
}

fn drum_row(ui: &mut egui::Ui, name: &str, stats: &DrumSessionStatistics) {
    egui::Frame::group(ui.style()).inner_margin(10.0).show(ui, |ui| {
        ui.strong(name);
        ui.horizontal_wrapped(|ui| {
            ui.label(format!("{:.1}% accuracy", stats.accuracy));
            ui.separator(); ui.label(stats.average_timing_offset_ms.map(|v| format!("{v:+.1} ms")).unwrap_or_else(|| "Timing unavailable".into()));
            ui.separator(); ui.label(format!("{} consistency", percent_or_unavailable(stats.consistency)));
            ui.separator(); ui.label(stats.average_velocity.map(|v| format!("Velocity {v:.1}")).unwrap_or_else(|| "Velocity unavailable".into()));
        });
    });
}

fn percent_or_unavailable(value: Option<f64>) -> String { value.map(|v| format!("{v:.1}%")).unwrap_or_else(|| "Not enough data".into()) }
fn tendency(value: f64) -> &'static str { if value < -10.0 { "Early" } else if value > 10.0 { "Late" } else { "Centered" } }

fn date_text(ms: u64) -> String { let (y, m, d, _, _) = date_parts(ms); format!("{y:04}-{m:02}-{d:02}") }
fn date_time_text(ms: u64) -> String { let (y, m, d, h, min) = date_parts(ms); format!("{y:04}-{m:02}-{d:02} {h:02}:{min:02}") }
fn date_parts(ms: u64) -> (i64, u64, u64, u64, u64) {
    let seconds = ms / 1_000; let days = (seconds / 86_400) as i64; let day_seconds = seconds % 86_400;
    let z = days + 719_468; let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe + era * 400; let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1; let month = mp + if mp < 10 { 3 } else { -9 }; year += if month <= 2 { 1 } else { 0 };
    (year, month as u64, day as u64, day_seconds / 3_600, day_seconds % 3_600 / 60)
}
