//! Phase 3A MIDI-file inspection view. Parsing remains in `drums-core`.

use drums_core::{MidiFile, MidiTiming};
use eframe::egui;
use crate::{DrumApp, View};

pub fn show(app: &mut DrumApp, ui: &mut egui::Ui) {
    ui.add_space(6.0);
    ui.horizontal(|ui| { if ui.button("Import MIDI...").clicked() { app.import_midi_clicked(); } if ui.button("Back to Monitor").clicked() { app.view = View::Monitor; } });
    if let Some(error) = &app.midi_import_error { ui.add_space(8.0); ui.colored_label(egui::Color32::from_rgb(240, 120, 120), error); }
    let Some(file) = app.imported_midi.as_ref() else { ui.add_space(12.0); ui.heading("MIDI FILE INSPECTION"); ui.separator(); ui.label("Choose a .mid or .midi file to inspect its tracks and drum events."); ui.weak("This view imports and inspects data only; it does not play MIDI files."); return; };
    metadata(ui, file);
    ui.add_space(14.0); ui.heading("TRACKS"); ui.separator();
    for track in &file.tracks {
        let title = format!("Track {} - {}  ({} events, {} notes, channels: {}){}", track.index + 1, track.name.as_deref().unwrap_or("Unnamed"), track.event_count, track.note_event_count, channels(&track.channels), if track.likely_drum { "  * likely drums" } else { "" });
        ui.selectable_value(&mut app.selected_drum_track, Some(track.index), title);
    }
    if file.suggested_drum_track().is_none() { ui.weak("No obvious drum track was detected. Select a track to inspect manually."); } else { ui.weak("Likely drum tracks are suggestions; choose any track to override them."); }
    ui.add_space(14.0); ui.heading("DRUM EVENTS"); ui.separator();
    let Some(selected_track) = app.selected_drum_track else { ui.weak("Select a track above."); return; };
    let expected = file.expected_notes_for_track(selected_track, app.active_config.as_ref());
    if expected.is_empty() { ui.weak("This track contains no Note On events with velocity above zero."); return; }
    egui::Grid::new("midi_file_events").striped(true).num_columns(7).spacing([16.0, 4.0]).show(ui, |ui| {
        ui.strong("Time"); ui.strong("Tick"); ui.strong("MIDI"); ui.strong("Component"); ui.strong("Target"); ui.strong("Velocity"); ui.strong("Channel"); ui.end_row();
        for event in expected.iter().take(1_000) {
            ui.monospace(format!("{:.3}s", event.timestamp_seconds));
            ui.monospace(event.tick.to_string());
            ui.monospace(event.midi_note.to_string());
            ui.monospace(event.drum_component.map(|component| component.label()).unwrap_or("Unknown"));
            ui.monospace(event.target_component.map(|component| component.label()).unwrap_or("Not configured"));
            ui.monospace(event.velocity.to_string());
            ui.monospace((event.channel + 1).to_string());
            ui.end_row();
        }
    });
    if expected.len() > 1_000 { ui.weak(format!("Showing the first 1,000 of {} note events.", expected.len())); }
    if app.active_config.is_none() { ui.weak("General MIDI components are shown directly. Connect a configured kit to see physical targets."); }
}

fn metadata(ui: &mut egui::Ui, file: &MidiFile) {
    ui.heading("MIDI FILE"); ui.separator();
    egui::Grid::new("midi_file_metadata").num_columns(2).spacing([24.0, 4.0]).show(ui, |ui| {
        ui.label("Filename:"); ui.monospace(&file.filename); ui.end_row(); ui.label("Format:"); ui.monospace(file.format.label()); ui.end_row(); ui.label("Tracks:"); ui.monospace(file.tracks.len().to_string()); ui.end_row(); ui.label("Timing:"); ui.monospace(timing_text(file.timing)); ui.end_row();
        ui.label("Tempo:"); ui.monospace(file.tempo_events.first().map(|tempo| format!("{:.1} BPM{}", tempo.bpm(), if file.tempo_events.len() > 1 { " (tempo changes present)" } else { "" })).unwrap_or_else(|| "Not specified".to_string())); ui.end_row();
        ui.label("Time Signature:"); ui.monospace(file.time_signature_events.first().map(|signature| format!("{}/{}{}", signature.numerator, signature.denominator(), if file.time_signature_events.len() > 1 { " (changes present)" } else { "" })).unwrap_or_else(|| "Not specified".to_string())); ui.end_row();
        ui.label("Duration:"); ui.monospace(file.duration_seconds.map(duration_text).unwrap_or_else(|| "Not available".to_string())); ui.end_row();
    });
}
fn timing_text(timing: MidiTiming) -> String { match timing { MidiTiming::TicksPerQuarter(ticks) => format!("{ticks} ticks per quarter note"), MidiTiming::Timecode { frames_per_second, ticks_per_frame } => format!("SMPTE: {frames_per_second} fps, {ticks_per_frame} ticks/frame") } }
fn duration_text(seconds: f64) -> String { format!("{:02}:{:02}", (seconds / 60.0).floor() as u64, (seconds as u64) % 60) }
fn channels(channels: &[u8]) -> String { if channels.is_empty() { "none".to_string() } else { channels.iter().map(|channel| (channel + 1).to_string()).collect::<Vec<_>>().join(", ") } }
