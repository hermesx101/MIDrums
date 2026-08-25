//! Dedicated application preferences and application-owned data reset.

use std::{
    fmt, fs, io,
    path::{Path, PathBuf},
};

use eframe::egui;

use crate::practice_settings::{AccentColour, ThemePreference, UiScale};
use crate::{apply_app_appearance, practice_settings, track_library, DrumApp};

#[derive(Default)]
pub(crate) struct ResetConfirmation {
    open: bool,
}

impl ResetConfirmation {
    fn request(&mut self) {
        self.open = true;
    }

    fn cancel(&mut self) {
        self.open = false;
    }

    fn confirm(&mut self) -> bool {
        std::mem::take(&mut self.open)
    }
}

pub(crate) fn show(app: &mut DrumApp, ui: &mut egui::Ui) {
    ui.heading("PREFERENCES");
    ui.add_space(10.0);
    ui.strong("APPEARANCE");
    ui.add_space(4.0);

    let mut appearance_changed = false;
    egui::Frame::group(ui.style())
        .inner_margin(egui::Margin::symmetric(12, 9))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width().min(560.0));
            egui::Grid::new("appearance_preferences")
                .num_columns(2)
                .spacing([24.0, 9.0])
                .show(ui, |ui| {
                    ui.label("Theme");
                    egui::ComboBox::from_id_salt("theme_preference")
                        .width(150.0)
                        .selected_text(app.theme_preference.label())
                        .show_ui(ui, |ui| {
                            for value in [
                                ThemePreference::System,
                                ThemePreference::Dark,
                                ThemePreference::Light,
                            ] {
                                appearance_changed |= ui
                                    .selectable_value(
                                        &mut app.theme_preference,
                                        value,
                                        value.label(),
                                    )
                                    .changed();
                            }
                        });
                    ui.end_row();

                    ui.label("Accent Colour");
                    ui.horizontal(|ui| {
                        ui.colored_label(app.accent_color(), "●");
                        egui::ComboBox::from_id_salt("accent_colour_preference")
                            .width(125.0)
                            .selected_text(app.accent_colour.label())
                            .show_ui(ui, |ui| {
                                for value in [
                                    AccentColour::Blue,
                                    AccentColour::Teal,
                                    AccentColour::Purple,
                                    AccentColour::Amber,
                                ] {
                                    appearance_changed |= ui
                                        .selectable_value(
                                            &mut app.accent_colour,
                                            value,
                                            value.label(),
                                        )
                                        .changed();
                                }
                            });
                    });
                    ui.end_row();

                    ui.label("UI Scale");
                    egui::ComboBox::from_id_salt("ui_scale_preference")
                        .width(150.0)
                        .selected_text(app.ui_scale.label())
                        .show_ui(ui, |ui| {
                            for value in [UiScale::Small, UiScale::Normal, UiScale::Large] {
                                appearance_changed |= ui
                                    .selectable_value(&mut app.ui_scale, value, value.label())
                                    .changed();
                            }
                        });
                    ui.end_row();
                });
        });

    if appearance_changed {
        apply_app_appearance(
            ui.ctx(),
            app.theme_preference,
            app.accent_colour,
            app.ui_scale,
        );
        match app.save_practice_settings() {
            Ok(()) => app.status = "Preferences saved.".into(),
            Err(error) => app.status = format!("Could not save Preferences: {error}"),
        }
    }

    show_audio_preferences(app, ui);

    ui.add_space(18.0);
    ui.strong("DATA");
    ui.add_space(4.0);
    egui::Frame::group(ui.style())
        .inner_margin(egui::Margin::symmetric(12, 9))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width().min(560.0));
            ui.strong("Reset All Data");
            ui.weak(
                "Remove application settings, mappings, library records, and practice history.",
            );
            ui.add_space(6.0);
            if ui.button("Reset All Data").clicked() {
                app.reset_confirmation.request();
            }
        });

    if !app.status.is_empty() {
        ui.add_space(8.0);
        ui.weak(&app.status);
    }

    show_reset_confirmation(app, ui.ctx());
}

