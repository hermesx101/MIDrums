use crate::{DrumApp, View};
use drums_core::{CompletedPracticeSession, DrumComponent, DrumSessionStatistics};
use eframe::egui;

const BLUE: egui::Color32 = egui::Color32::from_rgb(105, 195, 245);
const GREEN: egui::Color32 = egui::Color32::from_rgb(72, 214, 137);
const ORANGE: egui::Color32 = egui::Color32::from_rgb(235, 180, 80);
const EARLY: egui::Color32 = egui::Color32::from_rgb(244, 196, 70);
const LATE: egui::Color32 = egui::Color32::from_rgb(246, 132, 68);
const RED: egui::Color32 = egui::Color32::from_rgb(225, 115, 105);
const WRONG_DRUM: egui::Color32 = egui::Color32::from_rgb(244, 82, 118);
const MISSED: egui::Color32 = egui::Color32::from_rgb(120, 124, 135);
const EXTRA: egui::Color32 = egui::Color32::from_rgb(165, 105, 220);

pub fn show(app: &mut DrumApp, ui: &mut egui::Ui) {
    ui.set_width(ui.available_width());
    report(app, ui);
}

fn report(app: &mut DrumApp, ui: &mut egui::Ui) {
    let Some(track_id) = app.analysis_track_path.clone() else {
        ui.heading("PERFORMANCE ANALYSIS");
        ui.label("Select a track from the Track Library first.");
        return;
    };
    let name = app
        .track_library
        .iter()
        .find(|e| e.path == track_id)
        .map(|e| e.display_name.clone())
        .unwrap_or_else(|| track_id.clone());
    let sessions = match app.session_store.load_for_track(&track_id) {
        Ok(v) => v,
        Err(e) => {
            ui.colored_label(RED, format!("Could not load practice history: {e}"));
            return;
        }
    };
    nav(app, ui, &track_id, &sessions);
    ui.add_space(6.0);
    ui.heading("PERFORMANCE ANALYSIS");
    ui.label(egui::RichText::new(name).strong().size(18.0));
    if sessions.is_empty() {
        ui.add_space(14.0);
        ui.heading("No practice sessions yet.");
        ui.label("Practice this track to start building your performance history.");
        return;
    }
    let selected = app
        .analysis_session_id
        .as_ref()
        .and_then(|id| sessions.iter().find(|s| &s.id == id))
        .unwrap_or(&sessions[0]);
    ui.weak(format!(
        "Session: {}  •  {:.0} BPM  •  {}  •  {}",
        date_time(selected.completed_at_unix_ms),
        selected.practice_bpm,
        selected.time_signature,
        duration(selected.duration_seconds)
    ));
    ui.add_space(6.0);
    ui.separator();
    ui.add_space(6.0);
    primary(ui, selected);
    ui.add_space(6.0);
    scorecard(ui, selected);
    heading(ui, "SESSION RESULTS");
    hits(ui, selected);
    heading(ui, "TIMING");
    timing(ui, selected);
    heading(ui, "DYNAMICS");
    dynamics(ui, selected);
    heading(ui, "DRUM BREAKDOWN");
    drums(ui, selected);
    heading(ui, "PERFORMANCE INSIGHTS");
    insights(ui, selected);
    heading(ui, "PRACTICE HISTORY");
    history(ui, app, &sessions, selected);
}

fn nav(app: &mut DrumApp, ui: &mut egui::Ui, track: &str, sessions: &[CompletedPracticeSession]) {
    ui.horizontal_wrapped(|ui| {
        if ui.small_button("Back to Track Library").clicked() {
            app.view = View::TrackLibrary;
        }
        if app.current_library_path.as_deref() == Some(track)
            && ui.small_button("Back to Practice").clicked()
        {
            app.view = View::Practice;
        }
        if ui.small_button("Progress").clicked() {
            app.progress_data = Some(drums_core::TrackProgress::from_sessions(sessions));
            app.progress_track_path = Some(track.into());
            app.view = View::Progress;
        }
    });
}

