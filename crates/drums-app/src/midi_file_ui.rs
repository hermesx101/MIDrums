//! Track Library and Practice views. Parsing and scoring remain in `drums-core`.

use std::collections::BTreeSet;

use crate::{DrumApp, LibrarySort, PracticeLoopMode, PracticeVisual, View};
use drums_core::{
    count_tokens, DrumComponent, ExpectedNote, MidiFile, MidiTiming, MusicalTransport,
    PracticePerformance, PracticeResult, PracticeResultKind, Subdivision, TargetState,
    TimingJudgment, TimingTendency, TransportState,
};
use eframe::egui;

const RESULT_CORRECT: egui::Color32 = egui::Color32::from_rgb(72, 214, 137);
const RESULT_EARLY: egui::Color32 = egui::Color32::from_rgb(244, 196, 70);
const RESULT_LATE: egui::Color32 = egui::Color32::from_rgb(246, 132, 68);
const RESULT_WRONG_DRUM: egui::Color32 = egui::Color32::from_rgb(244, 82, 118);
const RESULT_MISSED: egui::Color32 = egui::Color32::from_rgb(120, 124, 135);
const RESULT_EXTRA: egui::Color32 = egui::Color32::from_rgb(165, 105, 220);

/// Timeline-only cache rebuilt with the active lesson rather than per frame.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ExpectedTimelineDrums {
    components: BTreeSet<DrumComponent>,
    has_unknown: bool,
}

impl ExpectedTimelineDrums {
    pub(crate) fn from_expected(expected: &[ExpectedNote]) -> Self {
        Self {
            components: expected
                .iter()
                .filter_map(|note| note.drum_component)
                .collect(),
            has_unknown: expected.iter().any(|note| note.drum_component.is_none()),
        }
    }
}

#[derive(Default)]
struct LibraryCardActions {
    practice_path: Option<String>,
    analysis_path: Option<String>,
    sheet_path: Option<String>,
    rename: Option<(String, String)>,
    remove: Option<String>,
}

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
    let mut actions = LibraryCardActions::default();
    for entry in &entries {
        ui.push_id(("track_row", &entry.path), |ui| {
            // Bound only the horizontal dimension. Height remains entirely
            // content-driven, and the page-level ScrollArea handles overflow.
            ui.set_width(ui.available_width());
            library_card(app, ui, entry, &mut actions);
        });
        ui.add_space(8.0);
    }

    if let Some((path, name)) = actions.rename {
        app.library_rename_path = Some(path);
        app.library_rename_buffer = name;
    }
    if let Some(path) = actions.remove {
        app.library_remove_path = Some(path);
    }
    if let Some(path) = actions.practice_path {
        match app.open_library_entry(&path) {
            Ok(()) => app.view = View::Practice,
            Err(error) => app.midi_import_error = Some(error),
        }
    }
    if let Some(path) = actions.analysis_path {
        app.analysis_track_path = Some(path);
        app.analysis_session_id = None;
        app.view = View::PerformanceAnalysis;
    }
    if let Some(path) = actions.sheet_path {
        if let Err(error) = app.open_sheet_music(&path) {
            app.midi_import_error = Some(error);
        }
    }
    library_edit_prompts(app, ui);
}