fn show_audio_preferences(app: &mut DrumApp, ui: &mut egui::Ui) {
    ui.add_space(18.0);
    ui.strong("AUDIO OUTPUT");
    ui.add_space(4.0);

    let available_devices = app.audio_output.available_devices().to_vec();
    let current_preference = app.audio_output.preferred_device().map(str::to_string);
    let preferred_unavailable = current_preference
        .as_ref()
        .is_some_and(|preferred| !available_devices.iter().any(|device| device == preferred));
    let mut selected_device = current_preference.clone();
    let mut volume_percent = app.audio_output.volume() * 100.0;
    let mut muted = app.audio_output.muted();
    let mut refresh_devices = false;
    let mut test_audio = false;

    egui::Frame::group(ui.style())
        .inner_margin(egui::Margin::symmetric(12, 9))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width().min(560.0));
            egui::Grid::new("audio_output_preferences")
                .num_columns(2)
                .spacing([24.0, 9.0])
                .show(ui, |ui| {
                    ui.label("Audio Output Device");
                    ui.horizontal(|ui| {
                        egui::ComboBox::from_id_salt("audio_output_device")
                            .width(260.0)
                            .selected_text(
                                selected_device
                                    .as_deref()
                                    .unwrap_or(crate::audio_output::DEFAULT_SYSTEM_DEVICE),
                            )
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut selected_device,
                                    None,
                                    crate::audio_output::DEFAULT_SYSTEM_DEVICE,
                                );
                                if preferred_unavailable {
                                    ui.add_enabled(
                                        false,
                                        egui::Label::new(format!(
                                            "{} (unavailable)",
                                            current_preference.as_deref().unwrap_or_default()
                                        )),
                                    );
                                }
                                for device in &available_devices {
                                    ui.selectable_value(
                                        &mut selected_device,
                                        Some(device.clone()),
                                        device,
                                    );
                                }
                            });
                        refresh_devices = ui.small_button("Refresh").clicked();
                    });
                    ui.end_row();

                    ui.label("Output Volume");
                    ui.horizontal(|ui| {
                        ui.add_sized(
                            [250.0, 20.0],
                            egui::Slider::new(&mut volume_percent, 0.0..=100.0).show_value(false),
                        );
                        ui.monospace(format!("{volume_percent:.0}%"));
                    });
                    ui.end_row();

                    ui.label("Mute");
                    ui.checkbox(&mut muted, "Mute application audio");
                    ui.end_row();

                    ui.label("");
                    test_audio = ui.button("Test Audio").clicked();
                    ui.end_row();
                });

            let active = app
                .audio_output
                .active_device()
                .unwrap_or("No audio output available");
            ui.weak(format!("Currently using: {active}"));
            if app.audio_output.using_fallback() {
                ui.weak("Selected audio device unavailable. Using Default System Device.");
            } else if let Some(notice) = app.audio_output.notice() {
                ui.weak(notice);
            }
        });

    let mut settings_changed = false;
    if selected_device != current_preference {
        let update = app.audio_output.select_device(selected_device);
        let route_error = app.sync_backing_audio_output().err();
        app.status = if let Some(error) = route_error {
            format!("Audio output changed, but backing audio could not be routed: {error}")
        } else if let Some(notice) = update.notice {
            notice
        } else {
            format!(
                "Audio output set to {}.",
                app.audio_output
                    .active_device()
                    .unwrap_or(crate::audio_output::DEFAULT_SYSTEM_DEVICE)
            )
        };
        settings_changed = true;
    }
    let volume = (volume_percent / 100.0).clamp(0.0, 1.0);
    if (volume - app.audio_output.volume()).abs() > f32::EPSILON {
        app.audio_output.set_volume(volume);
        app.apply_audio_output_gain();
        settings_changed = true;
    }
    if muted != app.audio_output.muted() {
        app.audio_output.set_muted(muted);
        app.apply_audio_output_gain();
        settings_changed = true;
    }
    if refresh_devices {
        let update = app.audio_output.refresh_devices();
        let route_error = app.sync_backing_audio_output().err();
        app.status = if let Some(error) = route_error {
            format!("Audio devices refreshed, but backing audio could not be routed: {error}")
        } else if let Some(notice) = update.notice {
            notice
        } else {
            "Audio output devices refreshed.".into()
        };
    }
    if test_audio {
        app.status = match app.audio_output.play_test_sound() {
            Ok(()) => format!(
                "Test sound sent to {}.",
                app.audio_output
                    .active_device()
                    .unwrap_or(crate::audio_output::DEFAULT_SYSTEM_DEVICE)
            ),
            Err(error) => format!("Could not play test audio: {error}"),
        };
    }
    if settings_changed {
        match app.save_practice_settings() {
            Ok(()) => {}
            Err(error) => app.status = format!("Could not save Audio preferences: {error}"),
        }
    }
}

fn show_reset_confirmation(app: &mut DrumApp, ctx: &egui::Context) {
    if !app.reset_confirmation.open {
        return;
    }
    let mut cancel = false;
    let mut confirm = false;
    let response = egui::Modal::new(egui::Id::new("reset_all_data_confirmation")).show(ctx, |ui| {
        ui.set_max_width(440.0);
        ui.heading("RESET ALL DATA?");
        ui.add_space(6.0);
        ui.label("This will permanently remove:");
        for item in [
            "Drum mappings and Setup Wizard completion",
            "Practice sessions, progress, and performance history",
            "Track Library records",
            "Practice, audio, and appearance preferences",
            "Other application-owned configuration",
        ] {
            ui.label(format!("• {item}"));
        }
        ui.add_space(6.0);
        ui.strong("Your original MIDI and audio files will not be deleted.");
        ui.weak("This action cannot be undone.");
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            cancel = ui.button("Cancel").clicked();
            confirm = ui.button("Reset Everything").clicked();
        });
    });
    if cancel || response.should_close() {
        app.reset_confirmation.cancel();
    } else if confirm && app.reset_confirmation.confirm() {
        let paths = ApplicationDataPaths::for_app(app);
        match reset_application_data(&paths) {
            Ok(()) => app.restore_first_launch_after_reset(ctx),
            Err(error) => {
                app.status = format!("Reset All Data was incomplete: {error}");
            }
        }
    }
}