fn primary(ui: &mut egui::Ui, s: &CompletedPracticeSession) {
    let st = &s.statistics;
    let total = st.correct_count + st.missed_count + st.wrong_drum_count;
    egui::Frame::group(ui.style())
        .fill(egui::Color32::from_rgb(20, 29, 39))
        .inner_margin(9.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.vertical(|ui| {
                    ui.strong("SESSION SCORE");
                    ui.label(
                        egui::RichText::new(format!("{:.1}%", st.accuracy))
                            .strong()
                            .size(28.0)
                            .color(score_color(st.accuracy)),
                    );
                    ui.label("Accuracy");
                });
                ui.add_space(16.0);
                ui.vertical(|ui| {
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(format!(
                            "{} / {} correct hits",
                            st.correct_count, total
                        ))
                        .strong()
                        .size(16.0),
                    );
                    ui.weak(format!(
                        "{:.0} BPM  •  {}  •  {} missed",
                        s.practice_bpm,
                        duration(s.duration_seconds),
                        st.missed_count
                    ));
                });
            });
        });
}

fn scorecard(ui: &mut egui::Ui, s: &CompletedPracticeSession) {
    let st = &s.statistics;
    ui.horizontal_wrapped(|ui| {
        metric(
            ui,
            "ACCURACY",
            &format!("{:.1}%", st.accuracy),
            "Correct note matching",
            score_color(st.accuracy),
        );
        metric(
            ui,
            "CONSISTENCY",
            &pct(st.consistency),
            "Timing stability",
            BLUE,
        );
        metric(
            ui,
            "TIMING",
            &ms(st.average_timing_offset_ms),
            st.average_timing_offset_ms
                .map(language)
                .unwrap_or("Insufficient data"),
            timing_color(st.average_timing_offset_ms),
        );
        metric(
            ui,
            "DYNAMICS",
            &num(st.average_velocity),
            "Average MIDI velocity",
            BLUE,
        );
    });
}
fn metric(ui: &mut egui::Ui, label: &str, value: &str, note: &str, color: egui::Color32) {
    egui::Frame::group(ui.style())
        .inner_margin(7.0)
        .show(ui, |ui| {
            ui.set_min_width(155.0);
            ui.weak(egui::RichText::new(label).size(12.0));
            ui.label(egui::RichText::new(value).strong().size(18.0).color(color));
            ui.weak(egui::RichText::new(note).size(12.0));
        });
}
fn heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(8.0);
    ui.separator();
    ui.add_space(4.0);
    ui.label(egui::RichText::new(text).strong().size(16.0));
    ui.add_space(2.0);
}

fn hits(ui: &mut egui::Ui, s: &CompletedPracticeSession) {
    let x = &s.statistics;
    ui.horizontal_wrapped(|ui| {
        for (l, v, c) in [
            ("CORRECT", x.correct_count, GREEN),
            ("EARLY", x.early_count, EARLY),
            ("LATE", x.late_count, LATE),
            ("WRONG DRUM", x.wrong_drum_count, WRONG_DRUM),
            ("MISSED", x.missed_count, MISSED),
            ("EXTRA", x.extra_count, EXTRA),
        ] {
            egui::Frame::group(ui.style())
                .inner_margin(6.0)
                .show(ui, |ui| {
                    ui.set_min_width(96.0);
                    ui.weak(egui::RichText::new(l).size(12.0));
                    ui.label(
                        egui::RichText::new(v.to_string())
                            .strong()
                            .size(18.0)
                            .color(c),
                    );
                });
        }
    });
}