fn library_card(
    app: &mut DrumApp,
    ui: &mut egui::Ui,
    entry: &crate::track_library::LibraryEntry,
    actions: &mut LibraryCardActions,
) {
    let selected = app.library_selected_path.as_deref() == Some(entry.path.as_str());
    let active = app.current_library_path.as_deref() == Some(entry.path.as_str());
    let card_fill = if selected {
        ui.visuals().widgets.active.weak_bg_fill
    } else {
        ui.visuals().faint_bg_color
    };
    egui::Frame::group(ui.style())
        .fill(card_fill)
        .stroke(egui::Stroke::new(
            if selected { 2.0_f32 } else { 1.0_f32 },
            if selected {
                app.accent_color()
            } else {
                ui.visuals().widgets.noninteractive.bg_stroke.color
            },
        ))
        .inner_margin(12.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());

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
                library_card_menu(ui, entry, actions);
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
                let history = app
                    .session_store
                    .load_for_track(&entry.path)
                    .unwrap_or_default();
                if let Some(latest) = history.first() {
                    ui.colored_label(
                        egui::Color32::from_rgb(90, 210, 135),
                        format!("Last: {:.1}%", latest.statistics.accuracy),
                    );
                    if let Some(best) = history
                        .iter()
                        .max_by(|a, b| a.statistics.accuracy.total_cmp(&b.statistics.accuracy))
                    {
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

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui.button("Practice  >").clicked() {
                    actions.practice_path = Some(entry.path.clone());
                }
                if ui.button("Performance Analysis").clicked() {
                    actions.analysis_path = Some(entry.path.clone());
                }
                if ui.button("Sheet Music").clicked() {
                    actions.sheet_path = Some(entry.path.clone());
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
    actions: &mut LibraryCardActions,
) {
    ui.menu_button("...", |ui| {
        if ui.button("Practice").clicked() {
            actions.practice_path = Some(entry.path.clone());
            ui.close_menu();
        }
        if ui.button("Performance Analysis").clicked() {
            actions.analysis_path = Some(entry.path.clone());
            ui.close_menu();
        }
        if ui.button("Sheet Music").clicked() {
            actions.sheet_path = Some(entry.path.clone());
            ui.close_menu();
        }
        if ui.button("Rename").clicked() {
            actions.rename = Some((entry.path.clone(), entry.display_name.clone()));
            ui.close_menu();
        }
        if ui.button("Remove from Library").clicked() {
            actions.remove = Some(entry.path.clone());
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
                app.save_track_library();
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
                app.save_track_library();
                app.library_remove_path = None;
            }
        });
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
    let expected = file.expected_notes_for_track(track, app.active_config.as_ref());
    practice_interface(app, ui, &expected);
}

pub fn show_practice_persistent_header(app: &mut DrumApp, ui: &mut egui::Ui) {
    if app.imported_midi.is_some() && app.selected_drum_track.is_some() {
        practice_header(app, ui);
    }
}

pub fn show_practice_sheet_content(app: &mut DrumApp, ui: &mut egui::Ui) {
    let (Some(file), Some(track)) = (&app.imported_midi, app.selected_drum_track) else {
        ui.label("No track selected");
        return;
    };
    let expected = file.expected_notes_for_track(track, app.active_config.as_ref());
    practice_content(app, ui, &expected);
}

fn backing_audio_controls(app: &mut DrumApp, ui: &mut egui::Ui) {
    let loaded = app.backing_audio.is_loaded();
    let mut choose_audio = false;
    let mut remove_audio = false;
    let mut seek = app.backing_audio.position();
    let duration = app.backing_audio.duration();
    let mut seek_changed = false;
    ui.horizontal_wrapped(|ui| {
        ui.strong("AUDIO BACKING TRACK");
        if !loaded {
            ui.weak("No backing track loaded");
            choose_audio = ui.button("Add Backing Track").clicked();
            return;
        }
        ui.label(
            app.backing_audio
                .path()
                .and_then(|p| p.file_name())
                .and_then(|p| p.to_str())
                .unwrap_or("Audio"),
        );
        if ui.small_button("Play").clicked() {
            app.backing_audio.play();
        }
        if ui.small_button("Pause").clicked() {
            app.backing_audio.pause();
        }
        if ui.small_button("Stop").clicked() {
            app.backing_audio.stop();
            seek = 0.0;
        }
        ui.monospace(format!("{} / {}", time_text(seek), time_text(duration)));
        seek_changed = ui
            .add_sized(
                [220.0, 20.0],
                egui::Slider::new(&mut seek, 0.0..=duration.max(0.001))
                    .show_value(false)
                    .text("Seek"),
            )
            .changed();
        let mut volume = app.backing_audio.volume();
        if ui
            .add_sized(
                [135.0, 20.0],
                egui::Slider::new(&mut volume, 0.0..=1.0).text("Volume"),
            )
            .changed()
        {
            app.backing_audio.set_volume(volume);
        }
        let mut muted = app.backing_audio.muted();
        if ui.checkbox(&mut muted, "Mute").changed() {
            app.backing_audio.set_muted(muted);
        }
        if ui
            .checkbox(
                &mut app.start_audio_with_practice,
                "Start audio with practice",
            )
            .changed()
        {
            let _ = app.save_practice_settings();
        }
        choose_audio = ui.small_button("Replace Audio").clicked();
        remove_audio = ui.small_button("Remove Audio").clicked();
    });
    if seek_changed {
        let _ = app.backing_audio.seek(seek);
        if let Some(transport) = app.transport.as_mut() {
            let midi_seconds = (seek - app.audio_sync_offset_ms / 1_000.0).max(0.0);
            transport.seek(
                transport
                    .effective_tick_at_seconds(midi_seconds)
                    .min(i64::MAX as u64) as i64,
            );
            if let Some(session) = app.practice_session.as_mut() {
                session.on_seek_to_tick(transport.current_tick(), transport);
            }
        }
    }
    if remove_audio {
        app.backing_audio.unload();
        app.status = "Backing audio removed from the current practice context.".into();
    }
    if choose_audio {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Audio", &["mp3", "wav", "ogg", "flac"])
            .set_title("Choose backing track")
            .pick_file()
        else {
            return;
        };
        match app.backing_audio.load(path) {
            Ok(()) => app.status = "Backing audio loaded for the current practice.".into(),
            Err(error) => app.status = format!("Could not load audio file. Reason: {error}"),
        }
    }
}

/// Phase 5A practice composition. Musical state remains owned by drums-core;
/// this function only arranges controls and renders a bounded view of it.
fn practice_interface(app: &mut DrumApp, ui: &mut egui::Ui, expected: &[ExpectedNote]) {
    practice_header(app, ui);
    practice_content(app, ui, expected);
}

fn practice_header(app: &mut DrumApp, ui: &mut egui::Ui) {
    practice_top_bar(app, ui);
    ui.add_space(10.0);

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
    practice_feedback_bar(app, ui);
    ui.add_space(6.0);
    egui::Frame::group(ui.style())
        .inner_margin(egui::Margin::symmetric(10, 7))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            backing_audio_controls(app, ui)
        });
    ui.add_space(6.0);
    practice_metronome_strip(app, ui);
    ui.add_space(4.0);
}

fn practice_content(app: &mut DrumApp, ui: &mut egui::Ui, expected: &[ExpectedNote]) {
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
        return;
    }

    if let Some(transport) = app.transport.as_ref() {
        let results = app
            .practice_session
            .as_ref()
            .map(|session| session.results())
            .unwrap_or(&[]);
        match app.practice_visual {
            PracticeVisual::Timeline => {
                rhythm_grid(
                    ui,
                    transport,
                    expected,
                    results,
                    RhythmGridOptions {
                        subdivision: app.metronome.subdivision(),
                        lookahead_seconds: app.practice_lookahead_seconds,
                        loop_region: app.active_loop_region(),
                        expected_drums_only: app.expected_drums_only,
                        expected_drums: &app.expected_timeline_drums,
                    },
                );
            }
            PracticeVisual::SheetMusic => {
                if ui.input(|input| input.raw_scroll_delta.y.abs() > 0.1) {
                    app.notation_follow_suspended_until =
                        Some(std::time::Instant::now() + std::time::Duration::from_secs(3));
                }
                let follow = app.notation_follow_playback
                    && app
                        .notation_follow_suspended_until
                        .is_none_or(|until| std::time::Instant::now() >= until);
                if let Some(score) = app.notation_score.as_ref() {
                    crate::notation_ui::show_practice_score(
                        ui,
                        score,
                        transport.current_tick(),
                        results,
                        follow,
                        app.notation_zoom,
                    )
                } else {
                    ui.label("Sheet music is unavailable for this MIDI timing format.");
                }
            }
        }
    }
}

