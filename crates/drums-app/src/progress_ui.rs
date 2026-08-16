use crate::{DrumApp, View};
use drums_core::TrackProgress;
use eframe::egui;

pub fn show(app: &mut DrumApp, ui: &mut egui::Ui) {
    let Some(track_id) = app.progress_track_path.clone() else {
        ui.heading("PROGRESS"); ui.label("Select a track from Performance Analysis first."); return;
    };
    let name = app.track_library.iter().find(|entry| entry.path == track_id)
        .map(|entry| entry.display_name.clone()).unwrap_or_else(|| track_id.clone());
    ui.horizontal_wrapped(|ui| {
        if ui.button("Back to Performance Analysis").clicked() { app.view = View::PerformanceAnalysis; }
        if ui.button("Back to Track Library").clicked() { app.view = View::TrackLibrary; }
        if app.current_library_path.as_deref() == Some(track_id.as_str()) && ui.button("Back to Practice").clicked() { app.view = View::Practice; }
        if ui.button("Refresh Progress").clicked() {
            match app.session_store.load_for_track(&track_id) {
                Ok(sessions) => app.progress_data = Some(TrackProgress::from_sessions(&sessions)),
                Err(error) => app.status = format!("Could not refresh progress: {error}"),
            }
        }
    });
    ui.add_space(8.0); ui.heading("TRACK PROGRESS"); ui.label(egui::RichText::new(name).strong().size(22.0)); ui.add_space(12.0);
    let Some(progress) = app.progress_data.as_ref() else { ui.label("No progress data yet."); return; };
    if progress.sessions.is_empty() { ui.heading("No progress data yet."); ui.label("Complete a practice session to begin tracking progress."); return; }

    let latest = progress.latest().unwrap(); let best = progress.best().unwrap();
    ui.horizontal_wrapped(|ui| {
        card(ui, "BEST ACCURACY", &format!("{:.1}%", best.accuracy));
        card(ui, "AVERAGE", &format!("{:.1}%", progress.average_accuracy.unwrap()));
        if let Some(consistency) = progress.average_consistency { card(ui, "CONSISTENCY", &format!("{consistency:.1}%")); }
        card(ui, "SESSIONS", &progress.sessions.len().to_string());
        card(ui, "LATEST", &format!("{:.1}%", latest.accuracy));
        card(ui, "LATEST BPM", &format!("{:.0}", latest.bpm));
        if let Some(change) = progress.improvement() { card(ui, "CHANGE", &format!("{change:+.1} points")); }
    });
    ui.add_space(10.0); ui.heading("BEST VS LATEST");
    ui.horizontal_wrapped(|ui| { card(ui, "BEST SESSION", &format!("{:.1}% at {:.0} BPM", best.accuracy, best.bpm)); card(ui, "LATEST SESSION", &format!("{:.1}% at {:.0} BPM", latest.accuracy, latest.bpm)); });

    let indexed_accuracy = progress.sessions.iter().enumerate().map(|(i, p)| ((i + 1) as f64, p.accuracy)).collect::<Vec<_>>();
    let indexed_consistency = progress.sessions.iter().enumerate().filter_map(|(i, p)| p.consistency.map(|v| ((i + 1) as f64, v))).collect::<Vec<_>>();
    let bpm_accuracy = progress.sessions.iter().map(|p| (p.bpm, p.accuracy)).collect::<Vec<_>>();
    let bpm_consistency = progress.sessions.iter().filter_map(|p| p.consistency.map(|v| (p.bpm, v))).collect::<Vec<_>>();
    line_chart(ui, "Accuracy over time", "Session", "Accuracy %", &indexed_accuracy, Some((0.0, 100.0)));
    line_chart(ui, "Consistency over time", "Session", "Consistency %", &indexed_consistency, Some((0.0, 100.0)));
    scatter_chart(ui, "Accuracy vs BPM", "BPM", "Accuracy %", &bpm_accuracy, Some((0.0, 100.0)));
    scatter_chart(ui, "Consistency vs BPM", "BPM", "Consistency %", &bpm_consistency, Some((0.0, 100.0)));

    ui.add_space(12.0); ui.heading("TIMING TENDENCY");
    if progress.timing_offsets_ms.len() < 10 { ui.label("Not enough timing data yet."); }
    else {
        let average = progress.timing_offsets_ms.iter().sum::<f64>() / progress.timing_offsets_ms.len() as f64;
        timing_gauge(ui, average);
        ui.label(if average < -10.0 { "Your recorded hits tend to be early." } else if average > 10.0 { "Your recorded hits tend to be late." } else { "Your recorded timing is centered." });
        histogram(ui, &progress.timing_offsets_ms);
    }

    let velocity = progress.sessions.iter().enumerate().filter_map(|(i, p)| p.average_velocity.map(|v| ((i + 1) as f64, v))).collect::<Vec<_>>();
    let velocity_spread = progress.sessions.iter().enumerate().filter_map(|(i, p)| p.velocity_spread.map(|v| ((i + 1) as f64, v))).collect::<Vec<_>>();
    if !velocity.is_empty() {
        line_chart(ui, "Average MIDI velocity over sessions", "Session", "Velocity", &velocity, Some((0.0, 127.0)));
        line_chart(ui, "Velocity consistency (spread)", "Session", "Spread", &velocity_spread, None);
        ui.weak("Lower velocity spread means more even MIDI velocity; higher velocity is not inherently better.");
    }

    ui.add_space(12.0); ui.heading("PER-DRUM PROGRESS");
    if progress.per_drum.is_empty() { ui.label("No mapped drum history is available."); }
    for (component, points) in &progress.per_drum {
        let data = points.iter().enumerate().map(|(i, p)| ((i + 1) as f64, p.accuracy)).collect::<Vec<_>>();
        line_chart(ui, component.instrument_label(), "Session", "Accuracy %", &data, Some((0.0, 100.0)));
        let timing = points.iter().filter_map(|p| p.average_timing_offset_ms).collect::<Vec<_>>();
        if timing.len() >= 2 { ui.label(format!("Average timing: {:+.1} ms", timing.iter().sum::<f64>() / timing.len() as f64)); }
    }
    ui.add_space(12.0); ui.heading("INSIGHTS");
    if let Some(weakest) = &progress.weakest_drum { ui.strong(format!("Current weakness: {} — {:.1}% weighted historical accuracy", weakest.component.instrument_label(), weakest.average_accuracy)); }
    else { ui.label("Not enough data to identify a weakness yet."); }
    if progress.insights.is_empty() { ui.label("More sessions are needed for measurable trend insights."); }
    for insight in &progress.insights { ui.label(format!("• {insight}")); }
}