#[derive(Clone, Debug)]
struct ApplicationDataPaths {
    mappings_dir: PathBuf,
    sessions_file: PathBuf,
    track_library_file: PathBuf,
    settings_file: PathBuf,
}

impl ApplicationDataPaths {
    fn for_app(app: &DrumApp) -> Self {
        Self {
            mappings_dir: app.store.dir().to_path_buf(),
            sessions_file: app.session_store.path().to_path_buf(),
            track_library_file: track_library::storage_path(),
            settings_file: practice_settings::path(),
        }
    }
}

#[derive(Debug)]
struct ResetError(Vec<String>);

impl fmt::Display for ResetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0.join("; "))
    }
}

fn reset_application_data(paths: &ApplicationDataPaths) -> Result<(), ResetError> {
    let mut failures = Vec::new();
    clear_mapping_records(&paths.mappings_dir, &mut failures);
    remove_file(
        &paths.sessions_file,
        "practice session history",
        &mut failures,
    );
    remove_file(
        &paths.track_library_file,
        "Track Library records",
        &mut failures,
    );
    remove_file(&paths.settings_file, "preferences", &mut failures);
    if failures.is_empty() {
        Ok(())
    } else {
        Err(ResetError(failures))
    }
}

fn clear_mapping_records(directory: &Path, failures: &mut Vec<String>) {
    if !directory.exists() {
        return;
    }
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) => {
            failures.push(format!("drum mappings: {error}"));
            return;
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                failures.push(format!("drum mappings: {error}"));
                continue;
            }
        };
        let path = entry.path();
        let is_mapping_file = entry.file_type().is_ok_and(|kind| kind.is_file())
            && path.extension().and_then(|extension| extension.to_str()) == Some("toml");
        if is_mapping_file {
            remove_file(&path, "drum mapping", failures);
        }
    }
}

fn remove_file(path: &Path, label: &str, failures: &mut Vec<String>) {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => failures.push(format!("{label}: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use drums_core::{
        DrumComponent, DrumKitConfiguration, DrumMapping, MappingEventKind, MappingStore,
        PracticeSessionStore,
    };

    fn temp_root(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "drums-reset-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn reset_requires_confirmation_state() {
        let mut confirmation = ResetConfirmation::default();
        assert!(!confirmation.open);
        confirmation.request();
        assert!(confirmation.open);
        confirmation.cancel();
        assert!(!confirmation.open);
        assert!(!confirmation.confirm());
        confirmation.request();
        assert!(confirmation.confirm());
        assert!(!confirmation.open);
    }

    #[test]
    fn reset_removes_only_application_owned_data_and_restores_first_launch_storage() {
        let root = temp_root("owned-data");
        let app_data = root.join("app-data");
        let mappings_dir = app_data.join("mappings");
        let source_midi = root.join("user-files").join("lesson.mid");
        fs::create_dir_all(&mappings_dir).unwrap();
        fs::create_dir_all(source_midi.parent().unwrap()).unwrap();
        fs::write(&source_midi, b"user midi source").unwrap();

        let mapping_store = MappingStore::new(&mappings_dir);
        let mut configuration = DrumKitConfiguration::new("kit");
        configuration
            .add_input(DrumMapping {
                component: DrumComponent::Kick,
                event_kind: MappingEventKind::Note,
                midi_note: 36,
                channel: 10,
                articulation: None,
                cc_range: None,
            })
            .unwrap();
        mapping_store.save(&configuration).unwrap();
        assert!(!crate::needs_initial_setup(
            &mapping_store.load_all().unwrap()
        ));

        let paths = ApplicationDataPaths {
            mappings_dir: mappings_dir.clone(),
            sessions_file: app_data.join("practice-sessions.toml"),
            track_library_file: app_data.join("track-library.toml"),
            settings_file: app_data.join("practice-settings.toml"),
        };
        fs::write(&paths.sessions_file, "sessions = []\n").unwrap();
        fs::write(
            &paths.track_library_file,
            format!("source = {:?}\n", source_midi),
        )
        .unwrap();
        fs::write(&paths.settings_file, "theme = 'light'\n").unwrap();

        let mut confirmation = ResetConfirmation::default();
        confirmation.request();
        assert!(paths.settings_file.exists());
        confirmation.cancel();
        assert!(paths.settings_file.exists());
        confirmation.request();
        assert!(confirmation.confirm());
        reset_application_data(&paths).unwrap();

        assert!(mapping_store.load_all().unwrap().is_empty());
        assert!(!paths.sessions_file.exists());
        assert!(!paths.track_library_file.exists());
        assert!(!paths.settings_file.exists());
        assert!(source_midi.exists());
        let stored = mapping_store.load_all().unwrap();
        assert!(crate::needs_initial_setup(&stored));
        assert!(PracticeSessionStore::new(&paths.sessions_file)
            .load_all()
            .unwrap()
            .is_empty());
        let reset_settings = practice_settings::load_from(&paths.settings_file);
        assert_eq!(
            reset_settings,
            practice_settings::PracticeSettings::default()
        );

        fs::remove_dir_all(root).unwrap();
    }
}