fn practice_metronome_strip(app: &mut DrumApp, ui: &mut egui::Ui) {
    let Some(transport) = app.transport.as_ref() else {
        return;
    };
    let subdivision = app.metronome.subdivision();
    let tokens = count_tokens(
        transport.position_map(),
        transport.current_tick(),
        subdivision,
    );
    ui.horizontal(|ui| {
        egui::Frame::group(ui.style())
            .fill(ui.visuals().extreme_bg_color)
            .inner_margin(egui::Margin::symmetric(8, 4))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.strong("METRONOME");
                    let toggle = if app.metronome_enabled { "ON" } else { "OFF" };
                    ui.checkbox(&mut app.metronome_enabled, toggle);
                    ui.weak(subdivision_long_label(subdivision));
                    ui.separator();
                    for token in &tokens {
                        let color = if token.active {
                            egui::Color32::from_rgb(105, 220, 255)
                        } else {
                            egui::Color32::from_gray(165)
                        };
                        ui.colored_label(
                            color,
                            egui::RichText::new(&token.label)
                                .strong()
                                .size(if token.active { 18.0 } else { 16.0 }),
                        );
                    }
                });
            });
        if app.practice_visual == PracticeVisual::SheetMusic {
            ui.add_space(12.0);
            ui.separator();
            ui.weak("SHEET MUSIC");
            if ui.small_button("−").clicked() {
                app.notation_zoom = (app.notation_zoom - 0.1).max(0.7);
            }
            ui.weak(format!("Zoom {:.0}%", app.notation_zoom * 100.0));
            if ui.small_button("+").clicked() {
                app.notation_zoom = (app.notation_zoom + 0.1).min(2.0);
            }
            ui.checkbox(&mut app.notation_follow_playback, "Follow Playback");
        } else {
            ui.add_space(12.0);
            ui.separator();
            if ui
                .checkbox(&mut app.expected_drums_only, "Expected Drums Only")
                .changed()
            {
                if let Err(error) = app.save_practice_settings() {
                    app.status = format!("Could not save Practice settings: {error}");
                }
            }
        }
    });
}

fn timeline_label_width(total_width: f32) -> f32 {
    total_width.clamp(110.0, 155.0) * 0.85
}

struct TimelineHorizontalGeometry {
    grid_left: f32,
    beat_origin: f32,
}

