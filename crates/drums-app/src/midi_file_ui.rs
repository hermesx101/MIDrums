//! Phase 3A MIDI-file inspection view. Parsing remains in `drums-core`.

use drums_core::{count_tokens, MidiFile, MidiTiming, PracticeResultKind, Subdivision, TargetState, TransportState};
use eframe::egui;
use crate::{DrumApp, View};

pub fn show(app: &mut DrumApp, ui: &mut egui::Ui) {
    ui.add_space(6.0);
    ui.horizontal(|ui| { if ui.button("Import MIDI...").clicked() { app.import_midi_clicked(); } if ui.button("Back to Monitor").clicked() { app.view = View::Monitor; } });
    if let Some(error) = &app.midi_import_error { ui.add_space(8.0); ui.colored_label(egui::Color32::from_rgb(240, 120, 120), error); }
    let Some(file) = app.imported_midi.as_ref() else { ui.add_space(12.0); ui.heading("MIDI FILE INSPECTION"); ui.separator(); ui.label("Choose a .mid or .midi file to inspect its tracks and drum events."); ui.weak("This view imports and inspects data only; it does not play MIDI files."); return; };
    metadata(ui, file);
    ui.add_space(14.0); ui.heading("TRACKS"); ui.separator();
    let mut selected_track_changed = false;
    for track in &file.tracks {
        let title = format!("Track {} - {}  ({} events, {} notes, channels: {}){}", track.index + 1, track.name.as_deref().unwrap_or("Unnamed"), track.event_count, track.note_event_count, channels(&track.channels), if track.likely_drum { "  * likely drums" } else { "" });
        selected_track_changed |= ui.selectable_value(&mut app.selected_drum_track, Some(track.index), title).changed();
    }
    if file.suggested_drum_track().is_none() { ui.weak("No obvious drum track was detected. Select a track to inspect manually."); } else { ui.weak("Likely drum tracks are suggestions; choose any track to override them."); }
    ui.add_space(14.0); ui.heading("DRUM EVENTS"); ui.separator();
    let Some(selected_track) = app.selected_drum_track else { ui.weak("Select a track above."); return; };
    let expected = file.expected_notes_for_track(selected_track, app.active_config.as_ref());
    transport_controls(app, ui);
    metronome_controls(app, ui);
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
    if selected_track_changed { app.rebuild_timeline(); }
}

