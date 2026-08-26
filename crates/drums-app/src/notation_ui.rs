//! egui renderer for standalone and Practice-integrated drum Sheet Music.
//!
//! The notation model comes from `drums-core`; this module only lays it out,
//! paints feedback, and optionally follows the active transport system.

use crate::{DrumApp, View};
use drums_core::{
    DrumComponent, DrumNotationScore, NotationDrumNote, NotationMeasure, PracticeResult,
    PracticeResultKind, RhythmicValue, TimingJudgment,
};
use eframe::egui;

const RESULT_CORRECT: egui::Color32 = egui::Color32::from_rgb(72, 214, 137);
const RESULT_EARLY: egui::Color32 = egui::Color32::from_rgb(244, 196, 70);
const RESULT_LATE: egui::Color32 = egui::Color32::from_rgb(246, 132, 68);
const RESULT_WRONG_DRUM: egui::Color32 = egui::Color32::from_rgb(244, 82, 118);
const RESULT_MISSED: egui::Color32 = egui::Color32::from_rgb(120, 124, 135);
const RESULT_EXTRA: egui::Color32 = egui::Color32::from_rgb(165, 105, 220);

#[derive(Clone, Copy)]
struct StaffGeometry {
    top: f32,
    spacing: f32,
    zoom: f32,
}

/// Renders the standalone Sheet Music page and its transport/follow controls.
pub fn show(app: &mut DrumApp, ui: &mut egui::Ui) {
    let Some(score) = app.notation_score.clone() else {
        ui.heading("DRUM SHEET MUSIC");
        ui.label("No notation is loaded.");
        if ui.button("Your Tracks").clicked() {
            app.view = View::TrackLibrary;
        }
        return;
    };
    ui.horizontal_wrapped(|ui| {
        if ui.button("← Your Tracks").clicked() {
            if let Some(t) = app.transport.as_mut() {
                t.pause();
            }
            app.view = View::TrackLibrary;
        }
        if ui.button("Practice This").clicked() {
            if let Some(t) = app.transport.as_mut() {
                t.pause();
            }
            app.restart_practice(false);
            app.view = View::Practice;
        }
        ui.separator();
        if ui.small_button("−").clicked() {
            app.notation_zoom = (app.notation_zoom - 0.1).max(0.7);
        }
        ui.strong(format!("Zoom: {:.0}%", app.notation_zoom * 100.0));
        if ui.small_button("+").clicked() {
            app.notation_zoom = (app.notation_zoom + 0.1).min(2.0);
        }
        ui.checkbox(&mut app.notation_follow_playback, "Follow Playback");
        if app.transport.as_ref().is_some_and(|t| t.is_playing()) {
            if ui.button("Pause").clicked() {
                app.transport.as_mut().unwrap().pause();
            }
        } else if ui.button("Play").clicked() {
            if let Some(t) = app.transport.as_mut() {
                t.play();
            }
        }
    });
    ui.add_space(8.0);
    ui.heading("DRUM SHEET MUSIC");
    ui.label(egui::RichText::new(&score.title).strong().size(19.0));
    let signature = score
        .measures
        .first()
        .map(|m| format!("{}/{}", m.numerator, m.denominator))
        .unwrap_or_else(|| "4/4".into());
    ui.label(format!("♩ = {:.0}    {signature}", score.initial_bpm()));
    ui.add_space(10.0);
    if score
        .measures
        .iter()
        .all(|m| m.slots.iter().all(|s| s.notes.is_empty()))
    {
        ui.label("No recognizable drum notation found.");
        return;
    }
    let current_tick = app
        .transport
        .as_ref()
        .map(|t| t.current_tick())
        .unwrap_or(0);
    let current_measure = score
        .measures
        .iter()
        .find(|m| current_tick >= m.start_tick && current_tick < m.end_tick)
        .map(|m| m.number);
    let measures_per_system = if ui.available_width() < 700.0 || app.notation_zoom > 1.35 {
        1
    } else {
        2
    };
    for system in score.measures.chunks(measures_per_system) {
        let active = system.iter().any(|m| Some(m.number) == current_measure);
        let response = render_system(
            ui,
            &score,
            system,
            current_tick,
            app.notation_zoom,
            active,
            None,
        );
        if active
            && app.notation_follow_playback
            && app.transport.as_ref().is_some_and(|t| t.is_playing())
        {
            ui.scroll_to_rect(response.rect, Some(egui::Align::Center));
        }
        ui.add_space(10.0);
    }
}

