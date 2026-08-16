//! Phase 3A MIDI-file inspection view. Parsing remains in `drums-core`.

use crate::{DrumApp, View};
use drums_core::{
    count_tokens, MidiFile, MidiTiming, PracticePerformance, PracticeResultKind, Subdivision,
    TargetState, TimingTendency, TransportState,
};
use eframe::egui;

pub fn show(app: &mut DrumApp, ui: &mut egui::Ui) {
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if ui.button("Import MIDI...").clicked() {
            app.import_midi_clicked();
        }
        if ui.button("Back to Monitor").clicked() {
            app.view = View::Monitor;
        }
    });
    if let Some(error) = &app.midi_import_error {
        ui.add_space(8.0);
        ui.colored_label(egui::Color32::from_rgb(240, 120, 120), error);
    }
    let Some(file) = app.imported_midi.as_ref() else {
        ui.add_space(12.0);
        ui.heading("MIDI FILE INSPECTION");
        ui.separator();
        ui.label("Choose a .mid or .midi file to inspect its tracks and drum events.");
        ui.weak("This view imports and inspects data only; it does not play MIDI files.");
        return;
    };
    let mut selected_track_changed = false;
    egui::CollapsingHeader::new("MIDI File / Track Selection").show(ui, |ui| {
        metadata(ui, file);
        ui.add_space(8.0);
        ui.strong("TRACKS");
        for track in &file.tracks {
            let title = format!(
                "Track {} - {}  ({} events, {} notes, channels: {}){}",
                track.index + 1,
                track.name.as_deref().unwrap_or("Unnamed"),
                track.event_count,
                track.note_event_count,
                channels(&track.channels),
                if track.likely_drum {
                    "  * likely drums"
                } else {
                    ""
                }
            );
            selected_track_changed |= ui
                .selectable_value(&mut app.selected_drum_track, Some(track.index), title)
                .changed();
        }
        if file.suggested_drum_track().is_none() {
            ui.weak("No obvious drum track was detected. Select a track to inspect manually.");
        } else {
            ui.weak("Likely drum tracks are suggestions; choose any track to override them.");
        }
    });
    let Some(selected_track) = app.selected_drum_track else {
        ui.weak("Select a track above.");
        return;
    };
    let expected = file.expected_notes_for_track(selected_track, app.active_config.as_ref());
    transport_controls(app, ui);
    metronome_controls(app, ui);
    if expected.is_empty() {
        ui.weak("This track contains no Note On events with velocity above zero.");
        return;
    }
    egui::CollapsingHeader::new("Imported MIDI Events").show(ui, |ui| { egui::Grid::new("midi_file_events").striped(true).num_columns(7).spacing([16.0, 4.0]).show(ui, |ui| {
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
    });
    if selected_track_changed {
        app.rebuild_timeline();
    }
}

fn transport_controls(app: &mut DrumApp, ui: &mut egui::Ui) {
    let Some(transport) = app.transport.as_mut() else {
        return;
    };
    if let Some(since) = app.displayed_result_since {
        ui.ctx().request_repaint_after(
            std::time::Duration::from_millis(1_200).saturating_sub(since.elapsed()),
        );
    }
    ui.add_space(8.0);
    ui.heading("PRACTICE");

    if let Some(session) = &app.practice_session {
        let performance = PracticePerformance::from_session(session);
        let total = session.matchable_count();
        let completed = session.finalized_matchable_count();
        let target = session.current_target(transport.current_tick(), transport);
        let state = if target.is_none() && total > 0 && completed == total {
            "SONG COMPLETE"
        } else {
            match transport.state() {
                TransportState::Playing => "PLAYING",
                TransportState::Paused => "PAUSED",
                TransportState::Stopped => "READY",
            }
        };
        ui.label(egui::RichText::new(state).strong().size(18.0));

        if let Some(target) = target {
            let components = target
                .notes
                .iter()
                .filter_map(|note| note.drum_component)
                .map(|component| component.label())
                .collect::<Vec<_>>()
                .join(" + ");
            ui.add_space(6.0);
            ui.weak("CURRENT TARGET");
            ui.label(egui::RichText::new(components).strong().size(34.0));
            ui.label(
                egui::RichText::new(if target.state == TargetState::Hittable {
                    "NOW".to_string()
                } else {
                    format!("In {:.2} s", target.time_until_seconds.max(0.0))
                })
                .strong()
                .size(24.0),
            );
            let approach = (1.0 - target.time_until_seconds.max(0.0) as f32 / 5.0).clamp(0.0, 1.0);
            ui.add(
                egui::ProgressBar::new(approach)
                    .desired_width(ui.available_width().min(440.0))
                    .text(if target.state == TargetState::Hittable {
                        "HIT WINDOW"
                    } else {
                        "Target timing"
                    }),
            );
            ui.label(if target.ordinal_start == target.ordinal_end {
                format!(
                    "Target {} / {}",
                    target.ordinal_start, target.total_matchable
                )
            } else {
                format!(
                    "Targets {}-{} / {}",
                    target.ordinal_start, target.ordinal_end, target.total_matchable
                )
            });
            ui.add(
                egui::ProgressBar::new(completed as f32 / total.max(1) as f32)
                    .desired_width(ui.available_width().min(440.0)),
            );

            ui.add_space(8.0);
            ui.columns(2, |columns| {
                columns[0].weak("LAST RESULT");
                if let Some(result) = &app.displayed_practice_result {
                    show_last_result(&mut columns[0], result);
                } else {
                    columns[0].label("Waiting for input…");
                }
                columns[1].weak("NEXT");
                if let Some(next) = session.next_target_after(
                    *target.expected_indices.last().unwrap(),
                    transport.current_tick(),
                    transport,
                ) {
                    let components = next
                        .notes
                        .iter()
                        .filter_map(|note| note.drum_component)
                        .map(|component| component.label())
                        .collect::<Vec<_>>()
                        .join(" + ");
                    columns[1].label(egui::RichText::new(components).strong().size(20.0));
                    columns[1].label(format!("in {:.2} s", next.time_until_seconds.max(0.0)));
                } else {
                    columns[1].label("—");
                }
            });
        } else if total > 0 && completed == total {
            show_performance_summary(ui, &performance);
        } else {
            ui.label("No matchable drum targets in the selected track.");
        }
        if completed < total {
            show_live_performance(ui, &performance);
        }
    }

    ui.add_space(10.0);
    ui.horizontal_wrapped(|ui| {
        ui.label(if app.connected {
            format!(
                "MIDI: ● {}",
                app.backend.connected_port_name().unwrap_or("Connected")
            )
        } else {
            "MIDI: ○ Disconnected".to_string()
        });
        ui.separator();
        ui.label(format!(
            "Metronome: {}",
            if app.metronome_enabled { "ON" } else { "OFF" }
        ));
        ui.label(format!(
            "Subdivision: {}",
            app.metronome.subdivision().label()
        ));
    });
    if let Some(hit) = &app.last_live_hit {
        ui.label(format!(
            "LAST INPUT: {}",
            hit.drum_component
                .map(|component| component.label())
                .unwrap_or("Unmapped")
        ));
    }

    ui.add_space(8.0);
    ui.strong("TRANSPORT");
    ui.horizontal(|ui| {
        if ui.button("Play").clicked() {
            if transport.state() == TransportState::Stopped && transport.current_tick() == 0 {
                app.metronome.reset();
                if let Some(session) = app.practice_session.as_mut() {
                    session.reset();
                }
            } else {
                app.metronome.sync_to_transport(transport);
            }
            transport.play();
        }
        if ui.button("Pause").clicked() {
            transport.pause();
            app.metronome.sync_to_transport(transport);
        }
        if ui.button("Stop").clicked() {
            transport.stop();
            app.metronome.reset();
            if let Some(session) = app.practice_session.as_mut() {
                session.on_stop();
            }
        }
        if ui.button("Beginning").clicked() {
            transport.seek(0);
            app.metronome.sync_to_transport(transport);
            if let Some(session) = app.practice_session.as_mut() {
                session.on_seek_to_tick(transport.current_tick(), transport);
            }
        }
        if ui.button("End").clicked() {
            transport.seek_to_end();
            app.metronome.sync_to_transport(transport);
            if let Some(session) = app.practice_session.as_mut() {
                session.on_seek_to_tick(transport.current_tick(), transport);
            }
        }
    });
    let mut requested_tick = transport.current_tick();
    if ui
        .add(egui::Slider::new(&mut requested_tick, 0..=transport.final_tick()).text("Seek tick"))
        .changed()
    {
        transport.seek(requested_tick.min(i64::MAX as u64) as i64);
        app.metronome.sync_to_transport(transport);
        if let Some(session) = app.practice_session.as_mut() {
            session.on_seek_to_tick(transport.current_tick(), transport);
        }
    }
    let musical = transport.musical_position();
    let (numerator, denominator) = transport
        .position_map()
        .time_signature_at_tick(transport.current_tick());
    let ppq = transport.position_map().ticks_per_quarter();
    ui.label(format!(
        "{} / {}   •   Bar {} • Beat {}",
        time_text(transport.elapsed_seconds()),
        time_text(transport.total_seconds()),
        musical.measure,
        musical.beat
    ));
    ui.add_space(10.0);
    ui.heading("TEMPO");
    ui.label(format!(
        "Original MIDI: {:.1} BPM{}",
        transport.original_bpm(),
        if transport.tempo_scale() != 1.0 {
            " (preserved)"
        } else {
            ""
        }
    ));
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Practice:").size(18.0));
        let decrement = ui
            .add_sized(
                [44.0, 36.0],
                egui::Button::new(egui::RichText::new("−").size(24.0)),
            )
            .clicked();
        ui.label(
            egui::RichText::new(format!("{:.0} BPM", transport.practice_starting_bpm()))
                .strong()
                .size(26.0),
        );
        let increment = ui
            .add_sized(
                [44.0, 36.0],
                egui::Button::new(egui::RichText::new("+").size(24.0)),
            )
            .clicked();
        if decrement {
            transport.set_practice_bpm(transport.practice_starting_bpm() - 1.0);
        }
        if increment {
            transport.set_practice_bpm(transport.practice_starting_bpm() + 1.0);
        }
        if ui.button("Reset to MIDI tempo").clicked() {
            transport.reset_practice_tempo();
        }
    });
    if transport.tempo_scale() != 1.0 {
        ui.weak(format!(
            "Current effective tempo: {:.1} BPM (original current tempo: {:.1} BPM)",
            transport.current_bpm(),
            transport.original_bpm()
        ));
    }
    egui::CollapsingHeader::new("Debug / Details").show(ui, |ui| {
        ui.monospace(format!("State: {:?}", transport.state()));
        ui.monospace(format!(
            "Transport tick: {} / {}",
            transport.current_tick(),
            transport.final_tick()
        ));
        ui.monospace(format!(
            "Time signature: {numerator}/{denominator} • PPQ: {}",
            ppq.map(|value| value.to_string())
                .unwrap_or_else(|| "SMPTE".to_string())
        ));
        ui.monospace(format!(
            "Original BPM: {:.1} • Effective BPM: {:.1}",
            transport.original_bpm(),
            transport.current_bpm()
        ));
        if let Some(session) = &app.practice_session {
            ui.monospace(format!(
                "Pending: {} • Results: {} • Match window: ±{} ms",
                session.pending_count(),
                session.results().len(),
                session.matching_window().as_millis()
            ));
            if let Some(target) = session.current_target(transport.current_tick(), transport) {
                ui.monospace(format!(
                    "Expected indices: {:?} • tick {} • distance {:+.1} ms",
                    target.expected_indices,
                    target.tick,
                    target.time_until_seconds * 1_000.0
                ));
            }
            if let Some(result) = session.results().last() {
                ui.monospace(format!(
                    "Last raw result: {:?} • ticks {:?} • micros {:?}",
                    result.kind, result.timing_difference_ticks, result.timing_difference_micros
                ));
            }
        }
        if let Some(hit) = &app.last_live_hit {
            ui.monospace(format!(
                "Last input: MIDI {} • channel {} • velocity {} • tick {}",
                hit.received_midi_note,
                hit.channel + 1,
                hit.velocity,
                hit.transport_tick
            ));
        }
    });
    ui.separator();
}