fn transport_controls(app: &mut DrumApp, ui: &mut egui::Ui) {
    let Some(transport) = app.transport.as_mut() else { return; };
    ui.add_space(8.0);
    ui.heading("TRANSPORT");
    ui.horizontal(|ui| {
        if ui.button("Play").clicked() { if transport.state() == TransportState::Stopped && transport.current_tick() == 0 { app.metronome.reset(); if let Some(session) = app.practice_session.as_mut() { session.reset(); } } else { app.metronome.sync_to_transport(transport); } transport.play(); }
        if ui.button("Pause").clicked() { transport.pause(); app.metronome.sync_to_transport(transport); }
        if ui.button("Stop").clicked() { transport.stop(); app.metronome.reset(); if let Some(session) = app.practice_session.as_mut() { session.on_stop(); } }
        if ui.button("Beginning").clicked() { transport.seek(0); app.metronome.sync_to_transport(transport); if let Some(session) = app.practice_session.as_mut() { session.on_seek_to_tick(transport.current_tick(), transport); } }
        if ui.button("End").clicked() { transport.seek_to_end(); app.metronome.sync_to_transport(transport); if let Some(session) = app.practice_session.as_mut() { session.on_seek_to_tick(transport.current_tick(), transport); } }
    });
    let mut requested_tick = transport.current_tick();
    if ui.add(egui::Slider::new(&mut requested_tick, 0..=transport.final_tick()).text("Seek tick")).changed() {
        transport.seek(requested_tick.min(i64::MAX as u64) as i64); app.metronome.sync_to_transport(transport); if let Some(session) = app.practice_session.as_mut() { session.on_seek_to_tick(transport.current_tick(), transport); }
    }
    let musical = transport.musical_position();
    let (numerator, denominator) = transport.position_map().time_signature_at_tick(transport.current_tick());
    let ppq = transport.position_map().ticks_per_quarter();
    egui::Grid::new("transport_position").num_columns(2).spacing([24.0, 4.0]).show(ui, |ui| {
        ui.label("State:"); ui.monospace(format!("{:?}", transport.state())); ui.end_row();
        ui.label("Musical position:"); ui.monospace(format!("Bar {}  Beat {}  + {}/{} ticks", musical.measure, musical.beat, musical.ticks_into_beat, musical.ticks_per_beat)); ui.end_row();
        ui.label("Elapsed:"); ui.monospace(format!("{} / {}", time_text(transport.elapsed_seconds()), time_text(transport.total_seconds()))); ui.end_row();
        ui.label("Current tick:"); ui.monospace(format!("{} / {}", transport.current_tick(), transport.final_tick())); ui.end_row();
        ui.label("Time signature:"); ui.monospace(format!("{numerator}/{denominator}")); ui.end_row();
        ui.label("PPQ:"); ui.monospace(ppq.map(|value| value.to_string()).unwrap_or_else(|| "SMPTE timing".to_string())); ui.end_row();
    });
    ui.add_space(10.0);
    ui.heading("TEMPO");
    ui.label(format!("Original MIDI: {:.1} BPM{}", transport.original_bpm(), if transport.tempo_scale() != 1.0 { " (preserved)" } else { "" }));
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Practice:").size(18.0));
        let decrement = ui.add_sized([44.0, 36.0], egui::Button::new(egui::RichText::new("−").size(24.0))).clicked();
        ui.label(egui::RichText::new(format!("{:.0} BPM", transport.practice_starting_bpm())).strong().size(26.0));
        let increment = ui.add_sized([44.0, 36.0], egui::Button::new(egui::RichText::new("+").size(24.0))).clicked();
        if decrement { transport.set_practice_bpm(transport.practice_starting_bpm() - 1.0); }
        if increment { transport.set_practice_bpm(transport.practice_starting_bpm() + 1.0); }
        if ui.button("Reset to MIDI tempo").clicked() { transport.reset_practice_tempo(); }
    });
    if transport.tempo_scale() != 1.0 { ui.weak(format!("Current effective tempo: {:.1} BPM (original current tempo: {:.1} BPM)", transport.current_bpm(), transport.original_bpm())); }
    if let Some(timeline) = &app.timeline { ui.weak(format!("Timeline model: {} expected events.", timeline.events.len())); }
    if let Some(session) = &app.practice_session {
        ui.add_space(6.0);
        ui.label(format!("Practice engine: {} pending • {} results • ±{} ms window", session.pending_count(), session.results().len(), session.matching_window().as_millis()));
        ui.monospace(format!("Current transport: tick {} • {}", transport.current_tick(), time_text(transport.elapsed_seconds())));
        ui.add_space(6.0);
        ui.strong("CURRENT TARGET");
        if let Some(target) = session.current_target(transport.current_tick(), transport) {
            let components = target.notes.iter().filter_map(|note| note.drum_component).map(|component| component.label()).collect::<Vec<_>>().join(" + ");
            let first_index = target.expected_indices[0];
            let last_index = *target.expected_indices.last().unwrap();
            ui.label(egui::RichText::new(components).strong().size(24.0));
            ui.monospace(if first_index == last_index { format!("Target #{} / {}", first_index + 1, session.expected().len()) } else { format!("Targets #{}-{} / {}", first_index + 1, last_index + 1, session.expected().len()) });
            ui.label(egui::RichText::new(if target.state == TargetState::Hittable { "NOW".to_string() } else { format!("In: {:.2} seconds", target.time_until_seconds.max(0.0)) }).strong().size(20.0));
            let approach = (1.0 - target.time_until_seconds.max(0.0) as f32 / 5.0).clamp(0.0, 1.0);
            ui.add(egui::ProgressBar::new(approach).desired_width(260.0).show_percentage());
            ui.monospace(format!("Tick {} • {} • {:+.1} ms", target.tick, precise_time_text(transport.effective_seconds_at_tick(target.tick)), target.time_until_seconds * 1_000.0));
            ui.strong("NEXT TARGET");
            if let Some((next_index, next)) = session.next_known_after(last_index) {
                ui.monospace(format!("#{} {} • MIDI {} • tick {} • {}", next_index, next.drum_component.unwrap().label(), next.midi_note, next.tick, precise_time_text(transport.effective_seconds_at_tick(next.tick))));
            } else { ui.monospace("None"); }
        } else { ui.monospace("No remaining matchable expected event"); }
        ui.add_space(6.0);
        ui.strong("LAST RESULT");
        if let Some(result) = session.results().last() {
            let expected = result.expected.as_ref().and_then(|note| note.drum_component).map(|value| value.label()).unwrap_or("—");
            let actual = result.actual.as_ref().and_then(|hit| hit.drum_component).map(|value| value.label()).unwrap_or("—");
            let timing = result.timing_difference_micros.map(|value| format!("{:+.1} ms", value as f64 / 1_000.0)).unwrap_or_else(|| "—".to_string());
            let result_label = match result.kind { PracticeResultKind::CorrectHit => "HIT", PracticeResultKind::WrongDrum => "WRONG DRUM", PracticeResultKind::Missed => "MISSED", PracticeResultKind::Ignored => "IGNORED" };
            ui.monospace(format!("{result_label}   Expected: {expected}   Actual: {actual}   Difference: {timing}"));
        } else { ui.monospace("No result yet"); }
        egui::CollapsingHeader::new("First expected events").show(ui, |ui| {
            for (index, note) in session.expected().iter().take(8).enumerate() {
                ui.monospace(format!("#{index}: {} • MIDI {} • tick {} • {:.3}s • track {} • channel {}", note.drum_component.map(|value| value.label()).unwrap_or("Unknown"), note.midi_note, note.tick, transport.effective_seconds_at_tick(note.tick), note.track + 1, note.channel + 1));
            }
        });
    }
    ui.separator();
}