fn timing(ui: &mut egui::Ui, s: &CompletedPracticeSession) {
    let values = s
        .hits
        .iter()
        .filter_map(|h| h.timing_offset_ms)
        .collect::<Vec<_>>();
    if ui.available_width() < 720.0 {
        timing_summary(ui, s, values.len());
        ui.add_space(5.0);
        distribution(ui, "TIMING DISTRIBUTION", &values, true);
    } else {
        ui.columns(2, |c| {
            timing_summary(&mut c[0], s, values.len());
            distribution(&mut c[1], "TIMING DISTRIBUTION", &values, true);
        });
    }
}
fn timing_summary(ui: &mut egui::Ui, s: &CompletedPracticeSession, count: usize) {
    let x = &s.statistics;
    ui.strong("TIMING SUMMARY");
    ui.add_space(4.0);
    if count < 3 {
        ui.weak("Not enough timed hits for a reliable summary.");
        return;
    }
    row(ui, "Average timing", &ms(x.average_timing_offset_ms));
    row(
        ui,
        "Tendency",
        x.average_timing_offset_ms.map(language).unwrap_or("—"),
    );
    row(
        ui,
        "Early / Late",
        &format!("{} / {}", x.early_count, x.late_count),
    );
    row(
        ui,
        "Timing spread",
        &x.timing_spread_ms
            .map(|v| format!("{v:.1} ms"))
            .unwrap_or_else(|| "—".into()),
    );
    row(ui, "Consistency", &pct(x.consistency));
    if let Some(v) = x.timing_spread_ms {
        ui.weak(format!(
            "Your recorded hits have a timing spread of approximately {v:.0} ms."
        ));
    }
}
fn row(ui: &mut egui::Ui, l: &str, v: &str) {
    ui.horizontal(|ui| {
        ui.label(l);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.strong(v);
        });
    });
}

fn dynamics(ui: &mut egui::Ui, s: &CompletedPracticeSession) {
    let values = s
        .hits
        .iter()
        .filter_map(|h| h.velocity)
        .map(|v| v as f64)
        .collect::<Vec<_>>();
    if values.is_empty() {
        ui.weak("Velocity data unavailable.");
        return;
    }
    ui.horizontal_wrapped(|ui| {
        metric(
            ui,
            "AVERAGE VELOCITY",
            &num(s.statistics.average_velocity),
            "MIDI hit-intensity value",
            BLUE,
        );
        metric(
            ui,
            "RANGE",
            &match (s.statistics.minimum_velocity, s.statistics.maximum_velocity) {
                (Some(a), Some(b)) => format!("{a}–{b}"),
                _ => "—".into(),
            },
            "Minimum to maximum",
            BLUE,
        );
        metric(
            ui,
            "VELOCITY SPREAD",
            &num(s.statistics.velocity_spread),
            "Variation between hits",
            BLUE,
        );
    });
    ui.add_space(5.0);
    distribution(ui, "VELOCITY DISTRIBUTION", &values, false);
    ui.weak("MIDI velocity is the recorded MIDI hit-intensity value, not a direct measurement of physical force.");
}