fn show_last_result(ui: &mut egui::Ui, result: &drums_core::PracticeResult) {
    let expected = result
        .expected
        .as_ref()
        .and_then(|note| note.drum_component)
        .map(|value| value.label());
    let actual = result
        .actual
        .as_ref()
        .and_then(|hit| hit.drum_component)
        .map(|value| value.label());
    let timing_micros = result.timing_difference_micros;
    match result.kind {
        PracticeResultKind::CorrectHit => {
            ui.label(
                egui::RichText::new(format!("✓ {}", actual.unwrap_or("HIT")))
                    .strong()
                    .size(20.0),
            );
            if let Some(judgment) = result.timing_judgment {
                ui.label(
                    egui::RichText::new(format!("{:?}", judgment).to_uppercase())
                        .strong()
                        .size(20.0),
                );
                if let Some(value) = timing_micros {
                    let milliseconds = value.unsigned_abs() as f64 / 1_000.0;
                    ui.label(match judgment {
                        drums_core::TimingJudgment::Early => format!("{milliseconds:.1} ms early"),
                        drums_core::TimingJudgment::Good => {
                            format!("{milliseconds:.1} ms from target")
                        }
                        drums_core::TimingJudgment::Late => format!("{milliseconds:.1} ms late"),
                    });
                }
            }
        }
        PracticeResultKind::WrongDrum => {
            ui.strong("✕ WRONG DRUM");
            ui.label(format!("Expected: {}", expected.unwrap_or("—")));
            ui.label(format!("Actual: {}", actual.unwrap_or("Unmapped")));
        }
        PracticeResultKind::Missed => {
            ui.strong("✕ MISSED");
            ui.label(format!("Expected: {}", expected.unwrap_or("Unknown")));
        }
        PracticeResultKind::Ignored => {
            ui.strong("IGNORED");
            ui.label(format!("Actual: {}", actual.unwrap_or("Unmapped")));
        }
    }
    if result.kind == PracticeResultKind::WrongDrum {
        if let Some(value) = timing_micros {
            ui.monospace(format!("{:+.1} ms", value as f64 / 1_000.0));
        }
    }
}