fn timeline_horizontal_geometry(rect: egui::Rect) -> TimelineHorizontalGeometry {
    let grid_left = rect.left() + timeline_label_width(rect.width());
    TimelineHorizontalGeometry {
        grid_left,
        beat_origin: grid_left + (rect.right() - grid_left) * 0.20,
    }
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
        .map(|file| file.filename.clone())
        .unwrap_or_else(|| "MIDI Practice".into());
    let track_meta = app
        .imported_midi
        .as_ref()
        .map(|file| {
            let (n, d) = transport.position_map().time_signature_at_tick(0);
            let notes = app
                .selected_drum_track
                .and_then(|i| file.tracks.get(i))
                .map(|t| t.note_event_count)
                .unwrap_or(0);
            format!("{n}/{d}  •  {}  •  {notes} notes", time_text(total))
        })
        .unwrap_or_default();

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
    let count_display_tokens = count_tokens(transport.position_map(), current_tick, subdivision);

    egui::Frame::group(ui.style())
        .inner_margin(10.0)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        go_to_tracks = ui.small_button("← Library").clicked();
                        ui.add(
                            egui::Label::new(egui::RichText::new(&song_name).strong().size(17.0))
                                .truncate(),
                        );
                        ui.weak(&track_meta);
                    });
                    ui.horizontal(|ui| {
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
                        ui.monospace(format!("{} / {}", time_text(elapsed), time_text(total)));
                        ui.weak(format!("Bar {}  Beat {}", musical.measure, musical.beat));
                    });
                });
                ui.separator();
                ui.vertical(|ui| {
                    ui.weak("PRACTICE BPM");
                    ui.horizontal(|ui| {
                        bpm_delta -= ui.button("−").clicked() as u8 as f64;
                        manual_bpm_changed |= ui
                            .add_sized(
                                [88.0, 30.0],
                                egui::DragValue::new(&mut manual_bpm)
                                    .range(20.0..=300.0)
                                    .speed(1.0)
                                    .suffix(" BPM"),
                            )
                            .changed();
                        bpm_delta += ui.button("+").clicked() as u8 as f64;
                        reset_bpm = ui.small_button("Reset").clicked();
                    });
                    ui.small(format!("Original {original_bpm:.0} BPM"));
                });
                ui.separator();
                ui.vertical(|ui| {
                    ui.weak("SUBDIVISION");
                    egui::ComboBox::from_id_salt("top_subdivision")
                        .width(150.0)
                        .selected_text(if subdivision_auto {
                            format!("Auto ({})", subdivision_long_label(subdivision))
                        } else {
                            subdivision_long_label(subdivision).to_string()
                        })
                        .show_ui(ui, |ui| {
                            if ui.selectable_label(subdivision_auto, "Auto").clicked() {
                                subdivision_auto = true;
                                subdivision_changed = true;
                                if let Some(s) = app.subdivision_suggestion {
                                    subdivision = s.subdivision;
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
                });
                ui.separator();
                ui.vertical(|ui| {
                    ui.weak("COUNT-IN");
                    ui.horizontal(|ui| {
                        egui::ComboBox::from_id_salt("count_in")
                            .width(58.0)
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
                                            format!(
                                                "{bars} Bar{}",
                                                if bars == 1 { "" } else { "s" }
                                            )
                                        },
                                    );
                                }
                            });
                        let mut count_enabled = app.count_in_bars > 0;
                        if ui.checkbox(&mut count_enabled, "Enabled").changed() {
                            app.count_in_bars = if count_enabled { 1 } else { 0 };
                        }
                    });
                    if app.count_in_active {
                        ui.horizontal(|ui| {
                            for token in &count_display_tokens {
                                if token.active {
                                    ui.colored_label(
                                        egui::Color32::from_rgb(120, 205, 255),
                                        egui::RichText::new(&token.label).strong(),
                                    );
                                } else {
                                    ui.label(&token.label);
                                }
                            }
                        });
                    }
                });
                ui.separator();
                ui.vertical(|ui| {
                    ui.weak("LOOKAHEAD");
                    egui::ComboBox::from_id_salt("lookahead")
                        .width(72.0)
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
                });
                ui.separator();
                ui.vertical(|ui| {
                    ui.weak("LOOP");
                    egui::ComboBox::from_id_salt("practice_loop")
                        .width(72.0)
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
                        ui.horizontal(|ui| {
                            ui.label("Bars");
                            custom_changed |= ui
                                .add(
                                    egui::DragValue::new(&mut app.practice_loop_start_bar)
                                        .range(1..=last_bar.max(1)),
                                )
                                .changed();
                            custom_changed |= ui
                                .add(
                                    egui::DragValue::new(&mut app.practice_loop_end_bar)
                                        .range(2..=last_bar + 1),
                                )
                                .changed();
                        });
                    } else if app.practice_loop_mode != PracticeLoopMode::Off {
                        ui.weak(format!(
                            "{}–{} · {}",
                            app.practice_loop_start_bar,
                            app.practice_loop_end_bar,
                            app.practice_loop_iteration
                        ));
                    }
                });
                ui.separator();
                ui.vertical(|ui| {
                    ui.weak("SEEK");
                    ui.add_sized(
                        [125.0, 28.0],
                        egui::Slider::new(&mut seek_tick, 0..=final_tick)
                            .show_value(false)
                            .text(""),
                    );
                });
            });
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
        if app.start_audio_with_practice && app.backing_audio.is_loaded() {
            if app.count_in_active {
                app.backing_audio.pause();
            } else {
                let p = drums_core::audio_position(
                    transport.elapsed_seconds(),
                    app.audio_sync_offset_ms,
                    app.backing_audio.duration(),
                );
                let _ = app.backing_audio.seek(p);
                app.backing_audio.play();
            }
        }
    }
    if pause {
        transport.pause();
        app.metronome.sync_to_transport(transport);
        app.backing_audio.pause();
    }
    if stop {
        app.count_in_active = false;
        transport.stop();
        app.metronome.reset();
        if let Some(session) = app.practice_session.as_mut() {
            session.on_stop();
        }
        app.backing_audio.stop();
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
        if app.backing_audio.is_loaded() {
            let p = drums_core::audio_position(
                transport.elapsed_seconds(),
                app.audio_sync_offset_ms,
                app.backing_audio.duration(),
            );
            let _ = app.backing_audio.seek(p);
        }
    }
    if go_to_tracks {
        transport.pause();
        app.metronome.sync_to_transport(transport);
        app.view = View::TrackLibrary;
        app.backing_audio.stop();
    }
}

fn valid_practice_bpm(value: f64) -> Option<f64> {
    (value.is_finite() && (20.0..=300.0).contains(&value)).then_some(value)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum HeaderLabelPriority {
    Subdivision,
    Beat,
    Bar,
}

#[derive(Clone, Debug)]
struct HeaderLabelCandidate {
    x: f32,
    width: f32,
    priority: HeaderLabelPriority,
    text: String,
}

/// Selects readable labels without moving them away from their musical x-coordinate.
fn select_header_labels(
    candidates: &[HeaderLabelCandidate],
    left: f32,
    right: f32,
    minimum_gap: f32,
) -> Vec<usize> {
    let mut priority_order = (0..candidates.len()).collect::<Vec<_>>();
    priority_order.sort_by(|left_index, right_index| {
        let left_label = &candidates[*left_index];
        let right_label = &candidates[*right_index];
        right_label
            .priority
            .cmp(&left_label.priority)
            .then_with(|| left_label.x.total_cmp(&right_label.x))
    });

    let mut occupied = Vec::<(f32, f32)>::new();
    let mut selected = Vec::new();
    for index in priority_order {
        let candidate = &candidates[index];
        // Labels remain left-anchored just after the exact grid/bar boundary.
        let start = candidate.x + 3.0;
        let end = start + candidate.width;
        if start < left || end > right {
            continue;
        }
        let collides = occupied.iter().any(|(other_start, other_end)| {
            start < *other_end + minimum_gap && end + minimum_gap > *other_start
        });
        if !collides {
            occupied.push((start, end));
            selected.push(index);
        }
    }
    selected.sort_by(|left_index, right_index| {
        candidates[*left_index]
            .x
            .total_cmp(&candidates[*right_index].x)
    });
    selected
}

#[derive(Clone, Copy)]
struct RhythmGridOptions<'a> {
    subdivision: Subdivision,
    lookahead_seconds: f64,
    loop_region: Option<(u64, u64)>,
    expected_drums_only: bool,
    expected_drums: &'a ExpectedTimelineDrums,
}