fn distribution(ui: &mut egui::Ui, title: &str, values: &[f64], centered: bool) {
    ui.strong(title);
    if values.len() < 3 {
        ui.weak("Not enough data for a distribution.");
        return;
    }
    const N: usize = 18;
    let (min, max) = if centered {
        let e = values
            .iter()
            .map(|v| v.abs())
            .fold(50.0_f64, f64::max)
            .ceil();
        (-e, e)
    } else {
        (0.0, 127.0)
    };
    let mut bins = [0usize; N];
    for v in values {
        let i = (((v.clamp(min, max) - min) / (max - min)) * N as f64).floor() as usize;
        bins[i.min(N - 1)] += 1;
    }
    let peak = *bins.iter().max().unwrap_or(&1);
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width().max(240.0), 120.0),
        egui::Sense::hover(),
    );
    let p = ui.painter_at(rect);
    let plot = rect.shrink2(egui::vec2(7.0, 21.0));
    if centered {
        p.line_segment(
            [
                egui::pos2(plot.center().x, plot.top()),
                egui::pos2(plot.center().x, plot.bottom()),
            ],
            egui::Stroke::new(2.0_f32, egui::Color32::from_gray(155)),
        );
    }
    for (i, c) in bins.iter().enumerate() {
        let x0 = plot.left() + i as f32 / N as f32 * plot.width();
        let x1 = plot.left() + (i + 1) as f32 / N as f32 * plot.width();
        let h = *c as f32 / peak.max(1) as f32 * plot.height();
        p.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(x0 + 1.0, plot.bottom() - h),
                egui::pos2(x1 - 1.0, plot.bottom()),
            ),
            1.0,
            if centered && i < N / 2 { ORANGE } else { BLUE },
        );
    }
    p.text(
        plot.left_bottom(),
        egui::Align2::LEFT_TOP,
        format!("{min:.0}"),
        egui::FontId::proportional(13.0),
        egui::Color32::LIGHT_GRAY,
    );
    if centered {
        p.text(
            plot.center_bottom(),
            egui::Align2::CENTER_TOP,
            "0 ms",
            egui::FontId::proportional(13.0),
            egui::Color32::WHITE,
        );
    }
    p.text(
        plot.right_bottom(),
        egui::Align2::RIGHT_TOP,
        format!("{max:.0}"),
        egui::FontId::proportional(13.0),
        egui::Color32::LIGHT_GRAY,
    );
}

fn drums(ui: &mut egui::Ui, s: &CompletedPracticeSession) {
    let list = ordered(s);
    if list.is_empty() {
        ui.weak("No mapped drum performance is available.");
        return;
    }
    if let Some((c, x)) = weakest(&list) {
        ui.label(
            egui::RichText::new(format!(
                "AREA TO WORK ON  •  {} — {:.1}% accuracy",
                c.instrument_label(),
                x.accuracy
            ))
            .strong()
            .color(ORANGE),
        );
        ui.add_space(5.0);
    }
    egui::ScrollArea::horizontal()
        .id_salt("drum_table_scroll")
        .show(ui, |ui| {
            egui::Grid::new("drum_table")
                .striped(true)
                .min_col_width(90.0)
                .show(ui, |ui| {
                    for h in [
                        "DRUM",
                        "HITS",
                        "ACCURACY",
                        "TIMING",
                        "CONSISTENCY",
                        "VELOCITY",
                        "TENDENCY",
                    ] {
                        ui.strong(h);
                    }
                    ui.end_row();
                    for (c, x) in list {
                        ui.strong(c.label());
                        ui.label(x.finalized_count.to_string());
                        ui.label(if x.finalized_count > 0 {
                            format!("{:.1}%", x.accuracy)
                        } else {
                            "—".into()
                        });
                        ui.label(ms(x.average_timing_offset_ms));
                        ui.label(pct(x.consistency));
                        ui.label(num(x.average_velocity));
                        ui.label(x.average_timing_offset_ms.map(language).unwrap_or("—"));
                        ui.end_row();
                    }
                });
        });
}
fn ordered(s: &CompletedPracticeSession) -> Vec<(DrumComponent, &DrumSessionStatistics)> {
    [
        DrumComponent::Kick,
        DrumComponent::Snare,
        DrumComponent::HiHat,
        DrumComponent::HiHatOpen,
        DrumComponent::HiHatPedal,
        DrumComponent::Ride,
        DrumComponent::Crash,
        DrumComponent::HiTom,
        DrumComponent::MidTom,
        DrumComponent::LowTom,
    ]
    .into_iter()
    .filter_map(|c| s.statistics.per_drum.get(&c).map(|x| (c, x)))
    .collect()
}
fn weakest<'a>(
    x: &'a [(DrumComponent, &'a DrumSessionStatistics)],
) -> Option<(DrumComponent, &'a DrumSessionStatistics)> {
    x.iter()
        .copied()
        .filter(|(_, s)| s.finalized_count >= 3)
        .min_by(|a, b| a.1.accuracy.total_cmp(&b.1.accuracy))
}

