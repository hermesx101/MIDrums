//! Phase 3A MIDI-file inspection view. Parsing remains in `drums-core`.

use crate::{DrumApp, LibrarySort, PracticeLoopMode, View};
use drums_core::{
    DrumComponent, ExpectedNote, MidiFile, MidiTiming, MusicalTransport, PracticePerformance,
    PracticeResult, PracticeResultKind, Subdivision, TargetState, TimingJudgment, TimingTendency,
    TransportState, count_tokens,
};
use eframe::egui;

pub fn show(app: &mut DrumApp, ui: &mut egui::Ui) {
    ui.add_space(8.0);
    ui.heading("YOUR TRACKS");
    ui.horizontal_wrapped(|ui| {
        if ui.button("+ IMPORT MIDI").clicked() {
            app.import_midi_clicked();
        }
        if app.transport.is_some()
            && app.practice_session.is_some()
            && ui.button("Open Practice").clicked()
        {
            app.view = View::Practice;
        }
        ui.add(
            egui::TextEdit::singleline(&mut app.library_search)
                .hint_text("Search tracks...")
                .desired_width(260.0),
        );
        ui.label("Sort by:");
        egui::ComboBox::from_id_salt("library_sort")
            .selected_text(app.library_sort.label())
            .show_ui(ui, |ui| {
                for value in [
                    LibrarySort::RecentlyPlayed,
                    LibrarySort::RecentlyAdded,
                    LibrarySort::Name,
                    LibrarySort::Bpm,
                    LibrarySort::Accuracy,
                ] {
                    ui.selectable_value(&mut app.library_sort, value, value.label());
                }
            });
    });
    if !app.connected {
        egui::Frame::group(ui.style())
            .inner_margin(10.0)
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.strong("NO MIDI DEVICE CONNECTED");
                    ui.label("You can still import, browse, and select tracks.");
                    if ui.button("Refresh Devices").clicked() {
                        app.refresh_devices();
                    }
                });
            });
    }

    if app.track_library.is_empty() {
        ui.add_space(30.0);
        ui.vertical_centered(|ui| {
            ui.heading("No MIDI tracks yet");
            ui.label("Import a MIDI file to start practicing.");
            if ui.button("+ IMPORT MIDI").clicked() {
                app.import_midi_clicked();
            }
        });
        return;
    }

    let query = app.library_search.trim().to_lowercase();
    let mut entries = app.track_library.clone();
    entries.retain(|entry| {
        query.is_empty()
            || entry.display_name.to_lowercase().contains(&query)
            || entry.path.to_lowercase().contains(&query)
    });
    entries.sort_by(|a, b| match app.library_sort {
        LibrarySort::RecentlyPlayed => b.last_practiced_at.cmp(&a.last_practiced_at),
        LibrarySort::RecentlyAdded => b.added_at.cmp(&a.added_at),
        LibrarySort::Name => a
            .display_name
            .to_lowercase()
            .cmp(&b.display_name.to_lowercase()),
        LibrarySort::Bpm => a
            .bpm
            .partial_cmp(&b.bpm)
            .unwrap_or(std::cmp::Ordering::Equal),
        LibrarySort::Accuracy => b
            .last_accuracy
            .partial_cmp(&a.last_accuracy)
            .unwrap_or(std::cmp::Ordering::Equal),
    });
    let mut practice_path = None;
    let mut analysis_path = None;
    let mut rename = None;
    let mut remove = None;
    for entry in &entries {
        ui.push_id(("track_row", &entry.path), |ui| {
            // Bound only the horizontal dimension. Height remains entirely
            // content-driven, and the page-level ScrollArea handles overflow.
            ui.set_width(ui.available_width().min(980.0));
            library_card(
                app,
                ui,
                entry,
                &mut practice_path,
                &mut analysis_path,
                &mut rename,
                &mut remove,
            );
        });
        ui.add_space(8.0);
    }

    if let Some((path, name)) = rename {
        app.library_rename_path = Some(path);
        app.library_rename_buffer = name;
    }
    if let Some(path) = remove {
        app.library_remove_path = Some(path);
    }
    if let Some(path) = practice_path {
        match app.open_library_entry(&path) {
            Ok(()) => app.view = View::Practice,
            Err(error) => app.midi_import_error = Some(error),
        }
    }
    if let Some(path) = analysis_path {
        app.analysis_track_path = Some(path);
        app.analysis_session_id = None;
        app.view = View::PerformanceAnalysis;
    }
    library_edit_prompts(app, ui);
}

fn library_card(
    app: &mut DrumApp,
    ui: &mut egui::Ui,
    entry: &crate::track_library::LibraryEntry,
    practice_path: &mut Option<String>,
    analysis_path: &mut Option<String>,
    rename: &mut Option<(String, String)>,
    remove: &mut Option<String>,
) {
    let selected = app.library_selected_path.as_deref() == Some(entry.path.as_str());
    let active = app.current_library_path.as_deref() == Some(entry.path.as_str());
    egui::Frame::group(ui.style())
        .fill(if selected {
            egui::Color32::from_rgb(29, 48, 66)
        } else {
            egui::Color32::from_rgb(22, 28, 38)
        })
        .stroke(egui::Stroke::new(
            if selected { 2.0_f32 } else { 1.0_f32 },
            if selected {
                egui::Color32::from_rgb(90, 175, 230)
            } else {
                egui::Color32::from_gray(55)
            },
        ))
        .inner_margin(12.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width().min(956.0));

            ui.horizontal(|ui| {
                let title_width = (ui.available_width() - 42.0).max(80.0);
                let title = ui.add_sized(
                    [title_width, 24.0],
                    egui::Label::new(egui::RichText::new(&entry.display_name).strong().size(18.0))
                        .truncate()
                        .sense(egui::Sense::click()),
                );
                if title.clicked() {
                    app.library_selected_path = Some(entry.path.clone());
                }
                title.on_hover_text("Select track");
                library_card_menu(ui, entry, practice_path, analysis_path, rename, remove);
            });

            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                ui.weak(
                    entry
                        .bpm
                        .map(|value| format!("{value:.0} BPM"))
                        .unwrap_or_else(|| "N/A BPM".into()),
                );
                ui.separator();
                ui.weak(entry.time_signature.as_deref().unwrap_or("N/A"));
                ui.separator();
                ui.weak(
                    entry
                        .duration_seconds
                        .map(duration_text)
                        .unwrap_or_else(|| "N/A".into()),
                );
                ui.separator();
                ui.weak(format!("{} notes", entry.note_count));
                if entry.likely_drums {
                    ui.colored_label(egui::Color32::from_rgb(225, 185, 75), "LIKELY DRUMS");
                }
                let history = app.session_store.load_for_track(&entry.path).unwrap_or_default();
                if let Some(latest) = history.first() {
                    ui.colored_label(
                        egui::Color32::from_rgb(90, 210, 135),
                        format!("Last: {:.1}%", latest.statistics.accuracy),
                    );
                    if let Some(best) = history.iter().max_by(|a, b| a.statistics.accuracy.total_cmp(&b.statistics.accuracy)) {
                        ui.weak(format!("Best: {:.1}%", best.statistics.accuracy));
                    }
                    ui.weak(format!("Sessions: {}", history.len()));
                } else {
                    ui.weak("Not practiced yet");
                }
                if active {
                    ui.colored_label(egui::Color32::from_rgb(90, 210, 135), "ACTIVE");
                }
            });

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("Practice  >").clicked() {
                    *practice_path = Some(entry.path.clone());
                }
                if ui.button("Performance Analysis").clicked() {
                    *analysis_path = Some(entry.path.clone());
                }
                egui::CollapsingHeader::new("Details")
                    .default_open(false)
                    .show(ui, |ui| {
                        ui.monospace(format!(
                            "Filename: {}",
                            std::path::Path::new(&entry.path)
                                .file_name()
                                .and_then(|value| value.to_str())
                                .unwrap_or("Unknown")
                        ));
                        ui.monospace(format!("Path: {}", entry.path));
                        if let Ok(file) = MidiFile::from_path(&entry.path) {
                            metadata(ui, &file);
                        } else {
                            ui.colored_label(
                                egui::Color32::from_rgb(230, 120, 120),
                                "File is unavailable",
                            );
                        }
                    });
            });
        });
}