fn rhythm_grid(
    ui: &mut egui::Ui,
    transport: &MusicalTransport,
    expected: &[ExpectedNote],
    results: &[PracticeResult],
    options: RhythmGridOptions<'_>,
) {
    let RhythmGridOptions {
        subdivision,
        lookahead_seconds,
        loop_region,
        expected_drums_only,
        expected_drums,
    } = options;
    // Loop iterations reuse musical ticks. Keep the complete result history in
    // drums-core for statistics, but bound visualization work and overlap.
    let results = &results[results.len().saturating_sub(256)..];
    let lanes = timeline_lanes(expected_drums_only, expected_drums, results);
    // The rhythm grid is the primary practice surface. At compact window
    // sizes it remains usable; on larger displays it receives the extra room.
    let height = (ui.available_height() - 42.0).clamp(320.0, 720.0);
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::hover(),
    );
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 6.0, egui::Color32::from_rgb(15, 20, 29));
    let geometry = timeline_horizontal_geometry(rect);
    let label_width = geometry.grid_left - rect.left();
    let header_height = 38.0;
    let chart_left = geometry.grid_left;
    let hit_x = geometry.beat_origin;
    let lane_height = (rect.height() - header_height) / lanes.len().max(1) as f32;
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
                    egui::pos2(x + 3.0, rect.top() + header_height + 3.0),
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
    if lanes.is_empty() {
        painter.text(
            egui::pos2(
                (chart_left + rect.right()) * 0.5,
                rect.top() + header_height + lane_height * 0.5,
            ),
            egui::Align2::CENTER_CENTER,
            "No expected drums in this lesson",
            egui::FontId::proportional(13.0),
            egui::Color32::from_gray(145),
        );
    }

    if let Some(ppq) = transport.position_map().ticks_per_quarter() {
        let step = (ppq as u64 / subdivision.units_per_quarter()).max(1);
        let start_tick = transport.effective_tick_at_seconds((now - past_seconds).max(0.0));
        let end_tick = transport
            .effective_tick_at_seconds((now + lookahead_seconds).min(transport.total_seconds()));
        let header_font = egui::FontId::monospace(11.0);
        let header_color = egui::Color32::from_gray(175);
        let mut bar_labels = Vec::new();
        let mut musical_labels = Vec::new();
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
            if is_bar {
                let text = format!("Bar {}", position.measure);
                let width = painter
                    .layout_no_wrap(text.clone(), header_font.clone(), header_color)
                    .size()
                    .x;
                bar_labels.push(HeaderLabelCandidate {
                    x,
                    width,
                    priority: HeaderLabelPriority::Bar,
                    text,
                });
            }
            let priority = if is_beat {
                HeaderLabelPriority::Beat
            } else {
                HeaderLabelPriority::Subdivision
            };
            let width = painter
                .layout_no_wrap(token.clone(), header_font.clone(), header_color)
                .size()
                .x;
            musical_labels.push(HeaderLabelCandidate {
                x,
                width,
                priority,
                text: token,
            });
            tick = tick.saturating_add(step);
            if step == 0 {
                break;
            }
        }

        // Bar numbers and musical counts use separate header rows. Within
        // each row the selector preserves exact x positions and progressively
        // drops lower-priority subdivision labels when space is constrained.
        for index in select_header_labels(&bar_labels, chart_left + 1.0, rect.right() - 2.0, 5.0) {
            let label = &bar_labels[index];
            painter.text(
                egui::pos2(label.x + 3.0, rect.top() + 9.0),
                egui::Align2::LEFT_CENTER,
                &label.text,
                header_font.clone(),
                header_color,
            );
        }
        for index in
            select_header_labels(&musical_labels, chart_left + 1.0, rect.right() - 2.0, 5.0)
        {
            let label = &musical_labels[index];
            painter.text(
                egui::pos2(label.x + 3.0, rect.top() + 27.0),
                egui::Align2::LEFT_CENTER,
                &label.text,
                header_font.clone(),
                header_color,
            );
        }
    }

    for note in expected {
        let seconds = transport.effective_seconds_at_tick(note.tick);
        if seconds < window_start || seconds > window_end {
            continue;
        }
        let Some(lane) = lane_index(&lanes, note.drum_component) else {
            continue;
        };
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
        if !timeline_actual_hit_visible(expected_drums_only, expected_drums, actual.drum_component)
        {
            continue;
        }
        let seconds = transport.effective_seconds_at_tick(actual.transport_tick);
        if seconds < window_start || seconds > window_end {
            continue;
        }
        let Some(lane) = lane_index(&lanes, actual.drum_component) else {
            continue;
        };
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
    paint_visual_legend(&painter, rect, label_width);
}