fn show_live_performance(ui: &mut egui::Ui, performance: &PracticePerformance) {
    ui.add_space(10.0);
    ui.weak("SESSION");
    ui.horizontal_wrapped(|ui| {
        ui.label(format!("Accuracy: {:.1}%", performance.accuracy_percent()));
        ui.label(format!(
            "Hits: {} / {} finalized",
            performance.correct_hits, performance.finalized_expected
        ));
        ui.label(format!("Good: {}", performance.good_hits));
        ui.label(format!("Early: {}", performance.early_hits));
        ui.label(format!("Late: {}", performance.late_hits));
        ui.label(format!("Missed: {}", performance.missed));
        ui.label(format!("Wrong: {}", performance.wrong_drums));
        ui.label(format!("Extra: {}", performance.ignored_extra_hits));
    });
}

fn show_performance_summary(ui: &mut egui::Ui, performance: &PracticePerformance) {
    ui.add_space(10.0);
    ui.label(egui::RichText::new("PRACTICE COMPLETE").strong().size(30.0));
    ui.label("Practice finished");
    ui.label(
        egui::RichText::new(format!("{:.1}% accuracy", performance.accuracy_percent()))
            .strong()
            .size(26.0),
    );
    ui.label(format!(
        "{} / {} correct hits",
        performance.correct_hits, performance.total_expected
    ));
    egui::Grid::new("performance_summary")
        .num_columns(2)
        .spacing([24.0, 4.0])
        .show(ui, |ui| {
            ui.label("GOOD");
            ui.monospace(performance.good_hits.to_string());
            ui.end_row();
            ui.label("EARLY");
            ui.monospace(performance.early_hits.to_string());
            ui.end_row();
            ui.label("LATE");
            ui.monospace(performance.late_hits.to_string());
            ui.end_row();
            ui.label("Good percentage");
            ui.monospace(format!("{:.1}%", performance.good_percent()));
            ui.end_row();
            ui.label("Average timing error");
            ui.monospace(
                performance
                    .mean_absolute_error_ms
                    .map(|value| format!("{value:.1} ms"))
                    .unwrap_or_else(|| "—".to_string()),
            );
            ui.end_row();
            ui.label("Tendency");
            ui.monospace(tendency_text(performance.tendency));
            ui.end_row();
            ui.label("Missed");
            ui.monospace(performance.missed.to_string());
            ui.end_row();
            ui.label("Wrong drums");
            ui.monospace(performance.wrong_drums.to_string());
            ui.end_row();
            ui.label("Extra hits");
            ui.monospace(performance.ignored_extra_hits.to_string());
            ui.end_row();
        });
    ui.add_space(8.0);
    ui.strong("DRUM BREAKDOWN");
    egui::Grid::new("component_performance")
        .striped(true)
        .num_columns(4)
        .spacing([18.0, 4.0])
        .show(ui, |ui| {
            ui.strong("Drum");
            ui.strong("Hits");
            ui.strong("Accuracy");
            ui.strong("Avg error");
            ui.end_row();
            for (component, stats) in &performance.per_component {
                ui.label(component.label());
                ui.monospace(format!("{} / {}", stats.correct_hits, stats.total_expected));
                ui.monospace(format!("{:.1}%", stats.accuracy_percent()));
                ui.monospace(
                    stats
                        .mean_absolute_error_ms
                        .map(|value| format!("{value:.1} ms"))
                        .unwrap_or_else(|| "—".to_string()),
                );
                ui.end_row();
            }
        });
}