fn library_card_menu(
    ui: &mut egui::Ui,
    entry: &crate::track_library::LibraryEntry,
    practice_path: &mut Option<String>,
    analysis_path: &mut Option<String>,
    rename: &mut Option<(String, String)>,
    remove: &mut Option<String>,
) {
    ui.menu_button("...", |ui| {
        if ui.button("Practice").clicked() {
            *practice_path = Some(entry.path.clone());
            ui.close_menu();
        }
        if ui.button("Performance Analysis").clicked() {
            *analysis_path = Some(entry.path.clone());
            ui.close_menu();
        }
        if ui.button("Rename").clicked() {
            *rename = Some((entry.path.clone(), entry.display_name.clone()));
            ui.close_menu();
        }
        if ui.button("Remove from Library").clicked() {
            *remove = Some(entry.path.clone());
            ui.close_menu();
        }
        if ui.button("Show File Location").clicked() {
            if let Some(parent) = std::path::Path::new(&entry.path).parent() {
                let _ = std::process::Command::new("explorer").arg(parent).spawn();
            }
            ui.close_menu();
        }
    });
}

fn library_edit_prompts(app: &mut DrumApp, ui: &mut egui::Ui) {
    if let Some(path) = app.library_rename_path.clone() {
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Display name:");
            ui.text_edit_singleline(&mut app.library_rename_buffer);
            if ui.button("Save Rename").clicked() {
                if let Some(entry) = app
                    .track_library
                    .iter_mut()
                    .find(|entry| entry.path == path)
                {
                    let name = app.library_rename_buffer.trim();
                    if !name.is_empty() {
                        entry.display_name = name.to_string();
                    }
                }
                let _ = crate::track_library::save(&app.track_library);
                app.library_rename_path = None;
            }
            if ui.button("Cancel").clicked() {
                app.library_rename_path = None;
            }
        });
    }
    if let Some(path) = app.library_remove_path.clone() {
        let name = app
            .track_library
            .iter()
            .find(|entry| entry.path == path)
            .map(|entry| entry.display_name.clone())
            .unwrap_or_else(|| "track".into());
        ui.separator();
        ui.label(format!(
            "Remove {name} from your library? The MIDI file will not be deleted."
        ));
        ui.horizontal(|ui| {
            if ui.button("Cancel").clicked() {
                app.library_remove_path = None;
            }
            if ui.button("Remove").clicked() {
                app.track_library.retain(|entry| entry.path != path);
                if app.library_selected_path.as_deref() == Some(path.as_str()) {
                    app.library_selected_path = None;
                }
                let _ = crate::track_library::save(&app.track_library);
                app.library_remove_path = None;
            }
        });
    }
}

#[allow(dead_code)]
fn show_card_legacy(app: &mut DrumApp, ui: &mut egui::Ui) {
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.heading("TRACK LIBRARY");
        if ui.button("+ Import MIDI").clicked() {
            app.import_midi_clicked();
        }
    });
    if let Some(error) = &app.midi_import_error {
        ui.colored_label(egui::Color32::from_rgb(240, 120, 120), error);
    }
    let Some(file) = app.imported_midi.as_ref() else {
        ui.add_space(16.0);
        ui.label("Import a .mid or .midi file to add it to the library.");
        return;
    };
    let filename = file.filename.clone();
    let bpm = file.tempo_events.first().map(|tempo| tempo.bpm());
    let signature = file
        .time_signature_events
        .first()
        .map(|value| format!("{}/{}", value.numerator, value.denominator()));
    let duration = file.duration_seconds;
    let mut selected_changed = false;
    let mut enter_practice = false;
    egui::Frame::group(ui.style())
        .inner_margin(14.0)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(&filename).strong().size(22.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    enter_practice = ui
                        .add_enabled(
                            app.selected_drum_track.is_some(),
                            egui::Button::new("Practice"),
                        )
                        .clicked();
                });
            });
            ui.horizontal_wrapped(|ui| {
                ui.weak(
                    bpm.map(|value| format!("{value:.0} BPM"))
                        .unwrap_or_else(|| "Tempo not specified".to_string()),
                );
                ui.weak("|");
                ui.weak(signature.unwrap_or_else(|| "Time signature not specified".to_string()));
                ui.weak("|");
                ui.weak(
                    duration
                        .map(duration_text)
                        .unwrap_or_else(|| "Duration unavailable".to_string()),
                );
            });
            ui.add_space(8.0);
            ui.strong("Tracks");
            for track in &file.tracks {
                let title = track.name.as_deref().unwrap_or("Unnamed track");
                selected_changed |= ui
                    .selectable_value(
                        &mut app.selected_drum_track,
                        Some(track.index),
                        format!("{} - {} notes", title, track.note_event_count),
                    )
                    .changed();
            }
            egui::CollapsingHeader::new("Details")
                .default_open(false)
                .show(ui, |ui| {
                    metadata(ui, file);
                    if let Some(track) = app.selected_drum_track {
                        let expected =
                            file.expected_notes_for_track(track, app.active_config.as_ref());
                        egui::CollapsingHeader::new("Inspect MIDI Events")
                            .default_open(false)
                            .show(ui, |ui| {
                                midi_event_table(ui, &expected);
                            });
                    }
                });
        });
    if selected_changed {
        app.rebuild_timeline();
    }
    if enter_practice {
        if app.practice_session.is_none() {
            app.rebuild_timeline();
        }
        app.view = View::Practice;
    }
}