fn actual_result_color(result: &PracticeResult) -> egui::Color32 {
    match result.kind {
        PracticeResultKind::CorrectHit => match result.timing_judgment {
            Some(TimingJudgment::Early) => RESULT_EARLY,
            Some(TimingJudgment::Late) => RESULT_LATE,
            _ => RESULT_CORRECT,
        },
        PracticeResultKind::WrongDrum => RESULT_WRONG_DRUM,
        PracticeResultKind::Ignored => RESULT_EXTRA,
        PracticeResultKind::Missed => RESULT_MISSED,
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

fn paint_visual_legend(painter: &egui::Painter, rect: egui::Rect, label_width: f32) {
    let items = [
        ("Extra", RESULT_EXTRA, false),
        ("Missed", RESULT_MISSED, false),
        ("Wrong Drum", RESULT_WRONG_DRUM, false),
        ("Late", RESULT_LATE, false),
        ("Early", RESULT_EARLY, false),
        ("Correct", RESULT_CORRECT, false),
        ("Expected", egui::Color32::from_rgb(238, 205, 70), true),
    ];
    let grid_left = rect.left() + label_width;
    let available = (rect.right() - grid_left - 16.0).max(1.0);
    let natural_width = items
        .iter()
        .map(|(label, _, _)| label.len() as f32 * 6.2 + 25.0)
        .sum::<f32>();
    let scale = (available / natural_width).min(1.0);
    let font = egui::FontId::proportional((12.0 * scale).max(9.0));
    let total_width = natural_width * scale;
    let start_x = (rect.right() - total_width - 8.0).max(grid_left + 8.0);
    let legend_rect = egui::Rect::from_min_max(
        egui::pos2(start_x - 6.0, rect.bottom() - 27.0),
        egui::pos2(rect.right() - 4.0, rect.bottom() - 4.0),
    );
    painter.rect_filled(
        legend_rect,
        2.0,
        egui::Color32::from_rgba_unmultiplied(15, 20, 29, 225),
    );
    let mut x = start_x;
    let y = legend_rect.center().y;
    for (label, color, outlined) in items {
        let radius = (5.0 * scale).max(3.5);
        if outlined {
            painter.circle_stroke(
                egui::pos2(x + radius, y),
                radius,
                egui::Stroke::new(1.5_f32, color),
            );
        } else {
            painter.circle_filled(egui::pos2(x + radius, y), radius, color);
        }
        painter.text(
            egui::pos2(x + radius * 2.0 + 5.0 * scale, y),
            egui::Align2::LEFT_CENTER,
            label,
            font.clone(),
            egui::Color32::from_gray(205),
        );
        x += (label.len() as f32 * 6.2 + 25.0) * scale;
    }
}

fn subdivision_token(
    beat: u32,
    ticks_into_beat: u64,
    ticks_per_beat: u64,
    subdivision: Subdivision,
) -> String {
    let units = subdivision.units_per_quarter().max(1);
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

fn timeline_lanes(
    expected_drums_only: bool,
    expected_drums: &ExpectedTimelineDrums,
    results: &[PracticeResult],
) -> Vec<Option<DrumComponent>> {
    if expected_drums_only {
        let mut lanes = DrumComponent::ALL
            .into_iter()
            .filter(|component| expected_drums.components.contains(component))
            .map(Some)
            .collect::<Vec<_>>();
        if expected_drums.has_unknown {
            lanes.push(None);
        }
        return lanes;
    }

    let mut lanes = DrumComponent::ALL.into_iter().map(Some).collect::<Vec<_>>();
    let has_unknown_actual = results.iter().any(|result| {
        result
            .actual
            .as_ref()
            .is_some_and(|hit| hit.drum_component.is_none())
    });
    if expected_drums.has_unknown || has_unknown_actual {
        lanes.push(None);
    }
    lanes
}

fn timeline_actual_hit_visible(
    expected_drums_only: bool,
    expected_drums: &ExpectedTimelineDrums,
    component: Option<DrumComponent>,
) -> bool {
    !expected_drums_only
        || component
            .map(|component| expected_drums.components.contains(&component))
            .unwrap_or(expected_drums.has_unknown)
}

fn lane_index(lanes: &[Option<DrumComponent>], component: Option<DrumComponent>) -> Option<usize> {
    lanes.iter().position(|lane| *lane == component)
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
        egui::vec2(ui.available_width(), 90.0),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            egui::Frame::group(ui.style())
                .inner_margin(8.0)
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
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
                        columns[2].horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{:.1}% accuracy",
                                        performance.accuracy_percent()
                                    ))
                                    .strong()
                                    .size(20.0),
                                );
                                ui.label(format!(
                                    "{} / {} finalized",
                                    performance.correct_hits, performance.finalized_expected
                                ));
                            });
                            ui.add_space(10.0);
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(10.0, 4.0);
                                let statistic = |text: String| {
                                    egui::RichText::new(text)
                                        .size(12.5)
                                        .color(egui::Color32::from_gray(185))
                                };
                                ui.horizontal(|ui| {
                                    ui.label(statistic(format!(
                                        "Correct {}",
                                        performance.good_hits
                                    )));
                                    ui.label(statistic(format!(
                                        "Early {}",
                                        performance.early_hits
                                    )));
                                    ui.label(statistic(format!("Late {}", performance.late_hits)));
                                });
                                ui.horizontal(|ui| {
                                    ui.label(statistic(format!(
                                        "Wrong Drum {}",
                                        performance.wrong_drums
                                    )));
                                    ui.label(statistic(format!("Missed {}", performance.missed)));
                                    ui.label(statistic(format!(
                                        "Extra {}",
                                        performance.ignored_extra_hits
                                    )));
                                });
                            });
                        });
                    });
                });
        },
    );
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
                let (label, color) = match judgment {
                    drums_core::TimingJudgment::Early => ("EARLY", RESULT_EARLY),
                    drums_core::TimingJudgment::Good => ("CORRECT", RESULT_CORRECT),
                    drums_core::TimingJudgment::Late => ("LATE", RESULT_LATE),
                };
                ui.label(egui::RichText::new(label).strong().size(20.0).color(color));
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
            ui.colored_label(
                RESULT_WRONG_DRUM,
                egui::RichText::new("WRONG DRUM").strong(),
            );
            ui.label(format!("Expected: {}", expected.unwrap_or("N/A")));
            ui.label(format!("Actual: {}", actual.unwrap_or("Unmapped")));
        }
        PracticeResultKind::Missed => {
            ui.colored_label(RESULT_MISSED, egui::RichText::new("MISSED").strong());
            ui.label(format!("Expected: {}", expected.unwrap_or("Unknown")));
        }
        PracticeResultKind::Ignored => {
            ui.colored_label(RESULT_EXTRA, egui::RichText::new("EXTRA").strong());
            ui.label(format!("Actual: {}", actual.unwrap_or("Unmapped")));
        }
    }
    if result.kind == PracticeResultKind::WrongDrum {
        if let Some(value) = timing_micros {
            ui.monospace(format!("{:+.1} ms", value as f64 / 1_000.0));
        }
    }
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
            ui.colored_label(RESULT_CORRECT, "CORRECT");
            ui.monospace(performance.good_hits.to_string());
            ui.end_row();
            ui.colored_label(RESULT_EARLY, "EARLY");
            ui.monospace(performance.early_hits.to_string());
            ui.end_row();
            ui.colored_label(RESULT_LATE, "LATE");
            ui.monospace(performance.late_hits.to_string());
            ui.end_row();
            ui.colored_label(RESULT_WRONG_DRUM, "WRONG DRUM");
            ui.monospace(performance.wrong_drums.to_string());
            ui.end_row();
            ui.colored_label(RESULT_MISSED, "MISSED");
            ui.monospace(performance.missed.to_string());
            ui.end_row();
            ui.colored_label(RESULT_EXTRA, "EXTRA");
            ui.monospace(performance.ignored_extra_hits.to_string());
            ui.end_row();
            ui.label("Correct timing percentage");
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