/// Practice visualization only: transport and evaluation remain owned by the
/// existing practice engine, and this consumes their read-only state.
/// Renders notation for the active PracticeSession.
///
/// Results are the same authoritative outcomes used by Timeline. When follow
/// mode is enabled, the response for the active system is scrolled into view;
/// this function never advances transport state.
pub fn show_practice_score(
    ui: &mut egui::Ui,
    score: &DrumNotationScore,
    current_tick: u64,
    results: &[PracticeResult],
    follow_playback: bool,
    zoom: f32,
) {
    if score
        .measures
        .iter()
        .all(|m| m.slots.iter().all(|s| s.notes.is_empty()))
    {
        ui.label("No recognizable drum notation found.");
        return;
    }
    let current_measure = score
        .measures
        .iter()
        .find(|m| current_tick >= m.start_tick && current_tick < m.end_tick)
        .map(|m| m.number);
    for system in score
        .measures
        .chunks(if ui.available_width() < 700.0 { 1 } else { 2 })
    {
        let active = system.iter().any(|m| Some(m.number) == current_measure);
        let response = render_system(ui, score, system, current_tick, zoom, active, Some(results));
        if active && follow_playback {
            ui.scroll_to_rect(response.rect, Some(egui::Align::Center));
        }
        ui.add_space(8.0);
    }
}