#[allow(dead_code)]
fn show_legacy(app: &mut DrumApp, ui: &mut egui::Ui) {
    ui.add_space(6.0);
    ui.heading("TRACK LIBRARY");
    ui.horizontal(|ui| {
        if ui.button("Import MIDI...").clicked() {
            app.import_midi_clicked();
        }
    });
    if let Some(error) = &app.midi_import_error {
        ui.add_space(8.0);
        ui.colored_label(egui::Color32::from_rgb(240, 120, 120), error);
    }
    let Some(file) = app.imported_midi.as_ref() else {
        ui.add_space(12.0);
        ui.heading("Import a MIDI track");
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
    let enter_practice = ui
        .add_enabled(
            !expected.is_empty(),
            egui::Button::new("Practice Selected Track"),
        )
        .clicked();
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
    if enter_practice {
        if app.practice_session.is_none() {
            app.rebuild_timeline();
        }
        app.view = View::Practice;
    }
}

pub fn show_practice(app: &mut DrumApp, ui: &mut egui::Ui) {
    let (Some(file), Some(track)) = (&app.imported_midi, app.selected_drum_track) else {
        ui.heading("No track selected");
        if ui.button("Your Tracks").clicked() {
            app.view = View::TrackLibrary;
        }
        return;
    };
    if !app.connected {
        ui.colored_label(
            egui::Color32::from_rgb(240, 190, 80),
            "MIDI DEVICE DISCONNECTED - connect a device for live drum input.",
        );
    }
    let expected = file.expected_notes_for_track(track, app.active_config.as_ref());
    practice_interface(app, ui, &expected);
}

/// Phase 5A practice composition. Musical state remains owned by drums-core;
/// this function only arranges controls and renders a bounded view of it.
fn practice_interface(app: &mut DrumApp, ui: &mut egui::Ui, expected: &[ExpectedNote]) {
    let Some(transport) = app.transport.as_ref() else {
        return;
    };
    let song_complete = app.practice_session.as_ref().is_some_and(|session| {
        session.matchable_count() > 0
            && session.finalized_matchable_count() == session.matchable_count()
            && session
                .current_target(transport.current_tick(), transport)
                .is_none()
    });

    practice_top_bar(app, ui);
    ui.add_space(6.0);

    if app.count_in_active {
        if let Some(transport) = app.transport.as_ref() {
            let position = transport.musical_position();
            let remaining = transport
                .position_map()
                .position_at_tick(app.count_in_end_tick)
                .measure
                .saturating_sub(position.measure)
                .max(1);
            ui.vertical_centered(|ui| {
                ui.label(egui::RichText::new("COUNT-IN").strong().size(28.0));
                ui.label(
                    egui::RichText::new(remaining.to_string())
                        .strong()
                        .size(52.0)
                        .color(egui::Color32::from_rgb(120, 205, 255)),
                );
                ui.horizontal(|ui| {
                    for token in count_tokens(
                        transport.position_map(),
                        transport.current_tick(),
                        app.metronome.subdivision(),
                    ) {
                        let text = egui::RichText::new(token.label)
                            .strong()
                            .size(if token.active { 22.0 } else { 18.0 });
                        if token.active {
                            ui.colored_label(egui::Color32::from_rgb(120, 205, 255), text);
                        } else {
                            ui.label(text);
                        }
                    }
                });
                ui.weak("Practice begins after the count-in");
            });
        }
    }

    if song_complete {
        // A final hit can complete the session before the transport reaches its
        // final tick, so completion itself (not only end-of-song) must persist.
        app.persist_completed_session();
        let performance = app
            .practice_session
            .as_ref()
            .map(PracticePerformance::from_session);
        if let Some(performance) = &performance {
            show_performance_summary(ui, performance);
        }
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui.button("Practice Again").clicked() {
                app.restart_practice(true);
            }
            if ui.button("Your Tracks").clicked() {
                app.view = View::TrackLibrary;
            }
            if ui.button("Performance Analysis").clicked() {
                app.analysis_track_path = app.current_library_path.clone();
                app.analysis_session_id = None;
                app.view = View::PerformanceAnalysis;
            }
        });
        practice_debug_details(app, ui);
        return;
    }

    if let Some(transport) = app.transport.as_ref() {
        show_visual_legend(ui);
        rhythm_grid(
            ui,
            transport,
            expected,
            app.practice_session
                .as_ref()
                .map(|session| session.results())
                .unwrap_or(&[]),
            app.metronome.subdivision(),
            app.practice_lookahead_seconds,
            app.active_loop_region(),
        );
    }

    practice_feedback_bar(app, ui);
    practice_debug_details(app, ui);
}