fn tendency_text(tendency: TimingTendency) -> String {
    match tendency {
        TimingTendency::Early(value) => format!("{value:.1} ms early"),
        TimingTendency::OnTime => "On time".to_string(),
        TimingTendency::Late(value) => format!("{value:.1} ms late"),
    }
}

fn metronome_controls(app: &mut DrumApp, ui: &mut egui::Ui) {
    let Some(transport) = app.transport.as_ref() else {
        return;
    };
    ui.add_space(10.0);
    ui.heading("METRONOME");
    ui.separator();
    ui.checkbox(&mut app.metronome_enabled, "Metronome on");
    // `Metronome` has mutable state, so subdivision selection is applied via
    // independent controls rather than replacing the generator.
    ui.horizontal(|ui| {
        let auto_clicked = ui.selectable_label(app.subdivision_auto, "Auto").clicked();
        if auto_clicked {
            app.subdivision_auto = true;
            if let Some(suggestion) = app.subdivision_suggestion {
                app.metronome.set_subdivision(suggestion.subdivision);
                app.metronome.sync_to_transport(transport);
            }
        }
        for subdivision in [
            Subdivision::Quarter,
            Subdivision::Eighth,
            Subdivision::Sixteenth,
        ] {
            if ui
                .selectable_label(
                    !app.subdivision_auto && app.metronome.subdivision() == subdivision,
                    subdivision.label(),
                )
                .clicked()
            {
                app.subdivision_auto = false;
                app.metronome.set_subdivision(subdivision);
                app.metronome.sync_to_transport(transport);
            }
        }
        ui.weak(if app.click_audio.available() {
            "Synthesized click output available"
        } else {
            "Audio output unavailable"
        });
    });
    if let Some(suggestion) = app.subdivision_suggestion {
        ui.label(format!(
            "Suggested: {} notes   •   Confidence: {}",
            suggestion.subdivision.label(),
            suggestion.confidence.label()
        ));
        ui.weak("Auto analyzes MIDI tick positions only; it never moves or quantizes notes.");
    }
    let tokens = count_tokens(
        transport.position_map(),
        transport.current_tick(),
        app.metronome.subdivision(),
    );
    ui.horizontal_wrapped(|ui| {
        for token in tokens {
            if token.active {
                ui.colored_label(
                    egui::Color32::from_rgb(120, 190, 255),
                    egui::RichText::new(token.label).strong().size(22.0),
                );
            } else {
                ui.label(egui::RichText::new(token.label).size(20.0));
            }
        }
    });
    ui.weak(format!(
        "Tempo: {:.1} BPM  •  {} subdivision",
        transport.current_bpm(),
        app.metronome.subdivision().label()
    ));
    ui.separator();
}