#[cfg(test)]
mod phase5_visual_tests {
    use super::*;

    fn expected_note(component: Option<DrumComponent>, tick: u64) -> ExpectedNote {
        ExpectedNote {
            tick,
            timestamp_seconds: tick as f64 / 960.0,
            midi_note: 36,
            velocity: 100,
            channel: 9,
            track: 0,
            drum_component: component,
            target_component: component,
        }
    }

    fn actual_hit(component: Option<DrumComponent>, tick: u64) -> drums_core::ActualHit {
        drums_core::ActualHit {
            received_midi_note: 38,
            drum_component: component,
            velocity: 100,
            channel: 9,
            input_timestamp_micros: 1,
            elapsed_seconds: tick as f64 / 960.0,
            transport_tick: tick,
        }
    }

    fn test_transport() -> MusicalTransport {
        use drums_core::midi::{TempoEvent, TimeSignatureEvent};
        MusicalTransport::new(
            MidiTiming::TicksPerQuarter(480),
            &[TempoEvent {
                tick: 0,
                micros_per_quarter: 500_000,
            }],
            &[TimeSignatureEvent {
                tick: 0,
                numerator: 4,
                denominator_exponent: 2,
                clocks_per_click: 24,
                thirty_seconds_per_quarter: 8,
            }],
            10_000,
        )
    }

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
        let expected = ExpectedTimelineDrums::default();
        let lanes = timeline_lanes(false, &expected, &[]);
        for (index, component) in DrumComponent::ALL.into_iter().enumerate() {
            assert_eq!(lane_index(&lanes, Some(component)), Some(index));
        }
        assert_eq!(lane_index(&lanes, None), None);