fn practice_top_bar(app: &mut DrumApp, ui: &mut egui::Ui) {
    let Some(transport) = app.transport.as_ref() else {
        return;
    };
    let current_tick = transport.current_tick();
    let final_tick = transport.final_tick();
    let state = transport.state();
    let musical = transport.musical_position();
    let elapsed = transport.elapsed_seconds();
    let total = transport.total_seconds();
    let original_bpm = transport.original_bpm();
    let practice_bpm = transport.practice_starting_bpm();
    let song_name = app
        .imported_midi
        .as_ref()
        .map(|file| file.filename.as_str())
        .unwrap_or("MIDI Practice");

    let mut play = false;
    let mut go_to_tracks = false;
    let mut pause = false;
    let mut stop = false;
    let mut beginning = false;
    let mut end = false;
    let mut bpm_delta = 0.0;
    let mut manual_bpm = practice_bpm;
    let mut manual_bpm_changed = false;
    let mut reset_bpm = false;
    let mut subdivision = app.metronome.subdivision();
    let mut subdivision_auto = app.subdivision_auto;
    let mut subdivision_changed = false;
    let mut seek_tick = current_tick;
    let mut loop_mode = app.practice_loop_mode;
    let mut loop_changed = false;
    let last_bar = transport
        .position_map()
        .position_at_tick(final_tick)
        .measure;
    let mut custom_changed = false;

    egui::Frame::group(ui.style())
        .inner_margin(10.0)
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                go_to_tracks = ui.button("Your Tracks").clicked();
                ui.strong(song_name);
                ui.separator();
                beginning = ui.button("|<").on_hover_text("Go to beginning").clicked();
                play = ui
                    .add_enabled(state != TransportState::Playing, egui::Button::new(">"))
                    .on_hover_text("Play / resume")
                    .clicked();
                pause = ui
                    .add_enabled(state == TransportState::Playing, egui::Button::new("||"))
                    .on_hover_text("Pause")
                    .clicked();
                stop = ui.button("[]").on_hover_text("Stop").clicked();
                end = ui.button(">|").on_hover_text("Go to end").clicked();
                ui.separator();
                ui.monospace(format!("{} / {}", time_text(elapsed), time_text(total)));
                ui.weak(format!("Bar {}  Beat {}", musical.measure, musical.beat));
            });
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.weak("PRACTICE BPM");
                bpm_delta -= ui.button("-").clicked() as u8 as f64;
                manual_bpm_changed |= ui
                    .add(
                        egui::DragValue::new(&mut manual_bpm)
                            .range(20.0..=300.0)
                            .speed(1.0)
                            .suffix(" BPM"),
                    )
                    .changed();
                bpm_delta += ui.button("+").clicked() as u8 as f64;
                reset_bpm = ui.small_button("Reset").clicked();
                ui.weak(format!("Original {original_bpm:.0} BPM"));
                ui.separator();
                ui.strong("SUBDIVISION");
                egui::ComboBox::from_id_salt("top_subdivision")
                    .selected_text(if subdivision_auto {
                        format!("Auto ({})", subdivision_long_label(subdivision))
                    } else {
                        subdivision_long_label(subdivision).to_string()
                    })
                    .show_ui(ui, |ui| {
                        if ui.selectable_label(subdivision_auto, "Auto").clicked() {
                            subdivision_auto = true;
                            subdivision_changed = true;
                            if let Some(suggestion) = app.subdivision_suggestion {
                                subdivision = suggestion.subdivision;
                            }
                        }
                        for value in [
                            Subdivision::Quarter,
                            Subdivision::Eighth,
                            Subdivision::Sixteenth,
                        ] {
                            if ui
                                .selectable_label(
                                    !subdivision_auto && subdivision == value,
                                    subdivision_long_label(value),
                                )
                                .clicked()
                            {
                                subdivision_auto = false;
                                subdivision = value;
                                subdivision_changed = true;
                            }
                        }
                    });
                ui.strong("METRONOME");
                let metronome_label = if app.metronome_enabled { "ON" } else { "OFF" };
                ui.checkbox(&mut app.metronome_enabled, metronome_label);
                ui.strong("COUNT-IN");
                egui::ComboBox::from_id_salt("count_in")
                    .selected_text(if app.count_in_bars == 0 {
                        "Off".to_string()
                    } else {
                        format!(
                            "{} Bar{}",
                            app.count_in_bars,
                            if app.count_in_bars == 1 { "" } else { "s" }
                        )
                    })
                    .show_ui(ui, |ui| {
                        for bars in [0, 1, 2, 4, 8] {
                            ui.selectable_value(
                                &mut app.count_in_bars,
                                bars,
                                if bars == 0 {
                                    "Off".to_string()
                                } else {
                                    format!("{bars} Bar{}", if bars == 1 { "" } else { "s" })
                                },
                            );
                        }
                    });
                ui.strong("LOOKAHEAD");
                egui::ComboBox::from_id_salt("lookahead")
                    .selected_text(format!("{:.0} sec", app.practice_lookahead_seconds))
                    .show_ui(ui, |ui| {
                        for seconds in [1.0, 2.0, 4.0, 6.0, 8.0] {
                            ui.selectable_value(
                                &mut app.practice_lookahead_seconds,
                                seconds,
                                format!("{seconds:.0} sec"),
                            );
                        }
                    });
                ui.strong("LOOP");
                egui::ComboBox::from_id_salt("practice_loop")
                    .selected_text(loop_mode.label())
                    .show_ui(ui, |ui| {
                        for mode in [
                            PracticeLoopMode::Off,
                            PracticeLoopMode::CurrentBar,
                            PracticeLoopMode::TwoBars,
                            PracticeLoopMode::FourBars,
                            PracticeLoopMode::EightBars,
                            PracticeLoopMode::Custom,
                        ] {
                            loop_changed |= ui
                                .selectable_value(&mut loop_mode, mode, mode.label())
                                .changed();
                        }
                    });
                if loop_mode == PracticeLoopMode::Custom {
                    ui.label("Start bar");
                    custom_changed |= ui
                        .add(
                            egui::DragValue::new(&mut app.practice_loop_start_bar)
                                .range(1..=last_bar.max(1)),
                        )
                        .changed();
                    ui.label("End bar");
                    custom_changed |= ui
                        .add(
                            egui::DragValue::new(&mut app.practice_loop_end_bar)
                                .range(2..=last_bar + 1),
                        )
                        .changed();
                }
                if app.practice_loop_mode != PracticeLoopMode::Off {
                    ui.weak(format!(
                        "Bars {} -> {} | Loop {}",
                        app.practice_loop_start_bar,
                        app.practice_loop_end_bar,
                        app.practice_loop_iteration
                    ));
                }
            });
            let tokens = count_tokens(transport.position_map(), current_tick, subdivision);
            ui.horizontal(|ui| {
                ui.weak(if app.count_in_active {
                    "COUNT-IN"
                } else {
                    "COUNT"
                });
                for token in tokens {
                    if token.active {
                        ui.colored_label(
                            egui::Color32::from_rgb(120, 205, 255),
                            egui::RichText::new(token.label).strong().size(17.0),
                        );
                    } else {
                        ui.label(egui::RichText::new(token.label).size(15.0));
                    }
                }
            });
            ui.add(
                egui::Slider::new(&mut seek_tick, 0..=final_tick)
                    .show_value(false)
                    .text("Seek"),
            );
        });

    if loop_changed {
        app.configure_loop(loop_mode);
    }
    if custom_changed {
        if app.practice_loop_end_bar <= app.practice_loop_start_bar {
            app.practice_loop_end_bar = app.practice_loop_start_bar + 1;
        }
        app.configure_loop(PracticeLoopMode::Custom);
    }
    let seek_changed = seek_tick != current_tick;
    let Some(transport) = app.transport.as_mut() else {
        return;
    };
    if reset_bpm {
        transport.reset_practice_tempo();
    }
    if manual_bpm_changed {
        if let Some(value) = valid_practice_bpm(manual_bpm) {
            transport.set_practice_bpm(value);
        }
    }
    if bpm_delta != 0.0 {
        transport.set_practice_bpm(practice_bpm + bpm_delta);
    }
    if subdivision_changed {
        app.subdivision_auto = subdivision_auto;
        app.metronome.set_subdivision(subdivision);
        app.metronome.sync_to_transport(transport);
    }
    if play {
        if state == TransportState::Stopped {
            app.metronome.reset();
            if let Some(session) = app.practice_session.as_mut() {
                session.reset();
            }
            if app.count_in_bars > 0 {
                app.count_in_active = true;
                app.count_in_target_tick = current_tick;
                let count_start_measure = transport.musical_position().measure;
                let count_start_tick = transport
                    .position_map()
                    .tick_at_measure(count_start_measure, transport.final_tick());
                app.count_in_end_tick = transport.position_map().tick_at_measure(
                    count_start_measure + app.count_in_bars as u64,
                    transport.final_tick(),
                );
                transport.seek(count_start_tick.min(i64::MAX as u64) as i64);
            }
        } else {
            app.metronome.sync_to_transport(transport);
        }
        transport.play();
    }
    if pause {
        transport.pause();
        app.metronome.sync_to_transport(transport);
    }
    if stop {
        app.count_in_active = false;
        transport.stop();
        app.metronome.reset();
        if let Some(session) = app.practice_session.as_mut() {
            session.on_stop();
        }
    }
    if beginning {
        seek_tick = 0;
    }
    if end {
        seek_tick = final_tick;
    }
    if seek_changed || beginning || end {
        transport.seek(seek_tick.min(i64::MAX as u64) as i64);
        app.metronome.sync_to_transport(transport);
        if let Some(session) = app.practice_session.as_mut() {
            session.on_seek_to_tick(transport.current_tick(), transport);
        }
    }
    if go_to_tracks {
        transport.pause();
        app.metronome.sync_to_transport(transport);
        app.view = View::TrackLibrary;
    }
}