fn render_system(
    ui: &mut egui::Ui,
    score: &DrumNotationScore,
    measures: &[NotationMeasure],
    current_tick: u64,
    zoom: f32,
    active: bool,
    results: Option<&[PracticeResult]>,
) -> egui::Response {
    let width = ui.available_width().max(420.0);
    let height = 190.0 * zoom;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(
        rect,
        3.0,
        if active {
            egui::Color32::from_rgb(35, 48, 58)
        } else {
            egui::Color32::from_rgb(28, 31, 36)
        },
    );
    let left = rect.left() + 44.0 * zoom;
    let right = rect.right() - 12.0 * zoom;
    let top = rect.top() + 48.0 * zoom;
    let spacing = 13.0 * zoom;
    let staff = StaffGeometry { top, spacing, zoom };
    for line in 0..5 {
        let y = top + line as f32 * spacing;
        p.line_segment(
            [egui::pos2(left, y), egui::pos2(right, y)],
            egui::Stroke::new(1.2_f32, egui::Color32::from_gray(205)),
        );
    }
    p.text(
        egui::pos2(rect.left() + 8.0 * zoom, top + 2.0 * spacing),
        egui::Align2::LEFT_CENTER,
        "𝄆",
        egui::FontId::proportional(30.0 * zoom),
        egui::Color32::LIGHT_GRAY,
    );
    let measure_width = (right - left) / measures.len() as f32;
    for (i, m) in measures.iter().enumerate() {
        let x0 = left + i as f32 * measure_width;
        let x1 = x0 + measure_width;
        p.line_segment(
            [egui::pos2(x0, top), egui::pos2(x0, top + 4.0 * spacing)],
            egui::Stroke::new(1.5_f32, egui::Color32::WHITE),
        );
        if i + 1 == measures.len() {
            p.line_segment(
                [egui::pos2(x1, top), egui::pos2(x1, top + 4.0 * spacing)],
                egui::Stroke::new(2.0_f32, egui::Color32::WHITE),
            );
        }
        p.text(
            egui::pos2(x0 + 5.0, rect.top() + 10.0),
            egui::Align2::LEFT_TOP,
            format!("{}   {}/{}", m.number, m.numerator, m.denominator),
            egui::FontId::proportional(13.0 * zoom),
            egui::Color32::LIGHT_GRAY,
        );
        if let Some(tempo) = score
            .tempos
            .iter()
            .find(|tempo| tempo.tick >= m.start_tick && tempo.tick < m.end_tick && tempo.tick > 0)
        {
            p.text(
                egui::pos2(x0 + measure_width * 0.55, rect.top() + 10.0),
                egui::Align2::LEFT_TOP,
                format!("♩={:.0}", tempo.bpm),
                egui::FontId::proportional(12.0 * zoom),
                egui::Color32::from_rgb(120, 200, 245),
            );
        }
        if i == 0 {
            p.text(
                egui::pos2(x0 + 5.0, top + 2.0 * spacing),
                egui::Align2::LEFT_CENTER,
                format!("{}\n{}", m.numerator, m.denominator),
                egui::FontId::proportional(14.0 * zoom),
                egui::Color32::WHITE,
            );
        }
        for slot in &m.slots {
            let fraction =
                (slot.tick - m.start_tick) as f32 / (m.end_tick - m.start_tick).max(1) as f32;
            let x = x0 + 18.0 * zoom + fraction * (measure_width - 22.0 * zoom);
            if slot.is_rest() {
                draw_rest(&p, egui::pos2(x, top + 2.0 * spacing), slot.value, zoom);
            } else {
                draw_chord(
                    &p,
                    x,
                    staff,
                    &slot.notes,
                    slot.value,
                    results.map(|r| (r, slot.tick, score.grid_ticks)),
                );
            }
        }
        if let Some(results) = results {
            for result in results.iter().rev().take(256).filter(|result| {
                matches!(
                    result.kind,
                    PracticeResultKind::WrongDrum | PracticeResultKind::Ignored
                ) && result.actual.as_ref().is_some_and(|actual| {
                    actual.transport_tick >= m.start_tick && actual.transport_tick < m.end_tick
                })
            }) {
                let actual = result.actual.as_ref().unwrap();
                let fraction = (actual.transport_tick - m.start_tick) as f32
                    / (m.end_tick - m.start_tick).max(1) as f32;
                let x = x0 + 18.0 * zoom + fraction * (measure_width - 22.0 * zoom);
                let y = actual
                    .drum_component
                    .map(|component| staff_y(component, top, spacing))
                    .unwrap_or(top + 2.0 * spacing);
                let color = if result.kind == PracticeResultKind::WrongDrum {
                    RESULT_WRONG_DRUM
                } else {
                    RESULT_EXTRA
                };
                p.circle_stroke(
                    egui::pos2(x, y),
                    8.0 * zoom,
                    egui::Stroke::new(2.0_f32, color),
                );
                p.line_segment(
                    [egui::pos2(x - 4.0 * zoom, y), egui::pos2(x + 4.0 * zoom, y)],
                    egui::Stroke::new(1.5_f32, color),
                );
            }
        }
        if current_tick >= m.start_tick && current_tick < m.end_tick {
            let f =
                (current_tick - m.start_tick) as f32 / (m.end_tick - m.start_tick).max(1) as f32;
            let x = x0 + f * measure_width;
            p.line_segment(
                [
                    egui::pos2(x, top - 13.0 * zoom),
                    egui::pos2(x, top + 4.0 * spacing + 13.0 * zoom),
                ],
                egui::Stroke::new(2.0_f32, egui::Color32::from_rgb(90, 195, 250)),
            );
        }
    }
    p.text(
        egui::pos2(left, rect.bottom() - 12.0 * zoom),
        egui::Align2::LEFT_BOTTOM,
        format!(
            "Grid: {}",
            grid_label(score.grid_ticks, score.ticks_per_quarter as u64)
        ),
        egui::FontId::proportional(12.0 * zoom),
        egui::Color32::GRAY,
    );
    response
}