        let all_expected_notes = DrumComponent::ALL
            .into_iter()
            .enumerate()
            .map(|(index, component)| expected_note(Some(component), index as u64 * 120))
            .collect::<Vec<_>>();
        let all_expected = ExpectedTimelineDrums::from_expected(&all_expected_notes);
        assert_eq!(timeline_lanes(true, &all_expected, &[]), lanes);
    }

    #[test]
    fn expected_drums_filter_is_dynamic_and_off_preserves_all_actual_hits() {
        let expected = ExpectedTimelineDrums::from_expected(&[
            expected_note(Some(DrumComponent::HiHat), 0),
            expected_note(Some(DrumComponent::Kick), 0),
        ]);
        assert!(timeline_actual_hit_visible(
            false,
            &expected,
            Some(DrumComponent::LowTom)
        ));
        assert!(timeline_actual_hit_visible(
            true,
            &expected,
            Some(DrumComponent::HiHat)
        ));
        assert!(timeline_actual_hit_visible(
            true,
            &expected,
            Some(DrumComponent::Kick)
        ));
        assert!(!timeline_actual_hit_visible(
            true,
            &expected,
            Some(DrumComponent::LowTom)
        ));

        let filtered_lanes = timeline_lanes(true, &expected, &[]);
        assert_eq!(
            filtered_lanes,
            vec![Some(DrumComponent::HiHat), Some(DrumComponent::Kick)]
        );
    }

    #[test]
    fn expected_filter_keeps_unknown_only_when_the_lesson_expects_it() {
        let known =
            ExpectedTimelineDrums::from_expected(&[expected_note(Some(DrumComponent::Snare), 0)]);
        assert!(!timeline_actual_hit_visible(true, &known, None));

        let unknown = ExpectedTimelineDrums::from_expected(&[expected_note(None, 0)]);
        assert!(timeline_actual_hit_visible(true, &unknown, None));
        assert_eq!(timeline_lanes(true, &unknown, &[]), vec![None]);

        let empty = ExpectedTimelineDrums::default();
        assert!(timeline_lanes(true, &empty, &[]).is_empty());
        assert!(!timeline_actual_hit_visible(
            true,
            &empty,
            Some(DrumComponent::Kick)
        ));
    }

    #[test]
    fn hi_hat_states_remain_distinct_expected_roles() {
        let expected = ExpectedTimelineDrums::from_expected(&[
            expected_note(Some(DrumComponent::HiHat), 0),
            expected_note(Some(DrumComponent::HiHatOpen), 120),
            expected_note(Some(DrumComponent::HiHatPedal), 240),
        ]);
        assert_eq!(
            timeline_lanes(true, &expected, &[]),
            vec![
                Some(DrumComponent::HiHat),
                Some(DrumComponent::HiHatOpen),
                Some(DrumComponent::HiHatPedal),
            ]
        );
    }

    #[test]
    fn expected_filter_cache_replaces_track_roles_and_survives_view_window_changes() {
        let first =
            ExpectedTimelineDrums::from_expected(&[expected_note(Some(DrumComponent::Kick), 0)]);
        let second = ExpectedTimelineDrums::from_expected(&[
            expected_note(Some(DrumComponent::Crash), 0),
            expected_note(Some(DrumComponent::Ride), 480),
        ]);
        assert_ne!(first, second);
        assert_eq!(
            timeline_lanes(true, &second, &[]),
            vec![Some(DrumComponent::Crash), Some(DrumComponent::Ride)]
        );

        let unchanged = second.clone();
        let _ = visible_time_window(12.0, 120.0, 1.5, 8.0);
        let _ = timeline_x(13.0, 12.0, 1.5, 8.0, 100.0, 250.0, 900.0);
        assert_eq!(second, unchanged);
    }

    #[test]
    fn hiding_unrelated_hits_does_not_change_wrong_extra_or_accuracy_results() {
        let transport = test_transport();
        let note = expected_note(Some(DrumComponent::Kick), 960);
        let expected = ExpectedTimelineDrums::from_expected(std::slice::from_ref(&note));
        let mut session =
            drums_core::PracticeSession::new(vec![note], drums_core::MatchingConfig::default());
        session.record_hit(actual_hit(Some(DrumComponent::Snare), 960), &transport);
        session.record_hit(actual_hit(Some(DrumComponent::LowTom), 3_000), &transport);

        let before_filter = PracticePerformance::from_session(&session);
        let visible_actuals = session
            .results()
            .iter()
            .filter_map(|result| result.actual.as_ref())
            .filter(|actual| timeline_actual_hit_visible(true, &expected, actual.drum_component))
            .count();
        let after_filter = PracticePerformance::from_session(&session);

        assert_eq!(visible_actuals, 0);
        assert_eq!(before_filter, after_filter);
        assert_eq!(after_filter.wrong_drums, 1);
        assert_eq!(after_filter.ignored_extra_hits, 1);
        assert_eq!(after_filter.accuracy_percent(), 0.0);
    }

    #[test]
    fn adaptive_header_keeps_beats_ahead_of_colliding_subdivisions() {
        let labels = vec![
            HeaderLabelCandidate {
                x: 20.0,
                width: 9.0,
                priority: HeaderLabelPriority::Subdivision,
                text: "&".into(),
            },
            HeaderLabelCandidate {
                x: 24.0,
                width: 9.0,
                priority: HeaderLabelPriority::Beat,
                text: "2".into(),
            },
            HeaderLabelCandidate {
                x: 60.0,
                width: 9.0,
                priority: HeaderLabelPriority::Subdivision,
                text: "&".into(),
            },
        ];
        let selected = select_header_labels(&labels, 0.0, 100.0, 5.0);
        assert!(!selected.contains(&0));
        assert!(selected.contains(&1));
        assert!(selected.contains(&2));
    }

    #[test]
    fn adaptive_header_drops_crowded_long_bar_labels_without_moving_boundaries() {
        let labels = vec![
            HeaderLabelCandidate {
                x: 10.0,
                width: 48.0,
                priority: HeaderLabelPriority::Bar,
                text: "Bar 999".into(),
            },
            HeaderLabelCandidate {
                x: 45.0,
                width: 55.0,
                priority: HeaderLabelPriority::Bar,
                text: "Bar 1000".into(),
            },
            HeaderLabelCandidate {
                x: 110.0,
                width: 55.0,
                priority: HeaderLabelPriority::Bar,
                text: "Bar 1001".into(),
            },
        ];
        let selected = select_header_labels(&labels, 0.0, 180.0, 5.0);
        assert_eq!(selected, vec![0, 2]);
        assert_eq!(labels[0].x, 10.0);
        assert_eq!(labels[2].x, 110.0);
    }

    #[test]
    fn header_labels_never_overlap_or_escape_chart_bounds() {
        let labels = (0..20)
            .map(|index| HeaderLabelCandidate {
                x: index as f32 * 8.0,
                width: 12.0,
                priority: if index % 4 == 0 {
                    HeaderLabelPriority::Beat
                } else {
                    HeaderLabelPriority::Subdivision
                },
                text: index.to_string(),
            })
            .collect::<Vec<_>>();
        let selected = select_header_labels(&labels, 10.0, 120.0, 5.0);
        let mut previous_end = None;
        for index in selected {
            let start = labels[index].x + 3.0;
            let end = start + labels[index].width;
            assert!(start >= 10.0);
            assert!(end <= 120.0);
            if let Some(previous_end) = previous_end {
                assert!(start >= previous_end + 5.0);
            }
            previous_end = Some(end);
        }
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
        let expected = ExpectedTimelineDrums::from_expected(std::slice::from_ref(&note));
        let lanes = timeline_lanes(true, &expected, &[]);
        assert_eq!(lane_index(&lanes, note.drum_component), Some(0));
        assert!(expected_is_missed(&note, &[missed]));
        assert!(!expected_is_missed(
            &note,
            &[result(PracticeResultKind::Ignored, None)]
        ));
    }
}