fn valid_practice_bpm(value: f64) -> Option<f64> {
    (value.is_finite() && (20.0..=300.0).contains(&value)).then_some(value)
}

fn rhythm_grid(
    ui: &mut egui::Ui,
    transport: &MusicalTransport,
    expected: &[ExpectedNote],
    results: &[PracticeResult],
    subdivision: Subdivision,
    lookahead_seconds: f64,
    loop_region: Option<(u64, u64)>,
) {
    // Loop iterations reuse musical ticks. Keep the complete result history in
    // drums-core for statistics, but bound visualization work and overlap.
    let results = &results[results.len().saturating_sub(256)..];
    let mut lanes = vec![
        Some(DrumComponent::Crash),
        Some(DrumComponent::Ride),
        Some(DrumComponent::HiHat),
        Some(DrumComponent::HiHatOpen),
        Some(DrumComponent::HiHatPedal),
        Some(DrumComponent::Snare),
        Some(DrumComponent::Kick),
        Some(DrumComponent::HiTom),
        Some(DrumComponent::MidTom),
        Some(DrumComponent::LowTom),
    ];
    let has_unknown = expected.iter().any(|note| note.drum_component.is_none())
        || results.iter().any(|result| {
            result
                .actual
                .as_ref()
                .is_some_and(|hit| hit.drum_component.is_none())
        });
    if has_unknown {
        lanes.push(None);
    }
    // Reserve enough room for the fixed feedback panel below the chart.
    let height = (ui.available_height() - 132.0).clamp(280.0, 460.0);
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::hover(),
    );
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 6.0, egui::Color32::from_rgb(15, 20, 29));
    let label_width = rect.width().clamp(110.0, 155.0) * 0.85;
    let header_height = 38.0;
    let chart_left = rect.left() + label_width;
    let hit_x = chart_left + (rect.right() - chart_left) * 0.20;
    let lane_height = (rect.height() - header_height) / lanes.len() as f32;
    let now = transport.elapsed_seconds();
    let past_seconds = 1.5;
    let (window_start, window_end) = visible_time_window(
        now,
        transport.total_seconds(),
        past_seconds,
        lookahead_seconds,
    );
    let seconds_to_x = |seconds: f64| {
        timeline_x(
            seconds,
            now,
            past_seconds,
            lookahead_seconds,
            chart_left,
            hit_x,
            rect.right(),
        )
    };

    if let Some((start_tick, end_tick)) = loop_region {
        let start_seconds = transport.effective_seconds_at_tick(start_tick);
        let end_seconds = transport.effective_seconds_at_tick(end_tick);
        let visible_start = start_seconds.max(window_start);
        let visible_end = end_seconds.min(window_end);
        if visible_start < visible_end {
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(seconds_to_x(visible_start), rect.top() + header_height),
                    egui::pos2(seconds_to_x(visible_end), rect.bottom()),
                ),
                0.0,
                egui::Color32::from_rgba_unmultiplied(80, 160, 220, 18),
            );
        }
        for (tick, label) in [(start_tick, "LOOP START"), (end_tick, "LOOP END")] {
            let seconds = transport.effective_seconds_at_tick(tick);
            if seconds >= window_start && seconds <= window_end {
                let x = seconds_to_x(seconds);
                painter.line_segment(
                    [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                    egui::Stroke::new(1.5_f32, egui::Color32::from_rgb(90, 175, 220)),
                );
                painter.text(
                    egui::pos2(x + 3.0, rect.top() + 3.0),
                    egui::Align2::LEFT_TOP,
                    label,
                    egui::FontId::monospace(9.0),
                    egui::Color32::from_rgb(120, 195, 230),
                );
            }
        }
    }

    painter.line_segment(
        [
            egui::pos2(chart_left, rect.top()),
            egui::pos2(chart_left, rect.bottom()),
        ],
        egui::Stroke::new(1.0_f32, egui::Color32::from_gray(70)),
    );
    for (index, component) in lanes.iter().enumerate() {
        let top = rect.top() + header_height + index as f32 * lane_height;
        let center = top + lane_height * 0.5;
        painter.line_segment(
            [egui::pos2(rect.left(), top), egui::pos2(rect.right(), top)],
            egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(35, 43, 56)),
        );
        let (label, color) = component
            .map(|value| (value.label(), component_color(value)))
            .unwrap_or(("UNKNOWN", egui::Color32::from_gray(145)));
        painter.circle_filled(egui::pos2(rect.left() + 16.0, center), 6.0, color);
        painter.text(
            egui::pos2(rect.left() + 30.0, center),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(13.0),
            color,
        );
    }

    if let Some(ppq) = transport.position_map().ticks_per_quarter() {
        let step = (ppq as u64 / subdivision.units_per_quarter()).max(1);
        let start_tick = transport.effective_tick_at_seconds((now - past_seconds).max(0.0));
        let end_tick = transport
            .effective_tick_at_seconds((now + lookahead_seconds).min(transport.total_seconds()));
        let mut tick = start_tick - start_tick % step;
        while tick <= end_tick {
            let x = seconds_to_x(transport.effective_seconds_at_tick(tick));
            let position = transport.position_map().position_at_tick(tick);
            let is_beat = position.ticks_into_beat == 0;
            let is_bar = is_beat && position.beat == 1;
            let color = if is_bar {
                egui::Color32::from_gray(105)
            } else if is_beat {
                egui::Color32::from_gray(70)
            } else {
                egui::Color32::from_gray(43)
            };
            painter.line_segment(
                [
                    egui::pos2(x, rect.top() + header_height),
                    egui::pos2(x, rect.bottom()),
                ],
                egui::Stroke::new(
                    if is_bar {
                        2.25_f32
                    } else if is_beat {
                        1.25_f32
                    } else {
                        0.6_f32
                    },
                    color,
                ),
            );
            let token = subdivision_token(
                position.beat,
                position.ticks_into_beat,
                position.ticks_per_beat,
                subdivision,
            );
            let label = if is_bar {
                format!("Bar {}   {}", position.measure, token)
            } else {
                token
            };
            painter.text(
                egui::pos2(x + 3.0, rect.top() + 18.0),
                egui::Align2::LEFT_CENTER,
                label,
                egui::FontId::monospace(11.0),
                egui::Color32::from_gray(175),
            );
            tick = tick.saturating_add(step);
            if step == 0 {
                break;
            }
        }
    }

    for note in expected {
        let seconds = transport.effective_seconds_at_tick(note.tick);
        if seconds < window_start || seconds > window_end {
            continue;
        }
        let lane = lane_index(note.drum_component, has_unknown);
        let y = rect.top() + header_height + (lane as f32 + 0.5) * lane_height;
        let x = seconds_to_x(seconds);
        let color = note
            .drum_component
            .map(component_color)
            .unwrap_or(egui::Color32::from_gray(155));
        let missed = expected_is_missed(note, results);
        if missed {
            let muted = color.gamma_multiply(0.45);
            painter.circle_stroke(egui::pos2(x, y), 9.0, egui::Stroke::new(2.0_f32, muted));
            painter.line_segment(
                [egui::pos2(x - 5.0, y - 5.0), egui::pos2(x + 5.0, y + 5.0)],
                egui::Stroke::new(2.0_f32, muted),
            );
            painter.line_segment(
                [egui::pos2(x + 5.0, y - 5.0), egui::pos2(x - 5.0, y + 5.0)],
                egui::Stroke::new(2.0_f32, muted),
            );
        } else {
            painter.circle_filled(egui::pos2(x, y), 8.0, color);
            painter.circle_stroke(
                egui::pos2(x, y),
                10.0,
                egui::Stroke::new(1.0_f32, color.gamma_multiply(0.55)),
            );
        }
        if note.drum_component.is_none() {
            painter.text(
                egui::pos2(x, y),
                egui::Align2::CENTER_CENTER,
                "?",
                egui::FontId::proportional(11.0),
                egui::Color32::BLACK,
            );
            painter.text(
                egui::pos2(x + 12.0, y),
                egui::Align2::LEFT_CENTER,
                format!("MIDI {}", note.midi_note),
                egui::FontId::monospace(9.0),
                color,
            );
        }
    }
    for result in results {
        let Some(actual) = &result.actual else {
            continue;
        };
        let seconds = transport.effective_seconds_at_tick(actual.transport_tick);
        if seconds < window_start || seconds > window_end {
            continue;
        }
        let lane = lane_index(actual.drum_component, has_unknown);
        let y = rect.top() + header_height + (lane as f32 + 0.5) * lane_height;
        let x = seconds_to_x(seconds);
        let state_color = actual_result_color(result);
        painter.circle_filled(egui::pos2(x, y), 4.5, state_color);
        painter.circle_stroke(
            egui::pos2(x, y),
            9.0,
            egui::Stroke::new(2.5_f32, state_color),
        );
        painter.line_segment(
            [egui::pos2(x - 4.0, y), egui::pos2(x + 4.0, y)],
            egui::Stroke::new(1.5_f32, state_color),
        );
        painter.line_segment(
            [egui::pos2(x, y - 4.0), egui::pos2(x, y + 4.0)],
            egui::Stroke::new(1.5_f32, state_color),
        );
    }
    painter.line_segment(
        [
            egui::pos2(hit_x, rect.top()),
            egui::pos2(hit_x, rect.bottom()),
        ],
        egui::Stroke::new(2.5_f32, egui::Color32::from_rgb(105, 220, 255)),
    );
}