fn metadata(ui: &mut egui::Ui, file: &MidiFile) {
    ui.heading("MIDI FILE");
    ui.separator();
    egui::Grid::new("midi_file_metadata")
        .num_columns(2)
        .spacing([24.0, 4.0])
        .show(ui, |ui| {
            ui.label("Filename:");
            ui.monospace(&file.filename);
            ui.end_row();
            ui.label("Format:");
            ui.monospace(file.format.label());
            ui.end_row();
            ui.label("Tracks:");
            ui.monospace(file.tracks.len().to_string());
            ui.end_row();
            ui.label("Timing:");
            ui.monospace(timing_text(file.timing));
            ui.end_row();
            ui.label("Tempo:");
            ui.monospace(
                file.tempo_events
                    .first()
                    .map(|tempo| {
                        format!(
                            "{:.1} BPM{}",
                            tempo.bpm(),
                            if file.tempo_events.len() > 1 {
                                " (tempo changes present)"
                            } else {
                                ""
                            }
                        )
                    })
                    .unwrap_or_else(|| "Not specified".to_string()),
            );
            ui.end_row();
            ui.label("Time Signature:");
            ui.monospace(
                file.time_signature_events
                    .first()
                    .map(|signature| {
                        format!(
                            "{}/{}{}",
                            signature.numerator,
                            signature.denominator(),
                            if file.time_signature_events.len() > 1 {
                                " (changes present)"
                            } else {
                                ""
                            }
                        )
                    })
                    .unwrap_or_else(|| "Not specified".to_string()),
            );
            ui.end_row();
            ui.label("Duration:");
            ui.monospace(
                file.duration_seconds
                    .map(duration_text)
                    .unwrap_or_else(|| "Not available".to_string()),
            );
            ui.end_row();
        });
}
fn timing_text(timing: MidiTiming) -> String {
    match timing {
        MidiTiming::TicksPerQuarter(ticks) => format!("{ticks} ticks per quarter note"),
        MidiTiming::Timecode {
            frames_per_second,
            ticks_per_frame,
        } => format!("SMPTE: {frames_per_second} fps, {ticks_per_frame} ticks/frame"),
    }
}
fn duration_text(seconds: f64) -> String {
    format!(
        "{:02}:{:02}",
        (seconds / 60.0).floor() as u64,
        (seconds as u64) % 60
    )
}
fn time_text(seconds: f64) -> String {
    format!(
        "{:02}:{:05.2}",
        (seconds / 60.0).floor() as u64,
        seconds % 60.0
    )
}
fn channels(channels: &[u8]) -> String {
    if channels.is_empty() {
        "none".to_string()
    } else {
        channels
            .iter()
            .map(|channel| (channel + 1).to_string())
            .collect::<Vec<_>>()
            .join(", ")
    }
}