fn metronome_controls(app: &mut DrumApp, ui: &mut egui::Ui) {
    let Some(transport) = app.transport.as_ref() else { return; };
    ui.add_space(10.0); ui.heading("METRONOME"); ui.separator();
    ui.checkbox(&mut app.metronome_enabled, "Metronome on");
    // `Metronome` has mutable state, so subdivision selection is applied via
    // independent controls rather than replacing the generator.
    ui.horizontal(|ui| {
        let auto_clicked = ui.selectable_label(app.subdivision_auto, "Auto").clicked();
        if auto_clicked {
            app.subdivision_auto = true;
            if let Some(suggestion) = app.subdivision_suggestion { app.metronome.set_subdivision(suggestion.subdivision); app.metronome.sync_to_transport(transport); }
        }
        for subdivision in [Subdivision::Quarter, Subdivision::Eighth, Subdivision::Sixteenth] {
            if ui.selectable_label(!app.subdivision_auto && app.metronome.subdivision() == subdivision, subdivision.label()).clicked() {
                app.subdivision_auto = false;
                app.metronome.set_subdivision(subdivision);
                app.metronome.sync_to_transport(transport);
            }
        }
        ui.weak(if app.click_audio.available() { "Synthesized click output available" } else { "Audio output unavailable" });
    });
    if let Some(suggestion) = app.subdivision_suggestion {
        ui.label(format!("Suggested: {} notes   •   Confidence: {}", suggestion.subdivision.label(), suggestion.confidence.label()));
        ui.weak("Auto analyzes MIDI tick positions only; it never moves or quantizes notes.");
    }
    let tokens = count_tokens(transport.position_map(), transport.current_tick(), app.metronome.subdivision());
    ui.horizontal_wrapped(|ui| { for token in tokens { if token.active { ui.colored_label(egui::Color32::from_rgb(120, 190, 255), egui::RichText::new(token.label).strong().size(22.0)); } else { ui.label(egui::RichText::new(token.label).size(20.0)); } } });
    ui.weak(format!("Tempo: {:.1} BPM  •  {} subdivision", transport.current_bpm(), app.metronome.subdivision().label()));
    ui.separator();
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
fn time_text(seconds: f64) -> String { format!("{:02}:{:05.2}", (seconds / 60.0).floor() as u64, seconds % 60.0) }
fn precise_time_text(seconds: f64) -> String { format!("{:02}:{:06.3}", (seconds / 60.0).floor() as u64, seconds % 60.0) }
fn channels(channels: &[u8]) -> String { if channels.is_empty() { "none".to_string() } else { channels.iter().map(|channel| (channel + 1).to_string()).collect::<Vec<_>>().join(", ") } }