fn actual_result_color(result: &PracticeResult) -> egui::Color32 {
    match result.kind {
        PracticeResultKind::CorrectHit => match result.timing_judgment {
            Some(TimingJudgment::Early) => egui::Color32::from_rgb(244, 196, 70),
            Some(TimingJudgment::Late) => egui::Color32::from_rgb(246, 132, 68),
            _ => egui::Color32::from_rgb(72, 214, 137),
        },
        PracticeResultKind::WrongDrum => egui::Color32::from_rgb(244, 82, 118),
        PracticeResultKind::Ignored => egui::Color32::from_rgb(175, 180, 192),
        PracticeResultKind::Missed => egui::Color32::from_rgb(120, 124, 135),
    }
}

fn expected_is_missed(note: &ExpectedNote, results: &[PracticeResult]) -> bool {
    results.iter().any(|result| {
        result.kind == PracticeResultKind::Missed
            && result.expected.as_ref().is_some_and(|value| {
                value.tick == note.tick
                    && value.midi_note == note.midi_note
                    && value.track == note.track
            })
    })
}

fn show_visual_legend(ui: &mut egui::Ui) {
    ui.horizontal_wrapped(|ui| {
        ui.strong("LEGEND");
        ui.colored_label(egui::Color32::from_gray(190), "Expected");
        ui.colored_label(egui::Color32::from_rgb(72, 214, 137), "Correct");
        ui.colored_label(egui::Color32::from_rgb(244, 196, 70), "Early");
        ui.colored_label(egui::Color32::from_rgb(246, 132, 68), "Late");
        ui.colored_label(egui::Color32::from_rgb(244, 82, 118), "Wrong drum");
        ui.colored_label(egui::Color32::from_gray(120), "Missed");
        ui.colored_label(egui::Color32::from_rgb(175, 180, 192), "Extra");
    });
}

fn subdivision_token(
    beat: u32,
    ticks_into_beat: u64,
    ticks_per_beat: u64,
    subdivision: Subdivision,
) -> String {
    let units = subdivision.units_per_quarter().max(1) as u64;
    let index = (ticks_into_beat.saturating_mul(units) / ticks_per_beat.max(1)) as usize;
    match subdivision {
        Subdivision::Quarter => beat.to_string(),
        Subdivision::Eighth => {
            if index == 0 {
                beat.to_string()
            } else {
                "&".to_string()
            }
        }
        Subdivision::Sixteenth => {
            if index == 0 {
                beat.to_string()
            } else {
                ["e", "&", "a"][index.saturating_sub(1).min(2)].to_string()
            }
        }
    }
}

fn subdivision_long_label(value: Subdivision) -> &'static str {
    match value {
        Subdivision::Quarter => "Quarter Notes",
        Subdivision::Eighth => "Eighth Notes",
        Subdivision::Sixteenth => "Sixteenth Notes",
    }
}

fn lane_index(component: Option<DrumComponent>, has_unknown: bool) -> usize {
    DrumComponent::ALL
        .iter()
        .position(|value| Some(*value) == component)
        .unwrap_or(if has_unknown {
            DrumComponent::ALL.len()
        } else {
            DrumComponent::ALL.len() - 1
        })
}

fn visible_time_window(
    current_seconds: f64,
    total_seconds: f64,
    past_seconds: f64,
    lookahead_seconds: f64,
) -> (f64, f64) {
    (
        (current_seconds - past_seconds).max(0.0),
        (current_seconds + lookahead_seconds.max(0.0)).min(total_seconds),
    )
}