fn draw_chord(
    p: &egui::Painter,
    x: f32,
    staff: StaffGeometry,
    notes: &[NotationDrumNote],
    value: RhythmicValue,
    feedback: Option<(&[PracticeResult], u64, u64)>,
) {
    let mut ys = notes
        .iter()
        .map(|n| staff_y(n.component, staff.top, staff.spacing))
        .collect::<Vec<_>>();
    ys.sort_by(|a, b| a.total_cmp(b));
    for (note, y) in notes.iter().zip(
        notes
            .iter()
            .map(|n| staff_y(n.component, staff.top, staff.spacing)),
    ) {
        let color = feedback
            .and_then(|(r, t, g)| note_feedback(r, t, g, note.component))
            .unwrap_or(egui::Color32::WHITE);
        draw_head(p, egui::pos2(x, y), note.component, staff.zoom, color);
    }
    let stem_top = ys.first().copied().unwrap_or(staff.top) - 25.0 * staff.zoom;
    let stem_bottom = ys.last().copied().unwrap_or(staff.top);
    p.line_segment(
        [
            egui::pos2(x + 5.0 * staff.zoom, stem_bottom),
            egui::pos2(x + 5.0 * staff.zoom, stem_top),
        ],
        egui::Stroke::new(1.2_f32, egui::Color32::WHITE),
    );
    let flags = match value {
        RhythmicValue::Eighth | RhythmicValue::DottedEighth => 1,
        RhythmicValue::Sixteenth => 2,
        RhythmicValue::ThirtySecond => 3,
        _ => 0,
    };
    for i in 0..flags {
        let y = stem_top + i as f32 * 6.0 * staff.zoom;
        p.line_segment(
            [
                egui::pos2(x + 5.0 * staff.zoom, y),
                egui::pos2(x + 14.0 * staff.zoom, y + 7.0 * staff.zoom),
            ],
            egui::Stroke::new(1.5_f32, egui::Color32::WHITE),
        );
    }
}
fn draw_head(p: &egui::Painter, pos: egui::Pos2, c: DrumComponent, z: f32, color: egui::Color32) {
    if matches!(
        c,
        DrumComponent::HiHat
            | DrumComponent::HiHatOpen
            | DrumComponent::HiHatPedal
            | DrumComponent::Crash
            | DrumComponent::Ride
    ) {
        let r = 5.0 * z;
        p.line_segment(
            [pos - egui::vec2(r, r), pos + egui::vec2(r, r)],
            egui::Stroke::new(1.8_f32, color),
        );
        p.line_segment(
            [pos + egui::vec2(-r, r), pos + egui::vec2(r, -r)],
            egui::Stroke::new(1.8_f32, color),
        );
        if c == DrumComponent::HiHatOpen {
            p.circle_stroke(
                pos - egui::vec2(0.0, 11.0 * z),
                3.5 * z,
                egui::Stroke::new(1.2_f32, color),
            );
        }
    } else {
        p.circle_filled(pos, 5.0 * z, color);
    }
}
fn note_feedback(
    results: &[PracticeResult],
    tick: u64,
    grid: u64,
    component: DrumComponent,
) -> Option<egui::Color32> {
    results
        .iter()
        .rev()
        .find(|r| {
            r.expected.as_ref().is_some_and(|e| {
                e.drum_component == Some(component) && e.tick.abs_diff(tick) <= grid / 2
            })
        })
        .map(|r| match r.kind {
            PracticeResultKind::CorrectHit => match r.timing_judgment {
                Some(TimingJudgment::Early) => RESULT_EARLY,
                Some(TimingJudgment::Late) => RESULT_LATE,
                _ => RESULT_CORRECT,
            },
            PracticeResultKind::WrongDrum => RESULT_WRONG_DRUM,
            PracticeResultKind::Missed => RESULT_MISSED,
            PracticeResultKind::Ignored => RESULT_EXTRA,
        })
}
fn draw_rest(p: &egui::Painter, pos: egui::Pos2, value: RhythmicValue, z: f32) {
    let size = match value {
        RhythmicValue::Whole | RhythmicValue::Half => egui::vec2(8.0, 3.0),
        _ => egui::vec2(5.0, 7.0),
    } * z;
    p.rect_filled(
        egui::Rect::from_center_size(pos, size),
        0.5,
        egui::Color32::from_gray(125),
    );
}
fn staff_y(c: DrumComponent, top: f32, s: f32) -> f32 {
    top + match c {
        DrumComponent::Crash => -1.3,
        DrumComponent::Ride => -0.7,
        DrumComponent::HiHat | DrumComponent::HiHatOpen => -0.2,
        DrumComponent::HiTom => 0.5,
        DrumComponent::MidTom => 1.2,
        DrumComponent::Snare => 2.0,
        DrumComponent::LowTom => 3.0,
        DrumComponent::Kick => 4.5,
        DrumComponent::HiHatPedal | DrumComponent::HiHatPedalPosition => 5.1,
    } * s
}
fn grid_label(grid: u64, tpq: u64) -> &'static str {
    match tpq / grid.max(1) {
        0 | 1 => "quarter notes",
        2 => "eighth notes",
        3 | 4 => "sixteenth notes",
        _ => "thirty-second notes",
    }
}