fn card(ui: &mut egui::Ui, label: &str, value: &str) { egui::Frame::group(ui.style()).inner_margin(12.0).show(ui, |ui| { ui.strong(label); ui.label(egui::RichText::new(value).strong().size(24.0)); }); }

fn line_chart(ui: &mut egui::Ui, title: &str, x_label: &str, y_label: &str, points: &[(f64, f64)], y_bounds: Option<(f64, f64)>) {
    chart(ui, title, x_label, y_label, points, y_bounds, true);
}
fn scatter_chart(ui: &mut egui::Ui, title: &str, x_label: &str, y_label: &str, points: &[(f64, f64)], y_bounds: Option<(f64, f64)>) {
    chart(ui, title, x_label, y_label, points, y_bounds, false);
}
fn chart(ui: &mut egui::Ui, title: &str, x_label: &str, y_label: &str, points: &[(f64, f64)], y_bounds: Option<(f64, f64)>, connect: bool) {
    ui.add_space(12.0); ui.heading(title);
    if points.is_empty() { ui.label("Not enough data yet."); return; }
    let width = ui.available_width().min(900.0).max(300.0); let height = 230.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover()); let painter = ui.painter_at(rect);
    let plot = rect.shrink2(egui::vec2(42.0, 28.0)); painter.rect_stroke(plot, 0.0, egui::Stroke::new(1.0_f32, egui::Color32::from_gray(90)), egui::StrokeKind::Inside);
    let mut min_x = points.iter().map(|p| p.0).fold(f64::INFINITY, f64::min); let mut max_x = points.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
    let (mut min_y, mut max_y) = y_bounds.unwrap_or_else(|| (points.iter().map(|p| p.1).fold(f64::INFINITY, f64::min), points.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max)));
    if min_x == max_x { min_x -= 1.0; max_x += 1.0; } if min_y == max_y { min_y -= 1.0; max_y += 1.0; }
    let map = |(x, y): (f64, f64)| egui::pos2(plot.left() + ((x-min_x)/(max_x-min_x)) as f32*plot.width(), plot.bottom() - ((y-min_y)/(max_y-min_y)) as f32*plot.height());
    let screen = points.iter().copied().map(map).collect::<Vec<_>>();
    if connect && screen.len() > 1 { painter.add(egui::Shape::line(screen.clone(), egui::Stroke::new(2.0_f32, egui::Color32::from_rgb(80, 180, 235)))); }
    for point in screen { painter.circle_filled(point, 4.5, egui::Color32::from_rgb(110, 210, 255)); }
    painter.text(egui::pos2(plot.left(), rect.bottom()-18.0), egui::Align2::LEFT_CENTER, format!("{min_x:.0}"), egui::FontId::proportional(14.0), egui::Color32::LIGHT_GRAY);
    painter.text(egui::pos2(plot.right(), rect.bottom()-18.0), egui::Align2::RIGHT_CENTER, format!("{max_x:.0}  {x_label}"), egui::FontId::proportional(14.0), egui::Color32::LIGHT_GRAY);
    painter.text(egui::pos2(rect.left()+2.0, plot.top()), egui::Align2::LEFT_TOP, format!("{max_y:.0}"), egui::FontId::proportional(14.0), egui::Color32::LIGHT_GRAY);
    painter.text(egui::pos2(rect.left()+2.0, plot.bottom()), egui::Align2::LEFT_BOTTOM, format!("{min_y:.0}\n{y_label}"), egui::FontId::proportional(14.0), egui::Color32::LIGHT_GRAY);
}