fn timeline_x(
    event_seconds: f64,
    now_seconds: f64,
    past_seconds: f64,
    lookahead_seconds: f64,
    left: f32,
    playhead: f32,
    right: f32,
) -> f32 {
    if event_seconds < now_seconds {
        playhead
            - ((now_seconds - event_seconds) / past_seconds.max(f64::EPSILON)) as f32
                * (playhead - left)
    } else {
        playhead
            + ((event_seconds - now_seconds) / lookahead_seconds.max(f64::EPSILON)) as f32
                * (right - playhead)
    }
}

fn component_color(component: DrumComponent) -> egui::Color32 {
    match component {
        DrumComponent::Crash => egui::Color32::from_rgb(244, 104, 72),
        DrumComponent::Ride => egui::Color32::from_rgb(235, 157, 63),
        DrumComponent::HiHat => egui::Color32::from_rgb(238, 205, 70),
        DrumComponent::HiHatOpen => egui::Color32::from_rgb(244, 218, 95),
        DrumComponent::HiHatPedal => egui::Color32::from_rgb(205, 178, 55),
        DrumComponent::HiHatPedalPosition => egui::Color32::from_rgb(180, 150, 45),
        DrumComponent::Snare => egui::Color32::from_rgb(72, 156, 244),
        DrumComponent::Kick => egui::Color32::from_rgb(232, 70, 85),
        DrumComponent::HiTom => egui::Color32::from_rgb(62, 201, 177),
        DrumComponent::MidTom => egui::Color32::from_rgb(84, 126, 232),
        DrumComponent::LowTom => egui::Color32::from_rgb(156, 99, 224),
    }
}

fn practice_feedback_bar(app: &DrumApp, ui: &mut egui::Ui) {
    let (Some(session), Some(transport)) = (&app.practice_session, &app.transport) else {
        return;
    };
    let performance = PracticePerformance::from_session(session);
    let target = session.current_target(transport.current_tick(), transport);
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), 112.0),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            egui::Frame::group(ui.style())
                .inner_margin(8.0)
                .show(ui, |ui| {
                    ui.columns(3, |columns| {
                        columns[0].weak("CURRENT");
                        if let Some(target) = target {
                            let names = target
                                .notes
                                .iter()
                                .map(|note| {
                                    note.drum_component
                                        .map(|value| value.label().to_string())
                                        .unwrap_or_else(|| format!("UNKNOWN {}", note.midi_note))
                                })
                                .collect::<Vec<_>>()
                                .join(" + ");
                            columns[0].label(egui::RichText::new(names).strong().size(22.0));
                            columns[0].weak(if target.state == TargetState::Hittable {
                                "NOW".to_string()
                            } else {
                                format!("in {:.2}s", target.time_until_seconds.max(0.0))
                            });
                        } else {
                            columns[0].label("No active target");
                        }
                        columns[1].weak("LAST RESULT");
                        if let Some(result) = &app.displayed_practice_result {
                            show_last_result(&mut columns[1], result);
                        } else {
                            columns[1].label("Waiting for input...");
                        }
                        columns[2].weak("SESSION");
                        columns[2].label(
                            egui::RichText::new(format!(
                                "{:.1}% accuracy",
                                performance.accuracy_percent()
                            ))
                            .strong()
                            .size(20.0),
                        );
                        columns[2].label(format!(
                            "{} / {} finalized",
                            performance.correct_hits, performance.finalized_expected
                        ));
                        columns[2].weak(format!(
                            "Good {}  Early {}  Late {}",
                            performance.good_hits, performance.early_hits, performance.late_hits
                        ));
                    });
                });
        },
    );
}

fn practice_debug_details(app: &DrumApp, ui: &mut egui::Ui) {
    let Some(transport) = app.transport.as_ref() else {
        return;
    };
    egui::CollapsingHeader::new("Debug / Details")
        .default_open(false)
        .show(ui, |ui| {
            let (numerator, denominator) = transport
                .position_map()
                .time_signature_at_tick(transport.current_tick());
            ui.monospace(format!(
                "State: {:?} | Tick: {} / {} | Time signature: {}/{} | PPQ: {:?}",
                transport.state(),
                transport.current_tick(),
                transport.final_tick(),
                numerator,
                denominator,
                transport.position_map().ticks_per_quarter()
            ));
            if let Some(session) = &app.practice_session {
                ui.monospace(format!(
                    "Pending: {} | Results: {} | Match window: +/-{} ms",
                    session.pending_count(),
                    session.results().len(),
                    session.matching_window().as_millis()
                ));
            }
            if let Some(hit) = &app.last_live_hit {
                ui.monospace(format!(
                    "Last input: MIDI {} | channel {} | velocity {} | tick {}",
                    hit.received_midi_note,
                    hit.channel + 1,
                    hit.velocity,
                    hit.transport_tick
                ));
            }
        });
}