fn insights(ui: &mut egui::Ui, s: &CompletedPracticeSession) {
    let mut any = false;
    let n = s
        .hits
        .iter()
        .filter(|h| h.timing_offset_ms.is_some())
        .count();
    if n >= 10 {
        if let Some(v) = s.statistics.average_timing_offset_ms {
            ui.label(format!(
                "Timing  •  Your recorded hits are {} on average ({v:+.1} ms).",
                language(v).to_lowercase()
            ));
            any = true;
        }
    }
    let d = ordered(s);
    if let Some((c, x)) = weakest(&d) {
        ui.label(format!(
            "Area to work on  •  {} currently has the lowest supported accuracy ({:.1}%).",
            c.instrument_label(),
            x.accuracy
        ));
        any = true;
    }
    if !any {
        ui.weak("Not enough data for supported session insights.");
    }
}
fn history(
    ui: &mut egui::Ui,
    app: &mut DrumApp,
    sessions: &[CompletedPracticeSession],
    selected: &CompletedPracticeSession,
) {
    egui::Grid::new("practice_history")
        .striped(true)
        .min_col_width(105.0)
        .show(ui, |ui| {
            for h in ["DATE", "BPM", "ACCURACY", "CONSISTENCY"] {
                ui.strong(h);
            }
            ui.end_row();
            for s in sessions {
                if ui
                    .selectable_label(s.id == selected.id, date(s.completed_at_unix_ms))
                    .clicked()
                {
                    app.analysis_session_id = Some(s.id.clone());
                }
                ui.label(format!("{:.0}", s.practice_bpm));
                ui.label(format!("{:.1}%", s.statistics.accuracy));
                ui.label(pct(s.statistics.consistency));
                ui.end_row();
            }
        });
}

fn score_color(v: f64) -> egui::Color32 {
    if v >= 85.0 {
        GREEN
    } else if v >= 60.0 {
        ORANGE
    } else {
        RED
    }
}
fn timing_color(v: Option<f64>) -> egui::Color32 {
    match v {
        Some(x) if x.abs() <= 10.0 => GREEN,
        Some(x) if x.abs() <= 30.0 => ORANGE,
        Some(_) => RED,
        None => egui::Color32::GRAY,
    }
}
fn language(v: f64) -> &'static str {
    if v < -30.0 {
        "Early"
    } else if v < -10.0 {
        "Slightly early"
    } else if v <= 10.0 {
        "Centered"
    } else if v <= 30.0 {
        "Slightly late"
    } else {
        "Late"
    }
}
fn pct(v: Option<f64>) -> String {
    v.map(|x| format!("{x:.1}%")).unwrap_or_else(|| "—".into())
}
fn num(v: Option<f64>) -> String {
    v.map(|x| format!("{x:.1}")).unwrap_or_else(|| "—".into())
}
fn ms(v: Option<f64>) -> String {
    v.map(|x| format!("{x:+.1} ms"))
        .unwrap_or_else(|| "—".into())
}
fn duration(s: f64) -> String {
    format!("{:.0}:{:02.0}", (s / 60.0).floor(), s % 60.0)
}
fn date(ms: u64) -> String {
    let (y, m, d, _, _) = parts(ms);
    format!("{y:04}-{m:02}-{d:02}")
}
fn date_time(ms: u64) -> String {
    let (y, m, d, h, n) = parts(ms);
    format!("{y:04}-{m:02}-{d:02} {h:02}:{n:02}")
}
fn parts(ms: u64) -> (i64, u64, u64, u64, u64) {
    let s = ms / 1000;
    let days = (s / 86400) as i64;
    let ds = s % 86400;
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    y += if m <= 2 { 1 } else { 0 };
    (y, m as u64, d as u64, ds / 3600, ds % 3600 / 60)
}