fn timing_gauge(ui: &mut egui::Ui, average: f64) {
    let width = ui.available_width().min(700.0).max(300.0); let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 65.0), egui::Sense::hover()); let painter = ui.painter_at(rect); let line = rect.shrink2(egui::vec2(20.0, 20.0)); let y = line.center().y;
    painter.line_segment([egui::pos2(line.left(), y), egui::pos2(line.right(), y)], egui::Stroke::new(2.0_f32, egui::Color32::GRAY));
    let clamped = average.clamp(-100.0, 100.0); let x = line.center().x + (clamped as f32 / 100.0) * line.width()/2.0; painter.circle_filled(egui::pos2(x,y), 7.0, egui::Color32::from_rgb(110,210,255));
    painter.text(egui::pos2(line.left(), line.bottom()+4.0), egui::Align2::LEFT_TOP, "Early -100", egui::FontId::proportional(14.0), egui::Color32::LIGHT_GRAY); painter.text(egui::pos2(line.center().x, line.bottom()+4.0), egui::Align2::CENTER_TOP, format!("{average:+.1} ms"), egui::FontId::proportional(14.0), egui::Color32::WHITE); painter.text(egui::pos2(line.right(), line.bottom()+4.0), egui::Align2::RIGHT_TOP, "Late +100 ms", egui::FontId::proportional(14.0), egui::Color32::LIGHT_GRAY);
}

fn histogram(ui: &mut egui::Ui, values: &[f64]) {
    ui.heading("Timing distribution"); const BINS: usize = 20;
    let extent = values.iter().map(|value| value.abs()).fold(50.0_f64, f64::max).ceil();
    let min = -extent; let max = extent; let mut bins = [0usize; BINS];
    for value in values { let index = (((value.clamp(min, max) - min)/(max-min))*BINS as f64).floor() as usize; bins[index.min(BINS-1)] += 1; }
    let peak = *bins.iter().max().unwrap_or(&1); let width = ui.available_width().min(900.0).max(300.0); let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 180.0), egui::Sense::hover()); let painter = ui.painter_at(rect); let plot = rect.shrink2(egui::vec2(30.0, 24.0));
    for (i, count) in bins.iter().enumerate() { let x0 = plot.left()+i as f32/BINS as f32*plot.width(); let x1 = plot.left()+(i+1) as f32/BINS as f32*plot.width(); let h = *count as f32/peak.max(1) as f32*plot.height(); painter.rect_filled(egui::Rect::from_min_max(egui::pos2(x0+1.0,plot.bottom()-h),egui::pos2(x1-1.0,plot.bottom())),0.0,egui::Color32::from_rgb(80,180,235)); }
    painter.text(plot.left_bottom(),egui::Align2::LEFT_TOP,format!("{min:.0} ms"),egui::FontId::proportional(14.0),egui::Color32::LIGHT_GRAY); painter.text(plot.center_bottom(),egui::Align2::CENTER_TOP,"0",egui::FontId::proportional(14.0),egui::Color32::LIGHT_GRAY); painter.text(plot.right_bottom(),egui::Align2::RIGHT_TOP,format!("+{max:.0} ms"),egui::FontId::proportional(14.0),egui::Color32::LIGHT_GRAY);
}