#[allow(dead_code)]
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
                    columns[0].label("Waiting for input...");
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
                    columns[1].label("N/A");
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
                "MIDI: {} (Connected)",
                app.backend.connected_port_name().unwrap_or("Connected")
            )
        } else {
            "MIDI: Disconnected".to_string()
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
        "{} / {} | Bar {} | Beat {}",
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
                egui::Button::new(egui::RichText::new("-").size(24.0)),
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
            "Time signature: {numerator}/{denominator} | PPQ: {}",
            ppq.map(|value| value.to_string())
                .unwrap_or_else(|| "SMPTE".to_string())
        ));
        ui.monospace(format!(
            "Original BPM: {:.1} | Effective BPM: {:.1}",
            transport.original_bpm(),
            transport.current_bpm()
        ));
        if let Some(session) = &app.practice_session {
            ui.monospace(format!(
                "Pending: {} | Results: {} | Match window: +/-{} ms",
                session.pending_count(),
                session.results().len(),
                session.matching_window().as_millis()
            ));
            if let Some(target) = session.current_target(transport.current_tick(), transport) {
                ui.monospace(format!(
                    "Expected indices: {:?} | tick {} | distance {:+.1} ms",
                    target.expected_indices,
                    target.tick,
                    target.time_until_seconds * 1_000.0
                ));
            }
            if let Some(result) = session.results().last() {
                ui.monospace(format!(
                    "Last raw result: {:?} | ticks {:?} | micros {:?}",
                    result.kind, result.timing_difference_ticks, result.timing_difference_micros
                ));
            }
        }
        if let Some(hit) = &app.last_live_hit {
            ui.monospace(format!(
                "Last input: MIDI {} | channel {} | velocity {} | tick {}",
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
                egui::RichText::new(actual.unwrap_or("HIT"))
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
            ui.strong("WRONG DRUM");
            ui.label(format!("Expected: {}", expected.unwrap_or("N/A")));
            ui.label(format!("Actual: {}", actual.unwrap_or("Unmapped")));
        }
        PracticeResultKind::Missed => {
            ui.strong("MISSED");
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

#[allow(dead_code)]
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
                    .unwrap_or_else(|| "N/A".to_string()),
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
                        .unwrap_or_else(|| "N/A".to_string()),
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

#[allow(dead_code)]
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
            "Suggested: {} notes | Confidence: {}",
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
        "Tempo: {:.1} BPM | {} subdivision",
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

fn midi_event_table(ui: &mut egui::Ui, expected: &[ExpectedNote]) {
    egui::Grid::new("library_midi_events")
        .striped(true)
        .num_columns(5)
        .spacing([16.0, 4.0])
        .show(ui, |ui| {
            ui.strong("Time");
            ui.strong("Tick");
            ui.strong("MIDI");
            ui.strong("Component");
            ui.strong("Velocity");
            ui.end_row();
            for event in expected.iter().take(1_000) {
                ui.monospace(format!("{:.3}s", event.timestamp_seconds));
                ui.monospace(event.tick.to_string());
                ui.monospace(event.midi_note.to_string());
                ui.monospace(
                    event
                        .drum_component
                        .map(|value| value.label())
                        .unwrap_or("Unknown"),
                );
                ui.monospace(event.velocity.to_string());
                ui.end_row();
            }
        });
    if expected.len() > 1_000 {
        ui.weak(format!("Showing 1,000 of {} events.", expected.len()));
    }
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

#[cfg(test)]
mod phase5_visual_tests {
    use super::*;

    fn result(kind: PracticeResultKind, judgment: Option<TimingJudgment>) -> PracticeResult {
        PracticeResult {
            kind,
            expected: None,
            actual: None,
            timing_difference_ticks: None,
            timing_difference_micros: None,
            timing_judgment: judgment,
        }
    }

    #[test]
    fn simultaneous_events_share_exact_horizontal_position() {
        let kick_x = timeline_x(3.25, 1.0, 0.6, 4.0, 100.0, 260.0, 900.0);
        let hi_hat_x = timeline_x(3.25, 1.0, 0.6, 4.0, 100.0, 260.0, 900.0);
        assert_eq!(kick_x, hi_hat_x);
    }

    #[test]
    fn actual_unquantized_times_keep_distinct_positions() {
        let tick_480 = timeline_x(1.0, 0.0, 0.6, 4.0, 100.0, 260.0, 900.0);
        let tick_482 = timeline_x(1.004_166, 0.0, 0.6, 4.0, 100.0, 260.0, 900.0);
        assert!(tick_482 > tick_480);
    }

    #[test]
    fn subdivision_tokens_follow_musical_counting() {
        assert_eq!(subdivision_token(3, 0, 480, Subdivision::Quarter), "3");
        assert_eq!(subdivision_token(2, 240, 480, Subdivision::Eighth), "&");
        assert_eq!(subdivision_token(4, 120, 480, Subdivision::Sixteenth), "e");
        assert_eq!(subdivision_token(4, 240, 480, Subdivision::Sixteenth), "&");
        assert_eq!(subdivision_token(4, 360, 480, Subdivision::Sixteenth), "a");
    }

    #[test]
    fn practice_bpm_validation_accepts_only_transport_range() {
        assert_eq!(valid_practice_bpm(20.0), Some(20.0));
        assert_eq!(valid_practice_bpm(120.0), Some(120.0));
        assert_eq!(valid_practice_bpm(300.0), Some(300.0));
        assert_eq!(valid_practice_bpm(0.0), None);
        assert_eq!(valid_practice_bpm(-10.0), None);
        assert_eq!(valid_practice_bpm(301.0), None);
        assert_eq!(valid_practice_bpm(f64::NAN), None);
    }

    #[test]
    fn subdivision_controls_use_unambiguous_note_labels() {
        assert_eq!(
            subdivision_long_label(Subdivision::Quarter),
            "Quarter Notes"
        );
        assert_eq!(subdivision_long_label(Subdivision::Eighth), "Eighth Notes");
        assert_eq!(
            subdivision_long_label(Subdivision::Sixteenth),
            "Sixteenth Notes"
        );
    }

    #[test]
    fn every_component_and_unknown_have_stable_lanes() {
        for (index, component) in DrumComponent::ALL.into_iter().enumerate() {
            assert_eq!(lane_index(Some(component), false), index);
        }
        assert_eq!(lane_index(None, true), DrumComponent::ALL.len());
    }

    #[test]
    fn lookahead_seconds_map_to_expected_bars_at_sixty_bpm() {
        use drums_core::midi::{TempoEvent, TimeSignatureEvent};
        let transport = MusicalTransport::new(
            MidiTiming::TicksPerQuarter(480),
            &[TempoEvent {
                tick: 0,
                micros_per_quarter: 1_000_000,
            }],
            &[TimeSignatureEvent {
                tick: 0,
                numerator: 4,
                denominator_exponent: 2,
                clocks_per_click: 24,
                thirty_seconds_per_quarter: 8,
            }],
            10_000,
        );
        for (seconds, expected_tick, expected_bars) in
            [(2.0, 960, 0.5), (4.0, 1_920, 1.0), (8.0, 3_840, 2.0)]
        {
            let (_, end) = visible_time_window(0.0, transport.total_seconds(), 0.6, seconds);
            assert_eq!(transport.effective_tick_at_seconds(end), expected_tick);
            assert_eq!(end / 4.0, expected_bars);
        }
    }

    #[test]
    fn actual_result_states_have_distinct_overlay_colors() {
        let good = actual_result_color(&result(
            PracticeResultKind::CorrectHit,
            Some(TimingJudgment::Good),
        ));
        let early = actual_result_color(&result(
            PracticeResultKind::CorrectHit,
            Some(TimingJudgment::Early),
        ));
        let late = actual_result_color(&result(
            PracticeResultKind::CorrectHit,
            Some(TimingJudgment::Late),
        ));
        let wrong = actual_result_color(&result(PracticeResultKind::WrongDrum, None));
        let extra = actual_result_color(&result(PracticeResultKind::Ignored, None));
        assert_ne!(good, early);
        assert_ne!(early, late);
        assert_ne!(late, wrong);
        assert_ne!(wrong, extra);
    }

    #[test]
    fn missed_result_identifies_its_expected_marker() {
        let note = ExpectedNote {
            tick: 480,
            timestamp_seconds: 1.0,
            midi_note: 36,
            velocity: 100,
            channel: 9,
            track: 0,
            drum_component: Some(DrumComponent::Kick),
            target_component: Some(DrumComponent::Kick),
        };
        let mut missed = result(PracticeResultKind::Missed, None);
        missed.expected = Some(note.clone());
        assert!(expected_is_missed(&note, &[missed]));
        assert!(!expected_is_missed(
            &note,
            &[result(PracticeResultKind::Ignored, None)]
        ));
    }
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
